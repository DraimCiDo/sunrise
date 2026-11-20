//! The `player_switch_hotbar_slot_post` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0292

pub trait Hooks {
    /// `PlayerSwitchHotbarSlotEvent.Post`: a server player switched from hotbar slot
    /// `old_slot` to `new_slot` (0 to 8), after the selected slot changed and a main-hand
    /// item use stopped. Fired only when the slot differs and
    /// `player_switch_hotbar_slot_pre` didn't cancel it. The player's state is not swapped in.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "PlayerSwitchHotbarSlotEvent.Post",
        cancellable = false,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_switch_hotbar_slot_post",
        native = "fn native_player_switch_hotbar_slot_post(&mut self, player: u128, player_entity_id: i32, old_slot: i32, new_slot: i32)"
    )]
    fn player_switch_hotbar_slot_post();
}
