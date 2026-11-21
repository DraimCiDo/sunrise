//! The records the writer thread takes: a tag, or a chunk still to encode on that thread.
use super::*;

/// A record waiting for the writer thread.
#[derive(Clone)]
pub(super) enum Record {
    Disk(Arc<chunk_disk::DiskRecord>),
    Tag(Arc<NamedTag>),
    /// A chunk to encode on the writer thread.
    /// The old chunk's calculated blending data is taken when the save is
    /// requested ([`SaveDir::calculated_blending`]): a snapshot of that save.
    Chunk(Arc<Chunk>, Arc<Encoding>, Option<Arc<BlendingData>>),
    Proto(Arc<SavedProto>, Arc<Encoding>),
    /// A chunk's points of interest, made on the writer thread against the
    /// saved ones (`resolve_poi`).
    Poi(
        Arc<Chunk>,
        Arc<SaveTables>,
        Option<Arc<crate::mobs::villager::ResolvedProfessions>>,
    ),
    Delete,
}

/// What encoding a chunk needs, captured when it is queued.
pub(super) struct Encoding {
    pub(super) items: Arc<crate::items::Items>,
    pub(super) registry: &'static BlockRegistry,
    pub(super) biomes: Arc<Biomes>,
    pub(super) tables: Option<Arc<SaveTables>>,
    pub(super) shape: LevelShape,
}

impl Record {
    pub(super) fn tag(&self) -> Option<Arc<NamedTag>> {
        match self {
            Self::Disk(record) => record.read(),
            Self::Tag(t) => Some(t.clone()),
            Self::Chunk(c, e, blending) => Some(Arc::new(encode_saved(
                c,
                e.registry,
                &e.biomes,
                e.tables.as_deref(),
                e.shape,
                blending.as_deref(),
                &e.items,
            ))),
            Self::Proto(p, e) => Some(Arc::new(encode_proto(
                p,
                e.registry,
                &e.biomes,
                e.tables.as_deref(),
                e.shape,
                &e.items,
            ))),
            Self::Poi(c, t, professions) => encode_poi(c, t, professions.as_deref()).map(Arc::new),
            Self::Delete => None,
        }
    }
}
