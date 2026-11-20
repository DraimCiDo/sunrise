//! The `player_enchant_item` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A server player enchanted `item` at an enchanting table
    /// (`EnchantmentMenu.clickMenuButton`): `enchantments` are the ones it was
    /// given, in order, as (id, level). `item` is the enchanted stack (an
    /// enchanted book for a book). Fires after the levels are paid, before
    /// `enchant_item`. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "PlayerEnchantItemEvent",
        cancellable = false,
        native = "fn native_player_enchant_item(&mut self, player: u128, player_entity_id: i32, item: &ItemStack, enchantments: &[(std::sync::Arc<str>, u32)])"
    )]
    fn player_enchant_item();
}
