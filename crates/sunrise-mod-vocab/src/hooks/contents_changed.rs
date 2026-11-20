//! The `contents_changed` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A server-only, non-cancellable backing-block notice at Container.setChanged / BlockEntity.setChanged, after the write and before dirty/comparator updates. Game-held vanilla container transactions coalesce implicit notices at root commit. Explicit mod-issued set_changed notifies immediately, even for equal contents and inside a transaction; the notice survives abort. Raw mod-owned storage writes and restores synthesize no notice. Nested writes to the backing block whose notice is active still apply but do not re-enter its notice; another block notifies normally. Double containers notify each backing component, not the other half merely refreshed for comparators. Vehicle/menu placeholder coordinates are excluded. The source level, pos and state always name the changed backing block. observer is None for its own Parts, or Some for Parts of a block entity watching that source through watch_contents. Source Parts run first, then a snapshot of observers in registration order; a stale observer is skipped. A self-watch never duplicates the source callback. Registration changes affect the next notice. Replaces the Create port positional contents_changed mixin for depot, package and schematic storage; there is no matching NeoForge bus event.
    #[hook(
        side = "server",
        version = 2,
        contract = "listen",
        scope = "block",
        key = "state",
        neoforge = "none (Container.setChanged has no bus event)",
        cancellable = false,
        native = "fn native_contents_changed(&mut self, level: LevelRef, pos: BlockPos, state: BlockStateId, observer: Option<BlockEntityRef>)"
    )]
    fn contents_changed();
}
