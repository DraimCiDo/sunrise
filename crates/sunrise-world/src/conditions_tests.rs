use super::*;
use serde_json::json;

#[test]
fn load_conditions_capture_package_ids_and_keep_their_generation() {
    let mut enabled = vec!["code_only".to_owned()];
    let old = LoadConditions::new(enabled.iter().map(String::as_str));
    enabled.clear();
    let next = LoadConditions::new(enabled.iter().map(String::as_str));
    let requires_code = json!({"neoforge:conditions":[{
        "type":"neoforge:mod_loaded", "modid":"code_only"
    }]});
    assert!(old.allows(&requires_code));
    assert!(!next.allows(&requires_code));
    assert!(old.mod_loaded("minecraft") && next.mod_loaded("minecraft"));
    assert!(!old.mod_loaded("resource_namespace"));
    assert!(old.allows(&json!({"values":[]})));
    assert!(!old.allows(&json!({"neoforge:conditions":[{"type":"future:condition"}]})));
}

#[test]
fn load_condition_cache_identity_uses_membership_not_input_order() {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let key = |conditions: &LoadConditions| {
        let mut hasher = DefaultHasher::new();
        conditions.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(
        key(&LoadConditions::new(["b", "a", "a"])),
        key(&LoadConditions::new(["a", "b"]))
    );
    assert_ne!(
        key(&LoadConditions::new(["a"])),
        key(&LoadConditions::vanilla())
    );
    assert_eq!(
        LoadConditions::new(["minecraft"]),
        LoadConditions::vanilla()
    );
}

fn loaded(id: &str) -> bool {
    matches!(id, "minecraft" | "create")
}

#[test]
fn a_file_without_conditions_loads() {
    assert!(conditions_hold(
        &json!({"type": "minecraft:crafting"}),
        &loaded
    ));
}

#[test]
fn mod_loaded_follows_the_answer_and_minecraft_always_holds() {
    assert!(conditions_hold(
        &json!({"neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "create"}]}),
        &loaded
    ));
    assert!(!conditions_hold(
        &json!({"neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "ic2"}]}),
        &loaded
    ));
    assert!(conditions_hold(
        &json!({"neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "minecraft"}]}),
        &loaded
    ));
}

#[test]
fn not_and_or_combine_and_all_must_hold() {
    let not =
        json!({"type": "neoforge:not", "value": {"type": "neoforge:mod_loaded", "modid": "ic2"}});
    let mut c = serde_json::Map::new();
    c.insert("neoforge:conditions".to_owned(), json!([not]));
    assert!(conditions_hold(&Value::Object(c.clone()), &loaded));
    let both = json!([
        {"type": "neoforge:mod_loaded", "modid": "create"},
        {"type": "neoforge:or", "values": [
            {"type": "neoforge:mod_loaded", "modid": "ic2"},
            {"type": "neoforge:always"}
        ]}
    ]);
    c.insert("neoforge:conditions".to_owned(), both);
    assert!(conditions_hold(&Value::Object(c), &loaded));
    // One failing condition in the list fails the file.
    let failing = json!([
        {"type": "neoforge:always"},
        {"type": "neoforge:mod_loaded", "modid": "ic2"}
    ]);
    let mut f = serde_json::Map::new();
    f.insert("neoforge:conditions".to_owned(), failing);
    assert!(!conditions_hold(&Value::Object(f), &loaded));
}

#[test]
fn an_unknown_kind_nested_in_the_tree_leaves_the_file_out() {
    // `not` of `not` of an unknown: the unknown is two levels down.
    let nested_not = json!({
        "neoforge:conditions": [
            {"type": "neoforge:not", "value":
                {"type": "neoforge:not", "value":
                    {"type": "example:unknown"}}}
        ]
    });
    assert!(!conditions_hold(&nested_not, &loaded));
    // `or` with `always` beside a `not` of an unknown: one child is
    // known-true, but the other's subtree holds an unknown.
    let nested_or = json!({
        "neoforge:conditions": [
            {"type": "neoforge:or", "values": [
                {"type": "neoforge:always"},
                {"type": "neoforge:not", "value":
                    {"type": "example:unknown"}}
            ]}
        ]
    });
    assert!(!conditions_hold(&nested_or, &loaded));
}

fn file(condition: Value) -> Value {
    json!({"neoforge:conditions": [condition]})
}

#[test]
fn always_and_never_evaluate_without_a_registered_handler() {
    let conditions = LoadConditions::new([]);
    assert!(conditions.allows(&file(json!({"type": "neoforge:always"}))));
    assert!(!conditions.allows(&file(json!({"type": "neoforge:never"}))));
    assert!(conditions.allows(&file(
        json!({"type": "neoforge:not", "value": {"type": "neoforge:never"}})
    )));
    assert!(
        conditions.allows(&file(json!({"type": "neoforge:or", "values": [
            {"type": "neoforge:never"}, {"type": "neoforge:always"}
        ]})))
    );
}

#[test]
fn registered_reads_the_loads_registries_and_defaults_to_items() {
    let stone = file(json!({"type": "neoforge:registered", "value": "minecraft:stone"}));
    let missing = file(
        json!({"type": "neoforge:registered", "registry": "minecraft:item", "value": "test:missing"}),
    );
    let biome = file(
        json!({"type": "neoforge:registered", "registry": "worldgen/biome", "value": "plains"}),
    );
    // No registries in this load: unsupported, so the file is left out.
    assert!(!LoadConditions::vanilla().allows(&stone));
    let conditions = LoadConditions::new([]).with_registries([
        ("minecraft:item", vec!["minecraft:stone"]),
        ("minecraft:worldgen/biome", vec!["minecraft:plains"]),
    ]);
    assert!(conditions.allows(&stone));
    assert!(!conditions.allows(&missing));
    assert!(conditions.allows(&biome));
}

#[test]
fn feature_flags_enabled_needs_every_flag() {
    let both = file(json!({"type": "neoforge:feature_flags_enabled",
            "flags": ["minecraft:vanilla", "minecraft:trade_rebalance"]}));
    let vanilla = file(json!({"type": "neoforge:feature_flags_enabled", "flags": ["vanilla"]}));
    assert!(!LoadConditions::vanilla().allows(&vanilla));
    let default = LoadConditions::new([]).with_features([]);
    assert!(default.allows(&vanilla) && !default.allows(&both));
    let experiment =
        LoadConditions::new([]).with_features(["minecraft:vanilla", "minecraft:trade_rebalance"]);
    assert!(experiment.allows(&both));
}

#[test]
fn tag_empty_reads_only_the_tags_this_load_bound() {
    let tin = file(json!({"type": "neoforge:tag_empty", "tag": "c:ingots/tin"}));
    let before_tags = LoadConditions::new([]);
    assert!(!before_tags.allows(&tin), "no tag view yet: left out");
    let empty = before_tags.with_tags([("minecraft:item", vec!["minecraft:planks"])]);
    assert!(empty.allows(&tin));
    let with_tin = before_tags.with_tags([("minecraft:item", vec!["c:ingots/tin"])]);
    assert!(!with_tin.allows(&tin));
    assert_ne!(empty, with_tin, "the tag view is part of cache identity");
}

#[test]
fn an_unsupplied_registry_is_empty_without_a_diagnostic() {
    let conditions = LoadConditions::new([])
        .with_registries([("item", ["stone"])])
        .with_tags([("item", ["planks"])]);
    for registry in ["minecraft:worldgen/biome", "test:missing"] {
        let registered = json!({"type": "neoforge:registered",
                "registry": registry, "value": "test:entry"});
        let empty = json!({"type": "neoforge:tag_empty",
                "registry": registry, "tag": "test:tag"});
        assert!(!conditions.allows_file(&file(registered.clone()), "data/test/recipe/a.json"));
        assert!(conditions.allows_file(&file(empty.clone()), "data/test/recipe/b.json"));
        assert!(conditions.allows(&file(json!({"type": "neoforge:not", "value": registered}))));
        assert!(!conditions.allows(&file(json!({"type": "neoforge:not", "value": empty}))));
    }
    assert_eq!(conditions.check_errors(), Ok(()));
}

#[test]
fn a_retained_candidate_does_not_reuse_an_answer_across_tag_views() {
    let tin = file(json!({"type": "neoforge:tag_empty", "tag": "c:ingots/tin"}));
    let candidate = LoadConditions::new([]).with_lookup([("test:flag", |_: &Value| Ok(true))]);
    let empty = candidate.with_tags([("minecraft:item", Vec::<&str>::new())]);
    let with_tin = candidate.with_tags([("minecraft:item", vec!["c:ingots/tin"])]);
    assert!(empty.allows_file(&tin, "data/a/recipe/b.json"));
    assert!(!with_tin.allows_file(&tin, "data/a/recipe/b.json"));
}

#[test]
fn malformed_builtin_operands_are_errors_before_testing() {
    for malformed in [
        json!({"type": "neoforge:feature_flags_enabled"}),
        json!({"type": "neoforge:feature_flags_enabled", "flags": null}),
        json!({"type": "neoforge:feature_flags_enabled", "flags": "vanilla"}),
        json!({"type": "neoforge:feature_flags_enabled", "flags": [false]}),
        json!({"type": "neoforge:feature_flags_enabled", "flags": ["test:missing", 1]}),
        json!({"type": "neoforge:tag_empty"}),
        json!({"type": "neoforge:tag_empty", "tag": null}),
        json!({"type": "neoforge:tag_empty", "tag": []}),
        json!({"type": "neoforge:tag_empty", "tag": 1}),
        json!({"type": "neoforge:feature_flags_enabled", "flags": []}),
        json!({"type": "neoforge:mod_loaded", "modid": false}),
        json!({"type": "neoforge:registered", "value": true}),
        json!({"type": "neoforge:registered", "registry": null, "value": "stone"}),
        json!({"type": "neoforge:tag_empty", "tag": "bad tag"}),
        json!({"type": "neoforge:not"}),
        json!({"type": "neoforge:and", "values": {}}),
        json!({"type": "neoforge:or", "values": [null]}),
    ] {
        let not = json!({"type": "neoforge:not", "value": malformed.clone()});
        let twice = json!({"type": "neoforge:not", "value": not.clone()});
        for condition in [malformed, not, twice] {
            let payload = json!([
                {"type":"neoforge:mod_loaded","modid":"should_not_test"},
                {"type":"neoforge:or","values":[{"type":"neoforge:always"},condition]}
            ]);
            assert!(
                matches!(
                    evaluate(
                        &payload,
                        &|_| panic!("tested before decode"),
                        None,
                        &Context::default()
                    ),
                    Err(ConditionError::Invalid(_))
                ),
                "{payload}"
            );
        }
    }
}

#[test]
fn decided_combinators_skip_pure_tests_but_not_missing_context() {
    for (op, deciding, answer) in [("and", "never", false), ("or", "always", true)] {
        let decisive = json!({ "type": format!("neoforge:{deciding}") });
        let pure = json!({"type":"neoforge:mod_loaded","modid":"create"});
        let payload = json!([{"type":format!("neoforge:{op}"),"values":[decisive,pure]}]);
        let loaded = |_: &str| panic!("tested an already decided pure branch");
        let context = Context::default();
        assert_eq!(evaluate(&payload, &loaded, None, &context).unwrap(), answer);
        let missing = json!([{"type":format!("neoforge:{op}"),"values":[decisive,
                {"type":"neoforge:not","value":{"type":"neoforge:tag_empty","tag":"test:x"}}]}]);
        assert!(matches!(
            evaluate(&missing, &loaded, None, &context),
            Err(ConditionError::MissingContext(
                "neoforge:tag_empty: tags not loaded yet"
            ))
        ));
    }
}

#[test]
fn unconditioned_elements_never_build_a_diagnostic_path() {
    let mut entries = vec![json!(1), json!({"name": "test:entry"})];
    let expected = entries.clone();
    LoadConditions::vanilla().filter_list(&mut entries, &|| {
        panic!("an unconditioned element has no diagnostic path")
    });
    assert_eq!(entries, expected);
}

#[test]
fn unconditioned_loot_pools_are_kept_as_written() {
    let mut value = json!({"pools": [{"rolls": 1, "entries": [1, {"name": "test:entry"}]}]});
    let expected = value.clone();
    assert!(LoadConditions::vanilla().admit_loot_table(&mut value, "test:table"));
    assert_eq!(value, expected);
}

#[test]
fn loot_pool_conditions_drop_failing_pools_and_strip_passing_keys() {
    let mut table = json!({"pools": [
        {"rolls": 1},
        {"rolls": 2, "neoforge:conditions": [
            {"type": "neoforge:mod_loaded", "modid": "create"}]},
        {"rolls": 3, "neoforge:conditions": [
            {"type": "neoforge:mod_loaded", "modid": "ic2"}]}
    ]});
    assert!(LoadConditions::new(["create"]).admit_loot_table(&mut table, "a:b"));
    assert_eq!(table["pools"], json!([{"rolls": 1}, {"rolls": 2}]));
}

#[test]
fn an_unknown_condition_does_not_load_the_file() {
    let mut c = serde_json::Map::new();
    c.insert(
        "neoforge:conditions".to_owned(),
        json!([{"type": "neoforge:future_thing"}]),
    );
    assert!(!conditions_hold(&Value::Object(c), &loaded));
}

fn conditions_hold(v: &Value, loaded: &dyn Fn(&str) -> bool) -> bool {
    evaluate(&v["neoforge:conditions"], loaded, None, &Context::default()).unwrap_or(false)
}
