use super::*;
use serde_json::json;

struct Biomes;
impl HolderLookup for Biomes {
    type Id = &'static str;
    fn what(&self) -> &str {
        "biome"
    }
    fn entry(&self, id: &str) -> Result<Vec<&'static str>, String> {
        ["minecraft:plains", "minecraft:ocean", "minecraft:desert"]
            .into_iter()
            .find(|b| *b == id)
            .map(|b| vec![b])
            .ok_or_else(|| format!("unknown biome {id}"))
    }
    fn tag(&self, tag: &str) -> Result<Vec<&'static str>, String> {
        match tag {
            "minecraft:is_ocean" => Ok(vec!["minecraft:ocean"]),
            "minecraft:is_dry" => Ok(vec!["minecraft:desert"]),
            _ => Err(format!("unknown biome tag {tag}")),
        }
    }
    fn all(&self) -> Result<Vec<&'static str>, String> {
        Ok(vec![
            "minecraft:plains",
            "minecraft:ocean",
            "minecraft:desert",
        ])
    }
}

fn set(value: Value) -> Result<Vec<&'static str>, String> {
    resolve(&value, &Biomes)
}

#[test]
fn unavailable_universes_and_missing_object_fields_name_the_registry() {
    let lookup = StringLookup {
        what: "instrument",
        ids: None,
        tag: &|_| Ok(Vec::new()),
        entry: &|id| Ok(vec![id.to_owned()]),
    };
    for value in [
        json!({"type":"neoforge:any"}),
        json!({"type":"neoforge:not", "value":[]}),
    ] {
        let error = resolve(&value, &lookup).unwrap_err();
        assert!(
            error.contains("instrument") && error.contains("unavailable"),
            "{error}"
        );
    }
    for kind in ["neoforge:and", "neoforge:or", "neoforge:not"] {
        let error = resolve(&json!({"type":kind}), &lookup).unwrap_err();
        assert!(
            error.contains("instrument") && error.contains(kind),
            "{error}"
        );
    }
}

#[test]
fn plain_forms_resolve_as_vanilla_reads_them() {
    assert_eq!(set(json!("minecraft:plains")), Ok(vec!["minecraft:plains"]));
    assert_eq!(
        set(json!("#minecraft:is_ocean")),
        Ok(vec!["minecraft:ocean"])
    );
    assert_eq!(
        set(json!([
            "minecraft:plains",
            "minecraft:desert",
            "minecraft:plains"
        ])),
        Ok(vec!["minecraft:plains", "minecraft:desert"])
    );
    assert!(set(json!(["#minecraft:is_ocean"])).is_err());
    assert!(set(json!(3)).is_err());
}

#[test]
fn neoforge_objects_nest() {
    assert_eq!(
        set(json!({"type": "neoforge:not", "value": "#minecraft:is_ocean"})),
        Ok(vec!["minecraft:plains", "minecraft:desert"])
    );
    assert_eq!(
        set(json!({"type": "neoforge:and", "values": [
            {"type": "neoforge:any"},
            {"type": "neoforge:not", "value": {"type": "neoforge:or", "values": [
                "#minecraft:is_ocean", "minecraft:plains"]}}
        ]})),
        Ok(vec!["minecraft:desert"])
    );
    assert_eq!(set(json!({"type": "neoforge:any"})).unwrap().len(), 3);
    assert!(set(json!({"type": "neoforge:or"})).is_err());
    assert!(set(json!({"type": "example:custom"})).is_err());
}
