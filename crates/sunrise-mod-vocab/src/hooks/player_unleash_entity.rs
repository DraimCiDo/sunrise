//! The `player_unleash_entity` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0319

pub trait Hooks {
    /// A server player removed an entity's leash, after it was cleared.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_unleash_entity",
        native = "fn native_player_unleash_entity(&mut self, player: u128, name: &str, entity_id: u32)"
    )]
    fn player_unleash_entity();
}
