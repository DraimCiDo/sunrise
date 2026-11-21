//! Owned snapshot data of attachment queries.
use crate::*;
use sunrise_mod_runtime::attachments::{self as runtime, index};

/// A SNAPSHOT of membership and value bindings in the host's current dimension.
/// Later changes require a fresh query. This is a residency view: removed but
/// unswept entities may appear, and stale writes fail. Acquisition neither clones values nor
/// decodes saved payloads; iteration decodes lazily.
// no wire: transfers only saved rows as scoped tokens plus network NBT; decode requires a complete compound envelope and rebuilds runtime entries.
#[derive(Clone)]
#[non_exhaustive]
pub struct AttachmentBatch(runtime::AttachmentBatch<AttachmentHolder>);
impl From<runtime::AttachmentBatch<AttachmentHolder>> for AttachmentBatch {
    fn from(batch: runtime::AttachmentBatch<AttachmentHolder>) -> Self {
        Self(batch)
    }
}
impl AttachmentBatch {
    /// Bind an issued attachment key without eagerly decoding or cloning any value.
    /// This view borrows only the key, never the host.
    /// Refuses keys from an earlier installation with [`AttachmentError::StaleKey`].
    pub fn query<'key, T: Send + Sync + 'static>(
        self,
        key: &'key AttachmentKey<T>,
    ) -> Result<AttachmentQuery<'key, T>, AttachmentError> {
        let checked = key.live()?;
        Ok(self.0.query_checked(checked))
    }

    pub(crate) fn query_checked<'key, T: Send + Sync + 'static>(
        self,
        checked: &'key runtime::CheckedAttachmentType<T>,
    ) -> AttachmentQuery<'key, T> {
        self.0.query_checked(checked)
    }
}
/// A typed snapshot whose iterator borrows values from its captured bindings.
/// `get(holder)` returns an owned clone with an expected O(1) probe before decode.
/// Missing or not yet synced holders return `Ok(None)`; no default is created.
/// `None` does not mean the holder is alive: absent and not-live holders alike
/// return `Ok(None)` when absent from the snapshot, without a host liveness check.
/// Only scoped residency probes report `StaleHolder`.
// no wire: borrows a live attachment index.
pub type AttachmentQuery<'key, T> = runtime::AttachmentQuery<'key, AttachmentHolder, T>;

/// A private-field scoped residency identity, not an access authorization token.
// no wire: projects the runtime residency owner to holder/generation parts; decoding rebuilds an owner, not access authorization.
#[derive(Clone)]
#[non_exhaustive]
pub struct AttachmentWriteToken(index::AttachmentOwner<AttachmentHolder>);
impl From<index::AttachmentOwner<AttachmentHolder>> for AttachmentWriteToken {
    fn from(token: index::AttachmentOwner<AttachmentHolder>) -> Self {
        Self(token)
    }
}
impl AttachmentWriteToken {
    /// Check the current dimension's residency before the ordinary write path.
    // hidden: the owning engine validates tokens; mods use the row's set/remove methods.
    #[doc(hidden)]
    pub fn validate(
        &self,
        index: &index::AttachmentIndex<AttachmentHolder>,
    ) -> Result<(), AttachmentRowError> {
        index.validate_owner(&self.0)
    }
}

impl crate::wire::Wire for AttachmentWriteToken {
    crate::wire::layout_fields!("holder" => crate::AttachmentHolder, "generation" => u64);
    const MIN: usize = 12;
    fn put(&self, out: &mut crate::wire::Writer) {
        let (holder, generation) = self.0.parts();
        out.put(holder);
        out.put(&generation);
    }
    fn take(input: &mut crate::wire::Reader<'_>) -> Result<Self, crate::wire::WireError> {
        Ok(Self(index::AttachmentOwner::from_parts(
            input.take()?,
            input.take()?,
        )))
    }
}

impl crate::wire::Wire for AttachmentBatch {
    crate::wire::layout_fields!("rows" => Vec<(crate::AttachmentWriteToken,Vec<u8>)>);
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        use sunrise_core::nbt::{self, Limits, Tag};
        let rows: Vec<_> = self
            .0
            .rows()
            .iter()
            .filter_map(|row| {
                let saved = row.entry.saved()?;
                let bytes = nbt::write_network(&Tag::Compound(saved), Limits::default()).ok()?;
                Some((AttachmentWriteToken::from(row.write_token()), bytes))
            })
            .collect();
        out.put(&rows);
    }
    fn take(input: &mut crate::wire::Reader<'_>) -> Result<Self, crate::wire::WireError> {
        use crate::wire::WireError;
        use sunrise_core::nbt::{self, Limits, Tag};
        let rows: Vec<(AttachmentWriteToken, Vec<u8>)> = input.take()?;
        let mut result = Vec::with_capacity(rows.len());
        let mut identities = std::collections::HashSet::with_capacity(rows.len());
        for (token, bytes) in rows {
            let (holder, _) = token.0.parts();
            if !identities.insert(*holder) {
                return Err(WireError::Invalid("duplicate attachment residency"));
            }
            let (tag, consumed) = nbt::read_network(&bytes, Limits::default())
                .map_err(|_| WireError::Invalid("invalid attachment NBT"))?;
            let Tag::Compound(saved) = tag else {
                return Err(WireError::Invalid("invalid attachment envelope"));
            };
            if consumed != bytes.len() {
                return Err(WireError::Invalid("trailing attachment NBT"));
            }
            result.push(
                token
                    .0
                    .into_row(Arc::new(runtime::AttachmentEntry::from_saved(saved))),
            );
        }
        Ok(Self(runtime::AttachmentBatch::new(Arc::new(
            runtime::index::AttachmentSnapshot::from_rows(result),
        ))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_attachment_batch_refuses_duplicate_residencies() {
        use crate::wire::{Reader, WireError, Writer};
        use sunrise_core::nbt::{self, Compound, Limits, Tag};
        let holder = AttachmentHolder::Entity(EntityRef::new(WorldEpoch::new(1), 42));
        let token = AttachmentWriteToken(index::AttachmentOwner::from_parts(holder, 7));
        let bytes = nbt::write_network(&Tag::Compound(Compound::new()), Limits::default()).unwrap();
        let mut out = Writer::new();
        out.put(&vec![(token.clone(), bytes.clone()), (token, bytes)]);
        assert!(matches!(
            Reader::new(out.bytes()).take::<AttachmentBatch>(),
            Err(WireError::Invalid("duplicate attachment residency"))
        ));
    }

    #[test]
    fn wire_attachment_batch_refuses_different_generations_for_one_holder() {
        use crate::wire::{Reader, WireError, Writer};
        use sunrise_core::nbt::{self, Compound, Limits, Tag};
        let holder = AttachmentHolder::Entity(EntityRef::new(WorldEpoch::new(1), 42));
        let token = AttachmentWriteToken(index::AttachmentOwner::from_parts(holder, 7));
        let bytes = nbt::write_network(&Tag::Compound(Compound::new()), Limits::default()).unwrap();
        let mut out = Writer::new();
        let newer = AttachmentWriteToken(index::AttachmentOwner::from_parts(holder, 8));
        out.put(&vec![(token, bytes.clone()), (newer, bytes)]);
        assert!(matches!(
            Reader::new(out.bytes()).take::<AttachmentBatch>(),
            Err(WireError::Invalid("duplicate attachment residency"))
        ));
    }

    #[test]
    fn wire_attachment_rows_and_stale_tokens() {
        use crate::wire::{Reader, Writer};
        use sunrise_core::nbt::{Compound, Tag};
        let codec = runtime::AttachmentCodec {
            encode: |v: &u32| Ok([("n".into(), Tag::Int(*v as i32))].into()),
            decode: |c: &Compound| match c.get(&"n".into()) {
                Some(Tag::Int(n)) => Ok(*n as u32),
                _ => Err(AttachmentError::Decode),
            },
        };
        let coded = crate::parts::install_attachment_fixture(
            "test",
            AttachmentDescriptor::new("test:coded", || 0u32).serialize(codec),
        );
        let holder = AttachmentHolder::Entity(EntityRef::new(WorldEpoch::new(1), 42));
        let index = Arc::new(std::sync::Mutex::new(index::AttachmentIndex::default()));
        let mut values = runtime::Attachments::default();
        coded
            .descriptor()
            .set(&mut values, runtime::Holder::Level, 73)
            .unwrap();
        values.bind_index(&index, holder);
        let batch = AttachmentBatch::from(runtime::AttachmentBatch::new(
            index.lock().unwrap().snapshot_named("test", "coded"),
        ));
        let mut out = Writer::new();
        out.put(&batch);
        let decoded: AttachmentBatch = Reader::new(out.bytes()).take().unwrap();
        let query = decoded.query(&coded).unwrap();
        let row = query.iter().next().unwrap().unwrap();
        assert_eq!(row.holder, holder);
        assert_eq!(*row.value, 73);
        assert_eq!(query.get(holder), Ok(Some(73)));
        let empty = AttachmentBatch::from(runtime::AttachmentBatch::empty());
        assert_eq!(empty.query(&coded).unwrap().get(holder), Ok(None));
        let token = AttachmentWriteToken::from(row.write_token());
        assert_eq!(token.validate(&index.lock().unwrap()), Ok(()));
        index.lock().unwrap().unload(holder).into_result();
        assert_eq!(
            token.validate(&index.lock().unwrap()),
            Err(AttachmentRowError::StaleHolder)
        );
        for end in 0..out.bytes().len() {
            assert!(
                Reader::new(&out.bytes()[..end])
                    .take::<AttachmentBatch>()
                    .is_err()
            );
        }
        assert!(
            Reader::new(&[255, 255, 255, 255])
                .take::<AttachmentHolder>()
                .is_err()
        );
        // Reinstalling the same ID and Rust type makes the old key stale for batch.query.
        let fresh = crate::parts::install_attachment_fixture(
            "test",
            AttachmentDescriptor::new("test:coded", || 0u32).serialize(codec),
        );
        let decoded_again: AttachmentBatch = Reader::new(out.bytes()).take().unwrap();
        assert_eq!(
            decoded_again.clone().query(&coded).err(),
            Some(AttachmentError::StaleKey)
        );
        assert_eq!(
            decoded_again.query(&fresh).unwrap().get(holder),
            Ok(Some(73))
        );
    }
    #[test]
    fn write_tokens_go_stale_with_their_residency() {
        let key = crate::parts::install_attachment_fixture(
            "test",
            AttachmentDescriptor::new("test:value", || 0u32),
        );
        let holder = AttachmentHolder::Entity(EntityRef::new(WorldEpoch::new(1), 42));
        let index = Arc::new(std::sync::Mutex::new(index::AttachmentIndex::default()));
        let mut values = runtime::Attachments::default();
        key.descriptor()
            .set(&mut values, runtime::Holder::Level, 7)
            .unwrap();
        values.bind_index(&index, holder);
        let batch = AttachmentBatch::from(runtime::AttachmentBatch::new(
            index.lock().unwrap().snapshot_named("test", "value"),
        ));
        let query = batch.query(&key).unwrap();
        let row = query.iter().next().unwrap().unwrap();
        assert_eq!(row.holder, holder);
        assert_eq!(*row.value, 7);
        let token = AttachmentWriteToken::from(row.write_token());
        assert_eq!(token.validate(&index.lock().unwrap()), Ok(()));
        assert_eq!(
            token.validate(&index::AttachmentIndex::default()),
            Err(AttachmentRowError::StaleHolder)
        );
        index.lock().unwrap().unload(holder).into_result();
        assert_eq!(
            token.validate(&index.lock().unwrap()),
            Err(AttachmentRowError::StaleHolder)
        );
        let mut again = runtime::Attachments::default();
        key.descriptor()
            .set(&mut again, runtime::Holder::Level, 7)
            .unwrap();
        again.bind_index(&index, holder);
        assert_eq!(
            token.validate(&index.lock().unwrap()),
            Err(AttachmentRowError::StaleHolder)
        );
        assert_eq!(*row.value, 7);
    }
}
