//! The `campfire_start` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0186

pub trait Hooks {
    /// An item was placed on a campfire at `pos` and began cooking.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_campfire_start",
        native = "fn native_campfire_start(&mut self, pos: [i32; 3], item: &str)"
    )]
    fn campfire_start();
}
