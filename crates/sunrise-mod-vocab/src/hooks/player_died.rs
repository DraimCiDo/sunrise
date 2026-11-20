//! The `player_died` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0047

pub trait Hooks {
    /// `player` died: where `ServerPlayer.die` starts, with the death
    /// message's key when the game knows it (`death.attack.lava`; none for a
    /// message it builds from the combat). It only hears. Server-side, for
    /// every player of the world, whether death messages are shown or not.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "LivingDeathEvent",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_died",
        native = "fn native_player_died(&mut self, player: u128, cause: Option<&str>)"
    )]
    fn player_died();
}
