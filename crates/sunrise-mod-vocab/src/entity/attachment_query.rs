//! Owned snapshot data of attachment queries.
use crate::*;
use sunrise_mod_runtime::attachments::{self as runtime, index};

/// A SNAPSHOT of membership and value bindings in the host's current dimension.
/// Later changes require a fresh query. This is a residency view: removed but
/// unswept entities may appear, and stale writes fail. Acquisition neither clones values nor
/// decodes saved payloads; iteration decodes lazily. The Wire form includes only rows with a
/// persistence encoding or original replica bytes. It does not run a sync codec or transfer
/// live, unsaved values from the server.
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
        let (checked, epoch) = key.live_with_epoch()?;
        Ok(self.0.query_checked(checked, epoch))
    }

    pub(crate) fn query_checked<'key, T: Send + Sync + 'static>(
        self,
        checked: &'key runtime::CheckedAttachmentType<T>,
        epoch: u64,
    ) -> AttachmentQuery<'key, T> {
        self.0.query_checked(checked, epoch)
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
    crate::wire::layout_fields!("rows" => Vec<(crate::AttachmentWriteToken,u8,Vec<u8>)>);
    const MIN: usize = 4;
    fn put(&self, out: &mut crate::wire::Writer) {
        use runtime::AttachmentRawKind;
        use sunrise_core::nbt::{self, Limits, Tag};
        let mut rows = Vec::new();
        for row in self.0.rows() {
            let Some((kind, payload)) = row.entry.transfer_form() else {
                continue;
            };
            let kind_tag = match kind {
                AttachmentRawKind::Saved => 0u8,
                AttachmentRawKind::Synced => 1u8,
            };
            let Ok(bytes) = nbt::write_network(&Tag::Compound(payload), Limits::default()) else {
                out.fail(crate::wire::WireError::Invalid("invalid attachment NBT"));
                return;
            };
            rows.push((
                AttachmentWriteToken::from(row.write_token()),
                kind_tag,
                bytes,
            ));
        }
        out.put(&rows);
    }
    fn take(input: &mut crate::wire::Reader<'_>) -> Result<Self, crate::wire::WireError> {
        use crate::wire::WireError;
        use sunrise_core::nbt::{self, Limits, Tag};
        let rows: Vec<(AttachmentWriteToken, u8, Vec<u8>)> = input.take()?;
        let mut result = Vec::with_capacity(rows.len());
        let mut identities = std::collections::HashSet::with_capacity(rows.len());
        for (token, kind_tag, bytes) in rows {
            let (holder, _) = token.0.parts();
            if !identities.insert(*holder) {
                return Err(WireError::Invalid("duplicate attachment residency"));
            }
            let (tag, consumed) = nbt::read_network(&bytes, Limits::default())
                .map_err(|_| WireError::Invalid("invalid attachment NBT"))?;
            let Tag::Compound(payload) = tag else {
                return Err(WireError::Invalid("invalid attachment compound"));
            };
            if consumed != bytes.len() {
                return Err(WireError::Invalid("trailing attachment NBT"));
            }
            let entry = match kind_tag {
                0 => runtime::AttachmentEntry::from_saved(payload),
                1 => runtime::AttachmentEntry::from_synced(payload),
                _ => return Err(WireError::Invalid("invalid attachment payload kind")),
            };
            result.push(token.0.into_row(Arc::new(entry)));
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
        out.put(&vec![
            (token.clone(), 0u8, bytes.clone()),
            (token, 0u8, bytes),
        ]);
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
        out.put(&vec![(token, 0u8, bytes.clone()), (newer, 0u8, bytes)]);
        assert!(matches!(
            Reader::new(out.bytes()).take::<AttachmentBatch>(),
            Err(WireError::Invalid("duplicate attachment residency"))
        ));
    }

    #[test]
    fn wire_attachment_batch_refuses_invalid_payload_kind() {
        use crate::wire::{Reader, WireError, Writer};
        use sunrise_core::nbt::{self, Compound, Limits, Tag};
        let holder = AttachmentHolder::Entity(EntityRef::new(WorldEpoch::new(1), 42));
        let token = AttachmentWriteToken(index::AttachmentOwner::from_parts(holder, 7));
        let bytes = nbt::write_network(&Tag::Compound(Compound::new()), Limits::default()).unwrap();
        let mut out = Writer::new();
        out.put(&vec![(token, 2u8, bytes)]);
        assert!(matches!(
            Reader::new(out.bytes()).take::<AttachmentBatch>(),
            Err(WireError::Invalid("invalid attachment payload kind"))
        ));
    }

    #[test]
    fn wire_attachment_batch_transfers_network_depth_and_reports_unwritable_nbt() {
        use crate::wire::{Reader, WireError, Writer};
        use sunrise_core::nbt::{Compound, Tag};

        let holder = AttachmentHolder::Entity(EntityRef::new(WorldEpoch::new(1), 43));
        let to_batch = |payload: Compound| {
            let entry = Arc::new(runtime::AttachmentEntry::from_synced(payload));
            let row = index::AttachmentOwner::from_parts(holder, 8).into_row(entry);
            AttachmentBatch::from(runtime::AttachmentBatch::new(Arc::new(
                index::AttachmentSnapshot::from_rows(vec![row]),
            )))
        };
        let leaf = Tag::Int(23);
        let Tag::Compound(deep) =
            (0..512).fold(leaf, |child, _| Tag::Compound([("v".into(), child)].into()))
        else {
            unreachable!()
        };
        let mut writer = Writer::new();
        writer.put(&to_batch(deep.clone()));
        assert_eq!(writer.error(), None);
        let decoded: AttachmentBatch = Reader::new(writer.bytes()).take().unwrap();
        assert_eq!(decoded.0.rows().len(), 1);
        assert_eq!(decoded.0.rows()[0].entry.transfer_form().unwrap().1, deep);

        let bad = [("s".into(), Tag::String("x".repeat(65_536).as_str().into()))].into();
        let mut writer = Writer::new();
        writer.put(&to_batch(bad));
        assert_eq!(
            writer.error(),
            Some(WireError::Invalid("invalid attachment NBT"))
        );
    }

    #[test]
    fn wire_attachment_batch_preserves_synced_only_and_dual_codec_across_replica_promotion() {
        use crate::wire::{Reader, Writer};
        use sunrise_core::nbt::{Compound, Tag};

        let disk_codec = runtime::AttachmentCodec {
            encode: |v: &u32| Ok([("disk".into(), Tag::Int(*v as i32))].into()),
            decode: |c: &Compound| match c.get(&"disk".into()) {
                Some(Tag::Int(n)) => Ok(*n as u32),
                _ => Err(AttachmentError::Decode),
            },
        };
        let sync_codec = runtime::AttachmentCodec {
            encode: |v: &u32| Ok([("net".into(), Tag::Int(*v as i32 + 100))].into()),
            decode: |c: &Compound| match c.get(&"net".into()) {
                Some(Tag::Int(n)) => Ok((*n - 100) as u32),
                _ => Err(AttachmentError::Decode),
            },
        };
        let round_trip = |batch: AttachmentBatch| -> AttachmentBatch {
            let mut out = Writer::new();
            out.put(&batch);
            Reader::new(out.bytes()).take().unwrap()
        };

        // 1. Sync-only attachment: server write, client raw replica, and client promoted replica.
        let sync_only = crate::parts::install_attachment_fixture(
            "test",
            AttachmentDescriptor::new("test:sync_only", || 0u32).sync(sync_codec),
        );
        let holder = AttachmentHolder::Entity(EntityRef::new(WorldEpoch::new(1), 51));
        let index = Arc::new(std::sync::Mutex::new(index::AttachmentIndex::default()));
        let mut server = runtime::Attachments::default();
        sync_only
            .descriptor()
            .set(&mut server, runtime::Holder::Level, 19)
            .unwrap();
        server.bind_index(&index, holder);
        let batch = AttachmentBatch::from(runtime::AttachmentBatch::new(
            index.lock().unwrap().snapshot_named("test", "sync_only"),
        ));
        assert_eq!(
            batch.clone().query(&sync_only).unwrap().get(holder),
            Ok(Some(19))
        );
        // No disk encoding or received sync bytes exist on the server. Wire must not call
        // the network handler outside the actual attachment send pass.
        assert_eq!(
            round_trip(batch).query(&sync_only).unwrap().get(holder),
            Ok(None)
        );

        let client_index = Arc::new(std::sync::Mutex::new(index::AttachmentIndex::default()));
        let mut client = runtime::Attachments::default();
        client.bind_index(&client_index, holder);
        client.apply_sync(runtime::AttachmentSync {
            namespace: "test".into(),
            name: "sync_only".into(),
            value: Some([("net".into(), Tag::Int(123))].into()),
        });
        let raw_client_batch = round_trip(AttachmentBatch::from(runtime::AttachmentBatch::new(
            client_index
                .lock()
                .unwrap()
                .snapshot_named("test", "sync_only"),
        )));
        assert_eq!(
            raw_client_batch.query(&sync_only).unwrap().get(holder),
            Ok(Some(23))
        );
        // Promote the client replica to Decoded { encoded: None, synced: Some(..) }.
        assert_eq!(
            sync_only
                .descriptor()
                .get_existing(&mut client, runtime::Holder::Level),
            Ok(Some(23))
        );
        let promoted_client_batch =
            round_trip(AttachmentBatch::from(runtime::AttachmentBatch::new(
                client_index
                    .lock()
                    .unwrap()
                    .snapshot_named("test", "sync_only"),
            )));
        assert_eq!(
            promoted_client_batch.query(&sync_only).unwrap().get(holder),
            Ok(Some(23))
        );

        // 2. Dual-codec attachment with distinct disk and sync shapes across client promotion.
        let dual = crate::parts::install_attachment_fixture(
            "test",
            AttachmentDescriptor::new("test:dual", || 0u32)
                .serialize(disk_codec)
                .sync(sync_codec),
        );
        let server_dual_index = Arc::new(std::sync::Mutex::new(index::AttachmentIndex::default()));
        let mut server_dual = runtime::Attachments::default();
        dual.descriptor()
            .set(&mut server_dual, runtime::Holder::Level, 31)
            .unwrap();
        server_dual.bind_index(&server_dual_index, holder);
        let server_dual_batch = round_trip(AttachmentBatch::from(runtime::AttachmentBatch::new(
            server_dual_index
                .lock()
                .unwrap()
                .snapshot_named("test", "dual"),
        )));
        assert_eq!(
            server_dual_batch.query(&dual).unwrap().get(holder),
            Ok(Some(31))
        );

        let client_dual_index = Arc::new(std::sync::Mutex::new(index::AttachmentIndex::default()));
        let mut client_dual = runtime::Attachments::default();
        client_dual.bind_index(&client_dual_index, holder);
        client_dual.apply_sync(runtime::AttachmentSync {
            namespace: "test".into(),
            name: "dual".into(),
            value: Some([("net".into(), Tag::Int(144))].into()),
        });
        let raw_dual_batch = round_trip(AttachmentBatch::from(runtime::AttachmentBatch::new(
            client_dual_index
                .lock()
                .unwrap()
                .snapshot_named("test", "dual"),
        )));
        assert_eq!(
            raw_dual_batch.query(&dual).unwrap().get(holder),
            Ok(Some(44))
        );
        // Promote client replica so it holds Decoded { encoded: None, synced: Some(net) }.
        assert_eq!(
            dual.descriptor()
                .get_existing(&mut client_dual, runtime::Holder::Level),
            Ok(Some(44))
        );
        let promoted_dual_batch = round_trip(AttachmentBatch::from(runtime::AttachmentBatch::new(
            client_dual_index
                .lock()
                .unwrap()
                .snapshot_named("test", "dual"),
        )));
        assert_eq!(
            promoted_dual_batch.query(&dual).unwrap().get(holder),
            Ok(Some(44))
        );
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
