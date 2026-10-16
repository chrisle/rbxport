//! The debug logger: everything the app and its crates say, on stdout and in
//! a file.
//!
//! The file is what a bug report can carry — a shipped app has no terminal,
//! and LINK's conversation with a player is only worth anything read back
//! after the fact. One file a day under the app's data directory, the
//! last five kept. Writes go through a background thread, so a log line
//! never waits on the disk from the thread that has a player waiting on it.
//!
//! The level is `debug` for every crate of ours unless `LOG_LEVEL` says
//! otherwise (`error`, `warn`, `info`, `debug` or `trace`); the
//! packet-by-packet lines LINK writes are at `trace`. Dependencies say
//! nothing below `warn` whatever the level. `RUST_LOG`, when set, is taken
//! as the whole filter instead, for a per-crate mix (`rbl_link=trace`).
//! Symphonia's recoverable MP3 warnings are hidden: a decoder reset at a seek
//! can legitimately start without preceding frame data, then the demuxer may
//! scan past an incomplete header while finding the next packet. Decode
//! failures still reach us through `rbl_deck`.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// Where the log files go: `<data dir>/rbxport/logs`, beside the backups,
/// unless `RBXPORT_LOG_DIR` names somewhere else.
const LOG_DIR: &str = "rbxport/logs";
/// Names the log directory, for a test or a support case.
pub const LOG_DIR_ENV: &str = "RBXPORT_LOG_DIR";
/// Daily files older than the newest this many are removed.
const KEEP_FILES: usize = 5;

/// The crates the level applies to: ours, and nothing pulled in.
const OUR_CRATES: [&str; 18] = [
    "rbxport", "rbxport_lib", "rbl_core", "rbl_db", "rbl_index", "rbl_anlz", "rbl_analysis", "rbl_audio", "rbl_deck",
    "rbl_prolink", "rbl_link", "rbl_dbserver", "rbl_nfs", "rbl_export", "rbl_devices", "rbl_pdb",
    "rbl_onelibrary", "rbl_difftool",
];

/// The level when `LOG_LEVEL` is not set.
const DEFAULT_LEVEL: &str = "debug";

/// The filter for `LOG_LEVEL=<level>`: our crates at that level, the
/// dependencies at `warn` (or `error`, when even less is asked for).
fn filter_for(level: &str) -> String {
    let deps = if level == "error" { "error" } else { "warn" };
    let ours: Vec<String> = OUR_CRATES.iter().map(|c| format!("{c}={level}")).collect();
    format!(
        "{deps},symphonia_bundle_mp3::layer3=error,symphonia_bundle_mp3::demuxer=error,{}",
        ours.join(",")
    )
}

/// The filter to run with, and a complaint about `LOG_LEVEL` when it names
/// no level, to be logged once the logger is up.
fn filter() -> (EnvFilter, Option<String>) {
    if let Ok(filter) = EnvFilter::try_from_default_env() {
        return (filter, None);
    }
    match std::env::var("LOG_LEVEL") {
        Ok(level) => {
            let level = level.trim().to_ascii_lowercase();
            if ["error", "warn", "info", "debug", "trace"].contains(&level.as_str()) {
                (EnvFilter::new(filter_for(&level)), None)
            } else {
                (
                    EnvFilter::new(filter_for(DEFAULT_LEVEL)),
                    Some(format!("LOG_LEVEL={level:?} is not error, warn, info, debug or trace; using {DEFAULT_LEVEL}")),
                )
            }
        }
        Err(_) => (EnvFilter::new(filter_for(DEFAULT_LEVEL)), None),
    }
}

/// How many of this process's lines the Log window can show; older ones
/// are only in the file.
pub const BUFFER_LINES: usize = 10_000;
/// A line longer than this is cut short in the window (the file keeps it
/// whole): LINK's trace lines can carry a packet dump each.
const LINE_CHARS: usize = 4_096;

/// One logged line, as the Log window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// Counts up from 1 for every line this process logs.
    pub seq: u64,
    /// When, in milliseconds since the Unix epoch.
    pub time_ms: u64,
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub level: &'static str,
    /// The module that logged it, `rbl_link::peer` for example.
    pub target: String,
    /// The message, then its fields as `name=value`.
    pub message: String,
}

/// The lines after a given one, and the oldest the buffer still holds.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub lines: Vec<LogLine>,
    /// The `seq` of the oldest line still held, or the next one to come when
    /// none is: lines before it have left the buffer.
    pub oldest: u64,
}

/// The newest lines this process has logged.
#[derive(Debug)]
pub struct LogBuffer {
    next: u64,
    capacity: usize,
    lines: VecDeque<LogLine>,
}

impl LogBuffer {
    #[must_use]
    pub const fn new(capacity: usize) -> Self {
        Self { next: 1, capacity, lines: VecDeque::new() }
    }

    fn push(&mut self, time_ms: u64, level: &'static str, target: &str, mut message: String) {
        if let Some((cut, _)) = message.char_indices().nth(LINE_CHARS) {
            message.truncate(cut);
            message.push('…');
        }
        let seq = self.next;
        self.next += 1;
        if self.lines.len() >= self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(LogLine { seq, time_ms, level, target: target.to_owned(), message });
    }

    /// Every line held whose `seq` is after `after`; 0 for all of them.
    #[must_use]
    pub fn after(&self, after: u64) -> LogPage {
        let oldest = self.lines.front().map_or(self.next, |line| line.seq);
        // `seq` rises by one per line, so the first wanted is an index.
        let skip = usize::try_from(after.saturating_add(1).saturating_sub(oldest)).unwrap_or(usize::MAX);
        LogPage { lines: self.lines.iter().skip(skip).cloned().collect(), oldest }
    }
}

/// The process's buffer, which [`install`] fills.
static BUFFER: Mutex<LogBuffer> = Mutex::new(LogBuffer::new(BUFFER_LINES));

/// The lines this process has logged after `after`, for the Log window.
pub fn lines_after(after: u64) -> LogPage {
    BUFFER.lock().unwrap_or_else(PoisonError::into_inner).after(after)
}

/// Puts every event the filter lets through into a [`LogBuffer`].
pub(crate) struct BufferLayer(pub(crate) &'static Mutex<LogBuffer>);

impl<S: tracing::Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let metadata = event.metadata();
        let time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        // A poisoned lock still holds good lines; a log line is no reason to
        // give up on the rest.
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(
            time_ms,
            level_name(*metadata.level()),
            metadata.target(),
            fields.finish(),
        );
    }
}

fn level_name(level: tracing::Level) -> &'static str {
    match level {
        tracing::Level::ERROR => "error",
        tracing::Level::WARN => "warn",
        tracing::Level::INFO => "info",
        tracing::Level::DEBUG => "debug",
        tracing::Level::TRACE => "trace",
    }
}

/// An event's message and its other fields, written the way the file has
/// them: the message, then ` name=value` for each field.
#[derive(Default)]
struct Fields {
    message: String,
    rest: String,
}

impl Fields {
    fn finish(self) -> String {
        if self.message.is_empty() {
            self.rest.trim_start().to_owned()
        } else {
            self.message + &self.rest
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.record_debug(field, &value);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.rest, " {}={value:?}", field.name());
        }
    }
}

/// Keeps the file writer's background thread alive for the life of the
/// process; dropped, it would flush and stop.
static FILE_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// The directory the log files are written to.
pub fn log_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(LOG_DIR_ENV).filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(LOG_DIR)
}

/// The newest daily log, which is the file this process is currently writing.
/// Daily filenames put their ISO date between the fixed prefix and suffix, so
/// filename order is date order and does not depend on buffered writes having
/// updated filesystem timestamps yet.
pub fn latest_log_file() -> std::io::Result<Option<PathBuf>> {
    latest_log_file_in(&log_dir())
}

fn latest_log_file_in(dir: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut newest: Option<(String, PathBuf)> = None;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("rbxport.") || Path::new(&name).extension().is_none_or(|ext| ext != "log") {
            continue;
        }
        if newest.as_ref().is_none_or(|(current, _)| name > *current) {
            newest = Some((name, entry.path()));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

/// Installs the logger, and a panic hook that puts a panic in the log rather
/// than on a stderr nobody is watching.
///
/// A log directory that cannot be made or written leaves stdout alone as the
/// only output, with a line saying so: a missing log file is not a reason
/// for the app not to start.
pub fn install() {
    let (filter, complaint) = filter();
    let stdout = tracing_subscriber::fmt::layer().with_writer(std::io::stdout);

    let dir = log_dir();
    let file = match std::fs::create_dir_all(&dir).and_then(|()| {
        tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("rbxport")
            .filename_suffix("log")
            .max_log_files(KEEP_FILES)
            .build(&dir)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }) {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            // Set once per process; a second install (a test's) keeps the first.
            if FILE_GUARD.set(guard).is_err() {
                eprintln!("rbxport: the logger was installed twice");
            }
            Some(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false))
        }
        Err(error) => {
            eprintln!("rbxport: no log file under {}: {error}", dir.display());
            None
        }
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout)
        .with(file)
        .with(BufferLayer(&BUFFER))
        .init();
    tracing::debug!(dir = %dir.display(), "logging to stdout and a daily file");
    if let Some(complaint) = complaint {
        tracing::warn!("{complaint}");
    }

    std::panic::set_hook(Box::new(|info| {
        tracing::error!(%info, "panic");
    }));
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn level_applies_to_our_crates_and_keeps_dependencies_quiet() {
        let filter = filter_for("trace");
        assert!(filter.starts_with("warn,"));
        assert!(filter.contains("symphonia_bundle_mp3::layer3=error"));
        assert!(filter.contains("symphonia_bundle_mp3::demuxer=error"));
        assert!(filter.contains("rbl_link=trace"));
        assert!(filter.contains("rbxport=trace"));
        assert!(filter_for("error").starts_with("error,"));
    }

    /// A buffer of its own, so lines other tests log do not land in it.
    fn buffer() -> &'static Mutex<LogBuffer> {
        Box::leak(Box::new(Mutex::new(LogBuffer::new(3))))
    }

    #[test]
    fn the_buffer_keeps_what_was_logged_with_its_level_and_fields() {
        let held = buffer();
        let subscriber = tracing_subscriber::registry().with(BufferLayer(held));
        tracing::subscriber::with_default(subscriber, || {
            tracing::warn!(port = 1234, "port in use");
            tracing::error!(path = "a b", "copy failed");
        });
        let page = held.lock().expect("buffer").after(0);
        assert_eq!(page.oldest, 1);
        let lines: Vec<_> = page.lines.iter().map(|l| (l.seq, l.level, l.message.as_str())).collect();
        assert_eq!(lines, [(1, "warn", "port in use port=1234"), (2, "error", "copy failed path=\"a b\"")]);
        assert!(page.lines.iter().all(|l| l.target == module_path!() && l.time_ms > 0));
    }

    #[test]
    fn the_buffer_drops_its_oldest_lines_and_pages_by_seq() {
        let mut buffer = LogBuffer::new(3);
        assert_eq!(buffer.after(0).oldest, 1, "an empty buffer points at the line to come");
        for n in 1..=5 {
            buffer.push(0, "info", "t", format!("line {n}"));
        }
        let all = buffer.after(0);
        assert_eq!(all.oldest, 3);
        assert_eq!(all.lines.iter().map(|l| l.seq).collect::<Vec<_>>(), [3, 4, 5]);
        assert_eq!(buffer.after(4).lines.iter().map(|l| l.seq).collect::<Vec<_>>(), [5]);
        assert_eq!(buffer.after(5).lines.len(), 0);
        assert_eq!(buffer.after(99).lines.len(), 0);
    }

    #[test]
    fn a_long_line_is_cut_short_on_a_character_boundary() {
        let mut buffer = LogBuffer::new(1);
        buffer.push(0, "trace", "t", "é".repeat(LINE_CHARS + 10));
        let line = &buffer.after(0).lines[0].message;
        assert_eq!(line.chars().count(), LINE_CHARS + 1);
        assert!(line.ends_with('…'));
    }

    #[test]
    fn latest_log_is_the_newest_daily_file() {
        let dir = tempfile::tempdir().expect("temporary log directory");
        for name in [
            "rbxport.2026-09-22.log",
            "rbxport.2026-09-24.log",
            "rbxport.2026-09-23.log",
            "notes.log",
        ] {
            std::fs::write(dir.path().join(name), name).expect("test log");
        }
        std::fs::create_dir(dir.path().join("rbxport.2099-01-01.log"))
            .expect("lookalike directory");

        assert_eq!(
            latest_log_file_in(dir.path()).expect("scan logs"),
            Some(dir.path().join("rbxport.2026-09-24.log")),
        );
    }
}
