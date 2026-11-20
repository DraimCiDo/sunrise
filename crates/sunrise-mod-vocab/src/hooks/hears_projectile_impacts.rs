//! The `hears_projectile_impacts` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0142

pub trait Hooks {
    /// Whether a mod hears projectiles' impacts (`native_projectile_impact`):
    /// set `hears`. Asked once a server tick before the projectiles fly; only
    /// then does the tick keep the copy of each flying projectile that a
    /// canceled impact flies on from, so a game without such a mod pays
    /// nothing. Server-side.
    #[hook(
        side = "server",
        contract = "collect",
        out = "hears",
        host = "read",
        neoforge = "none (Sunrise-only; NeoForge has no counterpart); an opt-in gate for a hook",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_hears_projectile_impacts",
        native = "fn native_hears_projectile_impacts(&self, hears: &mut bool)"
    )]
    fn hears_projectile_impacts();
}
