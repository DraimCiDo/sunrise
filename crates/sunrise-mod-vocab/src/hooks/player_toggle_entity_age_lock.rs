//! The `player_toggle_entity_age_lock` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0316

pub trait Hooks {
    /// A server player toggled an ageable mob's age lock with a golden
    /// dandelion, after the lock and item use changed.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_toggle_entity_age_lock",
        native = "fn native_player_toggle_entity_age_lock(&mut self, player: u128, name: &str, entity_id: u32, locked: bool)"
    )]
    fn player_toggle_entity_age_lock();
}
