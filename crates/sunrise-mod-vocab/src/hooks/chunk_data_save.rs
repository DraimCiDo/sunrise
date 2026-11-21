//! Chunk attachment snapshot encoding boundary.

pub trait Hooks {
    /// A committed chunk snapshot is about to encode on the chunk worker.
    /// `data` belongs only to that snapshot: typed edits change the disk record,
    /// never the live chunk. Exactly once per captured record, for full and proto
    /// chunks alike. The restricted Host cannot resolve live chunks or mutate the
    /// world; `host.job()` borrows the captured metadata. A failed job (for example
    /// an unwinding listener or invalid disk NBT) retains its snapshot and the prior
    /// disk record; codec and key errors on `data` return locally without poisoning
    /// unrelated edits.
    #[hook(
        side = "server",
        contract = "listen",
        context = "chunk_disk",
        target = "mod_api::chunk_disk::native_chunk_data_save",
        neoforge = "ChunkDataEvent.Save",
        version = 2
    )]
    fn chunk_data_save(data: &mut ChunkDiskData);
}
