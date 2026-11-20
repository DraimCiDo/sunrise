//! The `time_skip` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0336

pub trait Hooks {
    /// The overworld clock is about to jump, from sleep or `/time`. Set
    /// `cancel` to keep `from`.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_time_skip",
        native = "fn native_time_skip(&mut self, dimension: &str, from: i64, to: i64, cause: &str, cancel: &mut bool)"
    )]
    fn time_skip();
}
