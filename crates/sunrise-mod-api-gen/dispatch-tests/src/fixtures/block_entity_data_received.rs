impl BlockEntityRef {
    fn with_view<R>(
        self,
        _: &dyn ClientHost,
        read: impl FnOnce(&BlockEntityView) -> R,
    ) -> Option<R> {
        Some(read(&BlockEntityView))
    }
}

struct BlockEntityView;

impl BlockEntityView {
    fn kind(&self) -> Option<BlockEntityTypeKey> {
        Some(BlockEntityTypeKey(0))
    }
}

mod stubs {
    #[derive(Clone, Copy)]
    pub(super) enum BlockEntityDataSource {
        Chunk,
        UpdatePacket,
    }
}
