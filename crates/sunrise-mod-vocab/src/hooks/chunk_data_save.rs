//! Chunk attachment snapshot encoding boundary.

pub trait Hooks {
    /// A captured chunk snapshot is about to be encoded for disk.
    /// `data` belongs only to that snapshot: typed edits change the disk record,
    /// never the live chunk. Each enabled listener runs once per full or proto
    /// capture. The restricted Host cannot resolve live chunks or mutate the world;
    /// `host.job()` borrows the captured metadata. A failed attachment operation
    /// leaves its entry unchanged. A panic is logged and shown in an error toast
    /// naming the mod, and disables that listener for the session. Completed edits
    /// stand and the remaining listeners still run. The normal disk NBT writer logs each
    /// overlong modified UTF-8 name or string that it writes as empty.
    #[hook(
        side = "server",
        contract = "listen",
        context = "chunk_disk",
        target = "mod_api::chunk_disk::native_chunk_data_save",
        neoforge = "ChunkDataEvent.Save",
        version = 3
    )]
    fn chunk_data_save(data: &mut ChunkDiskData);
}
