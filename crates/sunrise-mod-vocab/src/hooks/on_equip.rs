//! The `on_equip` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0053

pub trait Hooks {
    /// A player's equipment slot `slot` (`head`, `chest`, `legs`, `feet`,
    /// `offhand`, and `mainhand` on a swap of the hands or `/item`) took a different
    /// item, `new` (none when it was emptied): `LivingEntity.onEquipItem`,
    /// before the equip sound. It only hears. Never for a spectator.
    /// Server-side, for every player of the world (the helpers read the
    /// acting one).
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "none (vanilla's `LivingEntity.onEquipItem`; `living_equipment_change` is the `LivingEquipmentChangeEvent` counterpart)",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_on_equip",
        native = "fn native_on_equip(&mut self, slot: &str, new: Option<&ItemStack>)"
    )]
    fn on_equip();
}
