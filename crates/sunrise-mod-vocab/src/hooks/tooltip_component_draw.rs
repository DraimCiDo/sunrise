//! The `tooltip_component_draw` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0077

pub trait Hooks {
    /// Render the measured component at origin, inside the tooltip background.
    /// Stack is the component's data. Use native_sprite/native_fill/native_text;
    /// Size is the whole tooltip content box: the widest component and the sum
    /// of component heights (minus 2 normally; no subtraction for an item
    /// tooltip with multiple components). Origin is this component's running
    /// top and the box's left edge. All text is drawn before any images.
    #[hook(
        side = "client",
        contract = "listen",
        scope = "item",
        key = "stack",
        neoforge = "RegisterClientTooltipComponentFactoriesEvent",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_tooltip_component_draw",
        native = "fn native_tooltip_component_draw(&self, stack: &ItemStack, draw: &mut HudBatch, origin: [f32; 2], size: [f32; 2])"
    )]
    fn tooltip_component_draw();
}
