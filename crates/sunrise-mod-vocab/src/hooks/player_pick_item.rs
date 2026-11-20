//! The `player_pick_item` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0301

pub trait Hooks {
    /// A server player picked an item from a block or entity, before it enters the hotbar. `pos` is
    /// zero for an entity; `entity_id` is zero for a block. The player's state is swapped in. Set
    /// `cancel` to leave the inventory and client unchanged.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event; Paper's PlayerPickItemEvent)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_pick_item",
        native = "fn native_player_pick_item(&mut self, player: u128, name: &str, from_entity: bool, pos: [i32; 3], entity_id: i32, cancel: &mut bool)"
    )]
    fn player_pick_item();
}
