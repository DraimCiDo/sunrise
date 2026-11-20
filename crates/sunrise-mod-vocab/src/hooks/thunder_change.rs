//! The `thunder_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0335

pub trait Hooks {
    /// The server changed its thunder state through `/weather`. Set `cancel`
    /// to keep the previous thunder state and timer. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_thunder_change",
        native = "fn native_thunder_change(&mut self, dimension: &str, thundering: bool, cancel: &mut bool)"
    )]
    fn thunder_change();
}
