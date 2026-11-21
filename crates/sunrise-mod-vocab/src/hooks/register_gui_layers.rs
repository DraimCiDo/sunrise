//! The `register_gui_layers` hooks: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Register, wrap or replace a HUD layer with an owned draw callback (NeoForge's
    /// `RegisterGuiLayersEvent`). Asked once at client startup. The installed body runs
    /// between `gui_layer_pre` and `gui_layer_post` unless Pre cancels it. Use
    /// `event.layers.register(id, placement, Box::new(draw))` to insert a new slot,
    /// or `replace_layer` and `wrap_layer` to change an existing slot's body.
    /// `minecraft:player_health` is the heart slot only. Use `minecraft:air_level` as the
    /// `Above` anchor to place a layer above all status rows. Every slot dispatches during
    /// the HUD pass, even when the original body would not draw or F1 hides vanilla paint.
    /// A listener sees its pending edits; they are published only if the listener
    /// returns without a panic or phase refusal.
    #[notes = r#"## HUD layers

`Hooks::register_gui_layers` is the client registration-phase counterpart of NeoForge 26.3's
`RegisterGuiLayersEvent`. It receives a `RegistrationContext` and a `GuiLayerRegistrar` as
`event.layers`, once at client startup. Call `layers.register(id, placement, Box::new(draw))`
with a namespaced ID, a `GuiLayerPlacement` and an owned draw callback. The four placements
are `BelowAll`, `Below(id)`, `Above(id)` and `AboveAll`, as `registerBelowAll`, `registerBelow`,
`registerAbove` and `registerAboveAll`. `Below` and `Above` name a vanilla layer (the IDs
`Hooks::gui_layer_pre` lists) or a layer already registered, including in this callback's own
batch. The game publishes a completed batch in call order before invoking the next listener.
A panic or phase refusal discards only the failing listener's pending insertions, replacements
and wrappers. Invalid, reserved or duplicate IDs and unknown relative anchors are logged and
refused at the call. The client calls each installed callback as the mod that registered it.

Use `layers.replace_layer(id, Box::new(draw))` to discard an existing body, or
`layers.wrap_layer(id, Box::new(wrap))` to add an outer wrapper. Both keep the slot's ID,
order and dispatch. A replacement does not inherit eligibility checks in the body it replaces.
A wrapper uses `ctx.with_draw` to paint with the current scoped drawing context and
`ctx.draw_previous()` to run the previous body, including its own checks, at that point.
It can omit that call to skip the previous body. Both methods refuse an unknown ID.

A layer fires `Hooks::gui_layer_pre` with its ID, runs its installed body, and fires
`Hooks::gui_layer_post` once, even if that body paints nothing. No body or Post runs if Pre
cancels. Drawing a previous body does not fire layer hooks again. A callback panic disables only that callback for the
session and reports its mod through the shared error log and toast. A non-panic refusal is
reported without disabling the callback.

The status slots run in order: `minecraft:player_health`, `minecraft:armor_level`,
`minecraft:food_level`, `minecraft:vehicle_health` and `minecraft:air_level`.
F1 hides their vanilla paint, not their installed bodies or layer events. The vanilla heart,
armor, food and air bodies need a player in survival or adventure, and vanilla vehicle health
needs a mount with hearts. Custom paint can check `host.hud_mode()` if it should respect F1.
`Above("minecraft:player_health".into())` puts a new layer between hearts and armor;
use `Above("minecraft:air_level".into())` to put it above all status rows. Use
`Below("minecraft:player_health".into())` to stay below the player rows or
`Above("minecraft:vehicle_health".into())` to stay above mount hearts. No single anchor
keeps a layer both above the mount hearts and below the player rows after the reorder.

Every slot dispatches in order, including inserted slots under F1 and the crosshair in third
person. F1 only changes the original vanilla paint. A canceled `Hooks::render_gui_pre` skips
the whole layer pass. Focused chat belongs to the chat screen, not the `minecraft:chat` slot.
For a foreground pass independent of the HUD slots, use `Hooks::gui_render_post`."#]
    #[hook(
        receiver = "associated",
        dispatch = "flat",
        side = "client",
        version = 5,
        contract = "listen",
        staged_surface = "layers",
        context = "registration",
        neoforge = "RegisterGuiLayersEvent"
    )]
    fn register_gui_layers(layers: &mut dyn GuiLayerRegistrar);
}
