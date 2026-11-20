//! The `player_teleport_end_gateway` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0315

pub trait Hooks {
    /// A server player is about to travel through an End gateway at `gateway`. Set `cancel` to leave
    /// them at the entrance; the gateway is not consumed and its cooldown does not start. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_teleport_end_gateway",
        native = "fn native_player_teleport_end_gateway(&mut self, player: u128, name: &str, gateway: [i32; 3], cancel: &mut bool)"
    )]
    fn player_teleport_end_gateway();
}
