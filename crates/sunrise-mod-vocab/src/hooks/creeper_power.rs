//! The `creeper_power` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0191

pub trait Hooks {
    /// Lightning charged a creeper.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_creeper_power",
        native = "fn native_creeper_power(&mut self, entity: u32, lightning: [f64; 3])"
    )]
    fn creeper_power();
}
