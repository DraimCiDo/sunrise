use super::*;
use crate::manifest::Package;

const MANIFEST: &str = "id = \"gb\"\nname = \"gb\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n";
fn jar() -> String {
    format!(
        "[[jar]]\nid = \"original\"\nversion = \"1\"\nsha512 = \"{}\"\n",
        "1".repeat(128)
    )
}
const GOOD: &[(&str, &str)] = &[
    (
        "data/gb/sunrise/block/slab.json",
        r#"{"template":"minecraft:smooth_stone_slab"}"#,
    ),
    (
        "data/gb/sunrise/item/shard.json",
        r#"{"template":"minecraft:diamond"}"#,
    ),
    (
        "assets/gb/blockstates/slab.json",
        r#"{"variants":{"":{"model":"gb:block/slab"}}}"#,
    ),
    (
        "assets/gb/models/block/slab.json",
        r##"{"parent":"minecraft:block/slab","textures":{"side":"gb:block/stone","top":"#side"}}"##,
    ),
    (
        "assets/gb/models/item/shard.json",
        r#"{"parent":"minecraft:item/generated","textures":{"layer0":"gb:item/shard"}}"#,
    ),
    (
        "assets/gb/items/slab.json",
        r#"{"model":{"type":"minecraft:model","model":"gb:block/slab"}}"#,
    ),
    (
        "assets/gb/items/shard.json",
        r#"{"model":{"type":"minecraft:model","model":"gb:item/shard"}}"#,
    ),
    ("assets/gb/textures/block/stone.png", "png"),
    ("assets/gb/textures/item/shard.png", "png"),
    (
        "assets/gb/lang/en_us.json",
        r#"{"block.gb.slab":"Slab","item.gb.shard":"Shard"}"#,
    ),
    (
        "data/gb/loot_table/blocks/slab.json",
        r#"{"type":"minecraft:block","pools":[{"entries":[{"type":"minecraft:item","name":"gb:slab"}]}]}"#,
    ),
    (
        "data/gb/recipe/slab.json",
        r#"{"type":"minecraft:crafting_shaped","pattern":["SS"],"key":{"S":"gb:shard"},"result":{"id":"gb:slab","count":6}}"#,
    ),
    (
        "data/gb/tags/item/things.json",
        r#"{"values":["gb:shard","gb:slab","minecraft:stick"]}"#,
    ),
];

/// The good mod with `changes` applied: a file replaced, or removed when its text is empty.
fn findings(manifest: &str, changes: &[(&str, &str)]) -> Findings {
    let dir = tempfile::tempdir().unwrap();
    let mut files: Vec<(&str, &str)> = GOOD.to_vec();
    for (name, text) in changes {
        files.retain(|(n, _)| n != name);
        if !text.is_empty() {
            files.push((name, text));
        }
    }
    std::fs::write(dir.path().join(crate::manifest::MANIFEST), manifest).unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let package = Package::read(dir.path()).unwrap();
    let content = Content::read(dir.path(), &package.manifest).unwrap();
    check_references(&content, &package.manifest).unwrap()
}

fn errors(changes: &[(&str, &str)]) -> Vec<String> {
    findings(MANIFEST, changes).errors
}

/// No error and no warning.
fn clean(manifest: &str, changes: &[(&str, &str)]) {
    assert_eq!(findings(manifest, changes), Findings::default());
}

#[test]
fn a_consistent_mod_has_no_findings() {
    assert_eq!(findings(MANIFEST, &[]), Findings::default());
}

#[test]
fn a_missing_texture_names_the_model_and_the_file() {
    let e = errors(&[("assets/gb/textures/block/stone.png", "")]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with("assets/gb/models/block/slab.json: the texture gb:block/stone"));
    assert!(e[0].contains("expected assets/gb/textures/block/stone.png"));
    // Two slots naming it are one finding.
    let two = r#"{"textures":{"a":"gb:block/gone","b":"gb:block/gone"}}"#;
    assert_eq!(
        errors(&[("assets/gb/models/block/slab.json", two)]).len(),
        1
    );
}

#[test]
fn a_missing_model_is_named_with_a_near_miss() {
    let e = errors(&[("assets/gb/models/block/slab.json", "")]);
    // The blockstate and the item definition both name it.
    assert_eq!(e.len(), 2, "{e:?}");
    assert!(e[0].contains("gb:block/slab"));
    assert!(e[0].contains("variants[\"\"].model"));
    let e = errors(&[(
        "assets/gb/blockstates/slab.json",
        r#"{"variants":{"":{"model":"gb:block/slabb"}}}"#,
    )]);
    assert!(e[0].contains("did you mean gb:block/slab?"), "{e:?}");
}

#[test]
fn a_block_without_a_blockstate_has_no_model_unless_it_draws_none() {
    // Declaring MODEL says the block is drawn from one: nothing else could supply it.
    let model = r#"{"template":"minecraft:stone","properties":{"render_shape":"MODEL"}}"#;
    let e = errors(&[
        ("assets/gb/blockstates/slab.json", ""),
        ("data/gb/sunrise/block/slab.json", model),
    ]);
    assert!(e[0].contains("the block gb:slab has no model"), "{e:?}");
    // The template's own shape is not known: a warning.
    let f = findings(MANIFEST, &[("assets/gb/blockstates/slab.json", "")]);
    assert!(f.errors.is_empty(), "{f:?}");
    assert_eq!(f.warnings.len(), 1, "{f:?}");
    assert!(f.warnings[0].contains("could not find a blockstate for the block gb:slab"));
}

#[test]
fn a_block_drawn_by_nothing_or_by_its_block_entity_needs_no_blockstate() {
    for shape in ["INVISIBLE", "ENTITYBLOCK_ANIMATED"] {
        let block = format!(
            r#"{{"template":"minecraft:stone","properties":{{"render_shape":"{shape}"}}}}"#
        );
        clean(
            MANIFEST,
            &[
                ("assets/gb/blockstates/slab.json", ""),
                ("data/gb/sunrise/block/slab.json", &block),
            ],
        );
    }
}

#[test]
fn a_recipe_item_of_this_mod_must_be_declared() {
    let recipe = r#"{"type":"minecraft:crafting_shaped","pattern":["S"],"key":{"S":"gb:shrad"},"result":{"id":"minecraft:stick","count":1}}"#;
    let e = errors(&[("data/gb/recipe/slab.json", recipe)]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with("data/gb/recipe/slab.json:"));
    assert!(e[0].contains("key[\"S\"]"));
    assert!(e[0].contains("gb:shrad is not an item"));
    assert!(e[0].contains("Did you mean gb:shard?"));
}

#[test]
fn an_ingredient_checks_only_the_selected_children_field() {
    let file = "data/gb/recipe/slab.json";
    for kind in ["compound", "intersection"] {
        let recipe = |fields: &str| {
            format!(
                r#"{{"type":"minecraft:stonecutting","ingredient":{{"neoforge:ingredient_type":"neoforge:{kind}",{fields}}},"result":{{"id":"gb:slab"}}}}"#
            )
        };
        clean(
            MANIFEST,
            &[(
                file,
                &recipe(r#""children":["gb:shard"],"ingredients":["gb:missing"]"#),
            )],
        );
        for field in ["children", "ingredients"] {
            let e = errors(&[(file, &recipe(&format!(r#""{field}":["gb:missing"]"#)))]);
            assert_eq!(e.len(), 1, "{e:?}");
            assert!(e[0].starts_with(file));
            assert!(e[0].contains(&format!("ingredient.{field}[0]")));
            assert!(e[0].contains("gb:missing is not an item"));
        }
    }
}

#[test]
fn local_recipe_filters_make_unused_or_replaced_references_nonfatal() {
    let file = "data/gb/recipe/slab.json";
    let recipe =
        r#"{"type":"minecraft:stonecutting","ingredient":"gb:missing","result":{"id":"gb:slab"}}"#;
    let filter = "data/gb/recipe_filter/nested/slab.json";
    for action in [
        r#""remove":true"#,
        r#""replace_input":{"from":"gb:missing","to":"gb:shard"}"#,
    ] {
        let text = format!(r#"{{"filters":[{{"id":"gb:slab",{action}}}]}}"#);
        let f = findings(MANIFEST, &[(file, recipe), (filter, &text)]);
        assert!(f.errors.is_empty(), "{f:?}");
        assert_eq!(f.warnings.len(), 1, "{f:?}");
        assert!(f.warnings[0].starts_with(file));
        assert!(f.warnings[0].contains("recipe filter"));
    }
    let e = errors(&[(file, recipe)]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with(file));
    assert!(e[0].contains("gb:missing is not an item"));
}

#[test]
fn a_tag_value_of_this_mod_must_be_declared() {
    let e = errors(&[(
        "data/gb/tags/item/things.json",
        r##"{"values":["gb:nope","#gb:other"]}"##,
    )]);
    assert_eq!(e.len(), 2, "{e:?}");
    assert!(e[0].contains("values[0]"));
    assert!(e[0].contains("gb:nope is not an item"));
    assert!(e[1].contains("values[1]"));
    assert!(e[1].contains("#gb:other"));
    // A vanilla tag file lists this mod's blocks; a block tag checks blocks.
    let e = errors(&[(
        "data/minecraft/tags/block/slabs.json",
        r#"{"values":["gb:slab","gb:shard"]}"#,
    )]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].contains("gb:shard is not a block"));
}

#[test]
fn a_loot_entry_of_this_mod_must_be_declared() {
    let table = r#"{"pools":[{"entries":[{"type":"minecraft:item","name":"gb:slabs"}]}]}"#;
    let e = errors(&[("data/gb/loot_table/blocks/slab.json", table)]);
    assert!(
        e[0].contains("entry.name") && e[0].contains("gb:slabs is not an item"),
        "{e:?}"
    );
    // Through a group's children, and a state condition's block.
    let nested = r#"{"pools":[{"entries":[{"type":"minecraft:alternatives","children":[{"type":"minecraft:item","name":"gb:slab","conditions":[{"type":"minecraft:block_state_property","block":"gb:slabs"}]}]}]}]}"#;
    let e = errors(&[("data/gb/loot_table/blocks/slab.json", nested)]);
    assert!(e[0].contains("gb:slabs is not a block"), "{e:?}");
}

#[test]
fn a_broken_json_file_is_an_error() {
    let e = errors(&[("data/gb/recipe/slab.json", "{")]);
    assert!(
        e[0].starts_with("data/gb/recipe/slab.json: not valid JSON"),
        "{e:?}"
    );
}

#[test]
fn a_block_with_no_loot_table_or_name_is_a_warning() {
    let f = findings(
        MANIFEST,
        &[
            ("data/gb/loot_table/blocks/slab.json", ""),
            ("assets/gb/lang/en_us.json", r#"{"item.gb.shard":"Shard"}"#),
        ],
    );
    assert!(f.errors.is_empty(), "{f:?}");
    assert_eq!(f.warnings.len(), 2, "{f:?}");
    assert!(f.warnings[0].contains("no local loot table"));
    assert!(f.warnings[1].contains("has no \"block.gb.slab\""));
    let f = findings(MANIFEST, &[("assets/gb/lang/en_us.json", "")]);
    assert!(
        f.warnings
            .iter()
            .all(|w| w.contains("there is no assets/gb/lang/en_us.json"))
    );
    assert_eq!(f.warnings.len(), 2);
}

#[test]
fn registry_content_on_a_client_package_is_a_warning() {
    let client = MANIFEST.replace("side = \"both\"\n", "");
    let f = findings(&client, &[]);
    assert!(f.errors.is_empty());
    assert_eq!(f.warnings.len(), 1, "{f:?}");
    assert!(f.warnings[0].contains("set side = \"both\""));
}

// One test per finding of the review of #2893: a valid mod the first version failed.

/// 1. An optional entry may name nothing, and a removal names what it takes out.
#[test]
fn optional_and_removed_tag_entries_name_nothing() {
    let tag = r##"{"values":["gb:shard",{"id":"gb:later","required":false},{"id":"#gb:nothing","required":false},{"id":"gb:gone","remove":true}],"remove":["gb:gone","#gb:gone"]}"##;
    clean(MANIFEST, &[("data/gb/tags/item/things.json", tag)]);
    // A required entry still must exist.
    let required = r#"{"values":[{"id":"gb:later","required":true}]}"#;
    let e = errors(&[("data/gb/tags/item/things.json", required)]);
    assert!(e[0].contains("gb:later is not an item"), "{e:?}");
}

/// 2. A tag's registry can be two folders, as `worldgen/biome`.
#[test]
fn a_worldgen_tag_is_found_under_its_registry_path() {
    let cold = r##"{"values":["#gb:warm","minecraft:plains"]}"##;
    clean(
        MANIFEST,
        &[
            ("data/gb/tags/worldgen/biome/cold.json", cold),
            ("data/gb/tags/worldgen/biome/warm.json", r#"{"values":[]}"#),
        ],
    );
    let e = errors(&[("data/gb/tags/worldgen/biome/cold.json", cold)]);
    assert!(
        e[0].contains("expected data/gb/tags/worldgen/biome/warm.json"),
        "{e:?}"
    );
}

#[test]
fn nested_tags_outside_the_checked_folders_produce_no_findings() {
    // A code mod can declare the registry `magic:spells/elemental`.
    let containing = "data/gb/tags/magic/spells/elemental/strong/fire.json";
    let member = "data/gb/tags/magic/spells/elemental/common/fire.json";
    let value = r##"{"values":[{"id":"#gb:common/fire"}]}"##;
    clean(
        MANIFEST,
        &[(containing, value), (member, r#"{"values":[]}"#)],
    );

    let known = "data/gb/tags/worldgen/structure/strong/fire.json";
    let e = errors(&[(known, value)]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with(known));
    assert!(e[0].contains("data/gb/tags/worldgen/structure/common/fire.json"));
}

/// 3. A `[[jar]]` layer may supply what the files lack: a warning that says so.
#[test]
fn a_jar_layer_may_supply_what_the_files_lack() {
    let manifest = format!("{MANIFEST}{}", jar());
    let tag = r##"{"values":["#gb:from_jar","gb:from_jar"]}"##;
    let f = findings(
        &manifest,
        &[
            ("data/gb/tags/item/things.json", tag),
            ("assets/gb/textures/block/stone.png", ""),
            ("assets/gb/blockstates/slab.json", ""),
        ],
    );
    assert!(f.errors.is_empty(), "{f:?}");
    assert_eq!(f.warnings.len(), 4, "{f:?}");
    assert!(f.warnings.iter().all(|w| w.contains("[[jar]] layer")));
    assert!(
        f.warnings
            .iter()
            .any(|w| w.contains("could not find the tag #gb:from_jar"))
    );
    // A declared dependency's jar can do the same; the tool does not fetch it.
    let dependent = format!("{MANIFEST}[dependencies]\nchem_lib = \"1\"\n");
    let f = findings(&dependent, &[("data/gb/tags/item/things.json", tag)]);
    assert!(f.errors.is_empty(), "{f:?}");
    assert_eq!(f.warnings.len(), 2, "{f:?}");
    // The same mod without a jar breaks.
    let e = errors(&[("data/gb/tags/item/things.json", tag)]);
    assert_eq!(e.len(), 2, "{e:?}");
}

/// 4. A world fluid's flowing form is a fluid, its bucket an item, its block a block.
#[test]
fn a_world_fluid_brings_its_generated_ids() {
    clean(
        MANIFEST,
        &[
            ("data/gb/sunrise/fluid/oil.json", r#"{"world":{}}"#),
            (
                "assets/gb/lang/en_us.json",
                r#"{"block.gb.slab":"Slab","item.gb.shard":"Shard"}"#,
            ),
            (
                "data/minecraft/tags/fluid/water.json",
                r#"{"values":["gb:oil","gb:flowing_oil"]}"#,
            ),
            (
                "data/minecraft/tags/block/mineable/shovel.json",
                r#"{"values":["gb:oil"]}"#,
            ),
            (
                "data/gb/recipe/bucket.json",
                r#"{"type":"minecraft:stonecutting","ingredient":"gb:oil_bucket","result":{"id":"minecraft:bucket","count":1}}"#,
            ),
        ],
    );
    let e = errors(&[
        ("data/gb/sunrise/fluid/oil.json", r#"{"world":{}}"#),
        (
            "data/minecraft/tags/fluid/water.json",
            r#"{"values":["gb:flowing_gas"]}"#,
        ),
    ]);
    assert!(e[0].contains("gb:flowing_gas is not a fluid"), "{e:?}");
}
/// 5. An atlas source may rename a sprite or take a directory; a child model sets its own sprite.
#[test]
fn atlas_sources_and_model_overrides_may_supply_a_texture() {
    let atlas =
        r#"{"sources":[{"type":"minecraft:directory","source":"stone","prefix":"block/"}]}"#;
    let f = findings(
        MANIFEST,
        &[
            ("assets/gb/atlases/blocks.json", atlas),
            ("assets/gb/textures/block/stone.png", ""),
        ],
    );
    assert!(f.errors.is_empty(), "{f:?}");
    assert_eq!(f.warnings.len(), 1, "{f:?}");
    assert!(f.warnings[0].contains("could not find the texture gb:block/stone"));
    assert!(f.warnings[0].contains("atlas source"));
    clean(
        MANIFEST,
        &[
            (
                "assets/gb/models/block/base.json",
                r#"{"textures":{"all":"gb:block/placeholder"}}"#,
            ),
            (
                "assets/gb/models/block/slab.json",
                r#"{"parent":"gb:block/base","textures":{"all":"gb:block/stone"}}"#,
            ),
        ],
    );
    // A slab model that leaves the slot alone still needs the parent's sprite.
    let e = errors(&[
        (
            "assets/gb/models/block/base.json",
            r#"{"textures":{"all":"gb:block/placeholder"}}"#,
        ),
        (
            "assets/gb/models/block/slab.json",
            r#"{"parent":"gb:block/base"}"#,
        ),
    ]);
    assert!(e[0].contains("the texture gb:block/placeholder"), "{e:?}");
}

#[test]
fn an_inline_composite_child_can_override_an_inherited_sprite() {
    let base = "assets/gb/models/block/base.json";
    let slab = "assets/gb/models/block/slab.json";
    let placeholder =
        r##"{"parent":"minecraft:block/cube_all","textures":{"all":"gb:block/placeholder"}}"##;
    let composite = |slots| {
        format!(
            r#"{{"loader":"neoforge:composite","children":{{"body":{{"parent":"gb:block/base"{slots}}}}}}}"#
        )
    };
    let f = findings(
        MANIFEST,
        &[
            (base, placeholder),
            (slab, &composite(r#", "textures":{"all":"gb:block/stone"}"#)),
        ],
    );
    assert!(f.errors.is_empty(), "{f:?}");
    assert_eq!(f.warnings.len(), 1, "{f:?}");
    assert!(f.warnings[0].starts_with(base));
    assert!(f.warnings[0].contains("inline composite child"));
    assert!(f.warnings[0].contains("textures[\"all\"]"));
    let e = errors(&[(base, placeholder), (slab, &composite(""))]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with(base));
    assert!(e[0].contains("gb:block/placeholder"));
}

/// 7. Payloads only a mod or the stack reads name no ids.
#[test]
fn opaque_payloads_are_not_scanned_for_ids() {
    clean(
        MANIFEST,
        &[
            (
                "data/gb/recipe/machine.json",
                r#"{"type":"gb:machine","extra":{"input":"gb:nope","items":["gb:nope"]},"result":{"id":"gb:slab"}}"#,
            ),
            (
                "data/gb/recipe/slab.json",
                r#"{"type":"minecraft:crafting_shapeless","ingredients":["gb:shard",{"type":"neoforge:components","items":"gb:shard","components":{"minecraft:custom_data":{"id":"gb:nope"}}}],"result":{"id":"gb:slab","components":{"minecraft:custom_data":{"item":"gb:nope","id":"gb:nope"}}}}"#,
            ),
            (
                "data/gb/loot_table/blocks/slab.json",
                r#"{"pools":[{"entries":[{"type":"minecraft:item","name":"gb:slab","functions":[{"function":"minecraft:set_custom_data","tag":{"type":"minecraft:item","name":"gb:nope","block":"gb:nope"}}]}]}]}"#,
            ),
            (
                "assets/gb/blockstates/slab.json",
                r#"{"neoforge:definition_type":"gb:custom","model":"gb:block/gone","config":{"model":"gb:block/gone"}}"#,
            ),
            (
                "assets/gb/items/slab.json",
                r#"{"model":{"type":"gb:custom","model":"gb:item/gone"}}"#,
            ),
        ],
    );
}

/// 8. A file the load conditions may disable is not read.
#[test]
fn files_the_load_conditions_may_disable_are_not_read() {
    let when = r#""neoforge:conditions":[{"type":"neoforge:mod_loaded","modid":"other"}]"#;
    clean(
        MANIFEST,
        &[
            (
                "data/gb/recipe/other.json",
                &format!(
                    r#"{{{when},"type":"minecraft:smelting","ingredient":"gb:nope","result":{{"id":"minecraft:stick"}}}}"#
                ),
            ),
            (
                "data/gb/tags/item/other.json",
                &format!(r##"{{{when},"values":["gb:nope","#gb:nope"]}}"##),
            ),
            (
                "data/gb/loot_table/blocks/slab.json",
                &format!(
                    r#"{{"pools":[{{{when},"entries":[{{"type":"minecraft:item","name":"gb:nope"}}]}},{{"entries":[{{"type":"minecraft:item","name":"gb:slab"}}]}}]}}"#
                ),
            ),
        ],
    );
}

#[test]
fn the_model_decoder_leaves_trailing_input_unread() {
    let file = "assets/gb/models/block/slab.json";
    let trailing = r#"{"textures":{"all":"gb:block/stone"}} this is not part of the model"#;
    clean(MANIFEST, &[(file, trailing)]);
    let e = errors(&[(file, r#"{"textures":{"all":}}"#)]);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].starts_with(file));
    assert!(e[0].contains("could not read this model"));
}

/// 9. The generic JSON reader accepts comments, and the model decoder accepts a BOM.
#[test]
fn a_byte_order_mark_and_comments_are_read_as_the_game_reads_them() {
    let recipe = "\u{feff}{\n// a note\n\"type\":\"minecraft:crafting_shaped\", /* the key */\n\"pattern\":[\"SS\"],\"key\":{\"S\":\"gb:shard\"},\"result\":{\"id\":\"gb:slab\"}}";
    clean(MANIFEST, &[("data/gb/recipe/slab.json", recipe)]);
    let model = "\u{feff}{\"parent\":\"minecraft:block/cube_all\",\"textures\":{\"all\":\"gb:block/stone\"}}\n// trailing";
    clean(MANIFEST, &[("assets/gb/models/block/slab.json", model)]);
    // A comment hides nothing: the id after it is still checked.
    let e = errors(&[(
        "data/gb/recipe/slab.json",
        "{\"type\":\"minecraft:smelting\",\"ingredient\":\"gb:nope\", // note\n\"result\":{\"id\":\"gb:slab\"}}",
    )]);
    assert!(e[0].contains("gb:nope is not an item"), "{e:?}");
}
