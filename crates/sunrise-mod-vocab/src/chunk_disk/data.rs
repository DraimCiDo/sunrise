//! The callback's typed store: only encoded values cross the worker boundary.
use super::*;
use std::sync::Arc;
use sunrise_core::nbt::{self, Limits, NamedTag};
use sunrise_mod_runtime::attachments::{ATTACHMENTS_KEY, CheckedAttachmentType};

/// A job-local attachment transaction. No decoded value is shared with live state.
/// Use [`Self::get`], [`Self::set`] and [`Self::remove`] with an issued [`AttachmentKey`].
/// Disk operations consume only the key's persistence codec; any refusal returns locally
/// without modifying the stored envelopes or poisoning unrelated edits.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ChunkDiskData {
    entries: Arc<Compound>,
}

impl ChunkDiskData {
    /// Check the captured encoded envelopes against the disk NBT writer's limits
    /// before opening the transaction. This does not run any attachment codec.
    pub fn from_nbt(entries: Compound) -> Result<Self, ChunkDiskFailure> {
        validate_disk(&entries).map_err(|error| failure("", error.reason))?;
        Ok(Self {
            entries: Arc::new(entries),
        })
    }

    /// Read the job's typed value through its persistence codec, inserting its default if absent.
    pub fn get<T: Clone + Send + Sync + 'static>(
        &mut self,
        key: &AttachmentKey<T>,
    ) -> Result<T, ChunkDiskFailure> {
        self.edit(key, |checked, store| checked.get(store, Holder::Level))
    }

    /// Replace only the snapshot's value through its persistence codec. Failed encoding
    /// leaves the original envelopes intact and returns locally without poisoning other edits.
    pub fn set<T: Clone + Send + Sync + 'static>(
        &mut self,
        key: &AttachmentKey<T>,
        value: T,
    ) -> Result<(), ChunkDiskFailure> {
        self.edit(key, |checked, store| {
            checked.set(store, Holder::Level, value)
        })
    }

    /// Remove a snapshot binding through its persistence codec.
    pub fn remove<T: Clone + Send + Sync + 'static>(
        &mut self,
        key: &AttachmentKey<T>,
    ) -> Result<Option<T>, ChunkDiskFailure> {
        self.edit(key, |checked, store| checked.remove(store, Holder::Level))
    }

    fn edit<T: 'static, R>(
        &mut self,
        key: &AttachmentKey<T>,
        apply: impl FnOnce(&CheckedAttachmentType<T>, &mut Attachments) -> Result<R, AttachmentError>,
    ) -> Result<R, ChunkDiskFailure> {
        let key_name = key.id();
        let checked = key
            .live()
            .map_err(|error| failure(key_name, &format!("{error:?}")))?;
        if !checked.has_codec() {
            return Err(failure(key_name, "missing persistence codec"));
        }
        let disk = checked.disk_only();
        let mut store = Attachments::from_nbt((*self.entries).clone());
        let value =
            apply(&disk, &mut store).map_err(|error| failure(key_name, &format!("{error:?}")))?;
        let encoded = store.to_nbt();
        validate_disk(&encoded).map_err(|error| failure(key_name, error.reason))?;
        self.entries = Arc::new(encoded);
        Ok(value)
    }

    /// Publish the accepted edits from this snapshot.
    pub fn finish(self) -> Compound {
        Arc::unwrap_or_clone(self.entries)
    }
}

fn failure(key: &str, reason: &str) -> ChunkDiskFailure {
    let mod_id = crate::content::crash::current_native_mod().unwrap_or_default();
    if !mod_id.is_empty() && !key.is_empty() {
        log::warn!("mod {mod_id}: chunk attachment {key} operation refused: {reason}");
    }
    ChunkDiskFailure {
        mod_id: mod_id.into(),
        key: key.into(),
        reason: reason.into(),
    }
}

fn validate_disk(entries: &Compound) -> Result<(), nbt::Error> {
    let limits = Limits {
        // The chunk root compound wraps `sunrise:attachments`.
        max_depth: nbt::DISK_WRITE_NESTING_GUARD.max(Limits::saved().max_depth) - 1,
        ..Limits::saved()
    };
    nbt::write_named(
        &NamedTag {
            name: ATTACHMENTS_KEY.into(),
            tag: Tag::Compound(entries.clone()),
        },
        limits,
    )
    .map(|_| ())
}
