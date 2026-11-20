//! The `beacon_state_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0168

pub trait Hooks {
    /// A beacon at `pos` activated or deactivated after its beam was checked.
    /// `active` says whether it now has a complete pyramid. Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (NeoForge has no such event)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_beacon_state_change",
        native = "fn native_beacon_state_change(&mut self, dimension: &str, pos: [i32; 3], active: bool)"
    )]
    fn beacon_state_change();
}
