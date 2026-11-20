//! The `compute_fog_color` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0115

pub trait Hooks {
    /// Computed world fog RGB, after ordinary effects and before upload. Title panoramas skip it.
    /// Only finite RGB components in 0..=1 are accepted; alpha and distances stay unchanged.
    #[hook(
        side = "client",
        contract = "listen",
        neoforge = "ViewportEvent.ComputeFogColor",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_compute_fog_color",
        native = "fn native_compute_fog_color(&self, partial: f32, camera: [f64; 3], fog_type: mod_api::client::fog::FogType, rgb: &mut [f32; 3])"
    )]
    fn compute_fog_color();
}
