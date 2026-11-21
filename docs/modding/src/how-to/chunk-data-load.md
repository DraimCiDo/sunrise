# Loading chunk attachments

`chunk_data_load` receives `&dyn ChunkDiskHost` and a `ChunkDataLoadEvent` after
a full or proto disk record has decoded, before its attachment store is
published. A missing record, fresh generation and proto-to-full promotion do
not fire this hook. The callback uses `event.data.get(&KEY)` and
`event.data.set(&KEY, value)` to read and edit the typed store of this load.
For example, reading disk value 8 and setting 9 publishes 9 on success.

The worker host supplies only the job identity and immutable resource reads.
Borrow the identity with `let job = host.job();` (`&ChunkDiskContext`).
`registry(name)` lends `Option<&[String]>` in registry ID order from the frozen generation
retained by the worker. `None` means that generation has no such registry. Use the slice within
the callback, or copy it if it must outlive the callback. Neither metadata getter reads files;
`ConfigHost::setting` still returns an owned `Option<String>` from the same snapshot.
The host has no live backend and is not a `ServerHost`. Typed values need a
persistence codec, not `Wire`, and disk operations never invoke `sync_codec`.
Unknown attachment entries are preserved opaquely.

A codec error, missing persistence codec, or stale key on `event.data` returns
`Err` for that operation without modifying its stored envelope. If a listener
panics, only its edits are discarded, and its load callback is disabled for the
session. The log and local error toast name the mod. Edits from listeners before
and after it still publish; a load callback cannot suppress the save hook.
If the captured input fails disk NBT validation, it is reported and the chunk
keeps its attachments as read.

The load runs on the thread that decodes the record, never with access to live
world state. [Disk NBT validation](chunk-data-save.md#disk-nbt-validation)
uses the same disk writer limits on load and save.
