//! The `player_item_smithed` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// `PlayerEvent.ItemSmithedEvent`: a server player took `result` from a
    /// smithing table (`SmithingMenu.onTake`, or `onQuickCraft` for a
    /// shift-click): `player` is their UUID and `player_entity_id` their
    /// network id, `inputs` the template, base and addition as they were
    /// before the take used them (`None` for an empty slot). It comes once the click is done, so the
    /// menu already holds the used inputs, but before the table sounds.
    /// Server-side.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "PlayerEvent.ItemSmithedEvent",
        cancellable = false,
        native = "fn native_player_item_smithed(&mut self, player: u128, player_entity_id: i32, inputs: &[Option<ItemStack>; 3], result: &ItemStack)"
    )]
    fn player_item_smithed();
}
