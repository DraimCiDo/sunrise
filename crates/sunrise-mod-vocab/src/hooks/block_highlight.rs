//! The `block_highlight` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0097

pub trait Hooks {
    /// Before an eligible targeted block's selection outline is built.
    /// Cancel skips only the default block highlight, after normal HUD and game-mode gates.
    #[hook(
        side = "client",
        contract = "cancel",
        out = "canceled",
        cancellable = true,
        neoforge = "ExtractBlockOutlineRenderStateEvent",
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_block_highlight",
        native = "fn native_block_highlight(&self, pos: [i32; 3], state: BlockStateId, partial: f32, canceled: &mut bool)"
    )]
    fn block_highlight();
}
