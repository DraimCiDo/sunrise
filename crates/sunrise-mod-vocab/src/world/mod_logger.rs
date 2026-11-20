//! Native runtime diagnostics bound to the installed mod's manifest ID.
use std::{fmt, panic::Location, sync::OnceLock};

/// Severity of a runtime diagnostic, independent of any logging crate.
// no wire: only used with this process's native logger and borrowed Rust formatting arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LogLevel {
    /// An error or failed operation.
    Error,
    /// A recoverable problem.
    Warn,
    /// An ordinary diagnostic.
    Info,
    /// Debugging detail.
    Debug,
    /// Fine-grained tracing detail.
    Trace,
}

/// The result of submitting a record to the game log.
// no wire: a native sink's queue acknowledgment, not a transport value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum LogOutcome {
    /// The writer accepted the record. This does not acknowledge a disk write.
    Queued,
    /// The level filter or warning/error flood control suppressed this record.
    Filtered,
    /// The game log is not available. No record was queued.
    Unavailable,
}

impl LogLevel {
    fn facade(self) -> log::Level {
        match self {
            Self::Error => log::Level::Error,
            Self::Warn => log::Level::Warn,
            Self::Info => log::Level::Info,
            Self::Debug => log::Level::Debug,
            Self::Trace => log::Level::Trace,
        }
    }
}

type Enqueue =
    fn(&'static str, log::Level, fmt::Arguments<'_>, &'static Location<'static>) -> LogOutcome;

struct Sink {
    enabled: fn(log::Level) -> bool,
    enqueue: Enqueue,
}

static SINK: OnceLock<Sink> = OnceLock::new();

/// Install the game's log sink before loading mods.
// hidden: engine plumbing, re-exported through __private for the game.
#[doc(hidden)]
pub fn install_mod_log_sink(enabled: fn(log::Level) -> bool, enqueue: Enqueue) {
    let _ = SINK.set(Sink { enabled, enqueue });
}

/// A native mod's own logger, from [`RegistrationContext::logger`](crate::RegistrationContext::logger).
/// Keep or copy it into hooks, Parts or workers. It needs no world, Host or transaction;
/// a queued record remains queued if registration is refused or a transaction rolls back.
// exhaustive: its field is private, so mods cannot construct or retarget it.
// no wire: bound to this process's sink and accepts borrowed Rust formatting arguments.
#[derive(Clone, Copy)]
pub struct ModLogger {
    id: Option<&'static str>,
}

impl ModLogger {
    /// Whether the game's log is available and its level filter accepts `level`.
    /// Check before computing expensive arguments. Warning/error flood control is applied
    /// separately at the call to [`log`](Self::log).
    pub fn enabled(&self, level: LogLevel) -> bool {
        self.id.is_some()
            && SINK
                .get()
                .is_some_and(|sink| (sink.enabled)(level.facade()))
    }

    /// Queue a record on the calling thread, with this mod's ID and `level`.
    /// Use `format_args!(...)` to borrow the message. Filtered records do not run their
    /// formatter. This writes a diagnostic, not chat or a Mods-screen loading warning.
    #[track_caller]
    pub fn log(&self, level: LogLevel, message: fmt::Arguments<'_>) -> LogOutcome {
        match (self.id, SINK.get()) {
            (Some(id), Some(sink)) => {
                (sink.enqueue)(id, level.facade(), message, Location::caller())
            }
            _ => LogOutcome::Unavailable,
        }
    }
}

impl crate::RegistrationContext {
    /// Get the native logger for the installed mod receiving this registration callback.
    /// The manifest ID is already bound; no target or namespace is supplied by the mod.
    /// A context used without a running native mod (for example by a tool) yields an
    /// unavailable logger.
    pub fn logger(&self) -> ModLogger {
        ModLogger {
            id: crate::content::crash::current_native_mod(),
        }
    }
}
