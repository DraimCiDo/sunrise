//! stable-line: content-declarations
//! The Sunrise blockstate-layer field shared by the pack overlay and loader.
use serde_json::Value;

/// The key of a stacked blockstate file: its definitions, bottom first, each
/// as the pack wrote it.
pub const BLOCKSTATE_LAYERS: &str = "sunrise:layers";

/// The definitions a blockstate file holds, bottom first: the layers of a
/// file the pack overlay stacked (`packs::BLOCKSTATE_LAYERS`), else the
/// file itself.
pub(crate) fn definition_layers(text: &str) -> std::result::Result<Vec<String>, String> {
    if !text.contains(crate::packs::BLOCKSTATE_LAYERS) {
        return Ok(vec![text.to_owned()]);
    }
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match value.get(crate::packs::BLOCKSTATE_LAYERS) {
        Some(Value::Array(layers)) => Ok(layers.iter().map(Value::to_string).collect()),
        _ => Ok(vec![text.to_owned()]),
    }
}
