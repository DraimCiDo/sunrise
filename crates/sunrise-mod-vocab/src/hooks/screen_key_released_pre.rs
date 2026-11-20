//! The `screen_key_released_pre` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0120

pub trait Hooks {
    /// Before an open screen handles a named key's release (same screen gate as key presses).
    /// Cancel bypasses screen handling; physical keys and mapped keyboard hold/toggle state release.
    #[hook(
        side = "client",
        contract = "cancel",
        out = "canceled",
        neoforge = "ScreenEvent.KeyReleased.Pre",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_screen_key_released_pre",
        native = "fn native_screen_key_released_pre(&self, screen: &ScreenView<'_>, key: &str, modifiers: u32, canceled: &mut bool)"
    )]
    fn screen_key_released_pre();
}
