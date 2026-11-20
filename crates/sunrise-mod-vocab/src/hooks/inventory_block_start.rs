//! The `inventory_block_start` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0227

pub trait Hooks {
    /// A server player is about to open an inventory supplied by a block at
    /// `pos`. Set `cancel` to leave the inventory closed. The player's state
    /// is not swapped in.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_inventory_block_start",
        native = "fn native_inventory_block_start(&mut self, player: u128, name: &str, pos: [i32; 3], cancel: &mut bool)"
    )]
    fn inventory_block_start();
}
