//! The `health_hud` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0056

pub trait Hooks {
    /// In the original `minecraft:player_health` body, when the player's hearts draw.
    /// `inputs` carries the HUD state and `wi`/`hi` the GUI size. Setting `draws` replaces
    /// just the heart paint. The original slot still reserves its vanilla column height,
    /// so the separate armor layer stays above the hearts. An unset answer paints vanilla
    /// hearts. Canceling the slot or replacing it without delegating skips this hook and
    /// reserves no height; F1 also skips the original body.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_health_hud",
        dispatch = "flat",
        side = "client",
        version = 2,
        contract = "replace",
        out = "draws",
        neoforge = "RenderGuiLayerEvent.Pre"
    )]
    fn health_hud(
        inputs: &mod_api::client::hud_layout::HealthInputs,
        wi: i32,
        hi: i32,
        draws: &mut Option<Vec<GuiDraw>>,
    );
}
