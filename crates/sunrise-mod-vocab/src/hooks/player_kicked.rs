//! The `player_kicked` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0294

pub trait Hooks {
    /// A network or fake guest was kicked, after the hub recorded the rendered `reason`. The guest's state is
    /// swapped in unless they were already removed in the same poll.
    #[hook(
        side = "server",
        version = 2,
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's PlayerKickEvent)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_kicked",
        native = "fn native_player_kicked(&mut self, player: u128, name: &str, reason: &str)"
    )]
    fn player_kicked();
}
