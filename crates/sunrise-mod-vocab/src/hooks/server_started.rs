//! The `server_started` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0006

pub trait Hooks {
    /// The server has opened its worlds and is about to run its tick loop
    /// (NeoForge's `ServerStartedEvent`): every level is loaded, no player has
    /// joined, and the first tick has not run. Fired where `WorldOpenReady` was,
    /// on the single player server thread and on a dedicated server, once per
    /// open that succeeds (a failed open fires `WorldOpenFailed` instead).
    /// `dedicated` tells a dedicated server from the single player one.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "ServerStartedEvent",
        cancellable = false,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_server_started",
        native = "fn native_server_started(&mut self, dedicated: bool)"
    )]
    fn server_started();
}
