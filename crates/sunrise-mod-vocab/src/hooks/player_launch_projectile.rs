//! The `player_launch_projectile` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0295

pub trait Hooks {
    /// A server player launched a projectile item, after it was created.
    /// `item` is the projectile's held item.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event; Bukkit's PlayerLaunchProjectile)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_launch_projectile",
        native = "fn native_player_launch_projectile(&mut self, player: u128, name: &str, item: &str)"
    )]
    fn player_launch_projectile();
}
