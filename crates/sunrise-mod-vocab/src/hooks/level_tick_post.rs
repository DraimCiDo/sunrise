//! The `level_tick_post` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0011

pub trait Hooks {
    /// After a successful level tick pass on either logical side; never after a failed tick.
    /// `host.server_has_time()` says whether optional work still fits in the tick
    /// (`LevelTickEvent.hasTime`); always true on a client.
    #[hook(
        side = "both",
        contract = "listen",
        neoforge = "LevelTickEvent.Post",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_level_tick_post",
        native = "fn native_level_tick_post(&mut self, level: LevelRef)"
    )]
    fn level_tick_post();
}
