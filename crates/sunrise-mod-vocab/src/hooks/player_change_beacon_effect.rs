//! The `player_change_beacon_effect` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0282

pub trait Hooks {
    /// A server player changed a beacon's selected effects, after they were stored. Each selected
    /// effect is an `Option<EffectKey>`; None means no effect. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_change_beacon_effect",
        native = "fn native_player_change_beacon_effect(&mut self, player: u128, name: &str, pos: [i32; 3], primary: Option<EffectKey>, secondary: Option<EffectKey>)"
    )]
    fn player_change_beacon_effect();
}
