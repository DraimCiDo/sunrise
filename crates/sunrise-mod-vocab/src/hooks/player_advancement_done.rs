//! The `player_advancement_done` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0275

pub trait Hooks {
    /// A server player completed an advancement, after progress was recorded.
    #[hook(
        side = "server",
        contract = "listen",
        neoforge = "AdvancementEvent.AdvancementEarnEvent",
        cancellable = false,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_advancement_done",
        native = "fn native_player_advancement_done(&mut self, player: u128, name: &str, advancement: &str)"
    )]
    fn player_advancement_done();
}
