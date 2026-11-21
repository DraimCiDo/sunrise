use super::*;
use serde_json::json;

#[test]
fn data_map_read_refuses_lossy_payloads_instead_of_successful_substitutes() {
    for value in [
        json!({"nested": [null]}),
        json!(9223372036854775809_u64),
        json!(u64::MAX),
    ] {
        let maps = DataMaps {
            maps: BTreeMap::from([(
                "minecraft:item".to_owned(),
                HashMap::from([(
                    "test:map".to_owned(),
                    MapValues::Json(BTreeMap::from([("minecraft:stone".to_owned(), value)])),
                )]),
            )]),
            ..DataMaps::default()
        };
        assert_eq!(
            maps.value("minecraft:item", "test:map", "minecraft:stone"),
            Err(mod_api::DataMapError::Undecodable)
        );
        assert_eq!(
            maps.value("minecraft:item", "test:map", "minecraft:air"),
            Ok(None)
        );
    }
}

#[test]
fn builtin_store_is_registered_without_mod_types() {
    let builtin = super::super::neoforge_mob_maps::load(
        &sunrise_world::loot::PackOverlay::NONE,
        |_| None,
        |_, _| true,
    )
    .unwrap();
    let maps = DataMaps::default().with_builtins(builtin);
    assert_eq!(maps.registries().len(), 5);
    assert_eq!(
        maps.value(
            "minecraft:item",
            "neoforge:villager_compostables",
            "minecraft:wheat_seeds"
        ),
        Ok(Some(Tag::compound([(
            "can_villager_compost",
            Tag::Byte(1)
        )])))
    );
}

/// Registries for tests: items, blocks and entity types; entity type ids
/// listed, item and block ids not, and the tags given by registry.
#[derive(Default)]
struct Fixture {
    tags: HashMap<(&'static str, &'static str), Vec<&'static str>>,
}
impl Fixture {
    fn with(tags: &[(&'static str, &'static str, &[&'static str])]) -> Self {
        Self {
            tags: tags
                .iter()
                .map(|(registry, tag, members)| ((*registry, *tag), members.to_vec()))
                .collect(),
        }
    }
}
impl MapRegistries for Fixture {
    fn known(&self, registry: &str) -> bool {
        ["minecraft:item", "minecraft:block", "minecraft:entity_type"].contains(&registry)
    }
    fn contains(&self, registry: &str, entry: &str) -> bool {
        registry != "minecraft:entity_type"
            || [
                "minecraft:pig",
                "minecraft:stray",
                "minecraft:skeleton",
                "minecraft:cow",
            ]
            .contains(&entry)
    }
    fn tag(&self, registry: &str, tag: &str) -> Vec<String> {
        self.tags
            .iter()
            .filter(|((r, t), _)| *r == registry && *t == tag)
            .flat_map(|(_, members)| members.iter().map(|m| (*m).to_owned()))
            .collect()
    }
}

fn ty(id: &str, registry: &str) -> DataMapType {
    DataMapType {
        id: id.to_owned(),
        registry: registry.to_owned(),
    }
}

fn files(list: &[(&str, &[Value])]) -> Vec<(String, Vec<Vec<u8>>)> {
    list.iter()
        .map(|(key, versions)| {
            (
                (*key).to_owned(),
                versions
                    .iter()
                    .map(|v| v.to_string().into_bytes())
                    .collect(),
            )
        })
        .collect()
}

fn load(files: &[(String, Vec<Vec<u8>>)]) -> DataMaps {
    let registries = Fixture::with(&[(
        "minecraft:item",
        "minecraft:logs",
        &["minecraft:oak_log", "minecraft:birch_log"],
    )]);
    DataMaps::load(
        &[ty("mymod:fuel", "minecraft:item")],
        &sunrise_world::loot::PackOverlay {
            files,
            ..sunrise_world::loot::PackOverlay::default()
        },
        &registries,
    )
}

fn get<'a>(maps: &'a DataMaps, entry: &str) -> Option<&'a Value> {
    let MapValues::Json(values) = &maps.maps["minecraft:item"]["mymod:fuel"] else {
        panic!("JSON map");
    };
    values.get(entry)
}

#[test]
fn a_value_reads_as_nbt_and_a_map_no_mod_registered_is_refused() {
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[json!({"values": {
            "minecraft:coal": 1600,
            "minecraft:stick": {"burn": 3_000_000_000_i64}
        }})],
    )]);
    let maps = load(&f);
    let value = |registry: &str, type_id: &str, entry: &str| maps.value(registry, type_id, entry);
    assert_eq!(
        value("minecraft:item", "mymod:fuel", "minecraft:coal"),
        Ok(Some(Tag::Int(1600)))
    );
    assert_eq!(
        value("minecraft:item", "mymod:fuel", "minecraft:stick"),
        Ok(Some(Tag::Compound(
            [("burn".into(), Tag::Long(3_000_000_000))]
                .into_iter()
                .collect()
        )))
    );
    assert_eq!(
        value("minecraft:item", "mymod:fuel", "minecraft:stone"),
        Ok(None)
    );
    assert_eq!(
        value("minecraft:block", "mymod:fuel", "minecraft:coal"),
        Err(mod_api::DataMapError::UnknownType)
    );
    assert_eq!(
        value("minecraft:item", "mymod:other", "minecraft:coal"),
        Err(mod_api::DataMapError::UnknownType)
    );
}

#[test]
fn registries_with_a_type_are_listed_once_in_id_order() {
    let maps = DataMaps::load(
        &[
            ty("mymod:b", "minecraft:block"),
            ty("mymod:a", "minecraft:item"),
            ty("mymod:c", "minecraft:block"),
            ty("mymod:d", "test:nowhere"),
        ],
        &sunrise_world::loot::PackOverlay::NONE,
        &Fixture::default(),
    );
    assert_eq!(maps.registries(), ["minecraft:block", "minecraft:item"]);
}

#[test]
fn a_map_on_any_registry_resolves_ids_tags_and_lists_whole() {
    // `minecraft:ghost` is not in the fixture's entity types: a tag naming
    // an id the registry doesn't hold publishes nothing for it.
    let registries = Fixture::with(&[(
        "minecraft:entity_type",
        "minecraft:skeletons",
        &["minecraft:skeleton", "minecraft:stray", "minecraft:ghost"],
    )]);
    let f = files(&[(
        "data_maps/test:entity_type/weight",
        &[json!({"values": {
            "minecraft:pig": 3,
            "#minecraft:skeletons": 5,
            "minecraft:missing": 9
        }})],
    )]);
    let maps = DataMaps::load(
        &[ty("test:weight", "minecraft:entity_type")],
        &sunrise_world::loot::PackOverlay {
            files: &f,
            ..sunrise_world::loot::PackOverlay::default()
        },
        &registries,
    );
    let value = |entry| maps.value("minecraft:entity_type", "test:weight", entry);
    assert_eq!(value("minecraft:pig"), Ok(Some(Tag::Int(3))));
    assert_eq!(value("minecraft:stray"), Ok(Some(Tag::Int(5))));
    assert_eq!(value("minecraft:cow"), Ok(None));
    // An id the registry doesn't hold is skipped.
    assert_eq!(value("minecraft:missing"), Ok(None));
    // A tag member the registry doesn't hold is skipped with the plain ids.
    assert_eq!(value("minecraft:ghost"), Ok(None));
    assert_eq!(
        maps.entries("minecraft:entity_type", "test:weight"),
        Ok(vec![
            ("minecraft:pig".to_owned(), Tag::Int(3)),
            ("minecraft:skeleton".to_owned(), Tag::Int(5)),
            ("minecraft:stray".to_owned(), Tag::Int(5)),
        ])
    );
    assert_eq!(maps.registries(), ["minecraft:entity_type"]);
}

/// A mod-declared datapack registry the command ids do not list is checked
/// against the world's loaded entries; a registry with no source of ids
/// still accepts an entry, as before.
#[test]
fn a_declared_datapack_registry_rejects_unknown_ids() {
    // The declaration the process reads, as a registering mod's would be.
    mod_api::__private::install_process_datapack_registry_declarations(vec![
        mod_api::DatapackRegistryDeclaration {
            mod_id: "example".into(),
            key: "example:spell".into(),
            fields: vec![mod_api::DatapackField::required(
                "power",
                mod_api::DatapackFieldKind::Int { min: 0, max: 100 },
            )],
            sync: mod_api::DatapackRegistrySync::Required,
        },
    ]);
    let set =
        mod_api::DatapackRegistrySet::with_entries_for_testing("example:spell", &["example:fire"]);
    let items = sunrise_client::items::Items::empty();
    let registries = WorldRegistries::new(
        [],
        Some(&set),
        &items,
        std::path::Path::new("unused"),
        &sunrise_world::loot::PackOverlay::NONE,
    );
    assert!(
        !registries.contains("example:spell", "example:ice"),
        "an id the world never loaded is refused"
    );
    assert!(registries.contains("example:spell", "example:fire"));
    // No source of ids, no refusal: a typed registry the sync did not
    // list (the ids branch above covers the listed ones).
    assert!(registries.contains("minecraft:some_typed", "whatever"));
}

#[test]
fn a_value_counts_only_when_its_own_conditions_hold() {
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[json!({"values": {
            "minecraft:coal": 1,
            "minecraft:stick": {"neoforge:conditions": [
                {"type": "neoforge:mod_loaded", "modid": "absent"}], "value": 2},
            "minecraft:planks": {"neoforge:conditions": [
                {"type": "neoforge:always"}], "value": 3}
        }})],
    )]);
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(1)));
    assert_eq!(get(&maps, "minecraft:stick"), None);
    assert_eq!(get(&maps, "minecraft:planks"), Some(&json!(3)));
}

#[test]
fn values_attach_to_entries_and_to_every_member_of_a_tag() {
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[json!({"values": {
            "minecraft:coal": {"burn": 1600},
            "stick": 100,
            "#minecraft:logs": {"burn": 300}
        }})],
    )]);
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:coal"), Some(&json!({"burn": 1600})));
    // An id without a namespace is a `minecraft` one.
    assert_eq!(get(&maps, "minecraft:stick"), Some(&json!(100)));
    assert_eq!(get(&maps, "minecraft:oak_log"), Some(&json!({"burn": 300})));
    assert_eq!(
        get(&maps, "minecraft:birch_log"),
        Some(&json!({"burn": 300}))
    );
    assert_eq!(get(&maps, "minecraft:stone"), None);
}

#[test]
fn a_bad_value_condition_does_not_replace_or_apply_any_values_in_its_layer() {
    let layers = files(&[(
        "data_maps/mymod:item/fuel",
        &[
            json!({"values":{"minecraft:stick":2,"minecraft:coal":3}}),
            json!({"replace":true,"values":{"minecraft:oak_log":4,"minecraft:planks":{
            "value":5,"neoforge:conditions":[{"type":"neoforge:not","value":{"type":"minecraft:missing"}}]
        }},"remove":["minecraft:coal"]}),
            json!({"values":{"minecraft:birch_log":6}}),
        ],
    )]);
    let maps = load(&layers);
    assert_eq!(get(&maps, "minecraft:stick"), Some(&json!(2)));
    assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(3)));
    assert_eq!(get(&maps, "minecraft:oak_log"), None);
    assert_eq!(get(&maps, "minecraft:planks"), None);
    assert_eq!(get(&maps, "minecraft:birch_log"), Some(&json!(6)));
}

#[test]
fn a_newer_file_overrides_and_replace_clears_what_came_before() {
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[
            json!({"values": {"minecraft:coal": 1, "minecraft:stick": 2}}),
            json!({"values": {"minecraft:coal": 10}}),
        ],
    )]);
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(10)));
    assert_eq!(get(&maps, "minecraft:stick"), Some(&json!(2)));
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[
            json!({"values": {"minecraft:coal": 1, "minecraft:stick": 2}}),
            json!({"replace": true, "values": {"minecraft:coal": 10}}),
        ],
    )]);
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(10)));
    assert_eq!(get(&maps, "minecraft:stick"), None);
}

#[test]
fn remove_takes_entries_and_tags_away_after_the_files_own_values() {
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[
            json!({"values": {"#minecraft:logs": 5, "minecraft:coal": 1}}),
            // The file's own value for coal is attached, then removed.
            json!({"values": {"minecraft:coal": 9}, "remove": ["minecraft:coal", "#minecraft:logs"]}),
        ],
    )]);
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:coal"), None);
    assert_eq!(get(&maps, "minecraft:oak_log"), None);
}

#[test]
fn the_wrapper_form_gives_its_value_and_a_plain_object_is_a_value() {
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[json!({"values": {
            "minecraft:coal": {"value": {"burn": 3}, "replace": true},
            "minecraft:stick": {"value": 1, "other": 2}
        }})],
    )]);
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:coal"), Some(&json!({"burn": 3})));
    // More than `value` and `replace`: the object is the data.
    assert_eq!(
        get(&maps, "minecraft:stick"),
        Some(&json!({"value": 1, "other": 2}))
    );
}

#[test]
fn a_bad_file_is_skipped_alone_and_bad_types_are_left_out() {
    let mut f = files(&[(
        "data_maps/mymod:item/fuel",
        &[json!({"values": {"minecraft:coal": 1}})],
    )]);
    f[0].1.insert(0, b"{nope".to_vec());
    f[0].1
        .push(json!({"nothing": true}).to_string().into_bytes());
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:coal"), Some(&json!(1)));
    let maps = DataMaps::load(
        &[
            ty("minecraft:fuel", "minecraft:item"),
            ty("mymod:x", "test:nowhere"),
            ty("nonamespace", "minecraft:item"),
            ty("mymod:a", "minecraft:block"),
            ty("mymod:a", "minecraft:block"),
        ],
        &sunrise_world::loot::PackOverlay::NONE,
        &Fixture::default(),
    );
    // Only the one good type, registered once, empty.
    assert_eq!(maps.maps.len(), 1);
    let blocks = &maps.maps["minecraft:block"];
    assert_eq!(blocks.len(), 1);
    assert!(matches!(&blocks["mymod:a"], MapValues::Json(values) if values.is_empty()));
}

#[test]
fn top_level_conditions_do_not_suppress_map_layers() {
    let f = files(&[(
        "data_maps/mymod:item/fuel",
        &[
            json!({"values":{"minecraft:coal":1}}),
            json!({"neoforge:conditions":[{"type":"neoforge:never"}],"replace":true,"values":{
                "minecraft:stick":2,
                "minecraft:coal":{"neoforge:conditions":[{"type":"neoforge:never"}],"value":3}
            }}),
        ],
    )]);
    let maps = load(&f);
    assert_eq!(get(&maps, "minecraft:stick"), Some(&json!(2)));
    assert!(get(&maps, "minecraft:coal").is_none());
}

#[test]
fn block_maps_read_the_block_tags() {
    let registries =
        Fixture::with(&[("minecraft:block", "minecraft:logs", &["minecraft:oak_log"])]);
    let f = files(&[(
        "data_maps/mymod:block/hardness",
        &[json!({"values": {"#minecraft:logs": 2}})],
    )]);
    let maps = DataMaps::load(
        &[ty("mymod:hardness", "minecraft:block")],
        &sunrise_world::loot::PackOverlay {
            files: &f,
            ..sunrise_world::loot::PackOverlay::default()
        },
        &registries,
    );
    let MapValues::Json(map) = &maps.maps["minecraft:block"]["mymod:hardness"] else {
        panic!("JSON map");
    };
    assert_eq!(map.get("minecraft:oak_log"), Some(&json!(2)));
}
