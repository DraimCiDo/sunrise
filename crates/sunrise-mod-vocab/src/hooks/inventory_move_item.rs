//! The `inventory_move_item` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0228

pub trait Hooks {
    /// A hopper is about to move one item between containers, a composter's
    /// bone meal included. Set `cancel` to leave it in its source slot. A chest
    /// boat or minecart is reported at the block it is in.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        host = "read",
        neoforge = "none (NeoForge has no such event; Bukkit's InventoryMoveItemEvent)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_inventory_move_item",
        native = "fn native_inventory_move_item(&self, source: [i32; 3], destination: [i32; 3], item: &str, cancel: &mut bool)"
    )]
    fn inventory_move_item();
}
