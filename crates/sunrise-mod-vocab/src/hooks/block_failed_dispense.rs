//! The `block_failed_dispense` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0176

pub trait Hooks {
    /// A dispenser at `pos` failed to dispense `item`. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_block_failed_dispense",
        native = "fn native_block_failed_dispense(&mut self, dimension: &str, pos: [i32; 3], item: &str)"
    )]
    fn block_failed_dispense();
}
