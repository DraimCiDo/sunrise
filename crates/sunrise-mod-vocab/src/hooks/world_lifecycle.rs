//! The `world_lifecycle` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0003

pub trait Hooks {
    /// The world's phases, read-only; `error_text` carries the engine's
    /// own open or close failure text, empty otherwise. Server-side. A hard close
    /// failure fires `WorldCloseFailed`; individually reported record failures do
    /// not prevent `WorldCloseFinal`, even when closing after a crash.
    #[hook(
        side = "server",
        contract = "listen",
        version = 2,
        neoforge = "ServerStartingEvent, ServerStartedEvent, ServerStoppingEvent, ServerStoppedEvent"
    )]
    fn world_lifecycle();
}
