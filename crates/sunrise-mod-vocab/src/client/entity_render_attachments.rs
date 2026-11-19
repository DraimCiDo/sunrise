//! Frozen, per-entity attachment data for the submission pass.
use crate::wire::{Reader, Wire, WireError, Writer};
use crate::{AttachmentError, AttachmentType};
use std::sync::Arc;
use sunrise_core::nbt::{self, Limits, Tag};
use sunrise_mod_runtime::attachments::{AttachmentEntry, AttachmentSync};

const MAX_BYTES: usize = 64 * 1024;

/// A general entity's synced fields as they were at render-state extraction. The same
/// snapshot (at most 64 KiB encoded) is available to native and Wasm renderers.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, sunrise_mod_api_macros::Wire)]
pub struct EntityRenderAttachments {
    data: Arc<Snapshot>,
}

struct Snapshot {
    // Pre-encoded when sync arrives, not for every render call.
    bytes: Vec<u8>,
    entries: Vec<(String, String, AttachmentEntry)>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("fields", &self.entries.len())
            .finish()
    }
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl EntityRenderAttachments {
    /// Builds a bounded copy from a holder's current synced values, in key order.
    // Engine-side only; mods consume snapshots handed to their renderer.
    #[doc(hidden)]
    pub fn from_synced(changes: Vec<AttachmentSync>) -> Result<Self, WireError> {
        let mut values = Vec::with_capacity(changes.len());
        for AttachmentSync {
            namespace,
            name,
            value,
        } in changes
        {
            if let Some(value) = value {
                values.push((
                    namespace
                        .to_string()
                        .map_err(|_| WireError::Invalid("attachment namespace"))?,
                    name.to_string()
                        .map_err(|_| WireError::Invalid("attachment name"))?,
                    value,
                ));
            }
        }
        values.sort_unstable_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        let mut writer = Writer::new();
        writer.limit(MAX_BYTES);
        writer.put_len(values.len());
        let mut entries = Vec::with_capacity(values.len());
        for (namespace, name, value) in values {
            let tag = Tag::Compound(value);
            let encoded = nbt::write_network(
                &tag,
                Limits {
                    max_bytes: MAX_BYTES,
                    ..Limits::default()
                },
            )
            .map_err(|_| WireError::Invalid("attachment NBT is invalid or too large"))?;
            writer.put_str(&namespace);
            writer.put_str(&name);
            writer.put(&encoded);
            if let Some(error) = writer.error() {
                return Err(error);
            }
            entries.push((
                namespace,
                name,
                AttachmentEntry::from_saved([("value".into(), tag)].into()),
            ));
        }
        Ok(Self {
            data: Arc::new(Snapshot {
                bytes: writer.into_bytes(),
                entries,
            }),
        })
    }

    /// Borrows the synced value of `key`, or returns `Ok(None)` when the field is absent.
    /// Never inserts a default and never reads the live world. A present value uses the key's
    /// codec, just as on the synced client holder.
    pub fn get<T: Send + Sync + 'static>(
        &self,
        key: &AttachmentType<T>,
    ) -> Result<Option<&T>, AttachmentError> {
        crate::parts::validate_attachment_use(key.namespace, key.name, key.sync)?;
        self.data
            .entries
            .binary_search_by(|(namespace, name, _)| {
                (namespace.as_str(), name.as_str()).cmp(&(key.namespace, key.name))
            })
            .ok()
            .map(|i| self.data.entries[i].2.read(key))
            .transpose()
    }
}

impl Wire for Snapshot {
    crate::wire::layout_fields!("bytes" => Vec<u8>);
    const MIN: usize = 4;

    fn put(&self, out: &mut Writer) {
        out.put(&self.bytes);
    }

    fn take(input: &mut Reader<'_>) -> Result<Self, WireError> {
        let len = input.take_len(1)?;
        if len > MAX_BYTES {
            return Err(WireError::Length);
        }
        let bytes = input.take_bytes(len)?;
        let mut payload = Reader::new(bytes);
        let count = payload.take_len(12)?; // Three lengths per field.
        let mut entries: Vec<(String, String, AttachmentEntry)> = Vec::with_capacity(count);
        for _ in 0..count {
            let (namespace, name): (String, String) = (payload.take()?, payload.take()?);
            if entries
                .last()
                .is_some_and(|(ns, path, _)| (ns, path) >= (&namespace, &name))
            {
                return Err(WireError::Invalid("unordered attachment keys"));
            }
            let n = payload.take_len(1)?;
            let (tag, consumed) = nbt::read_network(payload.take_bytes(n)?, Limits::default())
                .map_err(|_| WireError::Invalid("invalid attachment NBT"))?;
            if !matches!(tag, Tag::Compound(_)) || consumed != n {
                return Err(WireError::Invalid("invalid attachment compound"));
            }
            entries.push((
                namespace,
                name,
                AttachmentEntry::from_saved([("value".into(), tag)].into()),
            ));
        }
        if !payload.rest().is_empty() {
            return Err(WireError::Trailing);
        }
        Ok(Self {
            bytes: bytes.to_vec(),
            entries,
        })
    }
}
