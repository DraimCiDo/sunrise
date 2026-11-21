//! Frozen, per-entity attachment data for the submission pass.
use crate::{AttachmentError, AttachmentKey};
use sunrise_core::nbt::Compound;
use sunrise_mod_runtime::attachments::{Attachments, snapshot::SyncedAttachmentSnapshot};

/// A general entity's synced fields as they were at render-state extraction.
/// Clones share the captured generation and do not see later sync updates.
/// Snapshots compare by attachment key and canonical NBT value, regardless of generation
/// identity, compound iteration order, or which typed values have been read.
// no wire: native render input; attachments keep their separate disk and sync codecs.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct EntityRenderAttachments {
    data: SyncedAttachmentSnapshot,
}

impl EntityRenderAttachments {
    /// Borrows the synced value of `key`, or returns `Ok(None)` if the key is absent from this
    /// generation. Never inserts a default or reads the live world. The first typed read may
    /// allocate when it decodes the value using the key's sync codec.
    pub fn get<T: Send + Sync + 'static>(
        &self,
        key: &AttachmentKey<T>,
    ) -> Result<Option<&T>, AttachmentError> {
        let checked = key.live()?;
        if !checked.is_synced() {
            return Err(AttachmentError::InvalidSyncPolicy);
        }
        self.data.get_checked(checked)
    }
}

/// Capture the synced bindings at the engine's publication boundary, or refuse the whole
/// generation if it exceeds the engine's retention budget.
pub fn capture_entity_render_attachments(
    attachments: &Attachments,
    max_bytes: usize,
    compound_heap_bytes: impl Fn(&Compound) -> usize,
) -> Option<EntityRenderAttachments> {
    Some(EntityRenderAttachments {
        data: SyncedAttachmentSnapshot::capture(attachments, max_bytes, compound_heap_bytes)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AttachmentCodec, AttachmentDescriptor, Tag};
    use std::collections::HashMap;
    use sunrise_mod_runtime::attachments::AttachmentSync;

    fn i32_codec() -> AttachmentCodec<i32> {
        AttachmentCodec {
            encode: |n| Ok([("n".into(), Tag::Int(*n))].into()),
            decode: |tag| match tag.get(&"n".into()) {
                Some(Tag::Int(n)) => Ok(*n),
                _ => Err(AttachmentError::Decode),
            },
        }
    }

    #[test]
    fn sync_only_key_reads_from_snapshot_while_unsynced_and_stale_keys_refuse() {
        let (glow_rec, glow) = crate::parts::registered_attachment_fixture(
            "render_test",
            AttachmentDescriptor::new("render_test:glow", || 0i32).sync(i32_codec()),
        );
        let (local_rec, local) = crate::parts::registered_attachment_fixture(
            "render_test",
            AttachmentDescriptor::new("render_test:local", || 0i32),
        );
        crate::parts::install_input(crate::parts::installation::InstalledInput {
            attachment_types: HashMap::from([(
                "render_test",
                HashMap::from([("glow", glow_rec), ("local", local_rec)]),
            )]),
            ..Default::default()
        });

        let mut store = Attachments::default();
        store.apply_sync(AttachmentSync {
            namespace: "render_test".into(),
            name: "glow".into(),
            value: Some([("n".into(), Tag::Int(19))].into()),
        });
        let snapshot =
            capture_entity_render_attachments(&store, usize::MAX, Compound::storage_bytes).unwrap();
        assert_eq!(snapshot.get(&glow), Ok(Some(&19)));
        assert_eq!(
            snapshot.get(&local),
            Err(AttachmentError::InvalidSyncPolicy)
        );

        let empty = capture_entity_render_attachments(
            &Attachments::default(),
            usize::MAX,
            Compound::storage_bytes,
        )
        .unwrap();
        assert_eq!(empty.get(&glow), Ok(None));

        let _fresh = crate::parts::install_attachment_fixture(
            "render_test",
            AttachmentDescriptor::new("render_test:glow", || 0i32).sync(i32_codec()),
        );
        assert_eq!(snapshot.get(&glow), Err(AttachmentError::StaleKey));
    }
}
