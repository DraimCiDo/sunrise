//! The `player_elytra_boost` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0287

pub trait Hooks {
    /// A server player used a firework rocket to boost while gliding, after the rocket was launched
    /// and consumed. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_elytra_boost",
        native = "fn native_player_elytra_boost(&mut self, player: u128, name: &str)"
    )]
    fn player_elytra_boost();
}
