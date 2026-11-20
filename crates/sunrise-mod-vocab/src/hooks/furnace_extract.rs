//! The `furnace_extract` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0221

pub trait Hooks {
    /// A server player took an output from a furnace at `pos`; `item` is the output item id and
    /// `count` is how many were taken. The player's state is not swapped in.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_furnace_extract",
        native = "fn native_furnace_extract(&mut self, player: u128, name: &str, pos: [i32; 3], item: &str, count: u32)"
    )]
    fn furnace_extract();
}
