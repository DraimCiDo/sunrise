#!/usr/bin/env python3
"""One-shot item moves for Sunrise #3871, from ac50b6f4f7.

Items on that formatted tree end on their own line at the declaration's
indent. Keep the item and its doc comments/attributes byte-for-byte; imports,
module wiring, visibility and markers are added after this move.
"""

from pathlib import Path
import re


SRC = Path(__file__).resolve().parents[2] / "crates/sunrise-world/src"


def take(source, header, end=None, *, within=None):
    """Remove one line-anchored item, with optional bounds and terminator."""
    scope = source
    offset = 0
    if within:
        bounds = re.search(rf"^{re.escape(within)}\n.*?^}}$", source, re.M | re.S)
        if not bounds:
            raise ValueError(f"missing block {within!r}")
        scope, offset = bounds.group(), bounds.start()
    indent = " " * (len(header) - len(header.lstrip()))
    ending = r";" if end == ";" else rf"\n.*?^{re.escape(end or indent + '}')}"
    pattern = rf"^(?:{indent}(?:///|#\[)[^\n]*\n)*{re.escape(header)}[^\n]*{ending}$\n?"
    matches = list(re.finditer(pattern, scope, re.M | re.S))
    if len(matches) != 1:
        raise ValueError(f"expected one {header!r} in {within!r}, got {len(matches)}")
    match = matches[0]
    return source[:offset + match.start()] + source[offset + match.end():], match.group().rstrip()


def split(source, destination, items):
    original = SRC / source
    remaining = original.read_text()
    dest = SRC / destination
    if dest.exists():
        raise ValueError(f"{destination} already exists")
    moved = []
    for item in items:
        if isinstance(item, str):
            remaining, body = take(remaining, item)
        elif isinstance(item, list):
            remaining, body = take(remaining, *item)
        else:
            impl, methods = item
            members = []
            for method in methods:
                remaining, body = take(remaining, "    " + method, within=impl)
                members.append(body)
            body = impl + "\n" + "\n\n".join(members) + "\n}"
        moved.append(body)
    remaining = re.sub(r"^impl [^\n]* \{\s*?^}\n\n?", "", remaining, flags=re.M)
    remaining = re.sub(r"\n{3,}", "\n\n", remaining)
    original.write_text(remaining)
    dest.parent.mkdir(parents=True, exist_ok=True)
    dest.write_text("\n\n".join(moved) + "\n")


def main():
    split("conditions.rs", "conditions/declarations.rs", [
        ("impl LoadConditions {", [
            "fn filter_list(", "pub(crate) fn admit_loot_table(", "fn verdict(",
        ]),
        "pub fn is_builtin_condition(",
        "enum BuiltinCondition {",
        "impl BuiltinCondition {",
        "struct DecodeErrors<'a> {",
        ("impl<'a> Condition<'a> {", ["fn decode(", "fn decode_builtin("]),
        "fn evaluate(",
    ])
    split("loot/global_modifiers.rs", "loot/global_modifiers/declarations.rs", [
        ("impl GlobalLootModifiers {", ["pub fn load("]),
    ])
    split("loot_types.rs", "loot_types/declarations.rs", ["fn builtin("])
    split("loot/query.rs", "loot/query/declarations.rs", ["pub fn condition("])
    split("level/block_maps.rs", "level/block_maps/declarations.rs", [
        ("impl BlockMaps {", ["pub fn from_values<'a>("]),
    ])
    structure = "worldgen/structure/"
    for module, items in [
        ("start", [
            ("impl RingSections {", ["fn decode("]),
            ("impl AvoidBiomes {", ["fn decode("]),
            "fn heightmap(", "fn terrain(", "fn pair(", "fn jigsaw_depth_and_reach(",
            "fn jigsaw_reach_in_range(",
            ("impl StructureRegistry {", ["pub fn new("]),
        ]),
        ("start_checks", [("impl StartChecks {", ["pub fn decode("])]),
        ("piece_limited", [("impl Config {", ["pub fn decode("])]),
        ("formations", ["impl Placement {", "impl Def {"]),
        ("enhanced_terrain", [
            ("impl Adaptation {", ["pub fn decode("]),
            ("impl StructureAdaptation {", ["pub fn decode("]),
        ]),
        ("jigsaw", [
            "fn projection(",
            ("impl PoolElement {", ["fn decode("]),
            "impl Rules {",
            ("impl Limit {", ["fn of("]),
            ("impl Pools {", ["pub fn get("]),
        ]),
        ("processor", [
            ["const SPAWNER_FIELDS:", "];"],
            ("impl PosRule {", ["fn decode("]),
            ("impl Modifier {", ["fn decode("]),
            "fn below(",
            ("impl Pick {", ["fn decode("]),
            ("impl CountCap {", ["fn decode("]),
            ("impl SpawnerMobs {", ["fn decode("]),
            "fn holder_set(", "fn float(", "fn float_range(",
            ("impl Processor {", ["fn decode("]),
            "impl ProcessorList {",
        ]),
        ("processor/pillars", [
            ["const DIRECTION_NAMES:", ";"],
            "fn direction(", "fn nested(", "fn weighted_blocks(",
            ("impl HeightRandomizer {", ["fn decode("]),
            ("impl YungRandomizer {", ["fn decode("]),
            *[(f"impl {name} {{", ["pub(super) fn decode("]) for name in [
                "Pillar", "BottomPillar", "CloseOffFluidSources", "CloseOffAirSources",
                "YungPillar", "StairPillar",
            ]],
        ]),
        ("processor/sea_level", [("impl KeepWorldBlock {", ["pub(super) fn decode("])]),
    ]:
        split(f"{structure}{module}.rs", f"{structure}{module}/declarations.rs", items)


if __name__ == "__main__":
    main()
