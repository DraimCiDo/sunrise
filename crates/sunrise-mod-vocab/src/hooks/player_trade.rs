//! The `player_trade` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0318

pub trait Hooks {
    /// A server player completed a villager or wandering trader trade. `entity_id` is the merchant
    /// network id. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_trade",
        native = "fn native_player_trade(&mut self, player: u128, name: &str, entity_id: u32)"
    )]
    fn player_trade();
}
