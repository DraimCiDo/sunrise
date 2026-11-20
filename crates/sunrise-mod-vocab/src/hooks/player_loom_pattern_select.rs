//! The `player_loom_pattern_select` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0297

pub trait Hooks {
    /// A server player selected a loom pattern, after the loom menu accepted the button. `pattern` is
    /// the zero-based button index. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_loom_pattern_select",
        native = "fn native_player_loom_pattern_select(&mut self, player: u128, name: &str, pattern: i32)"
    )]
    fn player_loom_pattern_select();
}
