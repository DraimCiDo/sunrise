//! The `sculk_bloom` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0330

pub trait Hooks {
    /// A sculk catalyst at `pos` has begun blooming after taking experience.
    /// The catalyst block state has changed. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_sculk_bloom",
        native = "fn native_sculk_bloom(&mut self, dimension: &str, pos: [i32; 3])"
    )]
    fn sculk_bloom();
}
