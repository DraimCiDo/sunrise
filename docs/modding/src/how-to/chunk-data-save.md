# Saving chunk attachments

This hook saves chunk attachments. To serialize a mod block entity's live state into its own
payload, use [`block_entity_save_additional`](../concepts/parts/block-entity-types.md#saved-data)
on the server thread, before the disk worker sees the capture. If block-entity serializer edits
cannot be written, the worker retries with stored block-entity payloads and keeps the result of
`chunk_data_save`.

`chunk_data_save` receives `&dyn ChunkDiskHost` and a `ChunkDataSaveEvent`.
The event's `data` is a typed, job-local attachment store. Use its `get`, `set`
and `remove` methods with an issued `AttachmentKey<T>` whose descriptor has a
persistence codec (`.serialize(codec)`). Disk operations consume only the
persistence codec and never invoke `sync_codec`. `T` does not need `Wire`. A
codec error, missing persistence codec, or stale key returns `Err` locally
without modifying stored envelopes or poisoning unrelated edits in the job.

The callback runs on the persistence worker after capture and before encoding.
For example, capturing 7, writing 8 in the callback and later writing 9 in the
live chunk produces disk value 8 without changing live value 9. The host's
`job()` borrows a `&ChunkDiskContext` identifying the world epoch, dimension,
coordinates, full/proto kind and phase. It grants no live chunk lookup,
scheduling, packets or world writes.
`registry(name)` lends `Option<&[String]>` in registry ID order from the frozen generation
retained by the worker. `None` means that generation has no such registry. Use the slice within
the callback, or copy it if it must outlive the callback. Neither metadata getter reads files;
`ConfigHost::setting` still returns an owned `Option<String>` from the same snapshot.

A captured record caches its callback result. Disk encoding and pending reads
reuse it; they cannot invoke the callback twice. A panicking listener loses only
its edits and is disabled for this hook for the session. The log and local error
toast name the mod. Edits from the remaining listeners go to the ordinary writer;
a callback panic does not stop the save.

Every captured record runs the callback exactly once, for full saves (explicit,
forced, late, unload and closing) and proto saves alike. Saves are refused, with
no callback, while an action savepoint is open, and chunk unloading waits for it to close. If the disk write fails, the
old record stays on disk, the snapshot stays readable for recovery and the
wait or flush reports the failure; nothing is marked saved early. A
chunk whose save was refused or failed is marked modified again, so the game
tries it again at the next save.

## Disk NBT validation

The job checks the encoded `sunrise:attachments` compound against the disk NBT
writer's limits (`nbt::write_named` with `Limits::saved()` and the disk writer
nesting guard). Unknown attachment entries are preserved opaquely.
