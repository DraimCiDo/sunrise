//! The `gui_layer_pre` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0144

pub trait Hooks {
    /// Before a vanilla or mod HUD layer (`RenderGuiLayerEvent.Pre`). Draw below the
    /// layer through `event.draw`, or return `Flow::Cancel` to skip its entire body
    /// (including its render bookkeeping) and `gui_layer_post`. Gameplay ticks are unaffected.
    /// Each slot fires this once with its original ID, even when the vanilla body would not
    /// paint. This includes F1 and third-person view for the crosshair.
    /// To change a layer's body rather than veto the whole slot, wrap or replace it
    /// at startup in `register_gui_layers`.
    ///
    /// The vanilla anchors, in HUD order, are `minecraft:crosshair`, `hotbar`,
    /// `effects`, `boss_overlay`, `player_health` (hearts), `armor_level`, `food_level`,
    /// `vehicle_health`, `air_level`, `contextual_info_bar` (jump, locator or experience
    /// bar, with the level number), `selected_item_name`, `sleep_overlay`,
    /// `overlay_message` (action bar), `title`, `scoreboard_sidebar`, `subtitle_overlay`,
    /// `chat` and `tab_list`. F1 and other vanilla eligibility checks belong to the original
    /// bodies, not to slot dispatch. A canceled `render_gui_pre` skips every slot. The chat
    /// slot is for background chat; focused chat belongs to the chat screen. The camera overlays
    /// and debug screen are not HUD layers. The selected item name and action bar keep vanilla's
    /// fixed positions; adding bars doesn't move them. Cancel and redraw the
    /// corresponding layer if it needs to move.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_gui_layer_pre",
        receiver = "read",
        dispatch = "flat",
        side = "client",
        version = 3,
        contract = "cancel",
        out = "canceled",
        neoforge = "RenderGuiLayerEvent.Pre",
        cancellable = true
    )]
    fn gui_layer_pre(layer: &str, draw: &mut dyn DrawContext, canceled: &mut bool);
}
