use super::*;
use crate::manifest::Package;

#[test]
fn plain_content_files_over_the_size_limit_are_refused() {
    let dir = scratch("plain-size-limit");
    let name = "assets/ruby/textures/block/huge.png";
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::create(&path)
        .unwrap()
        .set_len(MAX_FILE + 1)
        .unwrap();
    let mut reader = FileReader::Dir(&dir);
    let error = reader.path(name).unwrap_err();
    assert!(error.contains("is over"), "{error}");
    let error = reader.read(name).unwrap_err();
    assert!(error.contains("is over"), "{error}");
    std::fs::remove_dir_all(dir).unwrap();
}

const MANIFEST_TEXT: &str = r#"
id = "ruby"
name = "Ruby"
version = "0.1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"#;
const BLOCK: &[u8] = br#"{"template": "minecraft:iron_block",
        "properties": {"hardness": 4.0, "light": 3, "sounds": "minecraft:amethyst_block"}}"#;
const ITEM: &[u8] = br#"{"template": "minecraft:diamond", "rarity": "uncommon"}"#;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sunrise-content-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn shape_tables_validate_tuples_defaults_and_collision() {
    let content = smod("shape-tuples", MANIFEST_TEXT, &[(
            "data/ruby/sunrise/block/a.json",
            br#"{"template":"minecraft:stone","shapes":{"properties":["facing","count"],"values":{"north,2":[[0,0,0,1,0.5,1]]},"default":[[0,0,0,1,1,1]]},"collision_shapes":{"default":[]}}"#,
        )]).unwrap();
    let block = &content.blocks[0];
    assert_eq!(
        block.shapes.as_ref().unwrap().properties,
        ["facing", "count"]
    );
    assert_eq!(
        block.collision_shapes.as_ref().unwrap().default,
        Some(vec![])
    );
    assert!(serde_json::from_str::<BlockShapes>(r#"{"property":"axis","default":[]}"#).is_err());
    for value in [
        r#"{"properties":["axis","axis"],"default":[]}"#,
        r#"{"properties":["Axis"],"default":[]}"#,
        r#"{"properties":["axis","count"],"values":{"x":[]}}"#,
        r#"{"properties":["axis"],"values":{"":[]}}"#,
        r#"{"values":{"x":[]},"default":[]}"#,
        r#"{"default":[[1,0,0,0,1,1]]}"#,
    ] {
        let shapes: BlockShapes = serde_json::from_str(value).unwrap();
        assert!(shapes.check().is_err(), "{value}");
        let declaration = format!(r#"{{"template":"minecraft:stone","collision_shapes":{value}}}"#);
        assert!(
            smod(
                "bad-collision",
                MANIFEST_TEXT,
                &[("data/ruby/sunrise/block/a.json", declaration.as_bytes(),)]
            )
            .is_err(),
            "{value}"
        );
    }
    let many = BlockShapes(StateValues {
        property: String::new(),
        properties: vec![],
        values: BTreeMap::new(),
        default: Some(vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]; 100]),
    });
    assert_eq!(many.check(), Ok(()));
    let mut nonfinite = many;
    nonfinite.default = Some(vec![[f64::NAN; 6]]);
    assert!(nonfinite.check().is_err());
}

fn smod(name: &str, manifest: &str, files: &[(&str, &[u8])]) -> Result<Content, String> {
    let dir = scratch(name);
    let path = dir.join("m.smod");
    std::fs::write(&path, Package::write_files(manifest, files)?).unwrap();
    let package = Package::read(&path);
    let content = package.and_then(|p| Content::read(&path, &p.manifest));
    std::fs::remove_dir_all(&dir).unwrap();
    content
}

#[test]
fn a_declared_friction_is_any_finite_number() {
    let load = |value: &str| {
        let file =
            format!(r#"{{"template":"minecraft:stone","properties":{{"friction":{value}}}}}"#);
        smod(
            "friction-range",
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/block/a.json", file.as_bytes())],
        )
    };
    for (value, expected) in [("3.0", 3.0), ("-0.5", -0.5)] {
        let content = load(value).unwrap();
        assert_eq!(content.blocks[0].properties.friction, Some(expected));
    }
    // JSON has no NaN or infinity: a literal is a parse error, and a number past f32's range
    // becomes infinity and is refused by the check.
    assert!(load("NaN").is_err());
    assert!(load("1e39").is_err());
}

#[test]
fn state_light_checks_ranges_selectors_and_native_declarations() {
    let load = |name: &str, table: &str| {
        let declaration = format!(
            r#"{{"template":"minecraft:stone","properties":{{"light":3}},"state_light":{table}}}"#
        );
        smod(
            name,
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/block/lamp.json", declaration.as_bytes())],
        )
    };
    let content = load(
        "state-light-valid",
        r#"{"properties":["facing","lit"],"values":{"north,true":15},"default":0}"#,
    )
    .unwrap();
    assert_eq!(content.blocks[0].properties.light, Some(3));
    let table = content.blocks[0].state_light.as_ref().unwrap();
    assert_eq!(table.default, Some(0));
    assert_eq!(table.values["north,true"], 15);
    for (i, table) in [
        r#"{"default":-1}"#,
        r#"{"default":16}"#,
        r#"{"default":256}"#,
        r#"{"default":2.5}"#,
        r#"{"default":true}"#,
        r#"{"default":"15"}"#,
        r#"{"properties":["axis","axis"],"default":0}"#,
        r#"{"property":"axis","properties":["axis"],"default":0}"#,
        r#"{"properties":["axis","lit"],"values":{"x":15},"default":0}"#,
        r#"{"values":{"x":15},"default":0}"#,
        r#"{}"#,
    ]
    .iter()
    .enumerate()
    {
        assert!(
            load(&format!("state-light-invalid-{i}"), table).is_err(),
            "{table}"
        );
    }
    let mut declared = Content::declared("ruby");
    let mut lamp = content.blocks[0].clone();
    lamp.state_light.as_mut().unwrap().default = Some(16);
    assert!(declared.declare(&[lamp], &[]).is_err());
    assert!(declared.blocks.is_empty());
}

#[test]
fn place_block_checks_files_and_native_declarations() {
    for (i, target) in ["ruby:frame", "minecraft:stone"].iter().enumerate() {
        let text = format!(r#"{{"template":"minecraft:stick","block":"{target}"}}"#);
        let content = smod(
            &format!("place-block-{i}"),
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/item/empty_frame.json", text.as_bytes())],
        )
        .unwrap();
        assert_eq!(content.items[0].block.as_deref(), Some(*target));
    }
    for (i, target) in ["stone", "other:frame", "ruby:Bad", "ruby:"]
        .iter()
        .enumerate()
    {
        let text = format!(r#"{{"template":"minecraft:stick","block":"{target}"}}"#);
        assert!(
            smod(
                &format!("bad-place-block-{i}"),
                MANIFEST_TEXT,
                &[("data/ruby/sunrise/item/empty_frame.json", text.as_bytes())]
            )
            .is_err()
        );
        let mut content = Content::declared("ruby");
        assert!(
            content
                .declare(
                    &[],
                    &[Item {
                        id: "ruby:empty_frame".into(),
                        template: "minecraft:stick".into(),
                        block: Some((*target).into()),
                        ..Default::default()
                    }]
                )
                .is_err()
        );
        assert!(content.items.is_empty());
    }
}

#[test]
fn blocks_and_items_declared_in_code_pass_the_files_checks() {
    let block = |id: &str| Block {
        id: id.into(),
        template: "minecraft:iron_block".into(),
        item: true,
        ..Block::default()
    };
    let item = |id: &str| Item {
        id: id.into(),
        template: "minecraft:diamond".into(),
        ..Item::default()
    };
    let mut content = Content::declared("ruby");
    assert!(content.is_empty());
    let wrench = Item {
        crafting_remainder: Some("ruby:gear".into()),
        ..item("ruby:wrench")
    };
    content
        .declare(&[block("ruby:gear"), block("ruby:casing")], &[wrench])
        .unwrap();
    assert_eq!(content.blocks.len(), 2);
    assert_eq!(content.items[0].id, "ruby:wrench");
    // Each refusal adds nothing.
    let refused = |blocks: &[Block], items: &[Item]| {
        let mut c = Content::declared("ruby");
        c.declare(&[block("ruby:gear")], &[]).unwrap();
        let e = c.declare(blocks, items).unwrap_err()[0].1.to_string();
        assert_eq!((c.blocks.len(), c.items.len()), (1, 0), "{e}");
        e
    };
    for error in [
        refused(&[block("emerald:gear")], &[]),
        refused(&[block("minecraft:gear")], &[]),
        refused(&[], &[item("ruby")]),
    ] {
        assert!(
            error.contains("owned namespaces") && error.contains("ruby"),
            "{error}"
        );
    }
    assert!(refused(&[block("ruby:gear")], &[]).contains("declared twice"));
    assert!(refused(&[], &[item("ruby:gear")]).contains("block's item"));
    let mut content = Content::declared("ruby");
    content.declare(&[block("ruby:gear")], &[]).unwrap();
    let errors = content
        .declare(
            &[block("ruby:gear"), block("ruby:lamp")],
            &[item("ruby:lamp")],
        )
        .unwrap_err();
    assert_eq!(
        errors,
        [
            (
                DeclarationKind::Block,
                DeclarationError::Rejected {
                    id: "ruby:gear".into(),
                    reason: "declared twice".into(),
                }
            ),
            (
                DeclarationKind::Item,
                DeclarationError::Rejected {
                    id: "ruby:lamp".into(),
                    reason: "declared twice, or is also a block's item".into(),
                }
            ),
        ]
    );
    assert_eq!(content.blocks.len(), 1);
    assert!(content.items.is_empty());
    let modded = Block {
        template: "ruby:other".into(),
        ..block("ruby:casing")
    };
    assert!(refused(&[modded], &[]).contains("not a vanilla block"));
    let dim = Block {
        properties: BlockProperties {
            light: Some(16),
            ..BlockProperties::default()
        },
        ..block("ruby:lamp")
    };
    assert!(refused(&[dim], &[]).contains("light is 0 to 15"));
    let stack = Item {
        max_stack_size: Some(100),
        ..item("ruby:ingot")
    };
    assert!(refused(&[], &[stack]).contains("max_stack_size"));
    let rest = Item {
        crafting_remainder: Some("emerald:dust".into()),
        ..item("ruby:dust")
    };
    assert!(refused(&[block("ruby:pipe")], &[rest]).contains("crafting_remainder"));
    // A cable's tag comes bare from code.
    let cable = Block {
        template: "minecraft:chorus_plant".into(),
        connects_to: Some("ruby:cables".into()),
        ..block("ruby:cable")
    };
    content.declare(&[cable], &[]).unwrap();
}

#[test]
fn a_connecting_block_file_names_its_tag_with_a_hash() {
    let cable = br#"{"template": "minecraft:chorus_plant", "connects_to": "ruby:cables"}"#;
    let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/cable.json", cable)];
    let e = smod("bare-tag", MANIFEST_TEXT, files).unwrap_err();
    assert!(e.contains("#namespace:path"), "{e}");
}

#[test]
fn content_only_mod_round_trips() {
    let files: &[(&str, &[u8])] = &[
        ("data/ruby/sunrise/block/ruby_block.json", BLOCK),
        ("data/ruby/sunrise/item/ruby.json", ITEM),
        ("data/ruby/recipe/ruby_block.json", b"{}"),
        ("data/minecraft/tags/block/mineable/pickaxe.json", b"{}"),
        ("data/c/tags/item/gems/ruby.json", b"{}"),
        ("assets/ruby/textures/block/ruby_block.png", b"png"),
        ("assets/minecraft/atlases/blocks.json", b"{}"),
    ];
    let content = smod("ok", MANIFEST_TEXT, files).unwrap();
    assert_eq!(content.files.len(), 7);
    assert_eq!(
        content.blocks,
        [Block {
            id: "ruby:ruby_block".into(),
            template: "minecraft:iron_block".into(),
            shapes: None,
            collision_shapes: None,
            outline_context_shapes: BTreeMap::new(),
            collision_context_shapes: BTreeMap::new(),
            interaction_shapes: None,
            shapes_from_block_entity: false,
            context_collision: None,
            state_light: None,
            state_properties: BTreeMap::new(),
            default_state: BTreeMap::new(),
            properties: BlockProperties {
                hardness: Some(4.0),
                light: Some(3),
                sounds: Some("minecraft:amethyst_block".into()),
                ..BlockProperties::default()
            },
            item: true,
            open_sound: None,
            close_sound: None,
            block_item: None,
            pick_item: None,
            components: BTreeMap::new(),
            suspicious_stew_effects: None,
            connects_to: None,
            thickness: None,
            wood_type: None,
            wall_block: None,
            sign_text_color: None,
            sign_text_glow: None,
            base_block: None,
        }]
    );
    assert_eq!(content.items[0].id, "ruby:ruby");
    assert_eq!(content.items[0].rarity, Some(Rarity::Uncommon));
    assert!(content.items[0].components.is_empty());
    assert_eq!(content.items[0].crafting_remainder, None);
}

#[test]
fn items_name_the_block_they_place() {
    let file = "data/ruby/sunrise/item/seeds.json";
    let content = smod(
        "seeds",
        MANIFEST_TEXT,
        &[(
            file,
            br#"{"template":"minecraft:wheat_seeds","block":"ruby:crop"}"#,
        )],
    )
    .unwrap();
    assert_eq!(content.items[0].block.as_deref(), Some("ruby:crop"));
    let error = smod(
        "bad-block",
        MANIFEST_TEXT,
        &[(
            file,
            br#"{"template":"minecraft:wheat_seeds","block":"crop"}"#,
        )],
    )
    .unwrap_err();
    assert!(
        error.contains("block must be a namespaced block id"),
        "{error}"
    );
}

#[test]
fn items_name_their_crafting_remainder() {
    let files: &[(&str, &[u8])] = &[
        (
            "data/ruby/sunrise/item/a_stew.json",
            br#"{"template": "minecraft:mushroom_stew", "crafting_remainder": "minecraft:bowl"}"#,
        ),
        (
            "data/ruby/sunrise/item/b_flask.json",
            br#"{"template": "minecraft:potion", "crafting_remainder": "ruby:c_flask"}"#,
        ),
        ("data/ruby/sunrise/item/c_flask.json", ITEM),
    ];
    let content = smod("remainder", MANIFEST_TEXT, files).unwrap();
    let rests: Vec<_> = content
        .items
        .iter()
        .map(|i| i.crafting_remainder.as_deref())
        .collect();
    assert_eq!(rests, [Some("minecraft:bowl"), Some("ruby:c_flask"), None]);
}

#[test]
fn items_name_an_arm_pose() {
    let item = |pose: &str| {
        let text = format!(r#"{{"template": "minecraft:stick", "arm_pose": "{pose}"}}"#);
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/item/a.json", text.as_bytes())];
        smod("arm-pose", MANIFEST_TEXT, files)
    };
    assert_eq!(
        item("toot_horn").unwrap().items[0].arm_pose,
        Some(ArmPose::TootHorn)
    );
    let refused = item("bow").unwrap_err();
    assert!(
        refused.contains("a.json") && refused.contains("bow"),
        "{refused}"
    );
    let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/item/a.json", ITEM)];
    let plain = smod("arm-pose", MANIFEST_TEXT, files).unwrap();
    assert_eq!(plain.items[0].arm_pose, None);
}

#[test]
fn a_file_item_may_leave_a_code_declared_item_as_its_remainder() {
    // `native_items` declares ruby:wrench in code; a file item names it as its remainder.
    let files: &[(&str, &[u8])] = &[(
        "data/ruby/sunrise/item/tool.json",
        br#"{"template": "minecraft:iron_pickaxe", "crafting_remainder": "ruby:wrench"}"#,
    )];
    let read = |declared: &[String]| {
        let dir = scratch("declared-remainder");
        let path = dir.join("m.smod");
        std::fs::write(&path, Package::write_files(MANIFEST_TEXT, files).unwrap()).unwrap();
        let package = Package::read(&path).unwrap();
        let content = Content::read_declaring(&path, &package.manifest, declared);
        std::fs::remove_dir_all(&dir).unwrap();
        content
    };
    assert!(read(&[]).is_err(), "a remainder nobody declares is refused");
    let content = read(&["ruby:wrench".to_owned()]).expect("the code-declared item is known");
    assert_eq!(
        content.items[0].crafting_remainder.as_deref(),
        Some("ruby:wrench")
    );
}

#[test]
fn items_patch_their_template_components() {
    let sword = br#"{"template": "minecraft:iron_sword", "components": {
            "minecraft:max_damage": 900,
            "minecraft:attribute_modifiers": [{"type": "minecraft:attack_damage",
                "id": "minecraft:base_attack_damage", "amount": 7.0,
                "operation": "add_value", "slot": "mainhand"}],
            "!minecraft:repairable": {}}}"#;
    let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/item/ruby_sword.json", sword)];
    let content = smod("components", MANIFEST_TEXT, files).unwrap();
    let components = &content.items[0].components;
    assert_eq!(
        components.keys().collect::<Vec<_>>(),
        [
            "!minecraft:repairable",
            "minecraft:attribute_modifiers",
            "minecraft:max_damage"
        ]
    );
    assert_eq!(components["minecraft:max_damage"], 900);
}

#[test]
fn mob_effects_take_a_category_a_colour_and_a_sort_order() {
    let files: &[(&str, &[u8])] = &[
        (
            "data/ruby/sunrise/mob_effect/glow.json",
            br#"{"category": "beneficial", "color": 16711680, "sort_order": -3}"#,
        ),
        (
            "data/ruby/sunrise/mob_effect/curse/dark.json",
            br#"{"category": "harmful", "color": 0}"#,
        ),
    ];
    let content = smod("effects", MANIFEST_TEXT, files).unwrap();
    let effects: Vec<_> = content
        .effects
        .iter()
        .map(|e| (e.id.as_str(), e.category, e.color, e.sort_order))
        .collect();
    assert_eq!(
        effects,
        [
            ("ruby:curse/dark", EffectCategory::Harmful, 0, None),
            ("ruby:glow", EffectCategory::Beneficial, 0xFF0000, Some(-3)),
        ]
    );
    for (case, file, why) in [
        (
            "effect-colour",
            r#"{"category": "neutral", "color": 16777216}"#,
            "color is RGB",
        ),
        (
            "effect-category",
            r#"{"category": "good", "color": 1}"#,
            "unknown variant",
        ),
        (
            "effect-key",
            r#"{"category": "neutral", "color": 1, "id": "x"}"#,
            "unknown field",
        ),
        (
            "effect-missing",
            r#"{"category": "neutral"}"#,
            "missing field",
        ),
    ] {
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/mob_effect/a.json", file.as_bytes())];
        let e = smod(case, MANIFEST_TEXT, files).unwrap_err();
        assert!(e.contains(why), "{case}: {e}");
    }
}

#[test]
fn menu_owned_slots_require_one_valid_stack_limit_per_slot() {
    let path = "data/ruby/sunrise/menu/filter.json";
    let menu = br#"{"slots":2,"owned_slots":[1,32]}"#;
    let content = smod("menu-owned", MANIFEST_TEXT, &[(path, menu)]).unwrap();
    assert_eq!(
        content.menu_types[0].owned_slots.as_deref(),
        Some(&[1, 32][..])
    );

    for (case, limits) in [
        ("too-short", "[1]"),
        ("too-long", "[1,2,3]"),
        ("zero", "[0,2]"),
        ("oversized", "[1,100]"),
    ] {
        let menu = format!(r#"{{"slots":2,"owned_slots":{limits}}}"#);
        let error = smod(case, MANIFEST_TEXT, &[(path, menu.as_bytes())]).unwrap_err();
        assert!(error.contains("owned_slots"), "{case}: {error}");
    }
}

#[test]
fn menu_owned_slots_reject_take_only_and_crafting_results() {
    let path = "data/ruby/sunrise/menu/filter.json";
    for (case, result) in [
        ("take-only", r#""results":[4]"#),
        ("crafting", r#""crafting":{"grid":0,"size":2,"result":4}"#),
    ] {
        let menu = format!(r#"{{"slots":5,"owned_slots":[1,1,1,1,1],{result}}}"#);
        let error = smod(case, MANIFEST_TEXT, &[(path, menu.as_bytes())]).unwrap_err();
        assert!(error.contains("ruby:filter"), "{case}: {error}");
    }
}

#[test]
fn menu_types_take_slots_data_slots_and_roles() {
    let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/sunrise/menu/press.json",
                br#"{"slots": 9, "data_slots": 2, "results": [7, 8], "inputs": [0, 6]}"#,
            ),
            ("data/ruby/sunrise/menu/bag.json", br#"{"slots": 4}"#),
            (
                "data/ruby/sunrise/menu/status.json",
                br#"{"slots": 0, "player_inventory": false, "data_slots": 2}"#,
            ),
            (
                "data/ruby/sunrise/menu/station.json",
                br#"{"slots": 16, "results": [11], "inputs": [15], "crafting": {"grid": 1, "result": 0}, "links": {"from": 12, "count": 4}}"#,
            ),
        ];
    let content = smod("menus", MANIFEST_TEXT, files).unwrap();
    let menus: Vec<_> = content
        .menu_types
        .iter()
        .map(|m| {
            (
                m.id.as_str(),
                m.slots,
                m.player_inventory,
                m.data_slots,
                m.results.as_slice(),
                m.inputs.as_slice(),
            )
        })
        .collect();
    assert_eq!(
        menus,
        [
            ("ruby:bag", 4, true, 0, &[][..], &[][..]),
            ("ruby:press", 9, true, 2, &[7, 8][..], &[0, 6][..]),
            ("ruby:station", 16, true, 0, &[11][..], &[15][..]),
            ("ruby:status", 0, false, 2, &[][..], &[][..]),
        ]
    );
    let links: Vec<_> = content.menu_types.iter().map(|m| m.links).collect();
    assert_eq!(
        links,
        [
            None,
            None,
            Some(super::MenuLinks { from: 12, count: 4 }),
            None
        ]
    );
    let crafting: Vec<_> = content.menu_types.iter().map(|m| m.crafting).collect();
    assert_eq!(
        crafting,
        [
            None,
            None,
            Some(super::MenuCrafting {
                grid: 1,
                size: 3,
                result: 0
            }),
            None,
        ]
    );
    for (case, file, details) in [
        ("menu-large", r#"{"slots": 257}"#, &["257"][..]),
        (
            "menu-data",
            r#"{"slots": 1, "data_slots": 257}"#,
            &["257"][..],
        ),
        (
            "menu-result",
            r#"{"slots": 0, "results": [0]}"#,
            &["result slot 0"][..],
        ),
        (
            "menu-input",
            r#"{"slots": 0, "inputs": [0]}"#,
            &["input 0"][..],
        ),
        (
            "menu-twice",
            r#"{"slots": 3, "results": [1, 1]}"#,
            &["result 1"][..],
        ),
        ("menu-key", r#"{"slots": 3, "rows": 1}"#, &["rows"][..]),
        (
            "craft-size",
            r#"{"slots": 20, "crafting": {"grid": 0, "size": 4, "result": 19}}"#,
            &[],
        ),
        (
            "craft-past",
            r#"{"slots": 9, "crafting": {"grid": 1, "result": 0}}"#,
            &["1..10", "result slot 0"],
        ),
        (
            "craft-result-past",
            r#"{"slots": 0, "crafting": {"grid": 0, "size": 2, "result": 4}}"#,
            &["0..4", "result slot 4"],
        ),
        (
            "craft-overlap",
            r#"{"slots": 10, "crafting": {"grid": 0, "result": 4}}"#,
            &[],
        ),
        (
            "craft-results",
            r#"{"slots": 10, "results": [2], "crafting": {"grid": 0, "result": 9}}"#,
            &[],
        ),
        (
            "craft-key",
            r#"{"slots": 10, "crafting": {"grid": 0, "result": 9, "width": 3}}"#,
            &["width"],
        ),
        (
            "links-past",
            r#"{"slots": 0, "links": {"from": 0, "count": 1}}"#,
            &["0..1"],
        ),
        (
            "links-empty",
            r#"{"slots": 10, "links": {"from": 2, "count": 0}}"#,
            &["2..2"],
        ),
        (
            "links-results",
            r#"{"slots": 10, "results": [5], "links": {"from": 4, "count": 2}}"#,
            &[],
        ),
        (
            "links-grid",
            r#"{"slots": 20, "crafting": {"grid": 1, "result": 0}, "links": {"from": 9, "count": 4}}"#,
            &[],
        ),
        (
            "links-key",
            r#"{"slots": 10, "links": {"from": 2, "count": 2, "rows": 1}}"#,
            &["rows"],
        ),
        ("menu-missing", r#"{"data_slots": 1}"#, &["slots"]),
    ] {
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/menu/a.json", file.as_bytes())];
        let error = smod(case, MANIFEST_TEXT, files).unwrap_err();
        assert!(error.contains("ruby:a"), "{case}: {error}");
        for detail in details {
            assert!(error.contains(detail), "{case}: {error}");
        }
    }
}

#[test]
fn menu_types_reject_invalid_inputs() {
    for (case, file) in [
        ("input-past", r#"{"slots": 3, "inputs": [3]}"#),
        ("input-twice", r#"{"slots": 3, "inputs": [1, 1]}"#),
        (
            "input-result",
            r#"{"slots": 3, "inputs": [1], "results": [1]}"#,
        ),
        (
            "input-grid",
            r#"{"slots": 6, "inputs": [2], "crafting": {"grid": 1, "size": 2, "result": 0}}"#,
        ),
        (
            "input-craft-result",
            r#"{"slots": 6, "inputs": [0], "crafting": {"grid": 1, "size": 2, "result": 0}}"#,
        ),
    ] {
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/menu/a.json", file.as_bytes())];
        assert!(smod(case, MANIFEST_TEXT, files).is_err(), "{case}");
    }
}

#[test]
fn entity_types_copy_a_throwable_and_name_its_item() {
    let files: &[(&str, &[u8])] = &[
        ("data/ruby/sunrise/item/gem.json", ITEM),
        (
            "data/ruby/sunrise/entity_type/bolt.json",
            br#"{"template": "minecraft:snowball", "item": "ruby:gem", "damage": 2.5}"#,
        ),
        ("data/ruby/sunrise/item/dust.json", ITEM),
        (
            "data/ruby/sunrise/entity_type/puff.json",
            br#"{"template": "minecraft:snowball", "item": "ruby:dust"}"#,
        ),
    ];
    let content = smod("types", MANIFEST_TEXT, files).unwrap();
    let types: Vec<_> = content
        .entity_types
        .iter()
        .map(|t| (t.id.as_str(), t.item.as_deref(), t.damage))
        .collect();
    assert_eq!(
        types,
        [
            ("ruby:bolt", Some("ruby:gem"), 2.5),
            ("ruby:puff", Some("ruby:dust"), 0.0)
        ]
    );
    let snowball = |rest: &str| format!(r#"{{"template": "minecraft:snowball"{rest}}}"#);
    for (case, file, why) in [
        (
            "type-template",
            r#"{"template": "minecraft:egg", "item": "ruby:gem"}"#.to_owned(),
            "is not one of",
        ),
        (
            "type-vanilla-item",
            snowball(r#", "item": "minecraft:stick""#),
            "is not one of this mod's items",
        ),
        (
            "type-unknown-item",
            snowball(r#", "item": "ruby:none""#),
            "is not one of this mod's items",
        ),
        (
            "type-damage",
            snowball(r#", "item": "ruby:gem", "damage": -1"#),
            "damage is 0 or more",
        ),
        (
            "type-key",
            snowball(r#", "item": "ruby:gem", "size": 1"#),
            "unknown field",
        ),
        (
            "type-no-item",
            snowball(""),
            "names the item that throws it",
        ),
        (
            "type-thrown-attributes",
            snowball(r#", "item": "ruby:gem", "attributes": {"minecraft:max_health": 4}"#),
            "only a living type has attributes",
        ),
        (
            "type-thrown-passenger-attachments",
            snowball(r#", "item": "ruby:gem", "passenger_attachments": [0.5]"#),
            "only a living type has passenger attachments",
        ),
    ] {
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/sunrise/item/gem.json", ITEM),
            ("data/ruby/sunrise/entity_type/a.json", file.as_bytes()),
        ];
        let e = smod(case, MANIFEST_TEXT, files).unwrap_err();
        assert!(e.contains(why), "{case}: {e}");
    }
    let shared = snowball(r#", "item": "ruby:gem""#);
    let files: &[(&str, &[u8])] = &[
        ("data/ruby/sunrise/item/gem.json", ITEM),
        ("data/ruby/sunrise/entity_type/a.json", shared.as_bytes()),
        ("data/ruby/sunrise/entity_type/b.json", shared.as_bytes()),
    ];
    let e = smod("type-shared", MANIFEST_TEXT, files).unwrap_err();
    assert!(e.contains("already throws another entity type"), "{e}");
}

#[test]
fn living_entity_types_copy_a_mob_with_their_own_attributes() {
    let files: &[(&str, &[u8])] = &[
        (
            "data/ruby/sunrise/entity_type/ghoul.json",
            br#"{"template": "minecraft:zombie",
                     "attributes": {"minecraft:max_health": 40, "minecraft:movement_speed": 0.3}}"#,
        ),
        (
            "data/ruby/sunrise/entity_type/settler.json",
            br#"{"template": "minecraft:villager"}"#,
        ),
        (
            "data/ruby/sunrise/entity_type/merchant.json",
            br#"{"template": "minecraft:wandering_trader"}"#,
        ),
        // The tameable ones (#4031).
        (
            "data/ruby/sunrise/entity_type/hound.json",
            br#"{"template": "minecraft:wolf", "attributes": {"minecraft:max_health": 20}}"#,
        ),
        (
            "data/ruby/sunrise/entity_type/macaw.json",
            br#"{"template": "minecraft:parrot"}"#,
        ),
        (
            "data/ruby/sunrise/entity_type/tabby.json",
            br#"{"template": "minecraft:cat"}"#,
        ),
        (
            "data/ruby/sunrise/entity_type/archer.json",
            br#"{"template": "minecraft:skeleton"}"#,
        ),
        (
            "data/ruby/sunrise/entity_type/diver.json",
            br#"{"template": "minecraft:drowned"}"#,
        ),
    ];
    let content = smod("living", MANIFEST_TEXT, files).unwrap();
    let types: Vec<_> = content
        .entity_types
        .iter()
        .map(|t| {
            let attributes: Vec<_> = t.attributes.iter().map(|(a, v)| (a.as_str(), *v)).collect();
            (
                t.id.as_str(),
                t.template.as_str(),
                t.is_living(),
                attributes,
            )
        })
        .collect();
    assert_eq!(
        types,
        [
            ("ruby:archer", "minecraft:skeleton", true, vec![]),
            ("ruby:diver", "minecraft:drowned", true, vec![]),
            (
                "ruby:ghoul",
                "minecraft:zombie",
                true,
                vec![
                    ("minecraft:max_health", 40.0),
                    ("minecraft:movement_speed", 0.3)
                ]
            ),
            (
                "ruby:hound",
                "minecraft:wolf",
                true,
                vec![("minecraft:max_health", 20.0)]
            ),
            ("ruby:macaw", "minecraft:parrot", true, vec![]),
            ("ruby:merchant", "minecraft:wandering_trader", true, vec![]),
            ("ruby:settler", "minecraft:villager", true, vec![]),
            ("ruby:tabby", "minecraft:cat", true, vec![]),
        ]
    );
    let zombie = |rest: &str| format!(r#"{{"template": "minecraft:zombie"{rest}}}"#);
    for (case, file, why) in [
        (
            "living-item",
            zombie(r#", "item": "ruby:gem""#),
            "a living type has no item or damage",
        ),
        (
            "living-damage",
            zombie(r#", "damage": 2"#),
            "a living type has no item or damage",
        ),
        (
            "living-attribute-id",
            zombie(r#", "attributes": {"max_health": 40}"#),
            "is an attribute id",
        ),
        (
            "living-attribute-value",
            zombie(r#", "attributes": {"minecraft:max_health": "40"}"#),
            "invalid type",
        ),
    ] {
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/sunrise/item/gem.json", ITEM),
            ("data/ruby/sunrise/entity_type/a.json", file.as_bytes()),
        ];
        let e = smod(case, MANIFEST_TEXT, files).unwrap_err();
        assert!(e.contains(why), "{case}: {e}");
    }
}

#[test]
fn living_entity_types_load_passenger_attachments() {
    let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/sunrise/entity_type/bird.json",
                br#"{"template": "minecraft:zombie", "passenger_attachments": [0.0, 0.7, -0.1]}"#,
            ),
            (
                "data/ruby/sunrise/entity_type/raft.json",
                br#"{"template": "minecraft:villager", "passenger_attachments": [[0.0, 0.5, 0.3], [0.0, 0.5, -0.3]]}"#,
            ),
            (
                "data/ruby/sunrise/entity_type/tall.json",
                br#"{"template": "minecraft:wandering_trader", "passenger_attachments": 1.25}"#,
            ),
        ];
    let content = smod("passengers", MANIFEST_TEXT, files).unwrap();
    assert_eq!(
        content.entity_types[0].passenger_attachments,
        vec![[0.0, 0.7, -0.1]]
    );
    assert_eq!(
        content.entity_types[1].passenger_attachments,
        vec![[0.0, 0.5, 0.3], [0.0, 0.5, -0.3]]
    );
    assert_eq!(
        content.entity_types[2].passenger_attachments,
        vec![[0.0, 1.25, 0.0]]
    );
}

#[test]
fn a_block_may_occlude_nothing() {
    let block = |occludes: &str| {
        let text = format!(
            r#"{{"template": "minecraft:barrel", "properties": {{"occludes": {occludes}}}}}"#
        );
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/panel.json", text.as_bytes())];
        smod("occludes", MANIFEST_TEXT, files)
    };
    let occludes = |c: Result<Content, String>| c.unwrap().blocks[0].properties.occludes;
    assert_eq!(occludes(block("false")), Some(false));
    assert_eq!(occludes(block("true")), Some(true));
    assert!(block(r#""no""#).is_err());
}

#[test]
fn blocks_choose_their_render_shape() {
    let block = |shape: &str| {
        let text = format!(
            r#"{{"template": "minecraft:stone", "properties": {{"render_shape": {shape}}}}}"#
        );
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/gear.json", text.as_bytes())];
        smod("render-shape", MANIFEST_TEXT, files)
    };
    let shape = |c: Result<Content, String>| c.unwrap().blocks[0].properties.render_shape;
    assert_eq!(shape(block(r#""MODEL""#)), Some(RenderShape::Model));
    assert_eq!(
        shape(block(r#""ENTITYBLOCK_ANIMATED""#)),
        Some(RenderShape::EntityblockAnimated)
    );
    assert_eq!(shape(block(r#""INVISIBLE""#)), Some(RenderShape::Invisible));
    // Only vanilla's constants, as written there.
    for bad in [r#""animated""#, r#""model""#, "1"] {
        assert!(block(bad).is_err(), "{bad}");
    }
}

#[test]
fn block_offset_refusals_name_the_file_and_key() {
    for (properties, key) in [
        (r#""offset":"XY""#, "offset"),
        (r#""max_horizontal_offset":-0.1"#, "max_horizontal_offset"),
        (r#""max_horizontal_offset":0.3"#, "max_horizontal_offset"),
        (r#""max_horizontal_offset":"wide""#, "max_horizontal_offset"),
        (r#""max_vertical_offset":0.3"#, "max_vertical_offset"),
    ] {
        let json = format!(r#"{{"template":"minecraft:stone","properties":{{{properties}}}}}"#);
        let file = "data/ruby/sunrise/block/offset.json";
        let error = smod("offset-refusal", MANIFEST_TEXT, &[(file, json.as_bytes())]).unwrap_err();
        assert!(error.contains(file) && error.contains(key), "{error}");
    }
}

#[test]
fn block_instruments_accept_every_vanilla_constant_and_name_unknown_values() {
    for instrument in mod_vocab::NoteInstrument::ALL {
        let name = instrument.name().to_ascii_uppercase();
        let properties: BlockProperties =
            serde_json::from_str(&format!(r#"{{"instrument":"{name}"}}"#)).unwrap();
        assert_eq!(properties.instrument, Some(instrument));
    }
    let error = serde_json::from_str::<BlockProperties>(r#"{"instrument":"FLUTEISH"}"#)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("instrument") && error.contains("FLUTEISH"),
        "{error}"
    );
}

#[test]
fn blocks_choose_vanilla_push_reactions() {
    for name in ["PUSH_PULL", "PUSH", "POPPED", "IMMOVEABLE", "IGNORE_ENTITY"] {
        let text = format!(
            r#"{{"template":"minecraft:stone","properties":{{"push_reaction":"{name}"}}}}"#
        );
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/gear.json", text.as_bytes())];
        let content = smod("push-reaction", MANIFEST_TEXT, files).unwrap();
        assert_eq!(
            content.blocks[0].properties.push_reaction.unwrap().name(),
            name
        );
    }
}

#[test]
fn blocks_pick_the_states_that_tick_randomly() {
    let own = r#""state_properties": {"grown": {"type": "boolean"}}"#;
    let block = |ticks: &str| {
        let text = format!(
            r#"{{"template": "minecraft:dirt", {own}, "properties": {{"random_ticks": {ticks}}}}}"#
        );
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/crop.json", text.as_bytes())];
        smod("random-ticks", MANIFEST_TEXT, files)
    };
    let ticks = |c: Result<Content, String>| c.unwrap().blocks[0].properties.random_ticks.clone();
    let state = |v: &str| [("grown".to_owned(), v.to_owned())];
    // Every state, whatever the template does; `false` stops a template that ticks.
    let all = ticks(block("true")).unwrap();
    assert!(all.applies(&state("true")) && all.applies(&[]));
    assert!(!ticks(block("false")).unwrap().applies(&state("false")));
    // Chosen states: a crop that stops ticking once it is grown.
    let chosen = ticks(block(r#"{"property": "grown", "values": ["false"]}"#)).unwrap();
    assert!(chosen.applies(&state("false")) && !chosen.applies(&state("true")));
    assert!(
        !chosen.applies(&[]),
        "a state without the property does not tick"
    );
    // Not the block's own property, no values, or a value it doesn't have.
    for bad in [
        r#"{"property": "age", "values": ["0"]}"#,
        r#"{"property": "grown", "values": []}"#,
        r#"{"property": "grown", "values": ["maybe"]}"#,
    ] {
        let e = block(bad).err().unwrap();
        assert!(e.contains("random_ticks"), "{bad}: {e}");
    }
    assert!(block("3").is_err());
}

#[test]
fn block_items_patch_their_components() {
    let block = br#"{"template": "minecraft:iron_block", "components": {
            "minecraft:max_stack_size": 16, "!minecraft:lore": {}}}"#;
    let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/ruby_block.json", block)];
    let content = smod("block-components", MANIFEST_TEXT, files).unwrap();
    let components = &content.blocks[0].components;
    assert_eq!(
        components.keys().collect::<Vec<_>>(),
        ["!minecraft:lore", "minecraft:max_stack_size"]
    );
    assert_eq!(components["minecraft:max_stack_size"], 16);
}

#[test]
fn a_server_mod_is_refused_for_registry_files_but_not_for_assets() {
    let server = MANIFEST_TEXT.replace("side = \"both\"", "side = \"server\"");
    let manifest = Manifest::parse(&server).unwrap();
    let recipe: &[u8] = br#"{"type": "minecraft:crafting_shapeless"}"#;
    for (file, bytes) in [
        ("data/ruby/sunrise/item/gem.json", ITEM),
        ("data/ruby/sunrise/block/ruby_block.json", BLOCK),
    ] {
        let content = smod("server-refused", &server, &[(file, bytes)]).unwrap();
        assert_eq!(
            content.server_only_refusal(&manifest).unwrap(),
            format!(
                "ruby: a side = \"server\" mod carries {file}, which every client must have; declare side = \"both\""
            )
        );
    }
    let content = smod(
        "server-loads",
        &server,
        &[
            ("assets/ruby/lang/en_us.json", &b"{}"[..]),
            ("data/ruby/recipe/gem.json", recipe),
        ],
    )
    .unwrap();
    assert_eq!(content.server_only_refusal(&manifest), None);
    assert_eq!(
        content.files,
        ["assets/ruby/lang/en_us.json", "data/ruby/recipe/gem.json"]
    );
}

#[test]
fn folder_mod_reads_the_same() {
    let dir = scratch("folder");
    std::fs::write(dir.join(crate::manifest::MANIFEST), MANIFEST_TEXT).unwrap();
    let block = dir.join("data/ruby/sunrise/block");
    std::fs::create_dir_all(&block).unwrap();
    std::fs::write(block.join("ruby_block.json"), BLOCK).unwrap();
    let package = Package::read(&dir).unwrap();
    let content = Content::read(&dir, &package.manifest).unwrap();
    assert_eq!(content.files, ["data/ruby/sunrise/block/ruby_block.json"]);
    assert_eq!(content.file(&content.files[0]).unwrap(), BLOCK);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A connecting block names a block tag and copies a `PipeBlock`.
#[test]
fn connecting_blocks_copy_a_pipe_and_name_a_tag() {
    let block = |json: &str| {
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/cable.json", json.as_bytes())];
        smod("cable", MANIFEST_TEXT, files)
    };
    let content =
        block(r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:cable_joins"}"##)
            .unwrap();
    assert_eq!(
        content.blocks[0].connects_to.as_deref(),
        Some("ruby:cable_joins")
    );
    let thin =
        r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:j", "thickness": 4}"##;
    assert_eq!(block(thin).unwrap().blocks[0].thickness, Some(4.0));
    let e = block(r#"{"template": "minecraft:chorus_plant", "thickness": 4}"#).unwrap_err();
    assert!(e.contains("connects_to"), "{e}");
    let e = block(
        r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:j", "thickness": 16}"##,
    )
    .unwrap_err();
    assert!(e.contains("2 to 14"), "{e}");
    let e = block(r##"{"template": "minecraft:stone", "connects_to": "#ruby:cable_joins"}"##)
        .unwrap_err();
    assert!(e.contains("chorus_plant"), "{e}");
    for tag in ["ruby:cable_joins", "#cable_joins", "#Ruby:Cable"] {
        let json = format!(r#"{{"template": "minecraft:chorus_plant", "connects_to": "{tag}"}}"#);
        let e = block(&json).unwrap_err();
        assert!(e.contains("block tag"), "{tag}: {e}");
    }
}

#[test]
fn declared_waterlogged_requires_a_boolean() {
    let parse = |template: &str, property: &str| {
        let json = format!(
            r#"{{"template":"minecraft:{template}","state_properties":{{"waterlogged":{property}}}}}"#
        );
        smod(
            "waterlogged",
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/block/plant.json", json.as_bytes())],
        )
    };
    assert!(parse("tall_grass", r#"{"type":"boolean"}"#).is_ok());
    assert!(parse("stone", r#"{"type":"boolean"}"#).is_ok());
    let error = parse("stone", r#"{"type":"enum","values":["false","true"]}"#).unwrap_err();
    assert!(
        error.contains("state_properties.waterlogged") && error.contains("plant.json"),
        "{error}"
    );
}

#[test]
fn gate_sounds_need_a_fence_gate_and_sound_ids() {
    let block = |json: &str| {
        smod(
            "gate",
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/block/gate.json", json.as_bytes())],
        )
    };
    let parsed = block(
            r#"{"template": "minecraft:oak_fence_gate", "open_sound": "ruby:gate.open", "close_sound": "minecraft:block.chain.break"}"#,
        )
        .unwrap();
    assert_eq!(
        parsed.blocks[0].open_sound,
        Some(GateSound::All("ruby:gate.open".into()))
    );
    assert_eq!(
        parsed.blocks[0].close_sound,
        Some(GateSound::All("minecraft:block.chain.break".into()))
    );
    let sites = block(r#"{"template":"minecraft:oak_fence_gate","open_sound":{"use":"ruby:rope.open","redstone":"ruby:wire.open"},"close_sound":{"explosion":"ruby:blast.close"}}"#).unwrap();
    assert_eq!(
        sites.blocks[0].open_sound,
        Some(GateSound::Sites {
            on_use: Some("ruby:rope.open".into()),
            on_explosion: None,
            on_redstone: Some("ruby:wire.open".into()),
        })
    );
    assert_eq!(
        sites.blocks[0].close_sound,
        Some(GateSound::Sites {
            on_use: None,
            on_explosion: Some("ruby:blast.close".into()),
            on_redstone: None,
        })
    );
    let plain = block(r#"{"template": "minecraft:oak_fence_gate"}"#).unwrap();
    assert_eq!(
        (&plain.blocks[0].open_sound, &plain.blocks[0].close_sound),
        (&None, &None)
    );
    for (bad, word) in [
        (
            r#"{"template": "minecraft:oak_fence", "open_sound": "ruby:gate.open"}"#,
            "minecraft:oak_fence",
        ),
        (
            r#"{"template": "minecraft:oak_fence_gate", "close_sound": "gate"}"#,
            "namespace:path",
        ),
        (
            r#"{"template":"minecraft:oak_fence","open_sound":{"use":"ruby:open"}}"#,
            "open_sound",
        ),
        (
            r#"{"template":"minecraft:oak_fence_gate","open_sound":{"other":"ruby:open"}}"#,
            "open_sound",
        ),
        (
            r#"{"template":"minecraft:oak_fence_gate","close_sound":{"redstone":"bad"}}"#,
            "close_sound.redstone",
        ),
    ] {
        let e = block(bad).unwrap_err();
        assert!(e.contains("data/ruby/sunrise/block/gate.json"), "{e}");
        assert!(e.contains(word), "{bad}: {e}");
    }
}

#[test]
fn block_item_names_an_item_and_needs_item_false() {
    let block = |json: &str| {
        smod(
            "blockitem",
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/block/slab.json", json.as_bytes())],
        )
    };
    let parsed =
        block(r#"{"template": "minecraft:stone", "item": false, "block_item": "ruby:slab_item"}"#)
            .unwrap();
    assert_eq!(
        parsed.blocks[0].block_item.as_deref(),
        Some("ruby:slab_item")
    );
    assert_eq!(
        block(r#"{"template": "minecraft:stone"}"#).unwrap().blocks[0].block_item,
        None
    );
    for (bad, word) in [
        (
            r#"{"template": "minecraft:stone", "block_item": "ruby:slab_item"}"#,
            "item is true",
        ),
        (
            r#"{"template": "minecraft:stone", "item": false, "block_item": "none"}"#,
            "\"item\": false",
        ),
        (
            r#"{"template": "minecraft:stone", "item": false, "block_item": "slab"}"#,
            "namespace:path",
        ),
    ] {
        let e = block(bad).unwrap_err();
        assert!(e.contains(word), "{bad}: {e}");
    }
}

#[test]
fn pick_item_names_an_item_whatever_the_blocks_own_is() {
    let block = |json: &str| {
        smod(
            "pickitem",
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/block/crop.json", json.as_bytes())],
        )
    };
    for json in [
        r#"{"template": "minecraft:stone", "item": false, "pick_item": "ruby:seed"}"#,
        r#"{"template": "minecraft:stone", "pick_item": "ruby:seed"}"#,
        r#"{"template": "minecraft:stone", "item": false, "block_item": "ruby:fruit", "pick_item": "ruby:seed"}"#,
    ] {
        let parsed = block(json).unwrap();
        assert_eq!(parsed.blocks[0].pick_item.as_deref(), Some("ruby:seed"));
    }
    assert_eq!(
        block(r#"{"template": "minecraft:stone"}"#).unwrap().blocks[0].pick_item,
        None
    );
    for bad in ["seed", "none", ""] {
        let e = block(&format!(
            r#"{{"template": "minecraft:stone", "pick_item": "{bad}"}}"#
        ))
        .unwrap_err();
        assert!(
            e.contains("pick_item") && e.contains("namespace:path"),
            "{bad}: {e}"
        );
    }
}

#[test]
fn farmland_base_requires_farmland_and_a_block_id() {
    let block = |json: &str| {
        smod(
            "farmland",
            MANIFEST_TEXT,
            &[("data/ruby/sunrise/block/farm.json", json.as_bytes())],
        )
    };
    let parsed = block(r#"{"template":"minecraft:farmland","base_block":"ruby:soil"}"#).unwrap();
    assert_eq!(parsed.blocks[0].base_block.as_deref(), Some("ruby:soil"));
    for bad in [
        r#"{"template":"minecraft:stone","base_block":"ruby:soil"}"#,
        r#"{"template":"minecraft:farmland","base_block":"soil"}"#,
    ] {
        assert!(block(bad).unwrap_err().contains("base_block"));
    }
}

/// A sign names its wood; a standing or ceiling sign names the wall form
/// its item places. Neither means anything on another template.
#[test]
fn signs_name_their_wood_and_wall_form() {
    let block = |json: &str| {
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/block/notice.json", json.as_bytes())];
        smod("sign", MANIFEST_TEXT, files)
    };
    let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/sunrise/block/notice.json",
                br#"{"template": "minecraft:oak_sign", "wood_type": "ruby:elm", "wall_block": "ruby:notice_wall"}"#,
            ),
            (
                "data/ruby/sunrise/block/notice_wall.json",
                br#"{"template": "minecraft:oak_wall_sign", "item": false}"#,
            ),
        ];
    let content = smod("sign-pair", MANIFEST_TEXT, files).unwrap();
    assert_eq!(content.blocks[0].wood_type.as_deref(), Some("ruby:elm"));
    assert_eq!(
        content.blocks[0].wall_block.as_deref(),
        Some("ruby:notice_wall")
    );
    // Vanilla's woods go by their bare names, as `WoodType.name` does.
    let cherry = r#"{"template": "minecraft:cherry_wall_hanging_sign", "wood_type": "cherry", "item": false}"#;
    assert_eq!(
        block(cherry).unwrap().blocks[0].wood_type.as_deref(),
        Some("cherry")
    );
    assert_eq!(
        block(r#"{"template": "minecraft:oak_sign", "wood_type": "ruby-wood:old/elm"}"#)
            .unwrap()
            .blocks[0]
            .wood_type
            .as_deref(),
        Some("ruby-wood:old/elm")
    );
    for bad in [
        r#"{"template": "minecraft:stone", "wood_type": "ruby:elm"}"#,
        r#"{"template": "minecraft:oak_sign", "wood_type": "Ruby:Elm"}"#,
        r#"{"template": "minecraft:oak_sign", "wood_type": "a:b:c"}"#,
        r#"{"template": "minecraft:oak_sign", "wood_type": "../elm"}"#,
    ] {
        assert!(block(bad).unwrap_err().contains("wood_type"), "{bad}");
    }
    let text = block(
            r#"{"template": "minecraft:oak_sign", "sign_text_color": "light_blue", "sign_text_glow": true}"#,
        )
        .unwrap();
    assert_eq!(
        text.blocks[0].sign_text_color.as_deref(),
        Some("light_blue")
    );
    assert_eq!(text.blocks[0].sign_text_glow, Some(true));
    for bad in [
        r#"{"template": "minecraft:stone", "sign_text_color": "white"}"#,
        r#"{"template": "minecraft:oak_sign", "sign_text_color": "White"}"#,
        r##"{"template": "minecraft:oak_sign", "sign_text_color": "#ffffff"}"##,
    ] {
        assert!(block(bad).unwrap_err().contains("sign_text_color"), "{bad}");
    }
    assert!(
        block(r#"{"template": "minecraft:stone", "sign_text_glow": true}"#)
            .unwrap_err()
            .contains("sign_text_glow")
    );
    for bad in [
        r#"{"template": "minecraft:stone", "wall_block": "ruby:notice_wall"}"#,
        r#"{"template": "minecraft:oak_wall_sign", "wall_block": "ruby:notice_wall", "item": false}"#,
        r#"{"template": "minecraft:oak_sign", "wall_block": "notice_wall"}"#,
        r#"{"template": "minecraft:oak_sign", "wall_block": ":wall"}"#,
        r#"{"template": "minecraft:oak_sign", "wall_block": "ruby:"}"#,
        r#"{"template": "minecraft:oak_sign", "wall_block": "Ruby:wall"}"#,
        r#"{"template": "minecraft:oak_sign", "wall_block": "ruby:wall:other"}"#,
        r#"{"template": "minecraft:oak_sign", "wall_block": "ruby:notice_wall", "item": false}"#,
    ] {
        assert!(block(bad).unwrap_err().contains("wall_block"), "{bad}");
    }
}

/// A sign's wall form is one of the mod's blocks, of the matching wall
/// template, with no item and no other sign. Anything else refuses this
/// mod when it is read, and leaves the other mods alone.
#[test]
fn a_signs_wall_block_is_its_own_mods_wall_form() {
    let refused = |template: &str, wall_block: &str, wall: &str, wanted: &str| {
        let sign =
            format!(r#"{{"template": "minecraft:{template}", "wall_block": "{wall_block}"}}"#);
        let second =
            format!(r#"{{"template": "minecraft:{template}", "wall_block": "ruby:wall"}}"#);
        let mut files: Vec<(&str, &[u8])> = vec![
            ("data/ruby/sunrise/block/sign.json", sign.as_bytes()),
            ("data/ruby/sunrise/block/wall.json", wall.as_bytes()),
        ];
        if wanted == "already paired" {
            files.push(("data/ruby/sunrise/block/second.json", second.as_bytes()));
        }
        let e = smod("sign-wall", MANIFEST_TEXT, &files).unwrap_err();
        assert!(e.contains(wanted), "{wanted}: {e}");
    };
    let wall = r#"{"template": "minecraft:oak_wall_sign", "item": false}"#;
    for (template, wall_template) in [
        ("oak_sign", "stone"),
        ("oak_sign", "oak_wall_hanging_sign"),
        ("oak_hanging_sign", "oak_wall_sign"),
    ] {
        let other = format!(r#"{{"template": "minecraft:{wall_template}", "item": false}}"#);
        refused(template, "ruby:wall", &other, "needs a template");
    }
    refused(
        "oak_sign",
        "ruby:wall",
        r#"{"template": "minecraft:oak_wall_sign"}"#,
        "\"item\" is false",
    );
    refused("oak_sign", "ruby:wall", wall, "already paired");
    for absent in ["ruby:elsewhere", "other:wall", "minecraft:oak_wall_sign"] {
        refused("oak_sign", absent, wall, "not one of this mod's blocks");
    }
    // A sign and the wall form it hangs as start with the same text, or it would change
    // colour on a wall.
    let pair = |sign_text: &str, wall_text: &str| {
        let sign = format!(
            r#"{{"template": "minecraft:oak_sign", "wall_block": "ruby:wall"{sign_text}}}"#
        );
        let wall =
            format!(r#"{{"template": "minecraft:oak_wall_sign", "item": false{wall_text}}}"#);
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/sunrise/block/sign.json", sign.as_bytes()),
            ("data/ruby/sunrise/block/wall.json", wall.as_bytes()),
        ];
        smod("sign-text", MANIFEST_TEXT, files)
    };
    let white = r#", "sign_text_color": "white""#;
    assert!(pair(white, white).is_ok());
    assert!(
        pair(
            "",
            r#", "sign_text_color": "black", "sign_text_glow": false"#
        )
        .is_ok()
    );
    for (sign, wall) in [
        (white, ""),
        ("", white),
        (white, r#", "sign_text_color": "red""#),
    ] {
        assert!(
            pair(sign, wall)
                .unwrap_err()
                .contains("different sign_text")
        );
    }
    assert!(pair(r#", "sign_text_glow": true"#, "").is_err());

    // Declared in code, the same; nothing is added.
    let block = |id: &str, template: &str, item, wall_block: Option<&str>| Block {
        id: id.into(),
        template: template.into(),
        item,
        wall_block: wall_block.map(str::to_owned),
        ..Block::default()
    };
    let mut content = Content::declared("ruby");
    let sign = block("ruby:sign", "minecraft:oak_sign", true, Some("ruby:wall"));
    assert!(content.declare(std::slice::from_ref(&sign), &[]).is_err());
    assert!(content.blocks.is_empty());
    let wall = block("ruby:wall", "minecraft:oak_wall_sign", false, None);
    content.declare(&[sign, wall], &[]).unwrap();
    assert_eq!(content.blocks.len(), 2);
}

/// A creative tab: a title, an icon and its items, all item ids.
#[test]
fn world_fluid_generated_names_cannot_collide() {
    let files: &[(&str, &[u8])] = &[
        ("data/ruby/sunrise/fluid/oil.json", br#"{"world":{}}"#),
        (
            "data/ruby/sunrise/fluid/flowing_oil.json",
            br#"{"world":{}}"#,
        ),
    ];
    assert!(
        smod("fluid-collision", MANIFEST_TEXT, files)
            .unwrap_err()
            .contains("declared twice")
    );
}

#[test]
fn fluid_interaction_files_load_in_name_order_and_refuse_what_cannot_resolve() {
    let file = |name: &str, body: &'static str| {
        vec![(
            format!("data/ruby/sunrise/fluid_interaction/{name}.json"),
            body,
        )]
    };
    let load = |files: Vec<(String, &'static str)>| {
        let files: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(n, b)| (n.as_str(), b.as_bytes()))
            .collect();
        smod("fluid-interaction", MANIFEST_TEXT, &files)
    };
    let slag = r#"{"source":"minecraft:lava","neighbour":{"fluid_tag":"minecraft:water"},
            "below":{"block_tag":"ruby:cold"},"becomes":{"source":"ruby:slag","flowing":"ruby:slag[age=1]"}}"#;
    let mut files = file("b_slag", slag);
    files.extend(file(
            "a_glass",
            r#"{"source":"ruby:oil","neighbour":{"block":"minecraft:sand"},"becomes":{"flowing":"minecraft:glass"}}"#,
        ));
    let content = load(files).unwrap();
    let sources: Vec<_> = content
        .fluid_interactions
        .iter()
        .map(|i| i.source.as_str())
        .collect();
    assert_eq!(sources, ["ruby:oil", "minecraft:lava"], "by file name");
    for (body, refusal) in [
        (
            r#"{"source":"lava","neighbour":{"block":"minecraft:sand"},"becomes":{"source":"minecraft:glass"}}"#,
            "source",
        ),
        (
            r#"{"source":"minecraft:lava","neighbour":{"block":"sand"},"becomes":{"source":"minecraft:glass"}}"#,
            "neighbour",
        ),
        (
            r#"{"source":"minecraft:lava","neighbour":{"block":"minecraft:sand"},"becomes":{}}"#,
            "becomes",
        ),
        (
            r#"{"source":"minecraft:lava","neighbour":{"block":"minecraft:sand"},"becomes":{"source":"glass"}}"#,
            "becomes",
        ),
        (
            r#"{"source":"minecraft:lava","neighbour":{"stone":"minecraft:sand"},"becomes":{"source":"minecraft:glass"}}"#,
            "stone",
        ),
        (
            r#"{"source":"minecraft:lava","neighbour":{"block":"minecraft:sand"},"becomes":{"source":"minecraft:glass"},"when":"any"}"#,
            "when",
        ),
    ] {
        let error = load(file("bad", body)).unwrap_err();
        assert!(error.contains(refusal), "{body}: {error}");
    }
}

#[test]
fn a_world_fluid_brings_its_block_and_bucket() {
    let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/sunrise/fluid/oil.json",
                br#"{"tint":4278190080,"world":{"tick_delay":20,"drop_off":2,"slope_find_distance":2}}"#,
            ),
            (
                "data/ruby/sunrise/fluid/mist.json",
                br#"{"world":{"bucket":false}}"#,
            ),
        ];
    let content = smod("world-fluid", MANIFEST_TEXT, files).unwrap();
    let oil = content.fluids.iter().find(|f| f.id == "ruby:oil").unwrap();
    assert_eq!(oil.flowing_id(), "ruby:flowing_oil");
    assert_eq!(oil.world.unwrap().tick_delay, 20);
    assert!(!oil.world.unwrap().source_conversion);
    let block = content.blocks.iter().find(|b| b.id == "ruby:oil").unwrap();
    assert_eq!(block.template, "minecraft:water");
    assert!(!block.item, "the fluid's block has no block item");
    let bucket = content
        .items
        .iter()
        .find(|i| i.id == "ruby:oil_bucket")
        .unwrap();
    assert_eq!(bucket.template, "minecraft:water_bucket");
    assert!(content.blocks.iter().any(|b| b.id == "ruby:mist"));
    assert!(!content.items.iter().any(|i| i.id == "ruby:mist_bucket"));
}

#[test]
fn a_world_fluid_refuses_what_cannot_flow_and_a_second_block() {
    for world in [
        br#"{"world":{"tick_delay":0}}"#.as_slice(),
        br#"{"world":{"drop_off":8}}"#,
        br#"{"world":{"slope_find_distance":5}}"#,
    ] {
        let files: &[(&str, &[u8])] = &[("data/ruby/sunrise/fluid/oil.json", world)];
        assert!(smod("bad-world-fluid", MANIFEST_TEXT, files).is_err());
    }
    let files: &[(&str, &[u8])] = &[
        ("data/ruby/sunrise/fluid/oil.json", br#"{"world":{}}"#),
        (
            "data/ruby/sunrise/block/oil.json",
            br#"{"template":"minecraft:stone"}"#,
        ),
    ];
    assert!(
        smod("clashing-world-fluid", MANIFEST_TEXT, files)
            .unwrap_err()
            .contains("declared twice")
    );
}

#[test]
fn fluids_declare_a_sprite_and_a_tint() {
    let files: &[(&str, &[u8])] = &[
        (
            "data/ruby/sunrise/fluid/steam.json",
            br#"{"sprite":"ruby:gui/fluids/steam","tint":4291343520}"#,
        ),
        ("data/ruby/sunrise/fluid/bare.json", b"{}"),
    ];
    let content = smod("fluid", MANIFEST_TEXT, files).unwrap();
    assert_eq!(
        content.fluids,
        [
            FluidLook {
                id: "ruby:bare".into(),
                sprite: None,
                tint: None,
                world: None,
            },
            FluidLook {
                id: "ruby:steam".into(),
                sprite: Some("ruby:gui/fluids/steam".into()),
                tint: Some(4291343520),
                world: None,
            },
        ]
    );
    for sprite in [
        &br#"{"sprite":"steam"}"#[..],
        br#"{"sprite":"ruby:has space"}"#,
        br#"{"sprite":"Ruby:steam"}"#,
        br#"{"sprite":"ruby:"}"#,
    ] {
        let bad: &[(&str, &[u8])] = &[("data/ruby/sunrise/fluid/x.json", sprite)];
        let e = smod("fluid", MANIFEST_TEXT, bad).unwrap_err();
        assert!(e.contains("sprite"), "{e}");
    }
}

#[test]
fn creative_tabs_name_their_items() {
    let tab = |json: &str| {
        let files: &[(&str, &[u8])] =
            &[("data/ruby/sunrise/creative_tab/gems.json", json.as_bytes())];
        smod("tab", MANIFEST_TEXT, files)
    };
    let content =
            tab(r#"{"title": "Ruby Gems", "icon": "ruby:ruby", "items": ["ruby:ruby", "minecraft:diamond"]}"#)
                .unwrap();
    assert_eq!(
        content.creative_tabs,
        [CreativeTab {
            id: "ruby:gems".into(),
            title: "Ruby Gems".into(),
            icon: "ruby:ruby".into(),
            items: vec!["ruby:ruby".into(), "minecraft:diamond".into()],
        }]
    );
    let e = tab(r#"{"title": "Gems", "icon": "ruby", "items": []}"#).unwrap_err();
    assert!(e.contains("item ids"), "{e}");
}

#[test]
fn two_packages_can_supply_the_merged_recipe_priorities_but_not_neighbouring_files() {
    let other_manifest = MANIFEST_TEXT.replace("ruby", "sapphire");
    for (name, manifest) in [("ruby", MANIFEST_TEXT), ("sapphire", &other_manifest)] {
        let content = smod(name, manifest, &[(RECIPE_PRIORITIES_PATH, b"{}")]).unwrap();
        assert!(
            content
                .files
                .iter()
                .any(|path| path == RECIPE_PRIORITIES_PATH)
        );
    }
    for path in [
        "data/neoforge/recipe/unowned.json",
        "data/neoforge/other.json",
        "data/c/recipe_priorities.json",
        "data/minecraft/recipe_priorities.json",
    ] {
        let error = smod("nonshared-priority-path", MANIFEST_TEXT, &[(path, b"{}")]).unwrap_err();
        assert!(error.contains(path), "{error}");
    }
}

#[test]
fn refuses_content_outside_its_fence() {
    let refused = |name: &str, files: &[(&str, &[u8])]| {
        let e = smod(name, MANIFEST_TEXT, files).unwrap_err();
        assert!(!e.is_empty());
        e
    };
    let e = refused("other-ns", &[("data/other/sunrise/block/a.json", BLOCK)]);
    assert!(e.contains("own namespaces"), "{e}");
    refused(
        "vanilla-asset",
        &[("assets/minecraft/textures/block/stone.png", b"x")],
    );
    refused(
        "vanilla-recipe",
        &[("data/minecraft/recipe/stone.json", b"{}")],
    );
    refused("convention-recipe", &[("data/c/recipe/stone.json", b"{}")]);
    refused("convention-asset", &[("assets/c/lang/en_us.json", b"{}")]);
    let e = refused(
        "modded-template",
        &[(
            "data/ruby/sunrise/block/a.json",
            br#"{"template": "ruby:other"}"#,
        )],
    );
    assert!(e.contains("not a vanilla block"), "{e}");
    refused(
        "light",
        &[(
            "data/ruby/sunrise/block/a.json",
            br#"{"template": "minecraft:stone", "properties": {"light": 16}}"#,
        )],
    );
    refused(
        "unknown-key",
        &[(
            "data/ruby/sunrise/block/a.json",
            br#"{"template": "minecraft:stone", "glow": true}"#,
        )],
    );
    let e = refused(
        "push-reaction",
        &[(
            "data/ruby/sunrise/block/a.json",
            br#"{"template":"minecraft:stone","properties":{"push_reaction":"NORMAL"}}"#,
        )],
    );
    assert!(e.contains("push_reaction"), "{e}");
    refused("shape-reversed", &[("data/ruby/sunrise/block/a.json", br#"{"template":"minecraft:stone","shapes":{"properties":["axis"],"values":{"x":[[1,0,0,0,1,1]]}}}"#)]);
    refused("shape-malformed", &[("data/ruby/sunrise/block/a.json", br#"{"template":"minecraft:stone","shapes":{"properties":["axis"],"values":{"x":[[0,0,0,1,1]]}}}"#)]);
    refused(
        "shape-no-values",
        &[(
            "data/ruby/sunrise/block/a.json",
            br#"{"template":"minecraft:stone","shapes":{"properties":["axis"],"values":{}}}"#,
        )],
    );
    let e = refused(
        "clash",
        &[
            ("data/ruby/sunrise/block/a.json", BLOCK),
            ("data/ruby/sunrise/item/a.json", ITEM),
        ],
    );
    assert!(e.contains("also a block's item"), "{e}");
    refused("other-kind", &[("data/ruby/sunrise/entity/a.json", b"{}")]);
    refused(
        "stack",
        &[(
            "data/ruby/sunrise/item/a.json",
            br#"{"template": "minecraft:stick", "max_stack_size": 100}"#,
        )],
    );
    for (case, components, why) in [
        (
            "bare-component",
            r#"{"max_damage": 3}"#,
            "must be namespaced",
        ),
        (
            "uppercase-path",
            r#"{"minecraft:Foo": 3}"#,
            "must be namespaced and valid",
        ),
        (
            "uppercase-namespace",
            r#"{"Example:charge": 3}"#,
            "must be namespaced and valid",
        ),
        (
            "empty-namespace",
            r#"{":charge": 3}"#,
            "must be namespaced and valid",
        ),
        (
            "empty-path",
            r#"{"example:": 3}"#,
            "must be namespaced and valid",
        ),
        (
            "invalid-removal",
            r#"{"!example:Foo": {}}"#,
            "must be namespaced and valid",
        ),
        (
            "own-key",
            r#"{"minecraft:rarity": "epic"}"#,
            "set rarity instead",
        ),
        (
            "own-removal",
            r#"{"!minecraft:max_stack_size": {}}"#,
            "set max_stack_size",
        ),
        (
            "removal-value",
            r#"{"!minecraft:food": true}"#,
            "a removal's value is {}",
        ),
    ] {
        let item = format!(r#"{{"template": "minecraft:stick", "components": {components}}}"#);
        let e = refused(case, &[("data/ruby/sunrise/item/a.json", item.as_bytes())]);
        assert!(e.contains(why), "{case}: {e}");
    }
    let glass = br#"{"template": "minecraft:glass", "item": false}"#;
    for (case, rest) in [
        ("bare-remainder", "bowl"),
        ("missing-remainder", "ruby:bowl"),
        ("other-mod-remainder", "jade:bowl"),
        ("itemless-remainder", "ruby:glass"),
        ("air-remainder", "minecraft:air"),
    ] {
        let item = format!(r#"{{"template": "minecraft:stick", "crafting_remainder": "{rest}"}}"#);
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/sunrise/item/a.json", item.as_bytes()),
            ("data/ruby/sunrise/block/glass.json", glass),
        ];
        let e = refused(case, files);
        assert!(e.contains("crafting_remainder"), "{case}: {e}");
    }
    for (case, block, why) in [
        (
            "block-removal-value",
            r#"{"template": "minecraft:stone", "components": {"!minecraft:lore": []}}"#,
            "a removal's value is {}",
        ),
        (
            "block-stack-zero",
            r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": 0}}"#,
            "integer 1 to 99",
        ),
        (
            "block-stack-100",
            r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": 100}}"#,
            "integer 1 to 99",
        ),
        (
            "block-stack-string",
            r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": "16"}}"#,
            "integer 1 to 99",
        ),
        (
            "block-rarity",
            r#"{"template": "minecraft:stone", "components": {"minecraft:rarity": "bogus"}}"#,
            "common, uncommon",
        ),
        (
            "block-without-item",
            r#"{"template": "minecraft:glass", "item": false,
                    "components": {"minecraft:max_stack_size": 16}}"#,
            "item is false",
        ),
    ] {
        let e = refused(
            case,
            &[("data/ruby/sunrise/block/a.json", block.as_bytes())],
        );
        assert!(e.contains(why), "{case}: {e}");
    }
}

#[test]
fn reserved_namespace_accepts_only_declared_extension_tags() {
    for tag in crate::extension_tags::ALL {
        let path = format!(
            "data/sunrise/tags/{}/{}.json",
            tag.registry,
            tag.id.strip_prefix("sunrise:").unwrap()
        );
        let content = smod(
            "extension",
            MANIFEST_TEXT,
            &[(path.as_str(), br#"{"values":[]}"#)],
        )
        .unwrap();
        assert_eq!(content.files.len(), 1);
    }
    for (path, hint) in [
        (
            "data/sunrise/tags/block/villager_secondary_poi/custom.json",
            "secondary_blocks",
        ),
        (
            "data/sunrise/tags/item/villager_secondary_poi/farmer.json",
            "extension_tags",
        ),
        ("data/sunrise/tags/block/class/crop.json", "block_part_tag"),
        ("data/sunrise/loot_table/a.json", "own namespaces"),
        ("assets/sunrise/lang/en_us.json", "own namespaces"),
    ] {
        let error = smod("extension-refused", MANIFEST_TEXT, &[(path, b"{}")]).unwrap_err();
        assert!(error.contains(path) && error.contains(hint), "{error}");
    }
}

#[test]
fn listed_namespaces_hold_content() {
    let manifest = format!("{MANIFEST_TEXT}namespaces = [\"kaisyn\"]\n");
    let files: &[(&str, &[u8])] = &[
        ("data/kaisyn/worldgen/template_pool/a.json", b"{}"),
        ("data/kaisyn/structure/a.nbt", b"nbt"),
        ("assets/kaisyn/lang/en_us.json", b"{}"),
    ];
    let content = smod("listed", &manifest, files).unwrap();
    assert_eq!(content.files.len(), 3);
    assert_eq!(content.namespaces, ["ruby", "kaisyn"]);
    // Files and code share the manifest-owned namespace fence.
    let decl: &[(&str, &[u8])] = &[("data/kaisyn/sunrise/block/a.json", BLOCK)];
    let content = smod("listed-decl", &manifest, decl).unwrap();
    assert_eq!(content.blocks[0].id, "kaisyn:a");
    let mut code = Content::declared("ruby");
    code.namespaces.push("kaisyn".into());
    code.declare(&content.blocks, &[]).unwrap();
    assert_eq!(code.blocks[0].id, "kaisyn:a");
    let mut foreign = content.blocks[0].clone();
    foreign.id = "unowned:a".into();
    let error = code.declare(&[foreign], &[]).unwrap_err()[0].1.to_string();
    assert!(
        error.contains("unowned:a")
            && error.contains("owned namespaces")
            && error.contains("ruby")
            && error.contains("kaisyn"),
        "{error}"
    );
    // A namespace it does not list is still outside.
    let other: &[(&str, &[u8])] = &[("data/other/structure/a.nbt", b"nbt")];
    assert!(smod("listed-other", &manifest, other).is_err());
}

#[test]
fn mods_sharing_a_namespace_clash() {
    let t_and_t = ["t_and_t".to_owned(), "kaisyn".to_owned()];
    let kaisyn = ["kaisyn".to_owned()];
    let ruby = ["ruby".to_owned(), "gems".to_owned()];
    assert_eq!(
        clash(("t_and_t", &t_and_t), ("kaisyn", &kaisyn)).as_deref(),
        Some("mods t_and_t and kaisyn both use the namespace kaisyn; turn one of them off")
    );
    assert_eq!(clash(("t_and_t", &t_and_t), ("ruby", &ruby)), None);
}

#[test]
fn paths_are_plain() {
    assert!(check_path("assets/ruby/textures/block/ruby_block.png").is_ok());
    assert!(check_path("data/ruby/loot_table/blocks/a.json").is_ok());
    for bad in [
        "assets/ruby",
        "assets/../x/y",
        "assets/ruby//a",
        "assets/Ruby/a",
        "assets/ruby/a b",
        "/assets/ruby/a",
        "other/ruby/a",
        "assets\\ruby\\a",
    ] {
        assert!(check_path(bad).is_err(), "{bad}");
    }
    let error = check_path("data/ruby/Glow-Slab.json").unwrap_err();
    assert!(error.contains("\"data/ruby/glow-slab.json\""), "{error}");
}
