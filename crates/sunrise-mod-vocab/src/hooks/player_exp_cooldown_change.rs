//! The `player_exp_cooldown_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0288

pub trait Hooks {
    /// A server player's XP orb pickup cooldown changed to `ticks`. It is
    /// reported after the value changes. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_exp_cooldown_change",
        native = "fn native_player_exp_cooldown_change(&mut self, player: u128, name: &str, ticks: u32)"
    )]
    fn player_exp_cooldown_change();
}
