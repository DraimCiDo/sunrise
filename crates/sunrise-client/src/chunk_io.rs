//! `IntegratedServer.reportChunkLoadFailure` / `reportChunkSaveFailure` /
//! `sendLowDiskSpaceWarning`: the region storage runs on the server thread and
//! its writer thread, and the toasts are the window's. Each failure is logged
//! and checked for low disk space (`MinecraftServer.warnOnLowDiskSpace`). The
//! window drains its queued notifications every frame (`Minecraft.execute`).
//! Save failures also leave server diagnostics without stopping the save.
//! `SystemToast.addOrUpdate` makes a repeat reset the toast it already shows,
//! so only the newest chunk of each kind is kept.
use crate::{save, text::Text, toasts::SystemToast};
use chrono::{DateTime, Local};
use std::{
    collections::VecDeque,
    error::Error,
    fmt::Write as _,
    fs, io,
    path::Path,
    sync::{Arc, Mutex, PoisonError, Weak},
};

/// What the window has still to toast.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    /// `SystemToast.onLowDiskSpace`.
    pub low_disk_space: bool,
    /// `SystemToast.onChunkLoadFailure`: the newest chunk.
    pub load_failure: Option<(i32, i32)>,
    /// `SystemToast.onChunkSaveFailure`: the newest chunk.
    pub save_failure: Option<(i32, i32)>,
}

static PENDING: Mutex<Pending> = Mutex::new(Pending {
    low_disk_space: false,
    load_failure: None,
    save_failure: None,
});

impl Pending {
    /// `reportChunkLoadFailure` after `warnOnLowDiskSpace` found `low_disk_space`.
    fn load_failed(&mut self, x: i32, z: i32, low_disk_space: bool) {
        self.low_disk_space |= low_disk_space;
        self.load_failure = Some((x, z));
    }

    /// `reportChunkSaveFailure` after `warnOnLowDiskSpace` found `low_disk_space`.
    fn save_failed(&mut self, x: i32, z: i32, low_disk_space: bool) {
        self.low_disk_space |= low_disk_space;
        self.save_failure = Some((x, z));
    }
}

fn with(update: impl FnOnce(&mut Pending)) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    update(&mut pending);
}

/// `MinecraftServer.warnOnLowDiskSpace` for the world folder `dir`.
fn warn_on_low_disk_space(dir: &Path, space: impl FnOnce(&Path) -> u64) -> bool {
    let low = space(dir) < save::LOW_DISK_SPACE;
    if low {
        // `MinecraftServer.sendLowDiskSpaceWarning`.
        log::warn!("Low disk space! Might not be able to save the world.");
    }
    low
}

/// `reportChunkLoadFailure` for the chunk at (`x`, `z`) of the dimension in `dir`.
pub fn report_load_failure(dir: &Path, x: i32, z: i32, error: &dyn std::fmt::Display) {
    log::error!("Failed to load chunk {x},{z}: {error}");
    let low = warn_on_low_disk_space(dir, save::estimate_disk_space);
    with(|p| p.load_failed(x, z, low));
}

const SAVE_LOCATION: &str = "chunk/save";
const LATEST_ENTRY_COUNT: usize = 8;

/// A server's suppressed chunk save failures, shared by all its dimensions.
#[derive(Default)]
pub struct SaveFailureReporter {
    integrated: bool,
    suppressed: Mutex<SuppressedSaveFailures>,
}

/// The launch's current server report source. An integrated server shuts down
/// independently of the launch; a dedicated one may return its error to main.
#[derive(Clone, Default)]
pub struct SaveFailureDiagnostics(Arc<Mutex<Option<ReportSource>>>);

enum ReportSource {
    Integrated(Weak<SaveFailureReporter>),
    Dedicated(Arc<SaveFailureReporter>),
}

impl SaveFailureDiagnostics {
    pub fn register(&self, reporter: &Arc<SaveFailureReporter>) {
        let source = if reporter.integrated {
            ReportSource::Integrated(Arc::downgrade(reporter))
        } else {
            ReportSource::Dedicated(reporter.clone())
        };
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(source);
    }

    /// A fatal report must not wait for locks held by a crashing server.
    pub fn suppressed_summary(&self) -> Option<String> {
        let source = match self.0.try_lock().ok()?.as_ref()? {
            ReportSource::Integrated(reporter) => reporter.upgrade()?,
            ReportSource::Dedicated(reporter) => reporter.clone(),
        };
        Some(source.suppressed.try_lock().ok()?.summary())
    }
}

#[derive(Default)]
struct SuppressedSaveFailures {
    latest: VecDeque<(i64, &'static str, String)>,
    // Small, most recently seen class first, as in SuppressedExceptionCollector.
    counts: Vec<(&'static str, u64)>,
}

struct SaveFailure<'a> {
    world_root: &'a Path,
    dimension: &'a str,
    storage: &'a str,
    pos: (i32, i32),
    error: &'a (dyn Error + 'static),
}

impl SaveFailureReporter {
    /// An integrated server also reports the failure to its local client.
    pub fn new(integrated: bool) -> Self {
        Self {
            integrated,
            ..Self::default()
        }
    }

    /// `SuppressedExceptionCollector.addEntry` for `chunk/save`.
    fn record(&self, timestamp: i64, class: &'static str, message: String) {
        let mut suppressed = self
            .suppressed
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if suppressed.latest.len() == LATEST_ENTRY_COUNT {
            suppressed.latest.pop_front();
        }
        suppressed.latest.push_back((timestamp, class, message));
        let count = suppressed
            .counts
            .iter()
            .position(|&(key, _)| key == class)
            .map_or(0, |index| suppressed.counts.remove(index).1);
        suppressed.counts.insert(0, (class, count + 1));
    }

    /// Total failures reported by this server, including entries no longer in the summary.
    pub fn reported_failures(&self) -> u64 {
        self.suppressed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .counts
            .iter()
            .map(|&(_, count)| count)
            .sum()
    }

    /// `SuppressedExceptionCollector.dump`: the server crash report's `Suppressed Exceptions` detail.
    pub fn suppressed_summary(&self) -> String {
        self.suppressed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .summary()
    }

    fn report(
        &self,
        failure: SaveFailure<'_>,
        pending: &Mutex<Pending>,
        space: impl Fn(&Path) -> io::Result<u64>,
    ) {
        let (x, z) = failure.pos;
        let message = failure.error.to_string();
        if failure.storage == "entities" {
            // `EntityStorage.reportSaveFailureIfPresent` logs before notifying the server.
            log::error!("Failed to store entity chunk [{x}, {z}]: {message}");
        }
        log::error!("Failed to save chunk {x},{z}: {message}");
        let time = Local::now();
        // The write path returns io::Error for record and filesystem failures;
        // opaque internal errors have no concrete Rust name available here.
        let class = if failure.error.is::<io::Error>() {
            "std::io::Error"
        } else {
            "Error"
        };
        self.record(time.timestamp_millis(), class, message);
        if let Err(error) = store_chunk_io_error(&failure, &time, &space) {
            log::warn!("Failed to store chunk IO exception: {error}");
        }
        let estimate = |path: &Path| space(path).unwrap_or(u64::MAX);
        let low = warn_on_low_disk_space(failure.world_root, estimate);
        // IntegratedServer.reportChunkSaveFailure checks again before showing the toast.
        let low_again = self.integrated && warn_on_low_disk_space(failure.world_root, estimate);
        let mut queued = pending.lock().unwrap_or_else(PoisonError::into_inner);
        if self.integrated {
            queued.save_failed(x, z, low || low_again);
        } else {
            // The dedicated server drains this flag to warn its operators.
            queued.low_disk_space |= low;
        }
    }
}

impl SuppressedSaveFailures {
    fn summary(&self) -> String {
        if self.latest.is_empty() {
            return "~~NONE~~".to_owned();
        }
        let now = Local::now().timestamp_millis();
        let mut out = String::from("\n\t\tLatest entries:\n");
        for (timestamp, class, message) in &self.latest {
            let _ = writeln!(
                out,
                "\t\t\t{SAVE_LOCATION}:{class}: {message} ({}ms ago)",
                now - timestamp
            );
        }
        out.push_str("\t\tEntry counts:\n");
        for (class, count) in &self.counts {
            let _ = writeln!(out, "\t\t\t{SAVE_LOCATION}:{class} x {count}");
        }
        out
    }
}

/// `MinecraftServer.storeChunkIoError`: a diagnostic in the world's `debug` folder,
/// separate from fatal crash handling. This runs on the region writer thread.
fn store_chunk_io_error(
    failure: &SaveFailure<'_>,
    time: &DateTime<Local>,
    space: impl FnOnce(&Path) -> io::Result<u64>,
) -> io::Result<()> {
    let debug = failure.world_root.join("debug");
    fs::create_dir_all(&debug)?;
    if space(&debug)? < 8192 {
        log::warn!(
            "Not storing chunk IO report due to low space at {}",
            debug.display()
        );
        return Ok(());
    }
    let level = failure
        .world_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let safe_level = crate::title::sanitize_world_name(&level);
    let date = time.format("%Y-%m-%d_%H.%M.%S");
    let path = debug.join(format!("chunk-{safe_level}-{date}-server.txt"));
    let (x, z) = failure.pos;
    let mut text = format!(
        "---- Minecraft Chunk IO Error Report ----\n\nTime: {}\nDescription: Chunk save failure\n\n{}\n",
        time.format("%Y-%m-%d %H:%M:%S"),
        failure.error,
    );
    let mut source = failure.error.source();
    while let Some(cause) = source {
        let _ = writeln!(text, "Caused by: {cause}");
        source = cause.source();
    }
    let _ = writeln!(
        text,
        "\n-- Chunk Info --\nDetails:\n\tLevel: {level}\n\tDimension: {}\n\tStorage: {}\n\tPosition: [{x}, {z}]",
        failure.dimension, failure.storage,
    );
    fs::write(&path, text)?;
    log::info!("Saved chunk IO details to {}", path.display());
    Ok(())
}

/// `MinecraftServer.reportChunkSaveFailure`: record a failure without retrying or aborting the save.
pub fn report_save_failure(
    reporter: &SaveFailureReporter,
    world_root: &Path,
    dimension: &str,
    storage: &str,
    pos: (i32, i32),
    error: &(dyn Error + 'static),
) {
    reporter.report(
        SaveFailure {
            world_root,
            dimension,
            storage,
            pos,
            error,
        },
        &PENDING,
        |path| fs4::available_space(path),
    );
}

/// `Minecraft.sendLowDiskSpaceWarning`: a server's `ClientboundLowDiskSpaceWarningPacket`
/// (`ClientPacketListener.handleLowDiskSpaceWarning`) toasts like the local report.
pub fn report_low_disk_space() {
    with(|p| p.low_disk_space = true);
}

/// The reports since the last call.
pub fn take() -> Pending {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut *pending)
}

/// `ChatFormatting.RED`.
fn red() -> u32 {
    crate::hud::rgba(255, 85, 85, 255)
}

/// `SystemToast.onLowDiskSpace` (`SystemToastId.LOW_DISK_SPACE`: ten seconds).
pub fn low_disk_space_toast() -> SystemToast {
    SystemToast::new(
        Text::tr("chunk.toast.lowDiskSpace", &[]),
        Some(Text::tr("chunk.toast.lowDiskSpace.description", &[])),
    )
    .with_display_time(SystemToast::LONG_DISPLAY_TIME)
}

fn chunk_toast(key: &str, x: i32, z: i32) -> SystemToast {
    // `Component.translationArg(ChunkPos)` is `ChunkPos.toString`.
    let pos = format!("[{x}, {z}]");
    SystemToast::new(
        Text::tr(key, &[&pos]),
        Some(Text::tr("chunk.toast.checkLog", &[])),
    )
    .with_title_color(red())
}

/// `SystemToast.onChunkLoadFailure`.
pub fn load_failure_toast(x: i32, z: i32) -> SystemToast {
    chunk_toast("chunk.toast.loadFailure", x, z)
}

/// `SystemToast.onChunkSaveFailure`.
pub fn save_failure_toast(x: i32, z: i32) -> SystemToast {
    chunk_toast("chunk.toast.saveFailure", x, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests keep off `PENDING` where they can: the save tests run in
    // the same process and report their own chunk failures into it (#4854).
    #[test]
    fn failures_keep_the_newest_chunk_of_each_kind() {
        let mut pending = Pending::default();
        pending.load_failed(1, 2, false);
        pending.load_failed(3, 4, false);
        pending.save_failed(5, 6, false);
        assert_eq!(
            pending,
            Pending {
                low_disk_space: false,
                load_failure: Some((3, 4)),
                save_failure: Some((5, 6)),
            }
        );
        // A later failure on a full disk raises the warning, and one with
        // space left does not lower it.
        pending.save_failed(7, 8, true);
        pending.load_failed(9, 10, false);
        assert_eq!(
            pending,
            Pending {
                low_disk_space: true,
                load_failure: Some((9, 10)),
                save_failure: Some((7, 8)),
            }
        );
    }

    /// The only test that drains `PENDING`; other tests only add to it, so
    /// the flag it raises is still there when it takes it.
    #[test]
    fn a_servers_warning_packet_toasts_low_disk_space() {
        report_low_disk_space();
        assert!(take().low_disk_space);
    }

    #[test]
    fn integrated_save_failure_logs_records_reports_and_toasts() {
        crate::test_log::install();
        crate::test_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let world = tmp.path().join("My.World");
        let pending = Mutex::new(Pending::default());
        let reporter = SaveFailureReporter::new(true);
        reporter.report(
            SaveFailure {
                world_root: &world,
                dimension: "minecraft:the_nether",
                storage: "poi",
                pos: (-2, 5),
                error: &io::Error::other("refused POI encoding"),
            },
            &pending,
            |_| Ok(8192),
        );

        let records = crate::test_log::records();
        assert!(records.contains(&(
            log::Level::Error,
            "Failed to save chunk -2,5: refused POI encoding".to_owned(),
        )));
        assert!(
            records
                .iter()
                .any(|(level, text)| *level == log::Level::Warn
                    && text == "Low disk space! Might not be able to save the world.")
        );
        assert_eq!(
            pending.into_inner().unwrap(),
            Pending {
                low_disk_space: true,
                save_failure: Some((-2, 5)),
                load_failure: None
            }
        );
        assert_eq!(reporter.reported_failures(), 1);
        assert!(
            reporter
                .suppressed_summary()
                .contains("chunk/save:std::io::Error: refused POI encoding")
        );

        let mut files = fs::read_dir(world.join("debug")).unwrap();
        let path = files.next().unwrap().unwrap().path();
        assert!(files.next().is_none());
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("chunk-My_World-") && name.ends_with("-server.txt"));
        let text = fs::read_to_string(path).unwrap();
        for expected in [
            "Minecraft Chunk IO Error Report",
            "Description: Chunk save failure",
            "refused POI encoding",
            "Level: My.World",
            "Dimension: minecraft:the_nether",
            "Storage: poi",
            "Position: [-2, 5]",
        ] {
            assert!(text.contains(expected), "report is missing {expected}");
        }
    }

    #[test]
    fn low_space_refuses_only_the_report_file_and_dedicated_does_not_toast() {
        crate::test_log::install();
        crate::test_log::clear();
        let world = tempfile::tempdir().unwrap();
        let pending = Mutex::new(Pending::default());
        let reporter = SaveFailureReporter::new(false);
        reporter.report(
            SaveFailure {
                world_root: world.path(),
                dimension: "minecraft:overworld",
                storage: "chunk",
                pos: (3, 4),
                error: &io::Error::other("write refused"),
            },
            &pending,
            |_| Ok(8191),
        );
        assert!(
            fs::read_dir(world.path().join("debug"))
                .unwrap()
                .next()
                .is_none()
        );
        assert_eq!(reporter.reported_failures(), 1);
        assert_eq!(
            pending.into_inner().unwrap(),
            Pending {
                low_disk_space: true,
                ..Pending::default()
            }
        );
        let records = crate::test_log::records();
        assert!(
            records
                .iter()
                .any(|(level, text)| *level == log::Level::Warn
                    && text.starts_with("Not storing chunk IO report due to low space"))
        );
        assert!(
            records
                .iter()
                .any(|(level, text)| *level == log::Level::Warn
                    && text == "Low disk space! Might not be able to save the world.")
        );
    }

    #[test]
    fn a_report_write_error_does_not_hide_the_chunk_failure() {
        crate::test_log::install();
        crate::test_log::clear();
        let world = tempfile::tempdir().unwrap();
        // An existing file blocks creation of the report directory on every platform.
        fs::write(world.path().join("debug"), "unrelated file").unwrap();
        let pending = Mutex::new(Pending::default());
        let reporter = SaveFailureReporter::new(true);
        reporter.report(
            SaveFailure {
                world_root: world.path(),
                dimension: "minecraft:overworld",
                storage: "entities",
                pos: (3, 4),
                error: &io::Error::other("entities refused"),
            },
            &pending,
            |_| Ok(save::LOW_DISK_SPACE),
        );
        assert_eq!(reporter.reported_failures(), 1);
        assert_eq!(
            pending.into_inner().unwrap(),
            Pending {
                low_disk_space: false,
                save_failure: Some((3, 4)),
                load_failure: None
            }
        );
        let records = crate::test_log::records();
        assert!(
            records
                .iter()
                .any(|(level, text)| *level == log::Level::Warn
                    && text.starts_with("Failed to store chunk IO exception:"))
        );
        assert_eq!(
            records[0],
            (
                log::Level::Error,
                "Failed to store entity chunk [3, 4]: entities refused".into()
            )
        );
        assert_eq!(
            records[1],
            (
                log::Level::Error,
                "Failed to save chunk 3,4: entities refused".into()
            )
        );
    }

    #[test]
    fn crash_diagnostics_follow_the_launch_servers_lifetime() {
        let diagnostics = SaveFailureDiagnostics::default();
        assert_eq!(diagnostics.suppressed_summary(), None);
        let integrated = Arc::new(SaveFailureReporter::new(true));
        integrated.record(0, "Error", "integrated failure".into());
        diagnostics.register(&integrated);
        assert!(
            diagnostics
                .suppressed_summary()
                .unwrap()
                .contains("integrated failure")
        );
        drop(integrated);
        assert_eq!(diagnostics.suppressed_summary(), None);
        let dedicated = Arc::new(SaveFailureReporter::new(false));
        dedicated.record(0, "Error", "dedicated failure".into());
        diagnostics.register(&dedicated);
        drop(dedicated);
        assert!(
            diagnostics
                .suppressed_summary()
                .unwrap()
                .contains("dedicated failure")
        );
    }

    #[test]
    fn summary_keeps_last_eight_messages_but_counts_older_failures() {
        let reporter = SaveFailureReporter::default();
        assert_eq!(reporter.suppressed_summary(), "~~NONE~~");
        for n in 0..9 {
            reporter.record(0, "std::io::Error", format!("message {n}"));
        }
        reporter.record(0, "Error", "another class".into());
        let summary = reporter.suppressed_summary();
        assert_eq!(reporter.reported_failures(), 10);
        assert_eq!(summary.matches("ms ago)").count(), 8);
        assert!(!summary.contains("message 0"));
        assert!(!summary.contains("message 1"));
        assert!(summary.contains("message 2"));
        assert!(summary.contains("another class"));
        assert!(summary.contains("chunk/save:Error x 1\n\t\t\tchunk/save:std::io::Error x 9"));
        assert_eq!(SaveFailureReporter::default().reported_failures(), 0);
    }

    #[test]
    fn low_disk_space_toast_stays_ten_seconds() {
        let toast = low_disk_space_toast();
        assert_eq!(toast.display_time(), 10000.0);
        assert_eq!(load_failure_toast(0, 0).display_time(), 5000.0);
    }
}
