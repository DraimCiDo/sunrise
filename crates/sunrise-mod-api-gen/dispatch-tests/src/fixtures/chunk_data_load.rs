//! `chunk_data_load` and `chunk_data_save`: the disk hooks run each mod through the unified
//! `runtime::invoke` dispatcher. Stand-ins for the restricted
//! host and the store.
mod stubs {
    pub(super) trait ChunkDiskHost {}
    #[derive(Clone, Debug, Default)]
    pub(super) struct ChunkDiskFailure {
        pub mod_id: String,
    }
    #[derive(Clone, Default)]
    pub(super) struct ChunkDiskData {
        failure: Option<ChunkDiskFailure>,
    }
    impl ChunkDiskData {
        pub(super) fn failure(&self) -> Option<&ChunkDiskFailure> {
            self.failure.as_ref()
        }
        pub(super) fn attribute_failure(&mut self, error: ChunkDiskFailure) {
            self.failure = Some(error);
        }
    }
    pub(super) mod chunk_disk {
        pub(crate) fn unwound_failure(mod_id: &'static str) -> super::ChunkDiskFailure {
            super::ChunkDiskFailure {
                mod_id: mod_id.into(),
            }
        }
    }
}
use stubs::chunk_disk;
