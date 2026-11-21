//! NeoForge structure modifier declaration codecs.
use super::{Action, Modifier, Result, Value, biomes};

pub(in crate::mod_content) fn is_modifier(file: &str) -> bool {
    file.strip_prefix("data/")
        .and_then(|s| s.split_once('/'))
        .is_some_and(|(_, s)| s.starts_with("neoforge/structure_modifier/") && s.ends_with(".json"))
}

impl Modifier {
    pub(in crate::mod_content) fn read(
        mod_id: &str,
        file: &str,
        text: &str,
        warnings: &mut Vec<String>,
    ) -> Result<Option<Self>> {
        let name = format!("mod {mod_id}: {file}");
        let at = |e| format!("{name}: {e}");
        let doc: Value = serde_json::from_str(text).map_err(|e| at(e.to_string()))?;
        let field = |key: &str| doc.get(key).cloned().ok_or_else(|| at(format!("no {key}")));
        let action = match doc["type"].as_str().ok_or_else(|| at("no type".into()))? {
            "neoforge:none" => return Ok(None),
            "neoforge:add_spawns" => {
                let raw = field("spawners")?;
                let values = match &raw {
                    Value::Array(v) => v.as_slice(),
                    _ => std::slice::from_ref(&raw),
                };
                Action::Add(
                    values
                        .iter()
                        .map(biomes::spawner)
                        .collect::<std::result::Result<_, _>>()
                        .map_err(at)?,
                )
            }
            "neoforge:remove_spawns" => Action::Remove(field("entity_types")?),
            "neoforge:clear_spawns" => Action::Clear(match doc.get("categories") {
                None => None,
                Some(raw) => {
                    let values = match raw {
                        Value::Array(v) => v.as_slice(),
                        _ => std::slice::from_ref(raw),
                    };
                    let mut categories = Vec::new();
                    for value in values {
                        let category = value
                            .as_str()
                            .ok_or_else(|| at("categories: expected names".into()))?;
                        if !crate::mobs::spawn_selection::Category::ALL
                            .iter()
                            .any(|known| known.name() == category)
                        {
                            return Err(at(format!("unknown mob category {category}")).into());
                        }
                        categories.push(category.to_owned());
                    }
                    Some(categories)
                }
            }),
            other => {
                warnings.push(at(format!("structure modifier type {other:?} skipped: only add_spawns, remove_spawns and clear_spawns modifiers are read")));
                return Ok(None);
            }
        };
        Ok(Some(Self {
            targets: field("structures")?,
            name,
            action,
        }))
    }
}
