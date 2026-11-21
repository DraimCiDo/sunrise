/// A tag file's `key` list (`values`, `remove`); empty where it has none.
pub(super) fn tag_list<'a>(tag: &'a J, key: &str) -> &'a [J] {
    match tag.get(key) {
        Some(J::Arr(list)) => list,
        _ => &[],
    }
}

/// `vanilla`'s tag file over what the mods made of it: the mods' layers join
/// its `values` as ordered entries, a removal carried as `{"id": ..., "remove":
/// true}` (`TagLoader.EntryWithSource.remove`), so the folder walks adds and
/// removals in the mods' load order instead of all adds before all removals.
pub(super) fn merge_tag(
    text: Option<&str>,
    layers: &[TagLayers<'_>],
    what: &str,
    _conditions: &sunrise_world::conditions::LoadConditions,
) -> Result<String> {
    let mut doc = match text {
        Some(t) => serde_json::from_str(t).map_err(|e| format!("{what}: {e}"))?,
        None => J::Obj(vec![("values".into(), J::Arr(Vec::new()))]),
    };
    // Tags do not evaluate load conditions.
    doc.remove("neoforge:conditions");
    // The vanilla file's own `remove` entries apply right after its values,
    // before the mods' layers: they join `values` as removal objects.
    let vanilla_remove = match doc.get("remove") {
        Some(J::Arr(remove)) => Some(remove.clone()),
        _ => None,
    };
    doc.remove("remove");
    let Some(J::Arr(values)) = doc.get_mut("values") else {
        return Err(format!("{what}: no values").into());
    };
    if let Some(remove) = vanilla_remove {
        for r in &remove {
            let removal = match r {
                J::Str(id) => J::Obj(vec![
                    ("id".into(), J::Str(id.clone())),
                    ("remove".into(), J::Bool(true)),
                ]),
                other => {
                    let mut entry = other.clone();
                    entry.set("remove", J::Bool(true));
                    entry
                }
            };
            values.push(removal);
        }
    }
    for layer in layers {
        for v in layer.values {
            values.push(v.clone());
        }
        for r in layer.remove {
            let removal = match r {
                J::Str(id) => J::Obj(vec![
                    ("id".into(), J::Str(id.clone())),
                    ("remove".into(), J::Bool(true)),
                ]),
                other => {
                    let mut entry = other.clone();
                    entry.set("remove", J::Bool(true));
                    entry
                }
            };
            values.push(removal);
        }
    }
    doc.remove("remove");
    Ok(serde_json::to_string_pretty(&doc)?)
}

impl Known {
    /// Whether tag `file` keeps entry `v`: a known id or an existing tag,
    /// for block, item and biome tags (other registries are kept as they
    /// are). The worldgen loader refuses a biome tag naming what it lacks
    /// (another mod's biomes, a loader's tags), so those entries go.
    fn keeps(&self, file: &str, v: &J, warnings: &mut Vec<String>) -> bool {
        let id = match v {
            J::Str(s) => s.as_str(),
            J::Obj(_) => match v.get("id") {
                Some(J::Str(s)) => s.as_str(),
                _ => return true,
            },
            _ => return true,
        };
        let Some((ns, rest)) = file
            .strip_prefix("data/")
            .and_then(|r| r.split_once("/tags/"))
        else {
            return true;
        };
        let Some((kind, tag_path)) = rest.strip_suffix(".json").and_then(|rest| {
            rest.strip_prefix("worldgen/biome/")
                .map(|path| ("worldgen/biome", path))
                .or_else(|| rest.split_once('/'))
        }) else {
            return true;
        };
        let ids = match kind {
            "block" => &self.blocks,
            "item" => &self.items,
            "worldgen/biome" => &self.biomes,
            _ => return true,
        };
        match id.strip_prefix('#') {
            Some(tag) => {
                let (tns, path) = tag.split_once(':').unwrap_or(("minecraft", tag));
                let rel = format!("data/{tns}/tags/{kind}/{path}.json");
                if self.tag_files.contains(&rel) {
                    return true;
                }
                match kind {
                    "worldgen/biome" => self.biome_tags.contains(&format!("{tns}:{path}")),
                    _ if tns != "minecraft" => false,
                    _ => {
                        let known = self
                            .vanilla_tags
                            .get(kind)
                            .is_none_or(|tags| tags.contains(path));
                        if !known
                            && v.get("required") != Some(&J::Bool(false))
                            && v.get("remove") != Some(&J::Bool(true))
                        {
                            warnings.push(format!(
                                "{file}: skipped {id} in tag {ns}:{tag_path}: no such tag in the base or installed mods"
                            ));
                        }
                        known
                    }
                }
            }
            None => ids.contains(id),
        }
    }
}
