#!/usr/bin/env python3
"""Debt 2w3 mechanical migrations.

Run with no arguments to apply the write-type rename, or with --engine-callers
for the straightforward concrete Game calls; --fixture-scopes applies the
reviewed raw open/close fixture pairs, including their explicitly listed
outputs. --audit prints a tracked Rust inventory. To also
check cumulative working-tree changes against wave 3, pass --audit
--wave3-hunks PATH [--base REV]. The hunk manifest uses wave-3 coordinates,
not the debt checkout's coordinates.
"""

import argparse
import re
import subprocess
from difflib import SequenceMatcher
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEBT_BASE = "80197a053f2a4a24821db41d9da0d0c78ff109f9"

# Reviewed from `git grep -l -w Transaction -- '*.rs'`. hooks/notes.rs is
# prose; sunrise-telemetry-server uses SQLite's unrelated transaction type.
RENAME = {
    "crates/sunrise-mod-vocab/src": """
        block/bundle_items.rs block/fluid_capability.rs block/item_place.rs
        content/attached_storage.rs content/resource.rs content/resource_util.rs
        content/resource_util_tests.rs entity/entity_capability.rs
        exports/transaction.rs host/block_entities.rs host/fluids_energy.rs
        host/items.rs host/mod_state.rs item/capability.rs item/energy_capability.rs
        item/energy_capability_tests.rs item/item_component_handlers.rs
        item/storage.rs item/transaction.rs testing.rs testing/journal.rs
    """,
    "crates/sunrise-mod-api/src/parts": """
        capabilities.rs energy.rs entities.rs fluids.rs items.rs
    """,
    "crates/sunrise-client/src/game": """
        blocks/dispense_container_tests.rs blocks/machines/native_energy.rs
        blocks/machines/native_fluids.rs blocks/machines/native_items.rs
        blocks/machines/storage_attempt_tests.rs blocks/world_fluids.rs
        modding/native_block_entity_item_tests.rs modding/native_block_entity_refs.rs
        modding/native_capability_cache_tests.rs modding/native_item_places.rs
        runtime/transactions.rs runtime/transactions/scratch.rs
    """,
}
# Only the concrete production callers from step 3. Tests with concrete Game
# receivers move in step 4; dyn ServerHost remains on the public API.
ENGINE_CALLS = {
    "blocks/cauldron_interactions.rs": ("self", 1),
    "blocks/cauldrons.rs": ("self", 1),
    "blocks/machines/mod.rs": ("self", 3),
    "blocks/machines/native_items.rs": ("self", 1),
    "entities/general/tick.rs": ("self", 1),
    "items/consume.rs": ("self", 2),
    "mobs/creatures/mod.rs": ("self", 1),
    "modding/native_item_places.rs": ("self", 1),
    "modding/native_lifecycle.rs": ("game", 1),
    "modding/native_quick_move.rs": ("self", 1),
    "modding/slot_takes.rs": ("self", 1),
    "native_hooks/farmland_trample.rs": ("self", 1),
    "native_hooks/living_breathe.rs": ("self", 1),
    "network/lan_crafting.rs": ("self", 1),
}
GAME = ROOT / "crates/sunrise-client/src/game"

# Reviewed simple pairs, by file and count. The exceptions below have
# conditional opening, a deliberately unclosed frame or specialized loans.
FIXTURE_COUNTS = {
    "blocks/cauldron_interactions/tests.rs": 1,
    "blocks/machines/contents_watch_tests.rs": 1,
    "blocks/world_fluids_tests.rs": 12,
    "interface/tests/stat_award_tests.rs": 5,
    "modding/attachment_sync/general_snapshot_tests.rs": 1,
    "modding/attachment_sync/transport_tests.rs": 2,
    "modding/block_entity_part_state_tests.rs": 7,
    "modding/entity_part_state_tests.rs": 7,
    "modding/grindstone_decisions_tests.rs": 1,
    "modding/mod_menu_decisions_tests.rs": 3,
    "modding/native_block_entity_item_tests.rs": 3,
    "modding/native_book_apply/tests.rs": 6,
    "modding/native_general_renderers_tests.rs": 2,
    "modding/native_level_refs.rs": 5,
    "modding/native_living_absorption.rs": 2,
    "modding/native_living_refs.rs": 8,
    "modding/native_menus.rs": 2,
    "modding/native_portal_destination.rs": 1,
    "modding/published_shape_game_tests.rs": 1,
    "modding/stacked_on_other_tests.rs": 5,
    "native_hooks/death_source_tests.rs": 2,
    "network/lan_trade_decisions_tests.rs": 4,
    "runtime/transaction_entities_tests.rs": 34,
    "runtime/transactions/decision_engine_tests.rs": 6,
    "runtime/transactions/decision_game_tests.rs": 9,
}
FIXTURE_OUTPUTS = {
    "blocks/world_fluids_tests.rs": {151: "placed"},
    "modding/block_entity_part_state_tests.rs": {519: "tentative", 612: "tentative"},
    "modding/entity_part_state_tests.rs": {
        390: "pig", 587: "moved", 621: "(b, c)", 665: "state", 694: "(second, state)",
    },
    "modding/native_general_renderers_tests.rs": {552: "row"},
    "network/lan_trade_decisions_tests.rs": {544: "trade_id", 546: "trade_id"},
    "runtime/transaction_entities_tests.rs": {
        205: "pig", 267: "pig", 291: "sheep", 329: "(held, kept_out)", 457: "pig",
        458: "pig", 480: "kept", 482: "dropped", 512: "kept", 534: "pig",
        554: "(aborted, handle)", 653: "aborted", 656: "committed", 832: "fresh",
        860: "pig", 917: "(a, b)", 953: "kept", 980: "pig", 983: "pig", 1017: "a",
        1061: "(a, b)", 1100: "pig", 1200: "(a, b)",
    },
}
# Concrete test receivers; explicitly erased `dyn ServerHost` receivers stay
# on the public API as tests of that API.
CONCRETE_FIXTURE_CALLS = {
    "blocks/composter/capability_tests.rs": 18,
    "blocks/machines/can_place_item_tests.rs": 5,
    "blocks/machines/contents_changed_tests.rs": 8,
    "blocks/machines/contents_watch_tests.rs": 5,
    "blocks/machines/native_fluids.rs": 15,
    "blocks/machines/tank_reopen_tests.rs": 2,
    "modding/attachment_index/block_entity_tests.rs": 1,
    "modding/attachment_index/mob_tests.rs": 2,
    "modding/attachment_index/mod.rs": 1,
    "modding/attachment_index/painting_tests.rs": 2,
    "modding/attachment_index/query_tests.rs": 2,
    "modding/native_block_entity_refs.rs": 1,
    "modding/native_item_bundle_tests.rs": 8,
    "modding/native_item_places.rs": 20,
    "modding/native_player_inventory_tests.rs": 7,
    "modding/native_scoreboard.rs": 2,
    "modding/native_scoreboard_teams.rs": 1,
    "runtime/transactions.rs": 3,
}
CONCRETE_FIXTURE_CALL = re.compile(
    r"\b(?:(?:mod_api::)?run_transaction\(\s*game\s*,\s*|game\s*\.\s*transaction\s*\(\s*)"
)
FIXTURE_EXCEPTIONS = {
    "blocks/world_fluids_tests.rs": {441, 464, 467},
    "runtime/transaction_entities_tests.rs": {110, 125, 129},
}
FIXTURE_OPEN = re.compile(r"(?P<indent> *)let (?P<seal>\w+) = (?P<game>game|server|host\.game)\.transaction_open\(\);")
FIXTURE_CLOSE = re.compile(r"(?P<indent> *)(?P<game>game|server|host|host\.game)\.transaction_close\(&(?P<seal>\w+), (?P<commit>true|false|commit)\);")

OLD = re.compile(r"(?<![\w'])Transaction\b")
NEW = re.compile(r"(?<![\w'])TransactionContext\b")

# Tokens to blank out while retaining offsets. The only doc format needed by
# this migration is consecutive Rust line docs with Markdown code fences.
NON_CODE = re.compile(
    r'(?m)(?P<docs>^[ \t]*//(?:/(?!/)|!)[^\n]*(?:\n[ \t]*//(?:/(?!/)|!)[^\n]*)*)'
    r'|//[^\n]*|/\*|(?:br|cr|r)(?P<hashes>#{0,255})"[\s\S]*?"(?P=hashes)'
    r'|"(?:[^"\\]|\\[\s\S])*"'
    r"|'(?:[^'\\\n]|\\(?:u\{[\da-fA-F_]+\}|x[\da-fA-F]{2}|[^\n]))'"
)
BLOCK_EDGE = re.compile(r"/\*|\*/")
DOC_PREFIX = re.compile(r"[ \t]*//[/!][ \t]?")
FENCE = re.compile(r"[ \t]*(?P<mark>`{3,}|~{3,})(?P<info>[^\n]*)")
RUST_FENCE = re.compile(
    r"rust|compile_fail|ignore(?:-[\w-]+)?|no_run|should_panic|test_harness|edition20\d\d"
)
TYPE_LINK = re.compile(r"\[`(?P<name>Transaction)`\](?!\()")

RAW_NAMES = (
    "transaction_open", "transaction_close", "transaction_current", "transaction_depth",
    "transaction_lifecycle", "transaction_of", "transaction_seal", "transaction_token",
    "run_transaction", "seal_tokens",
)
CALL_SUFFIX = r"\b\s*(?=\(|::\s*<)"
RAW = re.compile(rf"\b(?P<decl>fn\s+)?(?P<name>{'|'.join(RAW_NAMES)}){CALL_SUFFIX}")
CALL = rf"\.\s*(?P<call>transaction){CALL_SUFFIX}"
RESULT = r"(?P<result>(?:\w+\s*::\s*)*Result)\s*<"
LET_RESULT = re.compile(
    rf"\blet\s+(?:mut\s+)?\w+\s*:\s*{RESULT}[^;{{}}=]+?>\s*=\s*"
    rf"(?:[\w:.]+|\([\w:.\s&]+?\))\s*{CALL}"
)
CLOSURE_RESULT = re.compile(rf"{CALL}\(\s*(?:move\s+)?\|[^|]*\|\s*->\s*{RESULT}")

# Preimage commits whose coordinates match the manifest for this item's files.
# In particular, game.rs:96 is the module include, not line 96 at DEBT_BASE.
WAVE3_PREIMAGE = {
    "crates/sunrise-client/src/game.rs": "fbf5e51d8eab7c563d587b5319df98bfb2a2ebbe",
    "crates/sunrise-client/src/game/blocks/machines/mod.rs": "f4acbabd44526f70d1f8e7dbfd7d6c5763a88a14",
    "crates/sunrise-client/src/game/mobs/creatures/mod.rs": "5f57767f9349a9cdfc4d7ad79771f4fc46fc7bdd",
}
DIFF_HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+\d+(?:,\d+)? @@", re.MULTILINE)


def blank(text):
    return re.sub(r"[^\n]", " ", text)


def doc_code(doc):
    lines = []
    links = []
    offset = 0
    fence = None
    rust = False
    for line in doc.splitlines(keepends=True):
        start = DOC_PREFIX.match(line).end()
        body = line[start:].rstrip("\n")
        mark = FENCE.fullmatch(body)
        masked = blank(line)
        if fence is None and mark:
            fence = mark["mark"]
            tags = mark["info"].replace(",", " ").split()
            rust = all(RUST_FENCE.fullmatch(tag) for tag in tags)
        elif fence and mark and mark["mark"].startswith(fence) and not mark["info"].strip():
            fence = None
        elif fence and rust:
            # Rustdoc's hidden lines are Rust too, but their '# ' is not.
            hidden = re.match(r"[ \t]*#[ \t]", body)
            if hidden:
                start += hidden.end()
            masked = blank(line[:start]) + line[start:]
        elif fence is None:
            for link in TYPE_LINK.finditer(body):
                first, last = link.span("name")
                links.append((offset + start + first, offset + start + last))
        lines.append(masked)
        offset += len(line)
    code = list(rust_code("".join(lines), docs=False))
    for first, last in links:
        code[first:last] = doc[first:last]
    return "".join(code)


def rust_code(source, *, docs=True):
    code = list(source)
    end = 0
    while match := NON_CODE.search(source, end):
        start, end = match.span()
        if match[0] == "/*":
            depth = 1
            for edge in BLOCK_EDGE.finditer(source, end):
                depth += 1 if edge[0] == "/*" else -1
                if depth == 0:
                    end = edge.end()
                    break
            else:
                end = len(source)
        if docs and match["docs"]:
            code[start:end] = doc_code(match["docs"])
        else:
            code[start:end] = blank(source[start:end])
    return "".join(code)


def source_files(*words):
    args = [word for needle in words for word in ("-e", needle)]
    found = subprocess.run(
        [
            "git", "grep", "-lzwF", *args, "--", "*.rs",
            ":(exclude)crates/sunrise-telemetry-server/**",
        ],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        check=False,
    )
    if found.returncode == 1:
        return []
    found.check_returncode()
    return found.stdout.decode().removesuffix("\0").split("\0")


def apply_rename():
    paths = {f"{root}/{name}" for root, names in RENAME.items() for name in names.split()}
    updates = {}
    for name in sorted(paths | set(source_files("Transaction"))):
        path = ROOT / name
        source = path.read_bytes().decode()
        code = rust_code(source)
        hits = list(OLD.finditer(code))
        if name not in paths:
            if hits:
                raise SystemExit(f"{name}: unreviewed Rust Transaction use; no files written")
            continue
        if not hits or NEW.search(code):
            raise SystemExit(f"{name}: expected old Rust type and no new type; no files written")
        for hit in reversed(hits):
            source = source[:hit.start()] + "TransactionContext" + source[hit.end():]
        updates[path] = source
    for path, source in updates.items():
        path.write_bytes(source.encode())
        print(path.relative_to(ROOT))


def apply_engine_calls():
    updates = {}
    for name, (receiver, expected) in ENGINE_CALLS.items():
        path = ROOT / "crates/sunrise-client/src/game" / name
        source = path.read_bytes().decode()
        old = f"mod_api::run_transaction({receiver}, "
        hits = list(re.finditer(re.escape(old), rust_code(source, docs=False)))
        if len(hits) != expected:
            raise SystemExit(f"{name}: expected {expected} concrete calls, found {len(hits)}; no files written")
        for hit in reversed(hits):
            source = source[:hit.start()] + f"{receiver}.storage_transaction(" + source[hit.end():]
        updates[path] = source
    for path, source in updates.items():
        path.write_bytes(source.encode())
        print(path.relative_to(ROOT))


def apply_fixture_scopes():
    updates = {}
    for name, count in FIXTURE_COUNTS.items():
        path = GAME / name
        source = path.read_text()
        lines = source.splitlines(keepends=True)
        code = rust_code(source, docs=False).splitlines()
        outputs = dict(FIXTURE_OUTPUTS.get(name, {}))
        exceptions = FIXTURE_EXCEPTIONS.get(name, set())
        opened = []
        pairs = []
        for loc, line in enumerate(code, 1):
            if loc in exceptions:
                continue
            if start := FIXTURE_OPEN.fullmatch(line):
                opened.append((loc, start))
            elif end := FIXTURE_CLOSE.fullmatch(line):
                first, start = opened.pop()
                if start['seal'] != end['seal'] or start['indent'] != end['indent']:
                    raise SystemExit(f"{name}:{loc}: mismatched pair from line {first}")
                expected = start['game']
                decision_root = end['game'] == 'host.game' and expected == 'game'
                if end['game'] != expected and not decision_root:
                    raise SystemExit(f"{name}:{loc}: expected close on {expected}")
                pairs.append((first, loc, start, end))
            elif re.search(r"\btransaction_(?:open|close)\(", line) and not (
                name == 'blocks/world_fluids_tests.rs' and loc == 453
                or name == 'runtime/transaction_entities_tests.rs' and loc == 99
            ):
                raise SystemExit(f"{name}:{loc}: unreviewed lifecycle call")
        if opened or len(pairs) != count:
            raise SystemExit(f"{name}: expected {count} complete pairs; found {len(pairs)}")
        # End order is inside-out. Each buffer entry still represents an
        # original source line, even when its replacement spans several lines.
        for first, last, start, end in pairs:
            value = outputs.pop(first, '()')
            game, seal, indent = (start[k] for k in ('game', 'seal', 'indent'))
            tx = '_'
            pattern = re.compile(rf"\b{seal}\.transaction\(\)")
            for index in range(first, last - 1):
                if pattern.search(lines[index]):
                    lines[index] = pattern.sub('tx', lines[index])
                    tx = 'tx'
            commit = end['commit']
            binding = f"let {value} = " if value != '()' else ''
            if commit == 'commit':
                tail = f"if commit {{ Ok({value}) }} else {{ Err({value}) }}"
                suffix = '.unwrap_or_else(|value| value)' if binding else ''
                binding = binding or 'let _ = '
            elif commit == 'true':
                tail, suffix = f"Ok::<_, ()>({value})", '.unwrap()'
            else:
                tail, suffix = f"Err::<(), _>({value})", '.unwrap_err()'
            receiver = game.split('.')[-1]
            if game != receiver:
                for index in range(first, last - 1):
                    lines[index] = lines[index].replace('host.game', receiver)
            if end['game'] != game:
                # GameHost was local to the root fixture and cannot outlive the closure.
                index = last
                while index < len(lines) and (
                    not lines[index].strip() or lines[index].startswith(indent)
                ):
                    index += 1
                after = re.sub(r'\bhost\s*\.game\b', 'game', ''.join(lines[last:index]))
                lines[last:index] = [after] + [''] * (index - last - 1)
            lines[first - 1] = f"{indent}{binding}{game}.storage_transaction(|{receiver}, {tx}| {{\n"
            lines[last - 1] = f"{indent}    {tail}\n{indent}}}){suffix};\n"
        if outputs:
            raise SystemExit(f"{name}: missing pairs at output sites {sorted(outputs)}")
        updates[path] = ''.join(lines)
    for name, count in CONCRETE_FIXTURE_CALLS.items():
        path = GAME / name
        source = updates[path] if path in updates else path.read_text()
        hits = list(CONCRETE_FIXTURE_CALL.finditer(rust_code(source, docs=False)))
        if len(hits) != count:
            raise SystemExit(f"{name}: expected {count} typed Game calls; found {len(hits)}")
        for hit in reversed(hits):
            source = source[:hit.start()] + 'game.storage_transaction(' + source[hit.end():]
        updates[path] = source
    for path, source in updates.items():
        path.write_text(source)
        print(path.relative_to(ROOT))


def audit():
    for name in source_files("Transaction", "transaction", *RAW_NAMES):
        source = (ROOT / name).read_bytes().decode()
        code = rust_code(source)
        sites = [(m.start(), "residual Rust Transaction") for m in OLD.finditer(code)]
        for match in RAW.finditer(code):
            label = "lifecycle declaration" if match["decl"] else "lifecycle call"
            sites.append((match.start("name"), label))
        # ChunkDisk has an unrelated transaction method for a disk batch.
        if not name.startswith("crates/sunrise-mod-vocab/src/chunk_disk/"):
            for pattern, label in [(LET_RESULT, "outer Result"), (CLOSURE_RESULT, "closure Result")]:
                for match in pattern.finditer(code):
                    call_line = source.count("\n", 0, match.start("call")) + 1
                    sites.append(
                        (match.start("result"), f"explicit {label} for .transaction at line {call_line}")
                    )
            for match in re.finditer(CALL, code):
                if re.match(r"\(\s*\)", code[match.end():]):
                    label = "zero-argument transaction accessor"
                elif code.startswith("::", match.end()):
                    label = ".transaction call with explicit type arguments"
                else:
                    label = ".transaction call"
                sites.append((match.start("call"), label))
        lines = source.split("\n")
        for offset, label in sorted(sites):
            index = source.count("\n", 0, offset)
            print(f"{name}:{index + 1}: {label}: {lines[index].strip()}")


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT).decode()


def audit_wave3(manifest, base, extra_preimage):
    """Report protected text and compare changes at its mapped preimage location."""
    ranges = dict(line.split(": ", 1) for line in manifest.read_text().splitlines() if line)
    changed = set(git("diff", "--no-renames", "--name-only", "-z", base, "--").split("\0"))
    changed.update(git("ls-files", "--others", "--exclude-standard", "-z").split("\0"))
    protected = sorted(
        name for name in changed & ranges.keys()
        if Path(name).name not in {"Cargo.toml", "Cargo.lock"}
    )
    print(f"Wave 3: {len(protected)} changed paths with protected hunks")
    ok = True
    for name in protected:
        if ranges[name] == "0-0":
            print(f"{name}: wave-3 new file overlaps changed path")
            ok = False
            continue
        ref = WAVE3_PREIMAGE.get(name, extra_preimage)
        if ref is None:
            print(f"{name}: supply the preimage revision with --wave3-base; not audited")
            ok = False
            continue
        before = git("show", f"{ref}:{name}").split("\n")
        baseline = git("show", f"{base}:{name}").split("\n")
        matched = SequenceMatcher(None, before, baseline, autojunk=False).get_matching_blocks()
        diff = git(
            "diff", "--no-ext-diff", "--no-textconv", "--no-renames",
            "--inter-hunk-context=0", "-U0", base, "--", name,
        )
        edits = [(int(first), int(count or 1)) for first, count in DIFF_HUNK.findall(diff)]
        for region in ranges[name].split(", "):
            first, last = map(int, region.split("-"))
            print(f"{name}:{region}: wave-3 preimage {ref}:")
            for line in before[first - 1:last]:
                print(f"    {line}")
            for i, j, size in matched:
                if i <= first - 1 and last <= i + size:
                    left, right = first + j - i, last + j - i
                    break
            else:
                print(f"{name}:{region}: protected content does not map intact to {base}")
                ok = False
                continue
            print(f"{name}:{left}-{right}: protected lines at {base}")
            for start, count in edits:
                # A zero-length old hunk inserts after start, which is itself
                # protected when wave 3 also inserts at that anchor.
                if start <= right and left <= start + max(0, count - 1):
                    print(f"{name}:{start}: changed hunk overlaps wave-3 content")
                    ok = False
    return ok


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--audit", action="store_true", help="report Rust sites without editing")
    mode.add_argument("--engine-callers", action="store_true", help="migrate the step-3 engine callers")
    mode.add_argument("--fixture-scopes", action="store_true", help="migrate simple step-4 fixture pairs")
    parser.add_argument("--wave3-hunks", type=Path, help="also check changed lines against this manifest")
    parser.add_argument("--base", default=DEBT_BASE, help="debt diff base (default: item start)")
    parser.add_argument("--wave3-base", help="preimage revision for any additional changed wave-3 file")
    args = parser.parse_args()
    if not args.audit:
        if args.wave3_hunks or args.wave3_base or args.base != DEBT_BASE:
            parser.error("wave-3 and diff options require --audit")
        if args.engine_callers:
            apply_engine_calls()
        elif args.fixture_scopes:
            apply_fixture_scopes()
        else:
            apply_rename()
        return
    if (args.wave3_base or args.base != DEBT_BASE) and not args.wave3_hunks:
        parser.error("diff and preimage revisions require --wave3-hunks")
    audit()
    if args.wave3_hunks and not audit_wave3(args.wave3_hunks, args.base, args.wave3_base):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
