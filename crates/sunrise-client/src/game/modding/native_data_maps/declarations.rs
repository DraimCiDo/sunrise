/// The folder a registry's data maps are under (`DataMapLoader`): its path for
/// a `minecraft` registry, `<namespace>/<path>` for another.
fn folder(registry: &str) -> String {
    match registry.split_once(':') {
        Some(("minecraft", path)) => path.to_owned(),
        Some((namespace, path)) => format!("{namespace}/{path}"),
        None => registry.to_owned(),
    }
}

/// A file's value for an entry: the data itself, or the wrapper
/// `{"value": ..., "replace": false}` (`DataMapEntry`'s codec), which may also
/// carry `neoforge:conditions` (`WithConditions`): a value whose conditions
/// fail is no value. `replace` only matters to mergers, which types don't have
/// yet: a newer value overrides.
fn entry_value<'a>(
    raw: &'a Value,
    conditions: &sunrise_world::conditions::LoadConditions,
    path: &str,
) -> Result<Option<&'a Value>, String> {
    if let Value::Object(map) = raw
        && let Some(value) = map.get("value")
        && map.iter().all(|(k, v)| {
            k == "value" || k == "neoforge:conditions" || (k == "replace" && v.is_boolean())
        })
    {
        return Ok(conditions.allows_file_isolated(raw, path)?.then_some(value));
    }
    Ok((!raw.is_null()).then_some(raw))
}

/// One file over what earlier files made of the type: `replace` clears it,
/// `values` attach (a tag to every entry in it), then `remove` takes entries
/// away, as `DataMapFile` is applied. False when the file isn't a data map.
fn apply_file(
    map: &mut BTreeMap<String, Value>,
    file: &Value,
    registry: &str,
    registries: &dyn MapRegistries,
    conditions: &sunrise_world::conditions::LoadConditions,
    path: &str,
) -> Result<(), String> {
    let root = file.as_object().ok_or("not a data map object")?;
    let values = file["values"]
        .as_object()
        .ok_or("missing data map values")?;
    // Finish decoding the entire layer before replace, removals or inserts.
    let mut decoded = Vec::new();
    for (key, raw) in values {
        if let Some(value) = entry_value(raw, conditions, path)? {
            decoded.push((key, value));
        }
    }
    if root.get("replace").and_then(Value::as_bool) == Some(true) {
        map.clear();
    }
    let mut unknown = BTreeSet::new();
    // The members of a tag or the entry itself, if the registry holds it.
    let mut members = |key: &str| -> Vec<String> {
        match key.strip_prefix('#') {
            // A tag's members go through the same membership check the entry
            // does: a tag naming an id the registry doesn't hold publishes
            // nothing for it.
            Some(tag) => registries
                .tag(registry, &full_id(tag))
                .into_iter()
                .filter(|member| {
                    let held = registries.contains(registry, member);
                    if !held {
                        unknown.insert(member.clone());
                    }
                    held
                })
                .collect(),
            None if registries.contains(registry, &full_id(key)) => vec![full_id(key)],
            None => {
                unknown.insert(full_id(key));
                Vec::new()
            }
        }
    };
    for (key, value) in decoded {
        for entry in members(key) {
            map.insert(entry, value.clone());
        }
    }
    for removal in root
        .get("remove")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        // An entry or `{"key": entry}`, as `DataMapEntry.Removal` reads it.
        let key = removal.as_str().or_else(|| removal["key"].as_str());
        for entry in key.map(&mut members).unwrap_or_default() {
            map.remove(&entry);
        }
    }
    if !unknown.is_empty() {
        log::warn!(
            "Sunrise: {path}: {registry} has no {}: skipped",
            unknown.into_iter().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

impl DataMaps {
    /// The types' values from `files` (the server's tables' pack files, in
    /// pack order, mods' below the packs'), ids and tags resolved against
    /// `registries`. A type for a registry the game doesn't name, or with a
    /// bad id, is left out with a warning; a file that isn't a data map is
    /// skipped alone.
    pub(crate) fn load(
        types: &[DataMapType],
        overlay: &sunrise_world::loot::PackOverlay<'_>,
        registries: &dyn MapRegistries,
    ) -> Self {
        let mut maps: BTreeMap<String, HashMap<String, MapValues>> = BTreeMap::new();
        for ty in types {
            if !registries.known(&ty.registry) {
                log::warn!(
                    "Sunrise: data map {} is for {}, a registry the game doesn't name: left out",
                    ty.id,
                    ty.registry
                );
                continue;
            }
            let valid = |s: &str| {
                !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./".contains(c))
            };
            let ok = ty
                .id
                .split_once(':')
                .is_some_and(|(ns, path)| ns != "minecraft" && valid(ns) && valid(path));
            if !ok {
                log::warn!(
                    "Sunrise: data map type {} is not a mod's id: left out",
                    ty.id
                );
                continue;
            }
            let registered = maps.entry(ty.registry.clone()).or_default();
            if registered.contains_key(&ty.id) {
                log::warn!(
                    "Sunrise: data map {} registered twice: the first is kept",
                    ty.id
                );
                continue;
            }
            let (ns, path) = ty.id.split_once(':').unwrap_or_default();
            let directory = folder(&ty.registry);
            let wanted = format!("data_maps/{ns}:{directory}/{path}");
            let mut map = BTreeMap::new();
            if let Some((_, versions)) = overlay.files.iter().find(|(k, _)| *k == wanted) {
                for (layer, bytes) in versions.iter().enumerate() {
                    let source = format!(
                        "data/{ns}/data_maps/{directory}/{path}.json (layer {})",
                        layer + 1
                    );
                    let prepared = serde_json::from_slice::<Value>(bytes)
                        .map_err(|error| error.to_string())
                        .and_then(|file| {
                            apply_file(
                                &mut map,
                                &file,
                                &ty.registry,
                                registries,
                                overlay.conditions,
                                &source,
                            )
                        });
                    if let Err(error) = prepared {
                        log::error!(
                            "Sunrise: data map {} on {}: {source}: file skipped: {error}",
                            ty.id,
                            ty.registry
                        );
                    }
                }
            }
            registered.insert(ty.id.clone(), MapValues::Json(map));
        }
        Self {
            maps,
            ..Self::default()
        }
    }
}
