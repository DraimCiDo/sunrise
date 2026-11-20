//! The `client_services` declarations reconciled with the fork API.

pub trait Hooks {
    /// Before an actual connection is discarded, not on a dimension change.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "ClientPlayerNetworkEvent.LoggingOut",
        native = "fn native_client_disconnecting(&self)"
    )]
    fn client_disconnecting();
    /// The normal menu blur request, before finite/range validation. None disables it.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "none (NeoForge has no menu blur event)",
        native = "fn native_menu_background(&self, radius: &mut Option<f32>)"
    )]
    fn menu_background();
}
