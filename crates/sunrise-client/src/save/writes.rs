//! The writer thread: what a folder queues to be written, and waiting for it to be on disk.
use super::*;

mod saved_portals;

/// The background thread that encodes chunks and writes them into the region files, grouped
/// by folder within each drained pass. Submission order is preserved within each folder.
pub(super) struct Writer {
    send: mpsc::Sender<WriteBatch>,
    thread: std::thread::JoinHandle<()>,
}

/// One submitted record. Its sequence identifies the matching pending read.
pub(super) struct QueuedWrite {
    pub(super) key: (i32, i32),
    pub(super) record: Record,
    seq: u64,
}

/// Writes still queued or running, with a condition to wait for none.
#[derive(Default)]
pub(super) struct Outstanding {
    count: Mutex<usize>,
    chunks: AtomicUsize,
    /// First failure not yet returned by a durability barrier. Every failure is also reported.
    pub(super) error: Mutex<Option<String>>,
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
            let batch: Vec<_> = records
                .into_iter()
                .map(|(key, record)| QueuedWrite {
                    key,
                    record: self.capture_disk_record(record),
                    seq: self.sequence.fetch_add(1, Ordering::Relaxed),
                })
                .collect();
            {
                let mut pending = self.pending.lock().map_err(|_| "pending writes poisoned")?;
                for job in &batch {
                    pending.insert(
                        (folder, job.key.0, job.key.1),
                        (job.seq, job.record.clone()),
                    );
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
            if let Err(mpsc::SendError((_, batch))) = writer.send.send((folder, batch)) {
                retire_pending(&self.pending, folder, &batch);
                self.outstanding.chunks.fetch_sub(chunks, Ordering::Relaxed);
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
        let reports = self.chunk_reports.clone();
        let (regions, entity_regions, poi_regions) = (
            self.regions.clone(),
            self.entity_regions.clone(),
            self.poi_regions.clone(),
        );
        let pending = self.pending.clone();
        let outstanding = self.outstanding.clone();
        // The writer runs chunk_data_save callbacks before handing records to compression.
        let thread = std::thread::Builder::new()
            .stack_size(sunrise_threads::GAME_THREAD_STACK_BYTES)
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
                            .filter(|job| newest(&job.key, job.seq))
                            .filter_map(|job| {
                                Some((job.key, resolve_poi(&root, cache, job.key, &job.record)?))
                            })
                            .collect();
                        let records: Vec<_> = resolved.iter().map(|(k, r)| (*k, r)).collect();
                        chunk_disk::prepare_batch(&batch);
                        let failures = write_region_records(&root, cache, folder, &records);
                        retire_pending(&pending, kind, &batch);
                        for ((x, z), error) in &failures {
                            reports.report_save_failure(&root, *x, *z, error);
                        }
                        let chunk_count = batch.len();
                        drop(batch);
                        if kind == Folder::Region {
                            outstanding.chunks.fetch_sub(chunk_count, Ordering::Relaxed);
                        }
                        if let Ok(mut count) = outstanding.count.lock() {
                            if let Some(((x, z), error)) = failures.first() {
                                outstanding.error.lock().unwrap().get_or_insert_with(|| {
                                    format!("failed to write {folder} chunk {x},{z}: {error}")
                                });
                            }
                            *count -= batch_count;
                            outstanding.done.notify_all();
                        }
                    }
                }
            })
            .expect("spawn region writer");
        Writer { send, thread }
    }
    /// Wait until every queued write is on disk.
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
        // This is backpressure, not a durability barrier. The writer reports completed
        // failures separately; none should prevent submission of unrelated records.
        Ok(())
    }
    /// `ChunkMap.activeChunkWrites`: chunk records, excluding entity/POI IO.
    pub fn active_chunk_writes(&self) -> usize {
        self.outstanding.chunks.load(Ordering::Relaxed)
    }
    /// Poll the current durability barrier without holding up the event loop.
    /// A completed barrier reports accumulated failures once.
    pub fn writes_finished(&self) -> Result<bool> {
        let count = self
            .outstanding
            .count
            .lock()
            .map_err(|_| "writer poisoned")?;
        if *count != 0 {
            return Ok(false);
        }
        self.write_result()?;
        Ok(true)
    }
    fn write_result(&self) -> Result<()> {
        match self
            .outstanding
            .error
            .lock()
            .map_err(|_| "writer poisoned")?
            .take()
        {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }
}

/// A completed snapshot no longer shadows disk, even if the write failed. Leave a newer
/// submission at the same coordinates visible to readers.
fn retire_pending(pending: &PendingWrites, folder: Folder, batch: &[QueuedWrite]) {
    if let Ok(mut pending) = pending.lock() {
        for job in batch {
            let key = (folder, job.key.0, job.key.1);
            if pending.get(&key).is_some_and(|p| p.0 == job.seq) {
                pending.remove(&key);
            }
        }
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
