//! Load-scoped registration and per-candidate condition lookup.
use super::Game;
use mod_api::__private::game::RegistrationContextNew;
use std::{collections::BTreeMap, sync::Arc};

/// Read declared registry folders with the same scanner as the live registry loader.
/// Convert its resource paths to the content keys candidate_registries consumes.
fn candidate_registry_files(
    stack: &super::functions::PackStack,
) -> Result<Vec<(String, Vec<Vec<u8>>)>, String> {
    let declarations = mod_api::__private::datapack_registry_declarations();
    if declarations.is_empty() {
        return Ok(Vec::new());
    }
    let keys: Vec<_> = declarations
        .iter()
        .filter_map(|declaration| declaration.key.split_once(':'))
        .collect();
    let packs: Vec<_> = std::iter::once(&stack.content)
        .chain(stack.enabled_packs())
        .collect();
    // Admission is evaluated below, against the load-scoped conditions; no fallback.
    let files = sunrise_client::datapack::registry_files(&packs, &keys, |_, _| true)
        .map_err(|error| format!("{error:?}"))?
        .into_iter()
        .flat_map(|(file, versions)| {
            let bytes: Vec<Vec<u8>> = versions.into_iter().map(|(_, bytes)| bytes).collect();
            // A file under nested declared folders is an entry of each, as the live loader reads it.
            let entries: Vec<String> = file
                .strip_prefix("data/")
                .and_then(|rest| rest.split_once('/'))
                .map(|(namespace, path)| {
                    keys.iter()
                        .filter_map(|(ns, key)| {
                            let folder = format!("{ns}/{key}");
                            path.strip_prefix(&folder)
                                .and_then(|entry| entry.strip_prefix('/'))
                                .and_then(|entry| entry.strip_suffix(".json"))
                                .map(|entry| format!("{folder}/{namespace}:{entry}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            entries.into_iter().map(move |key| (key, bytes.clone()))
        })
        .collect();
    Ok(files)
}

/// Every registry's entries as the candidate load holds them, which `neoforge:registered` reads:
/// the built-in ones the prepared root exports (`command-registries.json`, never what an earlier
/// load added), and the entries the candidate's pack `files` add to a registry that takes them
/// (vanilla's, or a datapack registry a mod declared). So an id a pack adds counts on its first
/// load, and one a pack no longer has stops counting on the next.
fn candidate_registries<'a>(
    root: &std::path::Path,
    files: impl IntoIterator<Item = &'a (String, Vec<Vec<u8>>)>,
    admission: &sunrise_world::conditions::LoadConditions,
) -> BTreeMap<String, std::collections::BTreeSet<String>> {
    let export: serde_json::Value = std::fs::read(root.join("command-registries.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut registries: BTreeMap<String, std::collections::BTreeSet<String>> =
        export["by_registry"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(registry, entry)| {
                let ids = entry["ids"].as_array().into_iter().flatten();
                let ids = ids.filter_map(serde_json::Value::as_str).map(str::to_owned);
                (registry.clone(), ids.collect())
            })
            .collect();
    for declaration in mod_api::__private::datapack_registry_declarations().iter() {
        registries.entry(declaration.key.clone()).or_default();
    }
    // A pack file's key is `<folder>/<namespace>:<path>`: `worldgen/biome/test:dunes` for
    // `minecraft:worldgen/biome`, `mymod/spell/test:fire` for a mod's `mymod:spell`.
    let folder = |registry: &str| match registry.split_once(':') {
        Some(("minecraft", path)) => path.to_owned(),
        Some((namespace, path)) => format!("{namespace}/{path}"),
        None => registry.to_owned(),
    };
    let by_folder: BTreeMap<String, String> = registries
        .keys()
        .map(|registry| (folder(registry), registry.clone()))
        .collect();
    for (key, versions) in files {
        let Some(slash) = key.find(':').and_then(|colon| key[..colon].rfind('/')) else {
            continue;
        };
        let Some(registry) = by_folder.get(&key[..slash]) else {
            continue;
        };
        // The winning registry file, if its own conditions hold: an entry
        // they turn off is not there. Non-registry files belong to their loaders.
        // The path the loaders test it under (`data/<namespace>/<folder>/<path>.json`).
        let file_path = key[slash + 1..]
            .split_once(':')
            .map(|(namespace, path)| format!("data/{namespace}/{}/{path}.json", &key[..slash]))
            .unwrap_or_else(|| key.clone());
        let admitted = versions.last().is_some_and(|bytes| {
            let Ok(file) = serde_json::from_slice::<serde_json::Value>(bytes) else {
                return true;
            };
            let payload = file.get("neoforge:conditions").map(ToString::to_string);
            payload.is_none_or(|payload| {
                payload.contains("\"neoforge:registered\"")
                    || payload.contains("\"neoforge:tag_empty\"")
                    // An admission probe is not the entry's loader. A codec
                    // error omits it here; the owning loader will report it.
                    || admission.allows_file_isolated(&file, &file_path).unwrap_or(false)
            })
        });
        if !admitted {
            continue;
        }
        let id = key[slash + 1..].to_owned();
        registries
            .get_mut(registry)
            .expect("named above")
            .insert(id);
    }
    registries
}


#[cfg(test)]
#[path = "load_conditions/startup_tests.rs"]
mod startup_tests;

#[cfg(test)]
#[path = "load_conditions/tests.rs"]
mod tests;


#[cfg(test)]
#[path = "load_conditions/reload_tests.rs"]
mod reload_tests;
