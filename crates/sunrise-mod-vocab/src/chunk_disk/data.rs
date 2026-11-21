//! The callback's typed store: only encoded values cross the worker boundary.
use super::*;
use sunrise_core::nbt::{self, Limits, NamedTag};
use sunrise_mod_runtime::attachments::{ATTACHMENTS_KEY, CheckedAttachmentType};

/// A job-local attachment transaction. No decoded value is shared with live state.
/// Use [`Self::get`], [`Self::set`] and [`Self::remove`] with an issued [`AttachmentKey`].
/// Disk operations consume only the key's persistence codec; any refusal returns locally
/// without modifying the stored envelopes or poisoning unrelated edits.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ChunkDiskData {
    entries: Compound,
    failure: Option<ChunkDiskFailure>,
}

impl ChunkDiskData {
    /// Check the captured encoded envelopes against the disk NBT writer's limits
    /// before opening the transaction. This does not run any attachment codec.
    pub fn from_nbt(entries: Compound) -> Result<Self, ChunkDiskFailure> {
        validate_disk(&entries)?;
        Ok(Self {
            entries,
            failure: None,
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
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let key_name = key.id();
        let checked = key
            .live()
            .map_err(|error| failure(key_name, &format!("{error:?}")))?;
        if !checked.has_codec() {
            return Err(failure(key_name, "missing persistence codec"));
        }
        let disk = checked.disk_only();
        let mut store = Attachments::from_nbt(self.entries.clone());
        let value =
            apply(&disk, &mut store).map_err(|error| failure(key_name, &format!("{error:?}")))?;
        let encoded = store.to_nbt();
        validate_disk(&encoded)?;
        self.entries = encoded;
        Ok(value)
    }

    /// Run a nested local savepoint. An explicit abort restores the prior envelopes.
    pub fn transaction<R, E>(
        &mut self,
        apply: impl FnOnce(&mut Self) -> Result<R, E>,
    ) -> Result<R, E> {
        let previous = self.entries.clone();
        let result = apply(self);
        if result.is_err() {
            self.entries = previous;
        }
        result
    }

    /// The first job-wide refusal (for example an unwinding listener), if any.
    pub fn failure(&self) -> Option<&ChunkDiskFailure> {
        self.failure.as_ref()
    }

    pub(crate) fn refuse(&mut self, error: ChunkDiskFailure) {
        if self.failure.is_none() {
            self.failure = Some(error);
        }
    }

    pub(crate) fn attribute_failure(&mut self, error: ChunkDiskFailure) {
        self.failure = Some(error);
    }

    /// Publish a fully successful snapshot, or return its structured refusal.
    pub fn finish(self) -> Result<Compound, ChunkDiskFailure> {
        match self.failure {
            Some(error) => Err(error),
            None => Ok(self.entries),
        }
    }
}

fn failure(key: &str, reason: &str) -> ChunkDiskFailure {
    ChunkDiskFailure {
        mod_id: String::new(),
        key: key.into(),
        reason: reason.into(),
    }
}

fn validate_disk(entries: &Compound) -> Result<(), ChunkDiskFailure> {
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
    .map_err(|error| failure("", error.reason))
}
