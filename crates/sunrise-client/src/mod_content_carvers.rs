//! stable-line: content-declarations
//! Effective configured-carver registries for biome modifiers.
use super::{J, Result, SavedModifiers, biomes::Registry};
use sunrise_world::worldgen::overrides::WorldgenOverrides;

pub(super) fn expand(doc: &mut J, registry: &Registry<'_>) -> Result<()> {
    if let Some(J::Str(tag)) = doc.get("carvers")
        && tag.starts_with('#')
    {
        let selector = serde_json::Value::String(tag.clone());
        let ids = registry.resolve(&selector)?;
        doc.set("carvers", J::Arr(ids.into_iter().map(J::Str).collect()));
    }
    Ok(())
}

pub(super) fn refresh(saved: &mut SavedModifiers, packs: &WorldgenOverrides) -> Result<()> {
    saved
        .carver_ids
        .extend(packs.entries("carvers").map(|(id, _)| id.to_owned()));
    for (id, _, text) in packs.tags("carver_tags") {
        let tag: J = serde_json::from_str(text)?;
        let (values, remove) = (
            super::tag_list(&tag, "values"),
            super::tag_list(&tag, "remove"),
        );
        let old = if matches!(tag.get("replace"), Some(J::Bool(true))) {
            None
        } else {
            saved.carver_tags.get(id).map(String::as_str)
        };
        let layers = [super::TagLayers { values, remove }];
        let merged = super::merge_tag(
            old,
            &layers,
            id,
            &sunrise_world::conditions::LoadConditions::new(
                saved.enabled_mods.iter().map(String::as_str),
            ),
        )?;
        saved.carver_tags.insert(id.to_owned(), merged);
    }
    Ok(())
}

#[cfg(test)]
#[path = "mod_content_carvers_tests.rs"]
mod tests;
