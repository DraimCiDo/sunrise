use super::{
    DecodedModifier, GlobalLootModifiers, ModifierDiagnostic, ModifierFile, PreparedModifier,
};
use crate::conditions::LoadConditions;
use serde_json::Value;
use std::collections::BTreeMap;
use sunrise_core::identifier::Identifier;

impl GlobalLootModifiers {
    /// Files arrive in ascending pack priority. Select a winner before parsing
    /// or evaluating its load conditions; never fall back to a losing file.
    pub fn load(
        files: impl IntoIterator<Item = ModifierFile>,
        conditions: &LoadConditions,
        decode: impl Fn(&str, &str, &Value) -> Result<DecodedModifier, String>,
        mut report: impl FnMut(ModifierDiagnostic),
    ) -> Self {
        let mut selected = BTreeMap::new();
        for file in files {
            selected.insert(file.id.clone(), file);
        }
        let mut instances = Vec::new();
        for (_, file) in selected {
            let prepared = (|| {
                Identifier::parse(&file.id).map_err(|error| format!("id: {error}"))?;
                let bytes = (file.read)().map_err(|error| format!("read: {error}"))?;
                let data: Value =
                    serde_json::from_slice(&bytes).map_err(|error| format!("JSON: {error}"))?;
                if !conditions.allows_file_isolated(&data, &file.path)? {
                    log::debug!(
                        "loot modifier {} (pack {}, file {}) skipped by load conditions",
                        file.id,
                        file.pack,
                        file.path
                    );
                    return Ok(None);
                }
                let kind = data
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or("type: expected a serializer id")?;
                let kind = Identifier::parse(kind)
                    .map_err(|error| format!("type: {error}"))?
                    .to_string();
                let decoded = decode(&file.id, &kind, &data)?;
                Ok::<_, String>(Some(PreparedModifier {
                    id: file.id.clone(),
                    kind,
                    decoded,
                }))
            })();
            match prepared {
                Ok(Some(instance)) => instances.push(instance),
                Ok(None) => {}
                Err(reason) => report(ModifierDiagnostic {
                    id: file.id,
                    pack: file.pack,
                    path: file.path,
                    reason,
                }),
            }
        }
        instances.sort_by(|a, b| {
            b.decoded
                .priority
                .cmp(&a.decoded.priority)
                .then_with(|| a.id.cmp(&b.id))
        });
        Self { instances }
    }
}
