//! The `block_fade` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0175

pub trait Hooks {
    /// A block at `pos` is about to fade from `from` to `to` because of
    /// melting or coral death. Set `cancel` to keep it. Server-side.
    #[hook(
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        target = "sunrise_client::game::native_hooks::base::GameLevel<'_>::native_block_fade",
        native = "fn native_block_fade(&self, dimension: LevelDimension, pos: BlockPos, from: BlockStateId, to: BlockStateId, cancel: &mut bool)"
    )]
    fn block_fade();
}
