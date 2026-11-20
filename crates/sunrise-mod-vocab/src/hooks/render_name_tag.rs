//! The `render_name_tag` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Name tag visibility and optional replacement text.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "RenderNameTagEvent",
        cancellable = false,
        native = "fn native_render_name_tag(&self, entity_id: u32, kind: &str, vanilla_visible: bool, visible: &mut bool, text: &mut Option<String>)"
    )]
    fn render_name_tag();
}
