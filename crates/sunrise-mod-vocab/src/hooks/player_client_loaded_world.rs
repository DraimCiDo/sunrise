//! The `player_client_loaded_world` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0283

pub trait Hooks {
    /// A server player finished loading the world around them after joining, respawning or changing
    /// dimension. The guest's state is not swapped in. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's PlayerClientLoadedWorld)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_client_loaded_world",
        native = "fn native_player_client_loaded_world(&mut self, player: u128, name: &str)"
    )]
    fn player_client_loaded_world();
}
