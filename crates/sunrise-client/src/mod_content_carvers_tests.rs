use super::*;
#[test]
fn existing_tag_expands_to_effective_carvers() {
    let ids = ["test:cave".to_owned()].into_iter().collect();
    let tags = [(
        "test:all".to_owned(),
        r#"{"values":["test:cave"]}"#.to_owned(),
    )]
    .into_iter()
    .collect();
    let registry = Registry {
        what: "carver",
        ids: &ids,
        tags: &tags,
    };
    let mut doc: J = serde_json::from_str(r##"{"carvers":"#test:all"}"##).unwrap();
    expand(&mut doc, &registry).unwrap();
    assert_eq!(
        serde_json::to_string(doc.get("carvers").unwrap()).unwrap(),
        r#"["test:cave"]"#
    );
}
#[test]
fn pack_carvers_and_replacement_tags_refresh() {
    let mut saved: SavedModifiers = serde_json::from_str(r#"{"enabled_mods":[],"modifiers":[],"unmodified":{},"feature_tags":{},"entity_categories":{},"entity_tags":{},"carver_tags":{"test:all":"{\"values\":[\"test:old\"]}"}}"#).unwrap();
    let mut packs = WorldgenOverrides::default();
    packs.set("carvers", "test:new", "{}".into());
    packs.add_tag(
        "carver_tags",
        "test:all",
        "test",
        r#"{"replace":true,"values":["test:new"]}"#.into(),
    );
    refresh(&mut saved, &packs).unwrap();
    let registry = Registry {
        what: "carver",
        ids: &saved.carver_ids,
        tags: &saved.carver_tags,
    };
    assert_eq!(
        registry.resolve(&serde_json::json!("#test:all")).unwrap(),
        ["test:new"]
    );
}
#[test]
fn a_pack_tags_remove_list_takes_carvers_out() {
    let mut saved: SavedModifiers = serde_json::from_str(r##"{"enabled_mods":[],"modifiers":[],"unmodified":{},"feature_tags":{},"entity_categories":{},"entity_tags":{},"carver_tags":{"test:all":"{\"values\":[\"test:cave\",\"test:canyon\",\"#test:wet\"]}","test:wet":"{\"values\":[\"test:sea\"]}"}}"##).unwrap();
    saved
        .carver_ids
        .extend(["test:cave", "test:canyon", "test:sea"].map(String::from));
    let mut packs = WorldgenOverrides::default();
    packs.add_tag(
        "carver_tags",
        "test:all",
        "test",
        r##"{"values":[],"remove":["test:canyon","#test:wet","#test:absent"]}"##.into(),
    );
    refresh(&mut saved, &packs).unwrap();
    let registry = Registry {
        what: "carver",
        ids: &saved.carver_ids,
        tags: &saved.carver_tags,
    };
    assert_eq!(
        registry.resolve(&serde_json::json!("#test:all")).unwrap(),
        ["test:cave"]
    );
}
#[test]
fn pack_carvers_without_modifiers_leave_world_data_unchanged() {
    let root = std::env::temp_dir().join(format!("carver-invariance-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("biome-modifiers.json"), r#"{"enabled_mods":[],"modifiers":[],"unmodified":{},"feature_tags":{},"entity_categories":{},"entity_tags":{}}"#).unwrap();
    let mut packs = WorldgenOverrides::default();
    packs.set("carvers", "test:new", "{}".into());
    packs.set("biomes", "test:plains", r#"{"carvers":"test:new"}"#.into());
    let result = super::super::biome_modifiers_over_packs(&root, &packs, &[])
        .unwrap()
        .unwrap();
    assert_eq!(result, packs);
    std::fs::remove_dir_all(root).unwrap();
}
