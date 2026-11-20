//! The `creeper_ignite` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0190

pub trait Hooks {
    /// A player ignited a creeper with an item.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_creeper_ignite",
        native = "fn native_creeper_ignite(&mut self, entity: u32, player: u128, item: &str)"
    )]
    fn creeper_ignite();
}
