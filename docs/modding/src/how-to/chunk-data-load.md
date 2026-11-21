# Loading chunk attachments

`chunk_data_load` receives `&dyn ChunkDiskHost` and a `ChunkDataLoadEvent` after
a full or proto disk record has decoded, before its attachment store is
published. A missing record, fresh generation and proto-to-full promotion do
not fire this hook. The callback uses `event.data.get(&KEY)` and
`event.data.set(&KEY, value)` to read and edit the typed store of this load.
For example, reading disk value 8 and setting 9 publishes 9 with the chunk.

The worker host supplies only the job identity and immutable resource reads.
Borrow the identity with `let job = host.job();` (`&ChunkDiskContext`).
`registry(name)` lends `Option<&[String]>` in registry ID order from the frozen generation
retained by the worker. `None` means that generation has no such registry. Use the slice within
the callback, or copy it if it must outlive the callback. Neither metadata getter reads files;
`ConfigHost::setting` still returns an owned `Option<String>` from the same snapshot.
The host has no live backend and is not a `ServerHost`. Typed values need a
persistence codec, not `Wire`. Well-formed unknown attachment entries are kept as encoded.
Malformed entries are logged and skipped without affecting the rest of the store.

A failed `get`, `set` or `remove` reports the error and leaves that entry unchanged.
A panic is logged and shown in an error toast naming the mod. Only that mod's load
listener is disabled for the session; completed edits stand, other listeners still
run and the chunk loads. This does not disable any `chunk_data_save` listener.

The callback runs on the decoding thread, without access to live world state. Load
and save both use the [chunk disk NBT rules](chunk-data-save.md#disk-nbt).
