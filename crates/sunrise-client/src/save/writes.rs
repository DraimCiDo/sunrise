//! The writer thread: what a folder queues to be written, and waiting for it to be on disk.
use super::*;
mod saved_portals;

/// The background thread that encodes chunks and writes them into the region files, grouped
/// by folder within each drained pass. Submission order is preserved within each folder.
pub(super) struct Writer {
    send: mpsc::Sender<WriteBatch>,
    thread: std::thread::JoinHandle<()>,
}

struct WriteBatch {
    folder: Folder,
    jobs: Vec<QueuedWrite>,
    completion: BatchCompletion,
}

#[derive(Default)]
struct WriteGroup {
    jobs: Vec<QueuedWrite>,
    completion: Option<BatchCompletion>,
}

/// Owns a submission's contribution to both writer counts from send to completion.
/// On unwind, this accounts for the current group, unprocessed groups and queued messages.
struct BatchCompletion {
    outstanding: Arc<Outstanding>,
    chunks: usize,
    batches: usize,
}

impl BatchCompletion {
    fn merge(&mut self, mut other: Self) {
        self.chunks += other.chunks;
        self.batches += std::mem::take(&mut other.batches);
    }
}

impl Drop for BatchCompletion {
    fn drop(&mut self) {
        if self.batches == 0 {
            return;
        }
        let outstanding = &self.outstanding;
        if std::thread::panicking() {
            outstanding
                .error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get_or_insert_with(|| "the region writer panicked".into());
        }
        let mut count = outstanding
            .count
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        outstanding.chunks.fetch_sub(self.chunks, Ordering::Relaxed);
        *count -= self.batches;
        outstanding.done.notify_all();
    }
}

/// The sole owner of completion for a queued record. Pending readers borrow
/// only the record; dropping this job releases them even on cancellation,
/// coalescing, or an unwind before the record reaches the writer.
pub(super) struct QueuedWrite {
    pub(super) key: (i32, i32),
    pub(super) record: Record,
    seq: u64,
}

impl QueuedWrite {
    pub(super) fn new(key: (i32, i32), record: Record, seq: u64) -> Self {
        Self {
            key,
            record: record.track_block_entity_edits(),
            seq,
        }
    }
}

impl Drop for QueuedWrite {
    fn drop(&mut self) {
        if let Some(disk) = self.record.disk_job() {
            disk.cancel();
        }
        self.record.resolve_block_entity_edits(false);
    }
}

/// Writes still queued or running, with a condition to wait for none.
#[derive(Default)]
pub(super) struct Outstanding {
    count: Mutex<usize>,
    chunks: AtomicUsize,
    pub(super) error: Mutex<Option<String>>,
    done: Condvar,
    /// What the folder's writes make stale: the portal blocks a search read from saved chunks.
    portals: saved_portals::SavedPortals,
}

#[derive(Clone)]
pub(super) struct SaveReporting {
    world_root: PathBuf,
    dimension: &'static str,
    pub(super) reporter: Arc<crate::chunk_io::SaveFailureReporter>,
}

impl SaveReporting {
    pub(super) fn new(
        world_root: PathBuf,
        dimension: &'static str,
        reporter: Arc<crate::chunk_io::SaveFailureReporter>,
    ) -> Self {
        Self {
            world_root,
            dimension,
            reporter,
        }
    }

    fn report(&self, folder: Folder, key: (i32, i32), error: &std::io::Error) {
        let label = match folder {
            Folder::Region => "chunk",
            Folder::Entities => "entities",
            Folder::Poi => "poi",
        };
        crate::chunk_io::report_save_failure(
            &self.reporter,
            &self.world_root,
            self.dimension,
            label,
            key,
            error,
        );
    }
}

impl SaveDir {
    pub(super) fn report_record_failure(
        &self,
        folder: Folder,
        key: (i32, i32),
        error: &std::io::Error,
    ) {
        self.save_reporting.report(folder, key, error);
    }

    /// Queue `records` for the writer thread; with `wait`, return once
    /// they and every earlier write are on disk.
    pub(super) fn write_records(
        &self,
        folder: &str,
        records: Vec<((i32, i32), Option<NamedTag>)>,
        wait: bool,
    ) -> Result<()> {
        let records = records
            .into_iter()
            .map(|(k, t)| (k, t.map_or(Record::Delete, |t| Record::Tag(Arc::new(t)))))
            .collect();
        self.queue(Folder::of(folder), records, wait)
    }
    pub(super) fn queue(
        &self,
        folder: Folder,
        records: Vec<((i32, i32), Record)>,
        wait: bool,
    ) -> Result<()> {
        if !records.is_empty() {
            let mut writer = self.writer.lock().map_err(|_| "writer poisoned")?;
            let records = if self.disk.get().is_some()
                || mod_api::__private::listened(mod_api::HookId::ChunkDataSave)
            {
                records
                    .into_iter()
                    .map(|(key, record)| {
                        self.capture_disk_record(record).map(|record| (key, record))
                    })
                    .collect::<Result<Vec<_>>>()?
            } else {
                records
            };
            let writer = writer.get_or_insert_with(|| self.start_writer());
            let mut batch = Vec::with_capacity(records.len());
            {
                let mut pending = self.pending.lock().map_err(|_| "pending writes poisoned")?;
                for (key, record) in records {
                    let job = QueuedWrite::new(
                        key,
                        record,
                        self.sequence.fetch_add(1, Ordering::Relaxed),
                    );
                    pending.insert((folder, key.0, key.1), (job.seq, job.record.clone()));
                    batch.push(job);
                }
            }
            // After the pending writes: a search reads the new chunks from here on.
            self.outstanding
                .portals
                .written(folder, batch.iter().map(|job| job.key));
            *self
                .outstanding
                .count
                .lock()
                .map_err(|_| "writer poisoned")? += 1;
            let chunks = if folder == Folder::Region {
                batch.len()
            } else {
                0
            };
            self.outstanding.chunks.fetch_add(chunks, Ordering::Relaxed);
            if let Err(unsent) = writer.send.send(WriteBatch {
                folder,
                jobs: batch,
                completion: BatchCompletion {
                    outstanding: self.outstanding.clone(),
                    chunks,
                    batches: 1,
                },
            }) {
                self.outstanding
                    .error
                    .lock()
                    .map_err(|_| "writer poisoned")?
                    .get_or_insert_with(|| "the region writer stopped".into());
                drop(unsent);
                return Err("the region writer stopped".into());
            }
        }
        if wait {
            self.flush()?;
        }
        Ok(())
    }
    fn start_writer(&self) -> Writer {
        let (send, receive) = mpsc::channel::<WriteBatch>();
        let root = self.root.clone();
        let reporting = self.save_reporting.clone();
        let (regions, entity_regions, poi_regions) = (
            self.regions.clone(),
            self.entity_regions.clone(),
            self.poi_regions.clone(),
        );
        let pending = self.pending.clone();
        // The writer runs chunk_data_save callbacks before handing records to compression.
        let thread = sunrise_guest_stack::builder()
            .name("sunrise-region-writer".into())
            .spawn(move || {
                const MAX_PASS_CHUNKS: usize = 512;
                const KINDS: [Folder; 3] = [Folder::Region, Folder::Entities, Folder::Poi];
                let slot = |kind: Folder| match kind {
                    Folder::Region => 0,
                    Folder::Entities => 1,
                    Folder::Poi => 2,
                };
                while let Ok(first) = receive.recv() {
                    let mut groups: [WriteGroup; 3] = Default::default();
                    let mut total = 0;
                    let mut next = Some(first);
                    while let Some(WriteBatch {
                        folder,
                        jobs,
                        completion,
                    }) = next.take()
                    {
                        total += jobs.len();
                        let group = &mut groups[slot(folder)];
                        group.jobs.extend(jobs);
                        if let Some(group) = &mut group.completion {
                            group.merge(completion);
                        } else {
                            group.completion = Some(completion);
                        }
                        if total >= MAX_PASS_CHUNKS {
                            break;
                        }
                        next = receive.try_recv().ok();
                    }
                    for (i, group) in groups.into_iter().enumerate() {
                        let batch = &group.jobs;
                        if batch.is_empty() {
                            continue;
                        }
                        let kind = KINDS[i];
                        let cache = match kind {
                            Folder::Region => &regions,
                            Folder::Entities => &entity_regions,
                            Folder::Poi => &poi_regions,
                        };
                        let folder = kind.name();
                        let newest = |k: &(i32, i32), seq: u64| {
                            pending.lock().map_or(true, |p| {
                                p.get(&(kind, k.0, k.1)).is_none_or(|p| p.0 <= seq)
                            })
                        };
                        let resolved: Vec<_> = batch
                            .iter()
                            .filter_map(|job| {
                                let newest = newest(&job.key, job.seq);
                                // Coalescing skips the old write, not its captured disk callback.
                                // For current records, compress_record sees the cached outcome.
                                if job.record.disk_job().is_some()
                                    && let Err(error) = chunk_disk::encode_record(&job.record)
                                    && !newest
                                {
                                    reporting.report(kind, job.key, &error);
                                }
                                if !newest {
                                    return None;
                                }
                                match compression_pool::catch_encoding_panic(|| {
                                    Ok(resolve_poi(&root, cache, job.key, &job.record))
                                }) {
                                    Ok(record) => record.map(|record| (job.key, record)),
                                    Err(error) => {
                                        reporting.report(kind, job.key, &error);
                                        None
                                    }
                                }
                            })
                            .collect();
                        let records: Vec<_> = resolved.iter().map(|(k, r)| (*k, r)).collect();
                        for (key, error) in write_region_records(&root, cache, folder, &records) {
                            reporting.report(kind, key, &error);
                        }
                        if let Ok(mut pending) = pending.lock() {
                            for job in batch {
                                let key = (kind, job.key.0, job.key.1);
                                if pending.get(&key).is_some_and(|p| p.0 == job.seq) {
                                    // Like IOWorker, retire both successful and failed stores. A
                                    // reader that already borrowed the snapshot still owns it.
                                    pending.remove(&key);
                                }
                            }
                        }
                    }
                }
            })
            .expect("spawn region writer");
        Writer { send, thread }
    }
    /// Number of failed records reported for this world. Checkpoints use a before/after
    /// difference for diagnostics; this count does not change their save result.
    pub fn record_save_failures(&self) -> u64 {
        self.save_failure_reporter().reported_failures()
    }

    /// Wait until every queued write finishes. Individual failures are reported by the
    /// writer; only a failure of the writer itself fails this barrier.
    pub fn flush(&self) -> Result<()> {
        let mut count = self
            .outstanding
            .count
            .lock()
            .map_err(|_| "writer poisoned")?;
        while *count > 0 {
            count = self
                .outstanding
                .done
                .wait(count)
                .map_err(|_| "writer poisoned")?;
        }
        self.write_result()
    }
    /// Wait until at most `low` chunk records are still queued, without waiting
    /// for all of them; a save pass resuming at half the cap keeps the writer fed
    /// without stalling for records it did not have to.
    ///
    /// The predicate is `chunks`, not `count`: `count` holds queued batches and
    /// one batch can carry many records, so waiting on it would let the record
    /// cap (`MAX_ASYNC_CHUNK_WRITES`) drift with how the caller batches.
    pub(super) fn wait_for_chunk_writes_below(&self, low: usize) -> Result<()> {
        let mut count = self
            .outstanding
            .count
            .lock()
            .map_err(|_| "writer poisoned")?;
        // The writer lowers `chunks` before it takes this same mutex to notify,
        // so a wakeup cannot fall between the check and the wait.
        while self.outstanding.chunks.load(Ordering::Relaxed) > low {
            count = self
                .outstanding
                .done
                .wait(count)
                .map_err(|_| "writer poisoned")?;
        }
        self.write_result()
    }
    /// `ChunkMap.activeChunkWrites`: chunk records, excluding entity/POI IO.
    pub fn active_chunk_writes(&self) -> usize {
        self.outstanding.chunks.load(Ordering::Relaxed)
    }
    /// Poll durability without holding up the event loop.
    pub fn writes_finished(&self) -> Result<bool> {
        let count = self
            .outstanding
            .count
            .lock()
            .map_err(|_| "writer poisoned")?;
        self.write_result()?;
        Ok(*count == 0)
    }
    fn write_result(&self) -> Result<()> {
        if let Some(error) = &*self
            .outstanding
            .error
            .lock()
            .map_err(|_| "writer poisoned")?
        {
            return Err(error.clone().into());
        }
        Ok(())
    }
}

impl Drop for SaveDir {
    /// Finish queued writes before the world is let go.
    fn drop(&mut self) {
        if let Some(writer) = self.writer.get_mut().ok().and_then(Option::take) {
            drop(writer.send);
            let _ = writer.thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(debug_assertions)]
    fn a_poi_resolution_panic_refuses_only_that_record() {
        let dir = tempfile::tempdir().unwrap();
        let open = || SaveDir::new(dir.path().to_path_buf(), LevelShape::OVERWORLD, None);
        let tables = Arc::new(SaveTables {
            block_names: Vec::new(),
            block_ids: HashMap::new(),
            default_state: Vec::new(),
            block_entity: Vec::new(),
            heightmaps: crate::assets::Properties::for_tests(&[]),
            air: 0,
            poi: vec![Some(("minecraft:home", 1))],
        });
        // Resolving a POI for this synthetic out-of-world chunk overflows its block position.
        let key = (i32::MAX, 0);
        let invalid = Record::Poi(Arc::new(Chunk::empty(key.0, key.1)), tables, None);
        let good = Arc::new(NamedTag {
            name: "".into(),
            tag: Tag::compound([("marker", Tag::Int(3))]),
        });
        let save = open();
        save.queue(
            Folder::Poi,
            vec![(key, invalid), ((0, 0), Record::Tag(good.clone()))],
            true,
        )
        .unwrap();
        assert_eq!(save.record_save_failures(), 1);
        let disk = open();
        assert_eq!(
            disk.read_record_raw("poi", 0, 0).as_ref(),
            Some(good.as_ref())
        );
        assert!(disk.read_record_raw("poi", key.0, key.1).is_none());
    }

    #[test]
    fn a_failed_send_leaves_the_pending_disk_snapshot_readable() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, biomes) = super::super::loot_tests::fixture();
        let registry = Box::leak(Box::new(registry));
        let biomes = Arc::new(biomes);
        super::super::chunk_disk_tests::with_listener(|saves, _| {
            let save = SaveDir::new(
                dir.path().to_path_buf(),
                LevelShape::OVERWORLD,
                Some(Default::default()),
            );
            super::super::chunk_disk_tests::context(&save);
            let (send, receive) = mpsc::channel();
            drop(receive);
            *save.writer.lock().unwrap() = Some(Writer {
                send,
                thread: std::thread::spawn(|| {}),
            });

            let key = (2, -3);
            let mut chunk = Chunk::empty(key.0, key.1);
            let mut extras = ChunkExtras::default();
            extras.root.insert("test:marker".into(), Tag::Int(42));
            chunk.saved = Some(Arc::new(extras));
            assert!(
                save.save_later(&[Arc::new(chunk)], registry, &biomes)
                    .is_err()
            );
            assert!(
                save.pending.lock().unwrap()[&(Folder::Region, key.0, key.1)]
                    .1
                    .disk_job()
                    .is_some()
            );

            let (send, receive) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                send.send(save.raw_chunk(key.0, key.1)).unwrap();
            });
            let snapshot = receive
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("pending reader waited for a disconnected writer")
                .unwrap();
            reader.join().unwrap();
            assert_eq!(snapshot.tag.get("test:marker"), Some(&Tag::Int(42)));
            assert_eq!(saves.load(Ordering::Relaxed), 0);
        });
    }

    #[test]
    fn an_unexpected_writer_panic_releases_both_waits() {
        let dir = tempfile::tempdir().unwrap();
        let save = SaveDir::new(dir.path().to_path_buf(), LevelShape::OVERWORLD, None);
        let outstanding = save.outstanding.clone();
        *outstanding.count.lock().unwrap() = 1;
        outstanding.chunks.store(1, Ordering::Relaxed);
        let writer = std::thread::spawn(move || {
            let _completion = BatchCompletion {
                outstanding,
                chunks: 1,
                batches: 1,
            };
            panic!("injected unexpected writer panic");
        });
        let (send, receive) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            assert!(save.flush().is_err());
            assert_eq!(save.active_chunk_writes(), 0);
            assert!(save.wait_for_chunk_writes_below(0).is_err());
            send.send(()).unwrap();
        });
        assert!(writer.join().is_err());
        receive
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        waiter.join().unwrap();
    }
}
