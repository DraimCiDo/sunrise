use super::{LootQuery, LootStack, UNKNOWN_LOOT_TABLE};

/// Recognise stock conditions before the mod-kind fallback, without drawing randomness.
pub fn condition(
    value: &serde_json::Value,
    query: &LootQuery,
    tool: Option<&LootStack>,
    default: impl Fn(&str, &str) -> bool,
) -> Option<bool> {
    match value["type"].as_str()? {
        "neoforge:loot_table_id" => Some(
            value["loot_table_id"]
                .as_str()
                .is_some_and(|id| id == query.id.borrow().as_deref().unwrap_or(UNKNOWN_LOOT_TABLE)),
        ),
        "neoforge:can_item_perform_ability" => Some(
            tool.zip(value["ability"].as_str())
                .is_some_and(|(tool, ability)| {
                    let mut holds = default(&tool.item, ability);
                    crate::native_load_hooks::loot_ability(tool, ability, &mut holds);
                    holds
                }),
        ),
        _ => None,
    }
}
