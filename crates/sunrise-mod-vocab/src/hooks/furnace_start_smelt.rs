//! The `furnace_start_smelt` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0223

pub trait Hooks {
    /// A furnace at `pos` began cooking `item` (its cooking timer advanced
    /// from zero). Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_furnace_start_smelt",
        native = "fn native_furnace_start_smelt(&mut self, pos: [i32; 3], item: &str)"
    )]
    fn furnace_start_smelt();
}
