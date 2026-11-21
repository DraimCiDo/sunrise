//! `chunk_data_load` and `chunk_data_save`: stand-ins for the restricted host, store and
//! callback boundary used by the generated dispatch loops.
mod stubs {
    pub(super) trait ChunkDiskHost {}
    #[derive(Default)]
    pub(super) struct ChunkDiskData;
    pub(super) mod chunk_disk {
        pub(crate) fn invoke(
            _: super::super::generated::HookId,
            _: &'static str,
            _: &'static str,
            call: impl FnOnce(),
        ) {
            call();
        }
    }
}
use stubs::chunk_disk;
