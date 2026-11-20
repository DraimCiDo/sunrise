//! The `player_jump` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0293

pub trait Hooks {
    /// A guest's accepted movement made the server jump them from the ground. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's PlayerJumpEvent)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_jump",
        native = "fn native_player_jump(&mut self, player: u128, name: &str)"
    )]
    fn player_jump();
}
