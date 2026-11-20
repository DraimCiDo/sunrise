//! The `world_border_center_changed` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0274

pub trait Hooks {
    /// The server world's border center changed.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_world_border_center_changed",
        native = "fn native_world_border_center_changed(&mut self)"
    )]
    fn world_border_center_changed();
}
