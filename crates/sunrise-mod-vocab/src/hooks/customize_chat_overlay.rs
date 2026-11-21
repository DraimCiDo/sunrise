//! The `customize_chat_overlay` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The in-game chat is about to be drawn (NeoForge's `CustomizeGuiOverlayEvent.Chat`, in
    /// `Hud.extractChat`): `x` and `y` come in as the chat's bottom-left corner in GUI pixels
    /// (0 and the GUI height minus 40, vanilla's place); move them to draw the chat elsewhere.
    /// `focused` is whether the chat screen is open, and `gui_width`/`gui_height` the scaled GUI
    /// size. Asked when the original background chat body or the focused chat screen draws the
    /// chat, not when it is hidden. Only the drawing moves: the chat screen's input box, and
    /// clicks and hovers on messages, stay where vanilla puts them, as with NeoForge.
    /// Client-side.
    #[hook(
        dispatch = "flat",
        side = "client",
        contract = "listen",
        neoforge = "CustomizeGuiOverlayEvent.Chat",
        cancellable = false
    )]
    fn customize_chat_overlay(
        gui_width: i32,
        gui_height: i32,
        focused: bool,
        x: &mut i32,
        y: &mut i32,
    );
}
