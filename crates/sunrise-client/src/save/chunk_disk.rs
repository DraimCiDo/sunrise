//! The persistence owner's isolated mod attachment callback boundary.
use super::*;
use mod_api::{ChunkDiskContext, ChunkDiskJob, ChunkDiskKind, ChunkDiskPhase, WorldEpoch};
use sunrise_client_render::api_context::WorldgenReader;
use sunrise_mod_runtime::attachments::Attachments;

pub(super) struct DiskRuntime {
    world: WorldEpoch,
    dimension: mod_api::Identifier,
    resources: Arc<sunrise_world::native_load_context::WorldgenContext>,
}

impl DiskRuntime {
    fn capture(
        &self,
        chunk: &Chunk,
        kind: ChunkDiskKind,
        phase: ChunkDiskPhase,
    ) -> Option<ChunkDiskJob> {
        let context = ChunkDiskContext::new(
            self.world,
            self.dimension.clone(),
            [chunk.x, chunk.z],
            kind,
            phase,
        );
        let empty = Attachments::default();
        ChunkDiskJob::capture(
            &context,
            chunk
                .saved
                .as_ref()
                .map_or(&empty, |saved| &saved.attachments),
        )
    }

    fn run(&self, job: ChunkDiskJob) -> Compound {
        let reader = WorldgenReader(&self.resources);
        // A disk job has no block-state index (that is the game's, published for worldgen), so
        // `state_with_property` answers None, as worldgen does before the index is published.
        job.run(&mod_api::OwningWorldgen::new(&reader, &|_, _, _| None))
    }
}

pub(super) struct DiskRecord {
    pub(super) original: Record,
    job: Mutex<Option<ChunkDiskJob>>,
    runtime: Arc<DiskRuntime>,
    encoded: OnceLock<Option<Arc<NamedTag>>>,
}

impl DiskRecord {
    // Readers may wait for the owning writer, but never execute save callbacks.
    pub(super) fn read(&self) -> Option<Arc<NamedTag>> {
        self.encoded.wait().clone()
    }
}

pub(super) fn prepare_batch(batch: &[QueuedWrite]) {
    // Coalescing may skip a disk write, never the callback of a captured record.
    for write in batch {
        if let Record::Disk(record) = &write.record {
            record.encoded.get_or_init(|| {
                let job = record
                    .job
                    .lock()
                    .unwrap()
                    .take()
                    .expect("captured disk job");
                let entries = record.runtime.run(job);
                record.original.tag().map(|mut tag| {
                    replace_entries(Arc::make_mut(&mut tag), entries);
                    tag
                })
            });
        }
    }
}

fn replace_entries(tag: &mut NamedTag, entries: Compound) {
    let Tag::Compound(root) = &mut tag.tag else {
        log::error!("chunk disk attachments: invalid chunk root; keeping the original record");
        return;
    };
    if entries.is_empty() {
        root.remove(&ATTACHMENTS_KEY.into());
    } else {
        root.insert(ATTACHMENTS_KEY.into(), Tag::Compound(entries));
    }
}

impl SaveDir {
    /// Bind disk callbacks before this server level starts any chunk workers.
    /// The world epoch belongs to the existing lifecycle owner. Remote caches
    /// never call this method. With no subscribers no context is retained.
    pub fn set_chunk_disk_context(
        &self,
        world: WorldEpoch,
        dimension: mod_api::Identifier,
        resources: Arc<sunrise_world::native_load_context::WorldgenContext>,
    ) {
        if !mod_api::__private::listened(mod_api::HookId::ChunkDataLoad)
            && !mod_api::__private::listened(mod_api::HookId::ChunkDataSave)
        {
            return;
        }
        let _ = self.disk.set(Arc::new(DiskRuntime {
            world,
            dimension,
            resources,
        }));
    }

    pub(super) fn capture_disk_record(&self, record: Record) -> Record {
        let Some(runtime) = self.disk.get() else {
            return record;
        };
        if !mod_api::__private::listened(mod_api::HookId::ChunkDataSave) {
            return record;
        }
        let (chunk, kind) = match &record {
            Record::Chunk(chunk, ..) => (&**chunk, ChunkDiskKind::Full),
            Record::Proto(proto, ..) => (&proto.column, ChunkDiskKind::Proto),
            _ => return record,
        };
        let Some(job) = runtime.capture(chunk, kind, ChunkDiskPhase::Save) else {
            return record;
        };
        Record::Disk(Arc::new(DiskRecord {
            original: record,
            job: Mutex::new(Some(job)),
            runtime: runtime.clone(),
            encoded: OnceLock::new(),
        }))
    }

    pub(super) fn load_disk_attachments(&self, chunk: &mut Chunk, kind: ChunkDiskKind) {
        let Some(runtime) = self.disk.get() else {
            // This save directory can also be a non-authoritative remote cache.
            return;
        };
        if !mod_api::__private::listened(mod_api::HookId::ChunkDataLoad) {
            return;
        }
        if let Some(job) = runtime.capture(chunk, kind, ChunkDiskPhase::Load) {
            let entries = runtime.run(job);
            Arc::make_mut(chunk.saved.get_or_insert_with(Default::default)).attachments =
                Attachments::from_nbt(entries);
        }
    }

    pub(super) fn save_disk_sync(
        &self,
        chunks: &[Arc<Chunk>],
        registry: &BlockRegistry,
        biomes: &Biomes,
        items: &crate::items::Items,
    ) -> Option<Result<usize>> {
        let runtime = self.disk.get()?;
        if !mod_api::__private::listened(mod_api::HookId::ChunkDataSave) {
            return None;
        }
        Some((|| {
            let records = chunks
                .iter()
                .map(|chunk| {
                    let entries = runtime
                        .capture(chunk, ChunkDiskKind::Full, ChunkDiskPhase::Save)
                        .map(|job| runtime.run(job));
                    let blending = self.calculated_blending(chunk.x, chunk.z);
                    let mut tag = encode_saved(
                        chunk,
                        registry,
                        biomes,
                        self.tables.get().map(Arc::as_ref),
                        self.shape,
                        blending.as_deref(),
                        items,
                    );
                    if let Some(entries) = entries {
                        replace_entries(&mut tag, entries);
                    }
                    ((chunk.x, chunk.z), Some(tag))
                })
                .collect();
            self.write_records("region", records, false)?;
            self.save_poi(chunks, false)?;
            self.flush()?;
            Ok(chunks.len())
        })())
    }
}
