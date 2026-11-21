//! stable-line: content-declarations
//! Recipe ingredient shapes at the network projection's input boundary.
use serde_json::Value;

/// An ingredient object's flat item list: the shape NeoForge's standard
/// kinds resolve to, or a brewing predicate's `item`. `None`: the object is
/// a kind the loader does not know, a mod's own.
pub(super) fn flat(ing: &Value, items: &sunrise_client::items::Items) -> Option<Value> {
    if !ing.is_object() {
        return None;
    }
    let value = if ing
        .get("type")
        .or_else(|| ing.get("neoforge:ingredient_type"))
        .is_some()
    {
        ing
    } else {
        // Only an untyped brewing wrapper gives `item` a structural meaning.
        ing.get("item").unwrap_or(ing)
    };
    let parsed = sunrise_client::crafting::Ingredient::parse(value, items)?;
    Some(Value::Array(
        parsed
            .order
            .iter()
            .map(|i| Value::String(i.clone()))
            .collect(),
    ))
}
