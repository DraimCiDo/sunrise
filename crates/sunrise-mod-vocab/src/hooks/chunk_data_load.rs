//! Chunk attachment decoding boundary.

pub trait Hooks {
    /// The disk record's attachments have decoded, before the chunk is published.
    /// Runs on the decoding thread with a restricted Host. Borrow the captured
    /// metadata from `host.job()`. Missing records and proto-to-full promotion do
    /// not fire this hook. A failed attachment operation leaves its entry unchanged.
    /// A panic is logged and shown in an error toast naming the mod, and disables
    /// that load listener for the session. Completed edits stand and the other
    /// listeners, including later save listeners, still run.
    #[hook(
        side = "server",
        contract = "listen",
        context = "chunk_disk",
        target = "mod_api::chunk_disk::native_chunk_data_load",
        neoforge = "ChunkDataEvent.Load",
        version = 3
    )]
    fn chunk_data_load(data: &mut ChunkDiskData);
}
