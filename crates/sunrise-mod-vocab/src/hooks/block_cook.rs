//! The `block_cook` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0172

pub trait Hooks {
    /// A block finished cooking `item` into `result` at `pos`.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_block_cook",
        native = "fn native_block_cook(&mut self, pos: [i32; 3], item: &str, result: &str)"
    )]
    fn block_cook();
}
