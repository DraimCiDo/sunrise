//! The `player_map_filled` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0298

pub trait Hooks {
    /// A server player filled an empty map, after the filled map and its saved data were created.
    /// Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_map_filled",
        native = "fn native_player_map_filled(&mut self, player: u128, name: &str, map_id: i32)"
    )]
    fn player_map_filled();
}
