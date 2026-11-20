//! The `entity_shoot_bow` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0206

pub trait Hooks {
    /// A player bow or crossbow launched an arrow with `force`, after the
    /// projectile entered the level.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_entity_shoot_bow",
        native = "fn native_entity_shoot_bow(&mut self, player: u128, name: &str, projectile: u32, force: f32)"
    )]
    fn entity_shoot_bow();
}
