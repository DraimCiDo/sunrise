//! The `player_deep_sleep` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0284

pub trait Hooks {
    /// A server player has slept for 100 ticks in a bed and now counts as deeply asleep for the
    /// night-skip check.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_deep_sleep",
        native = "fn native_player_deep_sleep(&mut self, player: u128, name: &str)"
    )]
    fn player_deep_sleep();
}
