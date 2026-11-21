//! Stand-ins for chunk disk dispatch's restricted host, snapshot and listener boundary.
mod stubs {
    pub(super) trait ChunkDiskHost {}
    #[derive(Clone, Default)]
    pub(super) struct ChunkDiskData;
    pub(super) mod chunk_disk {
        pub(crate) fn invoke(
            _: super::super::HookId,
            _: &str,
            _: &str,
            call: impl FnOnce(),
        ) -> bool {
            call();
            true
        }
    }
}
use stubs::chunk_disk;
