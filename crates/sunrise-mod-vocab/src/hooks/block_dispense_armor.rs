//! The `block_dispense_armor` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0174

pub trait Hooks {
    /// A dispenser equipped `item` on the first eligible `target` in front
    /// of its block at `pos`. `target` is `player`, `mob` or `armor_stand`.
    /// The equip, item consumption, sound and game event have completed.
    /// Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_block_dispense_armor",
        native = "fn native_block_dispense_armor(&mut self, dimension: &str, pos: [i32; 3], item: &str, target: &str)"
    )]
    fn block_dispense_armor();
}
