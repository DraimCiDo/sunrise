//! The `block_receive_game` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0180

pub trait Hooks {
    /// A sculk sensor or shrieker received a vibration for `event` at `pos`.
    /// Set `cancel` to suppress its response. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_block_receive_game",
        native = "fn native_block_receive_game(&mut self, dimension: &str, pos: [i32; 3], block: &str, event: &str, cancel: &mut bool)"
    )]
    fn block_receive_game();
}
