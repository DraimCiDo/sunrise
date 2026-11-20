//! The `potion_splash` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0322

pub trait Hooks {
    /// A thrown splash or lingering potion applied its hit effects.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_potion_splash",
        native = "fn native_potion_splash(&mut self, item: &str, pos: [f64; 3])"
    )]
    fn potion_splash();
}
