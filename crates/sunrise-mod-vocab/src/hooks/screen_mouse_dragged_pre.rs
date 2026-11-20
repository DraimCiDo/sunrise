//! The `screen_mouse_dragged_pre` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0127

pub trait Hooks {
    /// Before a real held-button movement reaches screen drag handlers.
    /// `screen.mouse` is the current GUI position; `delta` is its GUI-space movement.
    /// Cancel keeps the new cursor position but skips all screen drag handlers.
    #[hook(
        side = "client",
        contract = "cancel",
        out = "canceled",
        neoforge = "ScreenEvent.MouseDragged.Pre",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_screen_mouse_dragged_pre",
        native = "fn native_screen_mouse_dragged_pre(&self, screen: &ScreenView<'_>, button: i32, delta: [f32; 2], modifiers: u32, canceled: &mut bool)"
    )]
    fn screen_mouse_dragged_pre();
}
