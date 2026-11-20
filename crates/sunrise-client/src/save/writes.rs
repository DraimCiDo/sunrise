//! The writer thread: what a folder queues to be written, and waiting for it to be on disk.
use super::*;

mod saved_portals;

/// The background thread that encodes chunks and writes them into the region files, grouped
/// by folder within each drained pass. Submission order is preserved within each folder.
pub(super) struct Writer {
    send: mpsc::Sender<WriteBatch>,
    thread: std::thread::JoinHandle<()>,
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
        self.record.resolve_block_entity_edits(false);
    }
}

/// Writes still queued or running, with a condition to wait for none.
#[derive(Default)]
pub(super) struct Outstanding {
    count: Mutex<usize>,
    chunks: AtomicUsize,
    pub(super) error: Mutex<Option<String>>,
    /// Record encoding failures, kept until a newer record reaches disk at the same position.
    record_errors: Mutex<BTreeMap<(&'static str, i32, i32), std::io::Error>>,
    /// Terrain records the game needs to mark dirty again, with or without a disk callback.
    failed_saves: Mutex<Vec<(i32, i32)>>,
    done: Condvar,
    /// What the folder's writes make stale: the portal blocks a search read from saved chunks.
    portals: saved_portals::SavedPortals,
}

impl SaveDir {
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
                let keys: Vec<_> = records.iter().map(|(key, _)| *key).collect();
                match records
                    .into_iter()
                    .map(|(key, record)| {
                        self.capture_disk_record(record).map(|record| (key, record))
                    })
                    .collect::<Result<Vec<_>>>()
                {
                    Ok(records) => records,
                    Err(error) => {
                        // A refused capture must leave its chunks dirty.
                        if folder == Folder::Region
                            && let Ok(mut failed) = self.outstanding.failed_saves.lock()
                        {
                            failed.extend(keys);
                        }
                        return Err(error);
                    }
                }
            } else {
                records
            };
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
            let writer = writer.get_or_insert_with(|| self.start_writer());
            let chunks = if folder == Folder::Region {
                batch.len()
            } else {
                0
            };
            self.outstanding.chunks.fetch_add(chunks, Ordering::Relaxed);
            if writer.send.send((folder, batch)).is_err() {
                self.outstanding.chunks.fetch_sub(chunks, Ordering::Relaxed);
                *self
                    .outstanding
                    .error
                    .lock()
                    .map_err(|_| "writer poisoned")? = Some("the region writer stopped".into());
                *self
                    .outstanding
                    .count
                    .lock()
                    .map_err(|_| "writer poisoned")? -= 1;
                self.outstanding.done.notify_all();
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
        let (regions, entity_regions, poi_regions) = (
            self.regions.clone(),
            self.entity_regions.clone(),
            self.poi_regions.clone(),
        );
        let pending = self.pending.clone();
        let outstanding = self.outstanding.clone();
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
                    let mut groups: [(Vec<QueuedWrite>, usize); 3] = Default::default();
                    let mut total = 0;
                    let mut next = Some(first);
                    while let Some((kind, batch)) = next.take() {
                        total += batch.len();
                        let group = &mut groups[slot(kind)];
                        group.0.extend(batch);
                        group.1 += 1;
                        if total >= MAX_PASS_CHUNKS {
                            break;
                        }
                        next = receive.try_recv().ok();
                    }
                    for (i, (batch, batch_count)) in groups.into_iter().enumerate() {
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
                                    let (x, z) = job.key;
                                    crate::chunk_io::report_save_failure(&root, x, z, &error);
                                }
                                if !newest {
                                    return None;
                                }
                                Some((job.key, resolve_poi(&root, cache, job.key, &job.record)?))
                            })
                            .collect();
                        let records: Vec<_> = resolved.iter().map(|(k, r)| (*k, r)).collect();
                        match write_region_records(&root, cache, folder, &records) {
                            Ok(failed) => {
                                if let Ok(mut pending) = pending.lock() {
                                    for job in &batch {
                                        let key = (kind, job.key.0, job.key.1);
                                        if !failed.contains_key(&job.key)
                                            && pending.get(&key).is_some_and(|p| p.0 == job.seq)
                                        {
                                            pending.remove(&key);
                                        }
                                    }
                                }
                                if kind == Folder::Region && !failed.is_empty() {
                                    outstanding
                                        .failed_saves
                                        .lock()
                                        .unwrap()
                                        .extend(failed.keys().copied());
                                }
                                let mut errors = outstanding.record_errors.lock().unwrap();
                                for &((x, z), _) in &records {
                                    errors.remove(&(folder, x, z));
                                }
                                errors.extend(
                                    failed
                                        .into_iter()
                                        .map(|((x, z), error)| ((folder, x, z), error)),
                                );
                            }
                            Err(e) => {
                                if kind == Folder::Region {
                                    outstanding
                                        .failed_saves
                                        .lock()
                                        .unwrap()
                                        .extend(records.iter().map(|(key, _)| *key));
                                }
                                let message = format!("failed to write {folder} files: {e}");
                                log::warn!("Sunrise: {message}");
                                *outstanding.error.lock().unwrap() = Some(message);
                                for job in &batch {
                                    crate::chunk_io::report_save_failure(
                                        &root, job.key.0, job.key.1, &e,
                                    );
                                }
                            }
                        }
                        let chunk_count = batch.len();
                        drop(batch);
                        if kind == Folder::Region {
                            outstanding.chunks.fetch_sub(chunk_count, Ordering::Relaxed);
                        }
                        if let Ok(mut count) = outstanding.count.lock() {
                            *count -= batch_count;
                            outstanding.done.notify_all();
                        }
                    }
                }
            })
            .expect("spawn region writer");
        Writer { send, thread }
    }
    /// Take the terrain positions to mark dirty again after unsuccessful writes.
    pub fn take_failed_chunk_saves(&self) -> Vec<(i32, i32)> {
        self.outstanding
            .failed_saves
            .lock()
            .map_or_else(|_| Vec::new(), |mut failed| std::mem::take(&mut *failed))
    }

    /// Wait until every queued write is on disk, or report what could not be saved.
    pub fn flush(&self) -> Result<()> {
        self.flush_writes(true)
    }

    /// Wait for all writes at shutdown. Individual encoding failures are already logged;
    /// a failure to write or sync the storage itself is still an error.
    pub fn flush_for_shutdown(&self) -> Result<()> {
        self.flush_writes(false)
    }

    fn flush_writes(&self, report_record_errors: bool) -> Result<()> {
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
        self.write_result(report_record_errors)
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
        // A bad record must not block submitting new records (including retries).
        self.write_result(false)
    }
    /// `ChunkMap.activeChunkWrites`: chunk records, excluding entity/POI IO.
    pub fn active_chunk_writes(&self) -> usize {
        self.outstanding.chunks.load(Ordering::Relaxed)
    }
    /// Poll durability without holding up the event loop.
    pub fn writes_finished(&self) -> Result<bool> {
        self.poll_writes(true)
    }

    /// Poll a closing save without escalating individual encoding failures.
    pub fn writes_finished_for_shutdown(&self) -> Result<bool> {
        self.poll_writes(false)
    }

    fn poll_writes(&self, report_record_errors: bool) -> Result<bool> {
        let count = self
            .outstanding
            .count
            .lock()
            .map_err(|_| "writer poisoned")?;
        self.write_result(report_record_errors && *count == 0)?;
        Ok(*count == 0)
    }
    fn write_result(&self, report_record_errors: bool) -> Result<()> {
        if let Some(error) = &*self
            .outstanding
            .error
            .lock()
            .map_err(|_| "writer poisoned")?
        {
            return Err(error.clone().into());
        }
        if report_record_errors {
            let errors = self
                .outstanding
                .record_errors
                .lock()
                .map_err(|_| "writer poisoned")?;
            if !errors.is_empty() {
                use std::fmt::Write;
                let mut message = String::from("failed to encode chunk records:");
                for ((folder, x, z), error) in errors.iter() {
                    let _ = write!(message, " {folder} {x},{z}: {error};");
                }
                return Err(message.into());
            }
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
