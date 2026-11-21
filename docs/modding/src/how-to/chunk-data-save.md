# Saving chunk attachments

This hook saves chunk attachments. To serialize a mod block entity's live state into its own
payload, use [`block_entity_save_additional`](../concepts/parts/block-entity-types.md#saved-data)
on the server thread before the chunk is encoded.

`chunk_data_save` receives `&dyn ChunkDiskHost` and a `ChunkDataSaveEvent`.
The event's `data` is a typed attachment store for the captured chunk. Use `get`, `set`
and `remove` with an `AttachmentType<T>` that has a persistence codec; `T` does not
need `Wire`. A failed operation reports the error and leaves its entry unchanged.

An asynchronous save runs its callbacks on the region writer; a synchronous save runs
its callbacks on the calling thread. Both run after capture and before the record is
written to disk. For example, capturing 7, writing 8 in the callback and later writing
9 in the live chunk produces disk value 8 without changing live value 9. The host's
`job()` borrows a `&ChunkDiskContext` identifying the world epoch, dimension,
coordinates, full/proto kind and phase. It grants no live chunk lookup, scheduling,
packets or world writes.
`registry(name)` lends `Option<&[String]>` in registry ID order from the frozen generation
retained by the callback. `None` means that generation has no such registry. Use the slice within
the callback, or copy it if it must outlive the callback. Neither metadata getter reads files;
`ConfigHost::setting` still returns an owned `Option<String>` from the same snapshot.

Each enabled listener runs once per captured record, for full and proto saves. Pending
reads reuse the result; they do not call the listener again. A panic is logged and shown
in an error toast naming the mod, and disables only that listener for the session.
Completed edits stand, the other listeners still run, and the chunk continues to save.
A chunk encode or write failure is reported for that chunk; other chunks still save.

## Disk NBT

Attachments use the same NBT reader and writer as the rest of the chunk. They have
no separate byte or nesting limits. Names and strings too long for modified UTF-8 are
written as empty strings by the disk writer. Use names and strings that fit in modified
UTF-8 if they must round-trip without changing.
