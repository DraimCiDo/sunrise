//! stable-line: content-declarations
//! File discovery and decoding for global loot modifier declarations.

use super::{Adapter, Serializers};
use mod_api::{GlobalLootModifier, Identifier, LootModifierError, LootModifierSettings};
use std::{path::Path, sync::Arc};
use sunrise_assets::resource_source::ResourceSource;
use sunrise_world::{
    loot::global_modifiers::{AddTableModifier, DecodedModifier, ModifierFile},
    native_load_context::LoadContext,
};

impl Serializers {
    pub(super) fn decode(
        &self,
        kind: &str,
        data: &serde_json::Value,
        load: &LoadContext,
    ) -> Result<(DecodedModifier, Arc<dyn GlobalLootModifier>), String> {
        if kind == "neoforge:add_table" {
            let settings = LootModifierSettings::decode(data).map_err(|error| error.to_string())?;
            let table = data
                .get("table")
                .and_then(serde_json::Value::as_str)
                .ok_or("table: expected a loot table id")?;
            let table = Identifier::parse(table).map_err(|error| format!("table: {error}"))?;
            let decoded = DecodedModifier {
                priority: settings.priority,
                condition: settings.condition.clone(),
                apply: Arc::new(AddTableModifier {
                    table: table.to_string(),
                }),
            };
            return Ok((decoded, Arc::new(ApiAddTable { settings, table })));
        }
        let serializer = self
            .0
            .get(kind)
            .ok_or_else(|| format!("type: unknown global loot modifier serializer {kind}"))?;
        let reader = sunrise_client_render::api_context::LoadReader(load);
        let instance = serializer
            .decode(data, &reader.context())
            .map_err(|error| error.to_string())?;
        let decoded = DecodedModifier {
            priority: instance.priority().map_err(|error| error.to_string())?,
            condition: instance.condition().map_err(|error| error.to_string())?,
            apply: Arc::new(Adapter(instance.clone())),
        };
        Ok((decoded, instance))
    }
}

struct ApiAddTable {
    settings: LootModifierSettings,
    table: Identifier,
}
impl GlobalLootModifier for ApiAddTable {
    fn priority(&self) -> Result<i32, LootModifierError> {
        Ok(self.settings.priority)
    }
    fn condition(&self) -> Result<Option<serde_json::Value>, LootModifierError> {
        Ok(self.settings.condition.clone())
    }
    fn apply(
        &self,
        context: &mut dyn mod_api::LootModifierContext,
        generated: Vec<mod_api::LootStack>,
    ) -> Result<Vec<mod_api::LootStack>, LootModifierError> {
        let appended = context.roll_raw(&self.table)?;
        Ok(sunrise_world::loot::global_modifiers::append_raw(
            &generated, appended,
        ))
    }
}

/// Root resources are lowest, then passive content, native contributions and
/// the enabled packs. Readers run only after the loader selects each winning id.
pub(in crate::game) fn files(
    root: &Path,
    stack: &super::super::functions::PackStack,
    contributed: Vec<ModifierFile>,
) -> Vec<ModifierFile> {
    let mut files = Vec::new();
    for namespace in sunrise_world::json_folder::namespaces(&root.join("data")) {
        let directory = root.join("data").join(&namespace).join("loot_modifiers");
        let folder = match sunrise_world::json_folder::read(&directory) {
            Ok(folder) => folder,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                log::warn!("loot modifier folder {}: {error}", directory.display());
                continue;
            }
        };
        for (index, file) in folder.files.iter().enumerate() {
            let id = format!("{namespace}:{}", file.name);
            let path = file.path.display().to_string();
            let folder = folder.clone();
            files.push(ModifierFile {
                id,
                pack: "resources".into(),
                path,
                read: Box::new(move || {
                    folder.files[index]
                        .bytes()
                        .map(<[u8]>::to_vec)
                        .map_err(|error| error.to_string())
                }),
            });
        }
    }
    append_pack(&mut files, &stack.content.id, stack.content.source());
    files.extend(contributed);
    for pack in stack.enabled_packs() {
        append_pack(&mut files, &pack.id, pack.source());
    }
    files
}
fn append_pack(files: &mut Vec<ModifierFile>, pack: &str, source: &ResourceSource) {
    for path in source.list_under("data", &["loot_modifiers"]) {
        let Some((namespace, rest)) = path
            .strip_prefix("data/")
            .and_then(|path| path.split_once('/'))
        else {
            continue;
        };
        let Some(name) = rest
            .strip_prefix("loot_modifiers/")
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        let id = format!("{namespace}:{name}");
        let source = source.clone();
        let read_path = path.clone();
        files.push(ModifierFile {
            id,
            pack: pack.into(),
            path,
            read: Box::new(move || source.read(&read_path).map_err(|error| error.to_string())),
        });
    }
}
/// Preserve native contributions even though DataStore does not interpret this
/// independently discovered resource folder. Their existing order is priority.
pub(in crate::game) fn contributed(files: &[(String, Vec<u8>)]) -> Vec<ModifierFile> {
    files
        .iter()
        .filter_map(|(key, bytes)| {
            let id = key.strip_prefix("loot_modifiers/")?.to_owned();
            let Some((namespace, name)) = id.split_once(':') else {
                log::warn!(
                    "loot modifier {id} (native contributions): expected a namespace:path file id"
                );
                return None;
            };
            let path = format!("data/{namespace}/loot_modifiers/{name}.json");
            let bytes = bytes.clone();
            Some(ModifierFile {
                id,
                pack: "native contributions".into(),
                path,
                read: Box::new(move || Ok(bytes)),
            })
        })
        .collect()
}
