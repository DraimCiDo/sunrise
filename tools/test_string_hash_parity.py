#!/usr/bin/env python3
"""`java_string_hash` against JDK 25's String.hashCode, bit for bit.

Every string of one and of two UTF-16 units (all 2^32 pairs, surrogates included), then seeded random
strings of 0 to 64 units mixing ASCII, Latin-1, BMP, surrogate pairs and lone surrogates (`--billions`).
The oracle (VanillaStringHashOracle, JDK only: no game jar) and the Rust probe (sunrise-core's
`sunrise-string-hash-parity`) generate the same strings from the same seed and print a streaming digest
per chunk; a chunk whose digests differ is dumped on both sides and its first difference written to
`.local/rust-string-hash-parity/first_difference.txt`. The probe also hashes every valid UTF-16 string
through `java_string_hash_str`, which must agree with `java_string_hash`. Prints the harness name and
counts only.
"""
import argparse
import os
import subprocess
import time
from pathlib import Path

try:
    from . import lab
except ImportError:
    import lab

NAME = "string_hash"
ORACLE = lab.ROOT / "mods/sunrise/src/test/java/dev/sunriselab/parity/VanillaStringHashOracle.java"
SEED = 0x5354_5249_4E47_4853  # "STRINGHS"
PER_CHUNK = 1_000_000


def lines(command):
    out = subprocess.run(list(map(str, command)), check=True, capture_output=True, text=True).stdout
    return out.splitlines()


def parse(output, tag):
    """{chunk: (count, digest)} and the &str check's (checked, differed), if printed."""
    chunks, checked = {}, None
    for line in output:
        parts = line.split()
        if parts[0] == tag:
            chunks[int(parts[1])] = (int(parts[2]), parts[3])
        elif parts[0] == "S":
            checked = (int(parts[1]), int(parts[2]))
    return chunks, checked


def first_difference(build, java, probe, dump_args):
    java_file, rust_file = build / "dump-java.txt", build / "dump-rust.txt"
    lines([*java, "dump", *dump_args, java_file])
    lines([probe, "dump", *dump_args, rust_file])
    with java_file.open() as j, rust_file.open() as r:
        for a, b in zip(j, r):
            if a != b:
                text = f"chunk {' '.join(map(str, dump_args))}\njava {a}rust {b}"
                (build / "first_difference.txt").write_text(text)
                return
    (build / "first_difference.txt").write_text(f"chunk {' '.join(map(str, dump_args))}: dumps agree\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--billions", type=float, default=0.01,
                        help="random strings, in billions (default 0.01; the full run uses 4 or more)")
    parser.add_argument("--threads", type=int, default=os.cpu_count() or 1)
    parser.add_argument("--no-exhaustive", action="store_true", help="skip the 2^32 one- and two-unit strings")
    args = parser.parse_args()
    build = lab.LOCAL / "rust-string-hash-parity"
    build.mkdir(parents=True, exist_ok=True)
    (build / "first_difference.txt").unlink(missing_ok=True)
    classes = build / "classes"
    classes.mkdir(exist_ok=True)
    subprocess.run(["javac", "--release", "25", "-d", classes, ORACLE], check=True)
    subprocess.run(["cargo", "build", "--release", "--quiet", "-p", "sunrise-core", "--bin",
                    "sunrise-string-hash-parity"], check=True, cwd=lab.ROOT)
    probe = lab.binary(lab.cargo_target() / "release", "sunrise-string-hash-parity")
    java = ["java", "-Xmx1G", "-cp", classes, "dev.sunriselab.parity.VanillaStringHashOracle"]
    threads = args.threads

    runs = []
    if not args.no_exhaustive:
        runs.append(("exhaustive", "E", ["exhaustive", threads], lambda c: ["exhaustive", c]))
    chunks = max(1, round(args.billions * 1e9 / PER_CHUNK))
    runs.append(("random", "R", ["random", SEED, 0, chunks, PER_CHUNK, threads],
                 lambda c: ["random", SEED, c, PER_CHUNK]))
    start = time.monotonic()
    failed = False
    totals = {"inputs": 0, "chunks matched": 0, "chunks differed": 0, "str checked": 0, "str differed": 0}
    for _, tag, command, dump in runs:
        j, _ = parse(lines([*java, *command]), tag)
        r, checked = parse(lines([probe, *command]), tag)
        differed = sorted(c for c in j.keys() | r.keys() if j.get(c) != r.get(c))
        totals["inputs"] += sum(count for count, _ in r.values())
        totals["chunks matched"] += len(j.keys() | r.keys()) - len(differed)
        totals["chunks differed"] += len(differed)
        totals["str checked"] += checked[0]
        totals["str differed"] += checked[1]
        if differed and not failed:
            first_difference(build, java, probe, dump(differed[0]))
        failed |= bool(differed) or checked[1] > 0
    seconds = time.monotonic() - start
    print(f"{NAME}: {'FAIL' if failed else 'PASS'}: " + ", ".join(f"{k} {v:,}" for k, v in totals.items())
          + f" ({seconds:.0f} s)")
    raise SystemExit(1 if failed else 0)


if __name__ == "__main__":
    main()
