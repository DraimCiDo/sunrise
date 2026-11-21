//! stable-line: content-declarations
//! Built-in loot type IDs, including the NeoForge loot predicate extensions.

use super::shapes;

pub(super) fn builtin(kind: &str, function: bool) -> bool {
    shapes::contains(kind, function)
        || (!function
            && matches!(
                kind,
                "neoforge:loot_table_id" | "neoforge:can_item_perform_ability"
            ))
}
