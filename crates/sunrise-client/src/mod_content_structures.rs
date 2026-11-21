//! stable-line: decodes-other-data
//! NeoForge structure modifiers, applied to the canonical structure bundle at
//! content-root build time. Contract: public 26.3 `StructureModifiers` and
//! `StructureSettingsBuilder` javadoc:
//! <https://aldak.netlify.app/javadoc/26.3.x/net/neoforged/neoforge/common/world/StructureModifiers.html>
//! <https://aldak.netlify.app/javadoc/26.3.x/net/neoforged/neoforge/common/world/StructureSettingsBuilder.html>
use super::{
    J, Overlay, Result, TagMerge, biomes, modifier_entities, modifier_tags, out_path, read_json,
};
#[path = "mod_content/structure_declarations.rs"]
mod declarations;
pub(super) use declarations::is_modifier;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) struct Modifier {
    name: String,
    targets: Value,
    action: Action,
}
enum Action {
    Add(Vec<biomes::Spawner>),
    Remove(Value),
    Clear(Option<Vec<String>>),
}

pub(super) fn apply(
    base: &Path,
    overlay: &Overlay<'_>,
    modifiers: &[Modifier],
    tags: &BTreeMap<String, TagMerge>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let bundle = "worldgen/structures.json";
    let mut doc = read_json(&out_path(overlay, bundle))?;
    let J::Obj(entries) = &doc else {
        return Err(format!("{bundle}: not an object").into());
    };
    let ids: BTreeSet<String> = entries.iter().map(|(id, _)| id.clone()).collect();
    let structure_tags = modifier_tags(base, overlay, tags, "worldgen/structure")?;
    let registry = biomes::Registry {
        what: "structure",
        ids: &ids,
        tags: &structure_tags,
    };
    let (categories, entity_tags) = modifier_entities(base, overlay, tags)?;
    let entity_ids = categories.keys().cloned().collect();
    let entities = biomes::Registry {
        what: "entity type",
        ids: &entity_ids,
        tags: &entity_tags,
    };
    let mut changed: BTreeMap<String, J> = BTreeMap::new();
    // Apply ADD before REMOVE, preserving overlay traversal order.
    for m in modifiers
        .iter()
        .filter(|m| matches!(m.action, Action::Add(_)))
        .chain(
            modifiers
                .iter()
                .filter(|m| !matches!(m.action, Action::Add(_))),
        )
    {
        let resolved = (|| -> std::result::Result<_, String> {
            let targets = registry.resolve(&m.targets)?;
            let removed = match &m.action {
                Action::Remove(set) => entities.resolve(set)?,
                Action::Add(spawners) => {
                    for s in spawners {
                        if !entity_ids.contains(&s.entity) {
                            return Err(format!("unknown entity type {}", s.entity));
                        }
                    }
                    Vec::new()
                }
                _ => Vec::new(),
            };
            Ok((targets, removed))
        })();
        let (targets, removed) = match resolved {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("{}: {e}: skipped", m.name));
                continue;
            }
        };
        if matches!(&m.action, Action::Add(spawners) if spawners.is_empty()) {
            continue;
        }
        for id in targets {
            if !changed.contains_key(&id) {
                let Some(J::Str(text)) = doc.get(&id) else {
                    continue;
                };
                changed.insert(id.clone(), serde_json::from_str(text)?);
            }
            edit(
                changed.get_mut(&id).unwrap(),
                &m.action,
                &removed,
                &categories,
            )
            .map_err(|e| format!("{}: {id}: {e}", m.name))?;
        }
    }
    for (id, structure) in changed {
        let text = serde_json::to_string(&structure)?;
        let (ns, path) = id.split_once(':').ok_or("structure id has no namespace")?;
        overlay.write(
            &format!("data/{ns}/worldgen/structure/{path}.json"),
            text.as_bytes(),
        )?;
        doc.set(&id, J::Str(text));
    }
    overlay.write(bundle, &serde_json::to_vec(&doc)?)
}

fn edit(
    doc: &mut J,
    action: &Action,
    removed: &[String],
    categories: &BTreeMap<String, String>,
) -> std::result::Result<(), String> {
    if doc.get("spawn_overrides").is_none() {
        if !matches!(action, Action::Add(s) if !s.is_empty()) {
            return Ok(());
        }
        doc.set("spawn_overrides", J::Obj(Vec::new()));
    }
    let Some(overrides @ J::Obj(_)) = doc.get_mut("spawn_overrides") else {
        return Err("spawn_overrides: not an object".into());
    };
    match action {
        Action::Add(spawners) => {
            for s in spawners {
                let cat = &categories[&s.entity];
                if overrides.get(cat).is_none() {
                    overrides.set(
                        cat,
                        J::Obj(vec![
                            ("bounding_box".into(), J::Str("piece".into())),
                            ("spawns".into(), J::Arr(Vec::new())),
                        ]),
                    );
                }
                let Some(J::Arr(list)) = overrides.get_mut(cat).and_then(|o| o.get_mut("spawns"))
                else {
                    return Err(format!("spawn_overrides.{cat}.spawns: not a list"));
                };
                list.push(s.entry());
            }
        }
        Action::Remove(_) => {
            let J::Obj(cats) = overrides else {
                unreachable!()
            };
            for (cat, value) in cats {
                let Some(J::Arr(list)) = value.get_mut("spawns") else {
                    return Err(format!("spawn_overrides.{cat}.spawns: not a list"));
                };
                biomes::remove_entities(list, removed);
            }
        }
        Action::Clear(categories) => {
            let J::Obj(cats) = overrides else {
                unreachable!()
            };
            cats.retain(|(cat, _)| {
                categories
                    .as_ref()
                    .is_some_and(|selected| !selected.contains(cat))
            });
        }
    }
    Ok(())
}
