//! The `render_fog` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0119

pub trait Hooks {
    /// Final world fog, after the computed color hook, ordinary calculations and Sunrise's fog
    /// option, before upload. Title panoramas skip it. Invalid color or distance values retain the
    /// original whole fog.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "ViewportEvent.RenderFog",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_render_fog",
        native = "fn native_render_fog(&self, partial: f32, camera: [f64; 3], fog_type: mod_api::client::fog::FogType, fog: &mut mod_api::client::fog::FogData)"
    )]
    fn render_fog();
}
