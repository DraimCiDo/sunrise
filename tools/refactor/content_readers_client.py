#!/usr/bin/env python3
"""One-off client content-reader split; run from the #3871 round-2 base.

Cut items at their named source boundaries. The generated commit deliberately
leaves module declarations, imports, visibility and classification for the
follow-up commit; item bodies travel unchanged.
"""

import re
from pathlib import Path
from textwrap import dedent

ROOT = Path(__file__).resolve().parents[2] / "crates"
CLIENT = ROOT / "sunrise-client/src"
RENDER = ROOT / "sunrise-client-render/src"

sources = {}
outputs = {}


def cut(path, signature, closing="}"):
    """Take a column-anchored item plus any directly preceding attributes/docs."""
    if path not in sources:
        sources[path] = path.read_text()
    source = sources[path]
    starts = list(re.finditer(r"(?m)^" + re.escape(signature), source))
    if len(starts) != 1:
        raise ValueError(f"{path}: expected one {signature!r}, found {len(starts)}")
    start = starts[0].start()
    while start:
        prev = source.rfind("\n", 0, start - 1) + 1
        if not source[prev:start].lstrip().startswith(("///", "#[", "// ")):
            break
        start = prev
    if closing == "line":
        end = source.index("\n", starts[0].start())
    else:
        end = source.index("\n" + closing + "\n", starts[0].start()) + len(closing) + 1
    sources[path] = source[:start] + source[end + 1 :]
    return source[start:end]


def write(path, *items):
    if path.exists() or path in outputs:
        raise ValueError(f"{path} already exists")
    outputs[path] = "\n\n".join(items) + "\n"


def impl(header, *methods):
    return f"impl {header} {{\n" + "\n\n".join(methods) + "\n}"


def tests(source, destination):
    module = cut(source, "mod tests {")
    write(destination, dedent(module.split("mod tests {\n", 1)[1].removesuffix("\n}")))


def move_client_content():
    source = CLIENT / "mod_content.rs"
    write(
        CLIENT / "mod_content/declarations.rs",
        cut(source, "const ADD_TO:", "line"),
        cut(source, "const PRIORITY:", "line"),
        cut(source, "const ADDABLE:", "];"),
        impl("Addition", cut(source, "    fn read(bundle:", "    }")),
    )
    write(
        CLIENT / "mod_content/biome_overlay.rs",
        cut(source, "pub fn biome_modifiers_over_packs("),
    )
    keeps = cut(source, "    fn keeps(", "    }")
    source = CLIENT / "mod_content/load_conditions.rs"
    write(
        CLIENT / "mod_content/tag_declarations.rs",
        cut(source, "pub(super) fn tag_list"),
        cut(source, "pub(super) fn merge_tag"),
        impl("Known", keeps),
    )
    write(
        CLIENT / "mod_content/tag_layers.rs",
        cut(source, "pub(super) struct TagLayers"),
        cut(source, "pub(super) struct TagMerge"),
        cut(source, "impl TagMerge"),
    )
    write(
        CLIENT / "mod_content/resource_root.rs",
        cut(source, "fn lock_roots"),
        cut(source, "pub fn resource_root_with_conditions"),
    )

    source = CLIENT / "mod_content_biomes.rs"
    write(
        CLIENT / "mod_content/biome_declarations.rs",
        cut(source, "const FOLDERS:", "line"),
        cut(source, "pub(super) fn is_modifier"),
        cut(source, "const ATTRIBUTES:", "];"),
        *(cut(source, f"fn {name}") for name in ("id(", "count(", "unit(", "attribute(")),
        cut(source, "const COLOURS:", "];"),
        cut(source, "const REMOVABLE:", "line"),
        cut(source, "fn colour("),
        cut(source, "impl Changes"),
        cut(source, "fn step("),
        cut(source, "pub(super) fn spawner"),
        cut(source, "impl Modifier"),
        cut(source, "struct Range"),
        cut(source, "fn present<"),
        cut(source, "struct Climate"),
        impl("Selector", cut(source, "    fn read(mut set:", "    }")),
    )
    write(
        CLIENT / "mod_content/biome_tag_declarations.rs",
        impl(
            "Registry<'_>",
            cut(source, "    fn removal_reaches(", "    }"),
            cut(source, "    fn expand(", "    }"),
        ),
    )
    source = CLIENT / "mod_content_structures.rs"
    write(
        CLIENT / "mod_content/structure_declarations.rs",
        cut(source, "pub(super) fn is_modifier"),
        cut(source, "impl Modifier"),
    )


def move_ingredients():
    ingredient = CLIENT / "crafting/ingredient.rs"
    components = CLIENT / "crafting/ingredient/components.rs"
    write(
        CLIENT / "crafting/ingredient/declarations.rs",
        cut(ingredient, "fn ingredient_kind("),
        cut(ingredient, "fn is_custom("),
        impl(
            "Ingredient",
            cut(ingredient, "    pub fn resolve(v:", "    }"),
            cut(ingredient, "    pub fn custom_kinds(", "    }"),
            cut(ingredient, "    fn collect_custom_kinds(", "    }"),
        ),
        *(cut(components, name) for name in (
            "pub(super) fn json_patch(", "pub(super) fn validate_recipe_components(",
            "pub(super) fn validate_recipe(", "pub(super) fn persistent_recipe(",
            "fn persistent_ingredient(", "fn persistent_display(",
        )),
    )
    write(CLIENT / "lan/recipes/declarations.rs", cut(CLIENT / "lan/recipes.rs", "fn flat("))


def move_data_maps():
    base = CLIENT / "game/modding"
    source = base / "native_data_maps.rs"
    write(
        base / "native_data_maps/declarations.rs",
        cut(source, "fn folder("),
        cut(source, "fn entry_value<"),
        cut(source, "fn apply_file("),
        impl("DataMaps", cut(source, "    pub(crate) fn load(", "    }")),
    )
    tests(source, base / "native_data_maps/native_data_maps_tests.rs")
    source = base / "neoforge_block_maps.rs"
    write(base / "neoforge_block_maps/declarations.rs", cut(source, "const MAPS:", "];"))
    tests(source, base / "neoforge_block_maps/neoforge_block_maps_tests.rs")

    source = base / "data_map_files.rs"
    write(
        base / "data_map_files/merge.rs",
        impl("Key", cut(source, "    fn visit(", "    }")),
        cut(source, "impl<T: Clone> File<T>"),
        cut(source, "fn merge_versions<"),
    )

    source = CLIENT / "neoforge_mob_maps.rs"
    write(
        CLIENT / "neoforge_mob_maps/declarations.rs",
        cut(source, "impl Kind"),
        cut(source, "pub const MAPS:", "];"),
        *(cut(source, name) for name in (
            "fn field<", "fn resource(", "fn number(", "pub fn decode_distance(",
            "pub fn decode_sound(", "pub fn decode_villager_type(", "pub fn decode_gift(",
            "pub fn decode_frequency(", "pub fn decode_compostable(",
        )),
    )


def move_pack_readers():
    write(
        RENDER / "packs/declarations.rs",
        cut(RENDER / "packs.rs", "pub const BLOCKSTATE_LAYERS:", "line"),
        cut(RENDER / "models/catalog.rs", "fn definition_layers("),
    )
    write(RENDER / "datapack/declarations.rs", cut(RENDER / "datapack.rs", "fn content_id("))


def move_condition_runtime():
    base = CLIENT / "game/modding"
    source = base / "load_conditions.rs"
    write(
        base / "load_conditions/reload.rs",
        cut(
            source,
            "pub(super) type ConditionTable",
            "    Arc<BTreeMap<mod_api::Identifier, (String, Arc<dyn mod_api::LoadCondition>)>>;",
        ),
        cut(source, "impl Game<'_>"),
        cut(source, "pub(super) fn conditions_for_mods("),
        cut(source, "pub(super) fn content_root_with_conditions("),
    )


def main():
    move_client_content()
    move_ingredients()
    move_data_maps()
    move_pack_readers()
    move_condition_runtime()
    for path, content in (sources | outputs).items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content.rstrip() + "\n")


if __name__ == "__main__":
    main()
