//! The `render_gui_pre` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Before the in-world HUD is drawn (NeoForge's `RenderGuiEvent.Pre`):
    /// Return `Flow::Cancel` to veto every HUD slot and its layer events, `render_gui_post`,
    /// and ordinary toasts. The chat screen and mandatory mod-error toasts are outside the veto;
    /// the player's F1 still hides the toasts.
    /// Fires before the in-world HUD pass, including under F1, but not while a server dialog,
    /// the connection screen or a loading screen covers the window. F1 hides vanilla paint,
    /// not slot dispatch.
    /// Client-side.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        version = 3,
        contract = "cancel",
        out = "canceled",
        neoforge = "RenderGuiEvent.Pre",
        cancellable = true
    )]
    fn render_gui_pre(canceled: &mut bool);
}
