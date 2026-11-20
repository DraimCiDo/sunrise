//! The `weather_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0341

pub trait Hooks {
    /// The server changed its weather parameters. `raining` and `thundering`
    /// are the requested final states. Set `cancel` to keep the previous
    /// weather parameters. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_weather_change",
        native = "fn native_weather_change(&mut self, dimension: &str, raining: bool, thundering: bool, cancel: &mut bool)"
    )]
    fn weather_change();
}
