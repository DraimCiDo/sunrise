//! The callback's typed store: only encoded values cross the worker boundary.
use super::*;

// no wire: job-local attachment store; only its encoded NBT goes to disk.
/// The encoded attachment store of one chunk disk callback job.
/// Use [`Self::get`], [`Self::set`] and [`Self::remove`] with a typed attachment key.
/// Chunk attachments follow the normal chunk disk NBT rules. A typed operation can
/// fail without changing its entry or preventing other operations from succeeding.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ChunkDiskData {
    entries: Compound,
}

impl ChunkDiskData {
    /// Open captured NBT without running any attachment codec. Malformed envelopes
    /// are logged and skipped; well-formed unknown entries are kept as encoded.
    pub fn from_nbt(mut entries: Compound) -> Self {
        let value = "value".into();
        entries.retain(|namespace, tag| {
            let Tag::Compound(names) = tag else {
                log::warn!("chunk disk attachments: invalid namespace {namespace:?}; skipping");
                return false;
            };
            names.retain(|name, tag| {
                let valid = matches!(tag, Tag::Compound(envelope)
                    if matches!(envelope.get(&value), Some(Tag::Compound(_))));
                if !valid {
                    log::warn!(
                        "chunk disk attachments: invalid envelope {namespace:?}:{name:?}; skipping"
                    );
                }
                valid
            });
            true
        });
        Self { entries }
    }

    /// Read the job's typed value, inserting its default if absent.
    /// A codec error is reported and leaves the entry unchanged.
    pub fn get<T: Clone + Send + Sync + 'static>(
        &mut self,
        key: &AttachmentType<T>,
    ) -> Result<T, ChunkDiskFailure> {
        self.edit(key, |store| key.get(store, Holder::Level))
    }

    /// Replace only the snapshot's value. Failed encoding is reported and leaves
    /// the original entry intact.
    pub fn set<T: Clone + Send + Sync + 'static>(
        &mut self,
        key: &AttachmentType<T>,
        value: T,
    ) -> Result<(), ChunkDiskFailure> {
        self.edit(key, |store| key.set(store, Holder::Level, value))
    }

    /// Remove a snapshot binding through its typed codec. If decoding fails,
    /// the error is reported and the entry stays in the store.
    pub fn remove<T: Clone + Send + Sync + 'static>(
        &mut self,
        key: &AttachmentType<T>,
    ) -> Result<Option<T>, ChunkDiskFailure> {
        self.edit(key, |store| key.remove(store, Holder::Level))
    }

    fn edit<T, R>(
        &mut self,
        key: &AttachmentType<T>,
        apply: impl FnOnce(&mut Attachments) -> Result<R, AttachmentError>,
    ) -> Result<R, ChunkDiskFailure> {
        let fail = |reason: String| {
            let failure = ChunkDiskFailure {
                mod_id: crate::content::crash::current_native_mod()
                    .unwrap_or_default()
                    .into(),
                key: format!("{}:{}", key.namespace, key.name),
                reason,
            };
            log::warn!("chunk disk attachment operation failed: {failure:?}");
            failure
        };
        if key.codec.is_none() {
            return Err(fail("missing persistence codec".into()));
        }
        let namespace = key.namespace.into();
        let name = key.name.into();
        // A typed operation edits one key. Stage only that key, so a failed remove
        // cannot drop an undecodable value and unrelated entries are never copied.
        let mut encoded = Compound::new();
        if let Some(Tag::Compound(names)) = self.entries.get(&namespace)
            && let Some(entry) = names.get(&name)
        {
            encoded.insert(
                namespace.clone(),
                Tag::Compound(Compound::from([(name.clone(), entry.clone())])),
            );
        }
        let mut store = Attachments::from_nbt(encoded);
        let result = apply(&mut store).map_err(|error| fail(format!("{error:?}")))?;
        if let Some(Tag::Compound(mut names)) = store.to_nbt().remove(&namespace)
            && let Some(entry) = names.remove(&name)
        {
            if let Tag::Compound(names) = self
                .entries
                .entry(namespace)
                .or_insert_with(|| Tag::Compound(Compound::new()))
            {
                names.insert(name, entry);
            }
        } else if let Some(Tag::Compound(names)) = self.entries.get_mut(&namespace) {
            names.remove(&name);
        }
        Ok(result)
    }

    /// Return the resulting encoded store, including each successful edit.
    pub fn finish(self) -> Compound {
        self.entries
    }
}
