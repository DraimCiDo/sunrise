#!/usr/bin/env python3
"""Speed of sunrise-core's `java_string_hash` (UTF-16 units) and `java_string_hash_str` (`&str`) against HotSpot's
String.hashCode on fresh strings, by the math kits' method.

- Fresh strings: each JVM batch makes its strings anew before timing one hashCode call on each, so the hash is never
  cached (a cached hash is a field read, not comparable). Rust's entries cache nothing.
- Rows: every mix (ascii, latin1: compact Latin-1 strings in the JDK; bmp; surrogates: pairs) at every length.
- Fresh JVMs: each row runs `--starts` JVMs of `--rounds` rounds each (400 timed batches per round), alternating with
  the Rust bench on the same pinned core; HotSpot goes first in every other round.
- Placement: `--copies` byte-identical copies of crates/sunrise-core/src/java_string.rs, each with its own loops; a
  filler before each copy's loops shifts it, and the kit rebuilds until the loops start at distinct offsets mod 64,
  read from the binary with nm, and prints them. A row's figure is the mean over the copies.
- Confidence: per round, the copies' mean divided by HotSpot's round; the median ratio with a percentile-bootstrap
  95% interval. `faster`: the whole interval under 0.98; `slower`: over 1.00; otherwise `inside the noise`.
- Allocations in the timed Rust loops are counted (0 expected); `check` compares both sides' hash sums per row.

Writes .local/string-hash-kit/report.json and prints the table. Needs cargo, java 25 (PATH or --java) and nm.
"""
import argparse
import json
import os
import random
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
MODULE = ROOT / "crates/sunrise-core/src/java_string.rs"
BENCH = ROOT / "mods/sunrise/src/test/java/dev/sunriselab/parity/StringHashBench.java"
BENCH_CLASS = "dev.sunriselab.parity.StringHashBench"
BUILD = ROOT / ".local/string-hash-kit"
MIXES = ["ascii", "latin1", "bmp", "surrogates"]
LENGTHS = [0, 1, 8, 32, 128, 1024]
ENTRIES = ["utf16", "str"]


def log(message):
    print(message, flush=True)


def run(*args, **kw):
    return subprocess.run([str(a) for a in args], check=True, **kw)


def median(values):
    s = sorted(values)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2


def cpu_list(text):
    out = set()
    for part in text.strip().split(","):
        if "-" in part:
            a, b = part.split("-")
            out.update(range(int(a), int(b) + 1))
        elif part:
            out.add(int(part))
    return out


def pick_cpu():
    """A performance core on a hybrid processor (/sys/devices/cpu_core/cpus, or the cores whose cpuinfo_max_freq is
    clearly the highest), else CPU 2 if allowed, else the last allowed CPU; and why."""
    allowed = sorted(os.sched_getaffinity(0))
    default = 2 if 2 in allowed else allowed[-1]
    try:
        p_cores = [c for c in allowed if c in cpu_list(Path("/sys/devices/cpu_core/cpus").read_text())]
    except (OSError, ValueError):
        p_cores = []
    if p_cores:
        c = default if default in p_cores else p_cores[min(2, len(p_cores) - 1)]
        return c, f"a P-core ({len(p_cores)} of {len(allowed)} allowed CPUs, from /sys/devices/cpu_core/cpus)"
    freqs = {}
    for c in allowed:
        try:
            freqs[c] = int(Path(f"/sys/devices/system/cpu/cpu{c}/cpufreq/cpuinfo_max_freq").read_text())
        except (OSError, ValueError):
            pass
    if freqs and max(freqs.values()) > 1.1 * min(freqs.values()):
        top = max(freqs.values())
        fast = [c for c, f in freqs.items() if f == top]
        c = default if default in fast else fast[min(2, len(fast) - 1)]
        return c, f"one of the fastest cores by cpuinfo_max_freq ({top / 1e6:.2f} GHz)"
    return default, "the default rule (CPU 2 if allowed, else the last allowed CPU): no hybrid cores found"


GROUPS = ["module", "utf16", "str"]


def filler(name, uid, n):
    body = [f"    let mut v = x ^ {uid};",
            *(f"    v = black_box(v).wrapping_mul({2 * k + 3}).rotate_left({k % 7 + 1});" for k in range(n)), "    v"]
    return ["#[inline(never)]", f"pub fn {name}(x: u64) -> u64 {{", *body, "}"]


def generate(copies, fill):
    """The bench crate: bench.rs as main.rs, and `copies` verbatim copies of the module, each with its own two loops.
    Before every copy but the first, a filler of fill[group] statements: named to sort just before what it shifts
    (the compiler emits a module's functions in symbol-name order), so consecutive copies of a group are the same
    distance apart."""
    crate = BUILD / "rust"
    src = crate / "src"
    shutil.rmtree(src, ignore_errors=True)
    (src / "copies").mkdir(parents=True)
    (crate / "Cargo.toml").write_text(
        '[package]\nname = "string-hash-kit"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n\n'
        '[workspace]\n\n[profile.release]\nopt-level = 3\ncodegen-units = 1\nlto = "thin"\n')
    shutil.copyfile(HERE / "bench.rs", src / "main.rs")
    lines = ["//! Written by tools/string_hash_kit/run.py.", "#![allow(dead_code)]", "use std::hint::black_box;", ""]
    utf16, text, fillers = [], [], []
    for c in range(copies):
        (src / "copies" / f"copy_{c}").mkdir()
        shutil.copyfile(MODULE, src / "copies" / f"copy_{c}" / "java_string.rs")
        # `java_strinf` sorts just before `java_string`'s functions.
        (src / "copies" / f"copy_{c}.rs").write_text(
            "use std::hint::black_box;\npub mod java_string;\n"
            + "\n".join(filler("java_strinf", 1000 + c, fill["module"] if c else 0)) + "\n")
        lines.append(f"pub mod copy_{c};")
        fillers.append(f"    copy_{c}::java_strinf,")
    for c in range(copies):
        for group, entry, arg in (("utf16", "java_string_hash", "&[&[u16]]"), ("str", "java_string_hash_str", "&[&str]")):
            lines += filler(f"{group}_{c}_g", 2000 + 10 * c + len(group), fill[group] if c else 0)
            # Each loop starts from a sum of its own (c + 1), so that no two copies' loops are one function.
            lines += ["#[inline(never)]", f"pub fn {group}_{c}_h(s: {arg}) -> u32 {{",
                      f"    s.iter().fold({c + 1}u32, |a, x| a.wrapping_add(copy_{c}::java_string::{entry}(black_box(x)) as u32))",
                      "}"]
            fillers.append(f"    {group}_{c}_g,")
        utf16.append(f"    ({c}, utf16_{c}_h),")
        text.append(f"    ({c}, str_{c}_h),")
    lines += ["pub const UTF16: &[(usize, fn(&[&[u16]]) -> u32)] = &[", *utf16, "];",
              "pub const STR: &[(usize, fn(&[&str]) -> u32)] = &[", *text, "];",
              "pub const FILLERS: &[fn(u64) -> u64] = &[", *fillers, "];", ""]
    (src / "copies.rs").write_text("\n".join(lines))
    return crate


def symbols(binary):
    """{(function, copy): address} for the copies' loops and the module's out-of-line functions."""
    out = {}
    for line in run("nm", "-C", "--defined-only", binary, capture_output=True, text=True).stdout.splitlines():
        parts = line.split(maxsplit=2)
        if len(parts) != 3:
            continue
        name = re.sub(r"::h[0-9a-f]{16}$", "", parts[2])
        m = re.search(r"copies::(?:(utf16|str)_(\d+)_h|copy_(\d+)::java_string::(\w+(?:::<\w+>)?))$", name)
        if m:
            key, c = (m.group(1), m.group(2)) if m.group(1) else (m.group(4), m.group(3))
            out.setdefault(key, {})[int(c)] = int(parts[0], 16)
    return out


def build_placed(copies, attempts=16):
    """Builds until every group's copies start at min(copies, 4) distinct offsets mod 64: while a group's copies share
    an offset, its filler grows by a statement. -Z merge-functions=disabled (RUSTC_BOOTSTRAP, this build only) keeps
    LLVM from folding the byte-identical copies of the module's out-of-line functions into one."""
    need = min(copies, 4)
    fill = {g: 1 for g in GROUPS}
    env = {**os.environ, "CARGO_TARGET_DIR": str(BUILD / "target"), "RUSTC_BOOTSTRAP": "1",
           "RUSTFLAGS": "-Z merge-functions=disabled"}
    for attempt in range(1, attempts + 1):
        crate = generate(copies, fill)
        run("cargo", "build", "--release", "--quiet", "--manifest-path", crate / "Cargo.toml", env=env)
        binary = BUILD / "target/release/string-hash-kit"
        groups = symbols(binary)
        ok = {k: len(g) == copies and len({a % 64 for a in g.values()}) >= need for k, g in groups.items()}
        if all(ok.values()):
            break
        if not all(ok[e] for e in ENTRIES if e in ok):
            for e in ENTRIES:
                fill[e] += not ok.get(e, False)
        if not all(v for k, v in ok.items() if k not in ENTRIES):
            fill["module"] += 1
    distinct = all(ok.values())
    log(f"placement: {attempt} build(s), fillers {fill}; " + ("every copy at a distinct offset mod 64" if distinct
                                                             else "NOT all distinct"))
    for key, g in sorted(groups.items()):
        offsets = [g[c] % 64 for c in sorted(g)]
        log(f"  {key:22} offsets mod 64 per copy: {offsets}" + ("" if ok[key] else " (shared)"))
    return binary, {k: {c: a % 64 for c, a in g.items()} for k, g in groups.items()}, distinct


def bootstrap_median_ci(values, rng, resamples=4000):
    n = len(values)
    meds = sorted(median([values[rng.randrange(n)] for _ in range(n)]) for _ in range(resamples))
    return median(values), meds[int(0.025 * resamples)], meds[int(0.975 * resamples) - 1]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--copies", type=int, default=4)
    parser.add_argument("--starts", type=int, default=8, help="fresh JVMs per row")
    parser.add_argument("--rounds", type=int, default=5, help="rounds per JVM")
    parser.add_argument("--mixes", default=",".join(MIXES))
    parser.add_argument("--lengths", default=",".join(map(str, LENGTHS)))
    parser.add_argument("--java", default=shutil.which("java"), help="a JDK 25's bin/java")
    parser.add_argument("--check-only", action="store_true",
                        help="build, place the copies and compare every row's hash sums; time nothing")
    args = parser.parse_args()
    BUILD.mkdir(parents=True, exist_ok=True)
    java = Path(args.java)
    log(run(java, "-version", capture_output=True, text=True).stderr.splitlines()[0])
    classes = BUILD / "classes"
    classes.mkdir(exist_ok=True)
    run(java.with_name("javac"), "--release", "25", "-d", classes, BENCH)
    binary, offsets, placed = build_placed(args.copies)
    cpu, why = pick_cpu()
    pin = ["taskset", "-c", str(cpu)] if shutil.which("taskset") else []
    log(f"pinned to CPU {cpu}: {why}" if pin else "not pinned (no taskset)")
    rng = random.Random(20261119)
    report = {"java": str(java), "cpu": cpu, "why": why, "copies": args.copies, "offsets": offsets,
              "placement_distinct": placed, "rows": {}}
    allocations = 0
    rows = [(m, int(n)) for m in args.mixes.split(",") for n in args.lengths.split(",")]
    for mix, length in rows:
        rust_sum = run(binary, "check", mix, length, capture_output=True, text=True).stdout.split()
        java_sum = run(java, "-cp", classes, BENCH_CLASS, mix, length, 0, "check",
                       capture_output=True, text=True).stdout.split()
        if rust_sum != java_sum * 2:
            sys.exit(f"{mix} {length}: hash sums differ: rust {rust_sum}, java {java_sum}")
        if args.check_only:
            log(f"{mix:10} {length:5}: hash sums agree")
            continue
        rounds = []
        for start in range(args.starts):
            jvm = subprocess.Popen([str(a) for a in (*pin, java, "-cp", classes, BENCH_CLASS, mix, length,
                                                     args.rounds, "stepped")],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

            def hotspot():
                jvm.stdin.write("\n")
                jvm.stdin.flush()
                return float(re.search(r"hotspot median\s+([\d.]+) ns", jvm.stdout.readline()).group(1))

            def rust():
                ns = {}
                for line in run(*pin, binary, "bench", mix, length, capture_output=True, text=True).stdout.splitlines():
                    row = json.loads(line)
                    ns.setdefault(row["entry"], {})[row["copy"]] = row["median_ns"]
                    nonlocal allocations
                    allocations += row["allocations"]
                return ns

            for i in range(args.rounds):
                h = hotspot() if i % 2 else None
                ns = rust()
                if h is None:
                    h = hotspot()
                rounds.append({"ns": ns, "hotspot_ns": h, "jvm_start": start})
            jvm.stdin.close()
            jvm.wait()
        per_start = [median([r["hotspot_ns"] for r in rounds if r["jvm_start"] == s]) for s in range(args.starts)]
        row = {"hotspot_ns": median([r["hotspot_ns"] for r in rounds]),
               "hotspot_start_spread": (max(per_start) - min(per_start)) / median(per_start), "entries": {},
               "raw": rounds}
        mean = lambda d: sum(d.values()) / len(d)
        for entry in ENTRIES:
            med, lo, hi = bootstrap_median_ci([mean(r["ns"][entry]) / r["hotspot_ns"] for r in rounds], rng)
            copies = {c: median([r["ns"][entry][c] / r["hotspot_ns"] for r in rounds])
                      for c in sorted(rounds[0]["ns"][entry])}
            row["entries"][entry] = {
                "rust_ns": median([mean(r["ns"][entry]) for r in rounds]), "ratio_median": med,
                "ratio_ci95": [lo, hi], "copies": copies,
                "status": ("faster" if hi < 0.98 else "slower" if lo > 1.0 else "inside the noise")
                + ("" if placed else ", not placement-averaged")}
        report["rows"][f"{mix} {length}"] = row
        log(f"{mix:10} {length:5}: hotspot {row['hotspot_ns']:8.3f} ns (JVM starts spread "
            f"{row['hotspot_start_spread'] * 100:4.1f}%)" + "".join(
                f"  {e} {d['rust_ns']:8.3f} ns ratio {d['ratio_median']:.3f} [{d['ratio_ci95'][0]:.3f}, "
                f"{d['ratio_ci95'][1]:.3f}] {d['status']}" for e, d in row["entries"].items()))
    if args.check_only:
        return
    report["allocations_in_bench_loops"] = allocations
    (BUILD / "report.json").write_text(json.dumps(report, indent=1))
    misses = [f"{k} {e}" for k, r in report["rows"].items() for e, d in r["entries"].items()
              if not d["status"].startswith("faster")]
    log(f"allocations in the timed Rust loops: {allocations}")
    log("verdict: " + ("PASS: every row faster than HotSpot by more than 2%" if not misses and not allocations
                       else "FAIL: rows that miss the bar: " + ", ".join(misses)))


if __name__ == "__main__":
    main()
