//! stable-line: content-declarations
//! Replay biome modifiers over the world's selected layers.
use super::*;

/// A world's data packs (`packs`) with the content root `root`'s biome
/// modifiers applied over them. Modifiers apply once every data
/// pack is in, to the final biomes and tags: a pack's file for a biome
/// (Terralith's `minecraft:plains`) keeps the mods' features and spawns, and
/// a pack's biome its tags put under a modifier's selector gets them too.
/// `None` when the root's biomes stand: no modifiers, or nothing declared and packs that bring
/// no biome and no tag. `declared` are mods' code modifiers (mod, file, text), applied last.
pub fn biome_modifiers_over_packs(
    root: &Path,
    packs: &sunrise_world::worldgen::overrides::WorldgenOverrides,
    declared: &[[String; 3]],
) -> Result<Option<sunrise_world::worldgen::overrides::WorldgenOverrides>> {
    let Some(bytes) = biomes::saved_bytes(root, packs, declared) else {
        return Ok(None);
    };
    let mut saved: SavedModifiers =
        serde_json::from_slice(&bytes).map_err(|e| format!("{MODIFIERS}: {e}"))?;
    let read = |name: &str| -> Result<BTreeMap<String, String>> {
        match fs::read(root.join(name)) {
            Ok(b) => Ok(serde_json::from_slice(&b).map_err(|e| format!("{name}: {e}"))?),
            Err(_) => Ok(BTreeMap::new()),
        }
    };
    let mut warnings = Vec::new();
    let mut modifiers = Vec::new();
    for [m, file, text] in saved.modifiers.iter().chain(declared) {
        modifiers.extend(biomes::Modifier::read(m, file, text, &mut warnings)?);
    }
    // What the generator reads without the modifiers over the packs.
    let packed = packs.apply("biomes", read(BIOMES)?);
    let mut start = packed.clone();
    for (id, text) in &saved.unmodified {
        if packs.entries("biomes").all(|(p, _)| p != id) {
            start.insert(id.clone(), text.clone());
        }
    }
    let mut doc = J::Obj(start.into_iter().map(|(id, t)| (id, J::Str(t))).collect());
    // Pack tag files after the root's, as the worldgen loader merges them.
    let mut biome_tags = read(BIOME_TAGS)?;
    for (id, pack, json) in packs.tags("biome_tags") {
        let tag: J = match serde_json::from_str(json) {
            Ok(tag) => tag,
            Err(e) => {
                warnings.push(format!("{pack}: tag {id}: {e}"));
                continue;
            }
        };
        let (values, remove) = (tag_list(&tag, "values"), tag_list(&tag, "remove"));
        let old = match tag.get("replace") {
            Some(J::Bool(true)) => None,
            _ => biome_tags.get(id).map(String::as_str),
        };
        let layers = [TagLayers { values, remove }];
        let merged = merge_tag(
            old,
            &layers,
            id,
            &sunrise_world::conditions::LoadConditions::new(
                saved.enabled_mods.iter().map(String::as_str),
            ),
        )?;
        biome_tags.insert(id.to_owned(), merged);
    }
    let placed = packs
        .apply("placed_features", read(PLACED_FEATURES)?)
        .into_keys()
        .collect();
    carver_registry::refresh(&mut saved, packs)?;
    saved.apply(&mut doc, &modifiers, &biome_tags, &placed, &mut warnings)?;
    for warning in warnings {
        log::warn!("Sunrise: biome modifiers over data packs: {warning}");
    }
    let mut out = packs.clone();
    if let J::Obj(entries) = doc {
        for (id, text) in entries {
            if let J::Str(text) = text
                && packed.get(&id) != Some(&text)
            {
                out.set("biomes", &id, text);
            }
        }
    }
    Ok(Some(out))
}
