//! The `arrow_nock` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A player is about to nock `bow` with `hand` (`BowItem.use`, NeoForge's
    /// `ArrowNockEvent`): `has_ammo` is whether `getProjectile` found ammunition
    /// or the player is in creative. Set `cancel` to fail the use.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "ArrowNockEvent",
        cancellable = true,
        native = "fn native_arrow_nock(&mut self, player: u128, name: &str, bow: &ItemStack, hand: Hand, has_ammo: bool, cancel: &mut bool)"
    )]
    fn arrow_nock();
}
