//! The `customize_chat_overlay` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The original `minecraft:chat` body is drawing unfocused chat (NeoForge's
    /// `CustomizeGuiOverlayEvent.Chat` in `Hud.extractChat`). `x` and `y` start at the chat's
    /// bottom-left corner in GUI pixels (0 and GUI height minus 40); change them to move the
    /// background chat. `gui_width` and `gui_height` are the scaled GUI dimensions.
    /// A hidden or skipped original body does not fire this hook. It never moves focused chat:
    /// use `screen_render_pre` or `screen_render_post` for custom paint on the chat screen.
    /// Client-side.
    #[hook(
        dispatch = "flat",
        side = "client",
        version = 2,
        contract = "listen",
        neoforge = "CustomizeGuiOverlayEvent.Chat",
        cancellable = false
    )]
    fn customize_chat_overlay(gui_width: i32, gui_height: i32, x: &mut i32, y: &mut i32);
}
