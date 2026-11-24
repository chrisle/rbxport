//! Beat grid editing: the GRID panel's buttons, from the playhead to the
//! user's library.
//!
//! Ghidra evidence and the assumption inventory live in the private
//! pre-release beat-grid audit. PQTZ edits conditionally preserve compatible
//! PQT2 payloads, update BPM and the analysis revision, and obey the library
//! writer gate. Analysis lock is bit 0x80 in djmdContent.Analysed; legacy
//! local locks are retained until explicitly unlocked. Undo uses bounded
//! session snapshots and crash-recoverable file/database writes.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rbl_anlz::grid::{ apply_with_duration, tempo_x100, Edit};
use rbl_anlz::Beat;
use rbl_index::Library;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{blocking, reload, write_error};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::state::AppState;

/// Grids kept for undo per track, per session. A grid is at most a few
/// hundred kilobytes, and a hundred steps is more than a hand edits.
const HISTORY_CAP: usize = 100;

/// One edit, as the panel sends it.
///
/// The shape mirrors `rbl_anlz::grid::Edit` field for field; the panel
/// spells the kind in camelCase, as the rest of the wire does.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum GridEdit {
    /// Shift every beat by this many milliseconds; positive is later.
    Nudge { ms: i32 },
    Double,
    Halve,
    /// Make the beat nearest this time the downbeat.
    Downbeat { time_ms: u32 },
    /// Re-space the grid at this tempo with a beat held at the anchor.
    Tempo { bpm_x100: u16, anchor_ms: u32 },
    /// Move the target beat by milliseconds, holding the first editable beat.
    Stretch { by_ms: i32, time_ms: u32 },
    Tap { bpm: f64, anchor_ms: u32 },
    /// Move the grid so the beat nearest this time lands on it.
    Align { time_ms: u32 },
}

impl From<GridEdit> for Edit {
    fn from(edit: GridEdit) -> Self {
        match edit {
            GridEdit::Nudge { ms } => Self::Nudge(ms),
            GridEdit::Double => Self::Double,
            GridEdit::Halve => Self::Halve,
            GridEdit::Downbeat { time_ms } => Self::Downbeat { time_ms },
            GridEdit::Tempo { bpm_x100, anchor_ms } => Self::Tempo { bpm_x100, anchor_ms },
            GridEdit::Stretch { by_ms, time_ms } => Self::Stretch { by_ms, time_ms },
            GridEdit::Tap { bpm, anchor_ms } => Self::Tap { bpm, anchor_ms },
            GridEdit::Align { time_ms } => Self::Align { time_ms },
        }
    }
}

/// What the panel needs to know about a track's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "the panel's wire shape: each flag greys a different control")]
pub struct GridStateDto {
    /// The grid's tempo x100, from its first beat; 0 without a grid.
    pub bpm_x100: u32,
    pub beats: u32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<&'static str>,
    pub redo_label: Option<&'static str>,
    pub locked: bool,
    /// Whether the GRID panel's lock can be set here: a collection track's
    /// can, a Devices tree track's cannot (its lock would be the stick's,
    /// which rbxport does not write).
    pub lockable: bool,
}

/// What one action asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridAction {
    /// An edit, to the whole grid or from the beat nearest `from_ms` on.
    Edit { edit: GridEdit, from_ms: Option<u32> },
    Undo,
    Redo,
}

/// Transient editor options; never serialized into an analysis file.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridOptions {
    #[serde(default)]
    allow_dynamic: bool,
    transaction: Option<String>,
    duration_ms: Option<u32>,
    /// The track's analysis when it was registered after the index was
    /// read; the command fills it in, the frontend never sends it.
    #[serde(skip)]
    registered: Option<String>,
    /// Info-only row marker; never sent by a grid or device command.
    #[serde(skip)]
    metadata_bpm: Option<u32>,
    device_source: Option<crate::dto::DeviceTrackSourceDto>,
}

/// rekordbox stores its analysis lock in bit 7 of Analysed.
pub(crate) fn database_locked(location: &rbl_db::LibraryLocation, track: &str) -> AppResult<bool> {
    let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).map_err(write_error)?;
    locked_in(&db, track).map_err(|e| AppError::internal(e.to_string()))
}

/// [`database_locked`] over a handle the caller already has, for a
/// loop over many tracks: opening the library derives its key again each
/// time, which is the cost of the whole check (#284).
pub(crate) fn locked_in(db: &rbl_db::Library, track: &str) -> Result<bool, rbl_db::DbError> {
    Ok(db.connection().query_row("SELECT (COALESCE(Analysed, 0) & 128) != 0 FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [track], |r| r.get(0))?)
}

/// What an action did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridOutcome {
    pub state: GridStateDto,
    /// Whether `djmdContent.BPM` changed, so the library has to be re-read.
    pub bpm_changed: bool,
    /// Whether files or row metadata changed: false for a no-op.
    pub written: bool,
    /// The grid as it now stands, for the deck's metronome: milliseconds and
    /// the beat's number in its bar.
    pub beats: Vec<(u32, u8)>,
}

#[derive(Debug, Default)]
struct History {
    transaction: Option<String>,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

/// One recorded edit: the grid either side of it, so Undo and Redo can
/// check that the grid on disk is still the one the edit left (or found)
/// before writing the other over it.
#[derive(Debug)]
struct HistoryEntry {
    /// The grid before the edit: what Undo writes back, and what Redo
    /// expects to find.
    before: Vec<Beat>,
    /// The grid the edit left: what Redo writes, and what Undo expects to
    /// find.
    after: Vec<Beat>,
    label: &'static str,
    /// Row-only zero markers must survive later grid edits and Undo/Redo.
    row_bpms: Option<(u32, u32, String)>,
}

impl GridEdit {
    fn label(self) -> &'static str {
        match self {
            Self::Nudge { ms } if ms < 0 => "Shift Beat Grid Left",
            Self::Nudge { .. } => "Shift Beat Grid Right",
            Self::Double => "Double Tempo",
            Self::Halve => "Halve Tempo",
            Self::Downbeat { .. } => "Set Downbeat",
            Self::Tempo { .. } => "Set Tempo",
            Self::Tap { .. } => "Tap Tempo",
            Self::Stretch { .. } => "Adjust Tempo",
            Self::Align { .. } => "Align Beat Grid",
        }
    }
}

/// The session's grid editing state: histories, the lock list, and which
/// files have been backed up. Managed by the app beside `AppState`.
#[derive(Debug)]
pub struct GridEditor {
    editing: parking_lot::Mutex<()>,
    journal_root: PathBuf,
    histories: parking_lot::Mutex<HashMap<String, History>>,
    /// Loaded from `locks_path` on first use; `None` until then.
    locks: parking_lot::Mutex<Option<BTreeSet<String>>>,
    locks_path: PathBuf,
    /// Where analysis files are copied before their first rewrite.
    backup_dir: PathBuf,
    /// Files checked for a backup this session, so the check is one per file.
    backed_up: parking_lot::Mutex<HashSet<PathBuf>>,
    /// A stick's analysis files as this session last wrote them: the file
    /// and a digest of its bytes. One still holding those bytes is the
    /// session's own edit, already backed up before it, and is not copied.
    device_written: parking_lot::Mutex<HashSet<(PathBuf, blake3::Hash)>>,
}

impl Default for GridEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl GridEditor {
    pub fn clear_history(&self) { self.histories.lock().clear(); }

    /// The editor for the installed library: locks and backups under the
    /// app's own data directory, beside the database backups.
    pub fn new() -> Self {
        let root = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("rbxport");
        Self::at(&root)
    }

    /// An editor keeping its lock list and backups under `root`.
    pub fn at(root: &Path) -> Self {
        Self {
            editing: parking_lot::Mutex::new(()),
            journal_root: root.join("backups"),
            histories: parking_lot::Mutex::new(HashMap::new()),
            locks: parking_lot::Mutex::new(None),
            locks_path: root.join("grid-locks.json"),
            backup_dir: root.join("backups/analysis"),
            backed_up: parking_lot::Mutex::new(HashSet::new()),
            device_written: parking_lot::Mutex::new(HashSet::new()),
        }
    }

    pub(crate) fn forget_history(&self, track: &str) { self.histories.lock().remove(track); }

    /// Gives the track one undoable edit, for tests of what clears it.
    #[cfg(test)]
    pub(crate) fn seed_history(&self, track: &str) {
        self.histories.lock().entry(track.to_owned()).or_default()
            .undo.push(HistoryEntry { before: Vec::new(), after: Vec::new(), label: "Seeded", row_bpms: None });
    }

    /// Whether the track has any undo or redo entry.
    #[cfg(test)]
    pub(crate) fn has_history(&self, track: &str) -> bool { self.histories.lock().contains_key(track) }

    /// Whether the track's grid is locked against editing.
    pub fn is_locked(&self, track: &str) -> bool {
        self.with_locks(|locks| locks.contains(track))
    }

    /// Locks or unlocks a track's grid, and records it for the next session.
    pub fn set_locked(&self, track: &str, on: bool) -> AppResult<()> {
        let mut held = self.locks.lock();
        let locks = held.get_or_insert_with(|| read_locks(&self.locks_path));
        let changed = if on { locks.insert(track.to_owned()) } else { locks.remove(track) };
        if !changed {
            return Ok(());
        }
        let text = serde_json::to_string(&locks.iter().collect::<Vec<_>>())
            .map_err(|e| AppError::internal(format!("the lock list could not be encoded: {e}")))?;
        if let Some(parent) = self.locks_path.parent() {
            // perf-ok: called from the commands' blocking closures only
            std::fs::create_dir_all(parent).map_err(|e| lock_error(&self.locks_path, &e))?;
        }
        write_atomically(&self.locks_path, text.as_bytes()).map_err(|e| lock_error(&self.locks_path, &e))
    }

    fn with_locks<T>(&self, f: impl FnOnce(&BTreeSet<String>) -> T) -> T {
        let mut held = self.locks.lock();
        f(held.get_or_insert_with(|| read_locks(&self.locks_path)))
    }

    fn state_for(&self, track: &str, beats: &[Beat]) -> GridStateDto {
        let histories = self.histories.lock();
        let history = histories.get(track);
        GridStateDto {
            bpm_x100: u32::from(tempo_x100(beats)),
            beats: u32::try_from(beats.len()).unwrap_or(u32::MAX),
            can_undo: history.is_some_and(|h| !h.undo.is_empty()),
            can_redo: history.is_some_and(|h| !h.redo.is_empty()),
            undo_label: history.and_then(|h| h.undo.last().map(|entry| entry.label)),
            redo_label: history.and_then(|h| h.redo.last().map(|entry| entry.label)),
            locked: self.is_locked(track),
            lockable: true,
        }
    }

    /// Copies a file under the backup directory at `relative`, once: a
    /// copy already there is the file as rekordbox wrote it, and stays.
    fn back_up(&self, file: &Path, relative: &str, extension: &str) -> AppResult<()> {
        if self.backed_up.lock().contains(file) || !file.is_file() {
            return Ok(());
        }
        let copy = self
            .backup_dir
            .join(relative.trim_start_matches(['/', '\\']))
            .with_extension(extension);
        if !copy.exists() {
            if let Some(parent) = copy.parent() {
                // perf-ok: called from the commands' blocking closures only
                std::fs::create_dir_all(parent).map_err(|e| file_error("backed up", &copy, &e))?;
            }
            std::fs::copy(file, &copy).map_err(|e| file_error("backed up", &copy, &e))?;
        }
        self.backed_up.lock().insert(file.to_path_buf());
        Ok(())
    }

    /// Copies a stick's analysis file under the backup directory before it
    /// is rewritten, at `relative` with a digest of its bytes in the name.
    /// Sticks often share a volume name and the same track has the same
    /// analysis path on each, so neither names one stick's file: the digest
    /// keeps each stick's original apart, and a copy already there holds
    /// these very bytes. Bytes this session wrote to the file are skipped,
    /// as what they replaced was copied first.
    fn back_up_device(&self, file: &Path, relative: &str) -> AppResult<()> {
        if !file.is_file() {
            return Ok(());
        }
        // perf-ok: called from the commands' blocking closures only
        let bytes = std::fs::read(file).map_err(|e| file_error("backed up", file, &e))?;
        let digest = blake3::hash(&bytes);
        if self.device_written.lock().contains(&(file.to_path_buf(), digest)) {
            return Ok(());
        }
        let named = self.backup_dir.join(relative.trim_start_matches(['/', '\\']));
        let stem = named.file_stem().map_or_else(|| "analysis".into(), |s| s.to_string_lossy().into_owned());
        let extension = named.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let copy = named.with_file_name(format!("{stem}-{}{extension}", &digest.to_hex()[..16]));
        if !copy.exists() {
            if let Some(parent) = copy.parent() {
                // perf-ok: called from the commands' blocking closures only
                std::fs::create_dir_all(parent).map_err(|e| file_error("backed up", &copy, &e))?;
            }
            write_atomically(&copy, &bytes).map_err(|e| file_error("backed up", &copy, &e))?;
        }
        Ok(())
    }
}

fn read_locks(path: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else { return BTreeSet::new() };
    match serde_json::from_str::<Vec<String>>(&text) {
        Ok(ids) => ids.into_iter().collect(),
        Err(e) => {
            // A list that cannot be read locks nothing, and says so once
            // rather than every edit; the next lock rewrites it whole.
            tracing::warn!(path = %path.display(), error = %e, "the grid lock list could not be read");
            BTreeSet::new()
        }
    }
}

fn lock_error(path: &Path, error: &std::io::Error) -> AppError {
    AppError::new(ErrorKind::Internal, "The grid lock could not be saved.")
        .with_detail(format!("{}: {error}", path.display()))
}

fn file_error(what: &str, path: &Path, error: &std::io::Error) -> AppError {
    AppError::new(ErrorKind::Internal, format!("The analysis file could not be {what}."))
        .with_detail(format!("{}: {error}", path.display()))
}

/// Writes a file whole, through a sibling and a rename, so a crash midway
/// leaves the old file rather than half of the new one.
pub(crate) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    crate::durable::write(path, bytes)
}

/// A track's analysis files, resolved.
struct Files {
    relative: String,
    dat: PathBuf,
    ext: PathBuf,
}

/// `registered` is the track's analysis when it was registered after the
/// index was read, which the index does not show yet.
fn files_of(library: &Library, share: &Path, track: &str, registered: Option<&str>) -> AppResult<Files> {
    let Some(row) = library.row_of(track) else {
        return Err(AppError::new(ErrorKind::NotFound, "That track is not in the library."));
    };
    let indexed = library.analysis_path.get(row as usize);
    let relative = if indexed.is_empty() { registered.unwrap_or_default() } else { indexed };
    if relative.is_empty() {
        return Err(AppError::new(ErrorKind::NotFound, "That track has not been analysed, so it has no grid to edit."));
    }
    let dat = rbl_anlz::resolve(share, relative);
    let ext = rbl_anlz::sibling(&dat, "EXT");
    Ok(Files { relative: relative.to_owned(), dat, ext })
}

fn read_dat(dat: &Path) -> AppResult<(rbl_anlz::Anlz, Vec<Beat>)> {
    let (parsed, beats, _) = read_dat_snapshot(dat)?;
    Ok((parsed, beats))
}

fn read_dat_snapshot(dat: &Path) -> AppResult<(rbl_anlz::Anlz, Vec<Beat>, Vec<u8>)> {
    let error = |detail: String| AppError::new(ErrorKind::NotFound, "That track's analysis file could not be read.")
        .with_detail(format!("{}: {detail}", dat.display()));
    let original = std::fs::read(dat).map_err(|e| error(e.to_string()))?;
    let parsed = rbl_anlz::parse(&original).map_err(|e| error(e.to_string()))?;
    let Some(beats) = parsed.beat_grid().filter(|beats| !beats.is_empty()) else {
        return Err(AppError::new(ErrorKind::NotFound, "That track has no beat grid to edit."));
    };
    let offset = i64::from(parsed.grid_offset().unwrap_or(0));
    let beats = beats.into_iter().filter_map(|beat| u32::try_from(i64::from(beat.time_ms) + offset).ok().map(|time_ms| Beat { time_ms, ..beat })).collect();
    Ok((parsed, beats, original))
}

/// The grid as the panel sees it, without changing anything.
pub fn state_of(editor: &GridEditor, library: &Library, share: &Path, track: &str) -> AppResult<GridStateDto> {
    state_with(editor, library, share, track, None)
}

/// [`state_of`] for a track whose analysis may have been registered after
/// the index was read.
pub(crate) fn state_with(editor: &GridEditor, library: &Library, share: &Path, track: &str,
    registered: Option<&str>) -> AppResult<GridStateDto> {
    let files = files_of(library, share, track, registered)?;
    let (_, beats) = read_dat(&files.dat)?;
    Ok(editor.state_for(track, &beats))
}

/// Applies one action to a track's grid: the files, the history and, through
/// `set_bpm`, the database row when the tempo changed.
///
/// Apart from the commands so it can be tested against a fixture in a
/// tempdir without a Tauri app. `set_bpm` is called with the new tempo x100
/// only when it differs from the old; it opens the writer, which is what
/// takes the database backup and re-checks the gate. If it fails, the files
/// are put back as they were, so the row and the grid never disagree.
pub fn apply(
    editor: &GridEditor,
    library: &Library,
    location: &rbl_db::LibraryLocation,
    track: &str,
    action: GridAction,
    set_bpm: &mut dyn FnMut(u32) -> AppResult<()>,
) -> AppResult<GridOutcome> {
    apply_options(editor, library, location, track, action, &GridOptions::default(), &mut |bpm, _, _| set_bpm(bpm))
}

/// Records what just happened in the track's undo/redo history — the part
/// of [`apply_options`] after the write has already landed. `before` and
/// `after` are the grid the action found and the grid it wrote.
fn record_history(editor: &GridEditor, track: &str, action: GridAction, before: Vec<Beat>, after: Vec<Beat>,
    options: &GridOptions, row_bpms: Option<(u32, u32, String)>) {
    let mut histories = editor.histories.lock();
    let history = histories.entry(track.to_owned()).or_default();
    match action {
        GridAction::Edit { edit, .. } => {
            history.redo.clear();
            let transaction = options.transaction.as_ref().filter(|_| matches!(edit, GridEdit::Tap { .. }));
            match history.undo.last_mut() {
                // A tap in the same run joins the entry the run started:
                // Undo still goes back to the grid before the first tap.
                Some(open) if transaction.is_some() && history.transaction.as_ref() == transaction => {
                    open.after = after;
                    if let Some((_, after, _)) = &mut open.row_bpms {
                        *after = row_bpms.as_ref().map_or_else(|| u32::from(tempo_x100(&open.after)), |(_, bpm, _)| *bpm);
                    }
                },
                _ => history.undo.push(HistoryEntry { before, after, label: edit.label(), row_bpms }),
            }
            history.transaction = transaction.cloned();
            if history.undo.len() > HISTORY_CAP { history.undo.remove(0); }
        }
        GridAction::Undo => {
            history.transaction = None;
            if let Some(entry) = history.undo.pop() { history.redo.push(entry); }
        }
        GridAction::Redo => {
            history.transaction = None;
            if let Some(entry) = history.redo.pop() { history.undo.push(entry); }
        }
    }
}

/// Undo or Redo found a grid other than the one its entry expects:
/// something outside this history (rekordbox, say, while RBXport stayed
/// open) changed the grid since. The newer grid is kept, as #506 keeps a
/// newer library value, in the same words.
fn conflict(action: GridAction, label: &str) -> AppError {
    let what = if action == GridAction::Undo { "undo" } else { "redo" };
    AppError::new(ErrorKind::Conflict,
        format!("Can't {what} {label}: the track's beat grid has changed since. The newer change was kept."))
}

/// The grid an action leaves, or `None` when an edit changes nothing.
/// `duration_ms` is the track's length when known; `locked` refuses an edit.
/// Undo and Redo refuse, and drop their entry, when the grid is no longer
/// the one the entry expects.
fn plan_next(editor: &GridEditor, track: &str, beats: &[Beat], action: GridAction, options: &GridOptions,
    duration_ms: Option<u32>, locked: bool) -> AppResult<Option<Vec<Beat>>> {
    match action {
        GridAction::Edit { edit, from_ms } => {
            if locked {
                return Err(AppError::new(ErrorKind::ReadOnly, "The beat grid is locked. Unlock it to edit."));
            }
            rbl_anlz::grid::validate(beats, from_ms, edit.into())
                .map_err(|message| AppError::new(ErrorKind::Malformed, message))?;
            if matches!(edit, GridEdit::Stretch { .. } | GridEdit::Tempo { .. })
                && !options.allow_dynamic && rbl_anlz::grid::is_dynamic_from(beats, from_ms) {
                return Err(AppError::new(ErrorKind::Malformed, "Confirm replacing this section's tempo changes before adjusting its tempo."));
            }
            let duration = duration_ms.unwrap_or_else(|| beats.last().map_or(0, |b| b.time_ms));
            let next = apply_with_duration(beats, from_ms, edit.into(), duration);
            if next.is_empty() {
                return Err(AppError::new(ErrorKind::Malformed, "That edit would leave the track without a beat."));
            }
            Ok((next != beats).then_some(next))
        }
        GridAction::Undo | GridAction::Redo => {
            let mut histories = editor.histories.lock();
            let history = histories.entry(track.to_owned()).or_default();
            let from = if action == GridAction::Undo { &history.undo } else { &history.redo };
            let Some(entry) = from.last() else {
                let what = if action == GridAction::Undo { "undo" } else { "redo" };
                return Err(AppError::new(ErrorKind::NotFound, format!("Nothing to {what}.")));
            };
            let (expected, target) = if action == GridAction::Undo { (&entry.after, &entry.before) } else { (&entry.before, &entry.after) };
            if expected.as_slice() != beats {
                // Only that entry goes, as #506 drops a conflicting library
                // entry: left on top it would block every older one, which
                // may still fit (an outside write that put the grid back).
                let label = entry.label;
                let from = if action == GridAction::Undo { &mut history.undo } else { &mut history.redo };
                from.pop();
                return Err(conflict(action, label));
            }
            Ok(Some(target.clone()))
        }
    }
}

fn apply_options(editor: &GridEditor, library: &Library, location: &rbl_db::LibraryLocation,
    track: &str, action: GridAction, options: &GridOptions,
    set_bpm: &mut dyn FnMut(u32, bool, u32) -> AppResult<()>) -> AppResult<GridOutcome> {
    // The writer's own rule, applied to the files as well as the row: a
    // fixture in a tempdir is not the installed library, so its tests run
    // whether or not rekordbox is open.
    if let Some(reason) = rbl_db::write_refusal_reason(
        location.is_real_install,
        rbl_db::test_mode(),
        rbl_db::is_rekordbox_running(),
    ) {
        return Err(AppError::new(ErrorKind::ReadOnly, reason));
    }
    let _editing = editor.editing.lock();
    crate::file_journal::recover(&editor.journal_root, location)?;
    let files = files_of(library, &location.share_root, track, options.registered.as_deref())?;
    let (parsed, beats) = read_dat(&files.dat)?;

    let old_row_bpm = row_tempo(location, track)?;
    let history_bpms = {
        let mut histories = editor.histories.lock();
        let history = histories.entry(track.to_owned()).or_default();
        let entries = match action {
            GridAction::Undo => Some(&mut history.undo),
            GridAction::Redo => Some(&mut history.redo),
            GridAction::Edit { .. } => None,
        };
        if let Some(entries) = entries {
            if let Some(entry) = entries.last() {
                if let Some((before, after, analysis)) = &entry.row_bpms {
                    let expected = if action == GridAction::Undo { *after } else { *before };
                    if old_row_bpm != expected || *analysis != files.relative {
                        let label = entry.label;
                        entries.pop();
                        return Err(conflict(action, label));
                    }
                    Some((*before, *after))
                } else { None }
            } else { None }
        } else { None }
    };
    let locked = matches!(action, GridAction::Edit { .. }) && (editor.is_locked(track) || database_locked(location, track)?);
    let row = library.row_of(track).map(|row| row as usize);
    let duration = options.duration_ms.or_else(|| row.and_then(|i| library.length_sec.get(i)).map(|s| s.saturating_mul(1000)));
    let planned = if options.metadata_bpm == Some(0) {
        if locked { return Err(AppError::new(ErrorKind::ReadOnly, "The beat grid is locked. Unlock it to edit.")); }
        Some(beats.clone())
    } else {
        plan_next(editor, track, &beats, action, options, duration, locked)?
            .or_else(|| options.metadata_bpm.filter(|bpm| *bpm != old_row_bpm).map(|_| beats.clone()))
    };
    let Some(next) = planned else {
        return Ok(GridOutcome {
            state: editor.state_for(track, &beats),
            bpm_changed: false,
            written: false,
            beats: wire_beats(&beats),
        });
    };

    let new_bpm = history_bpms.map(|(before, after)| if action == GridAction::Undo { before } else { after })
        .or(options.metadata_bpm).unwrap_or_else(|| u32::from(tempo_x100(&next)));
    let row_bpms = history_bpms.map(|(before, after)| (before, after, files.relative.clone()))
        .or_else(|| (old_row_bpm == 0 || options.metadata_bpm == Some(0)).then(|| (old_row_bpm, new_bpm, files.relative.clone())));
    let bpm_changed = if row_bpms.is_some() { new_bpm != old_row_bpm } else { new_bpm != u32::from(tempo_x100(&beats)) };
    if next == beats {
        if bpm_changed { set_bpm(new_bpm, false, old_row_bpm)?; }
        if bpm_changed || !matches!(action, GridAction::Edit { .. }) {
            record_history(editor, track, action, beats, next.clone(), options, row_bpms);
        }
        return Ok(GridOutcome {
            state: editor.state_for(track, &next), bpm_changed, written: bpm_changed, beats: wire_beats(&next),
        });
    }
    editor.back_up(&files.dat, &files.relative, "DAT")?;
    editor.back_up(&files.ext, &files.relative, "EXT")?;
    let mut changed_files = vec![(files.dat.clone(), parsed.with_beat_grid(&next))];
    if files.ext.exists() {
        let ext = rbl_anlz::Anlz::read(&files.ext).map_err(|e| AppError::internal(e.to_string()))?;
        if let Some(cleared) = ext.with_extended_grid_edit(&parsed.beat_grid().unwrap_or_default(), &next, parsed.grid_offset().unwrap_or(0)) { changed_files.push((files.ext.clone(), cleared)); }
    }
    let journal = crate::file_journal::FileJournal::prepare_grid(&editor.journal_root, location, track,
        new_bpm, &files.relative, &changed_files)?;
    if let Err(e) = journal.publish() {
        journal.rollback()?;
        return Err(e);
    }
    if let Err(refused) = set_bpm(new_bpm, true, old_row_bpm) {
        journal.reconcile(location)?;
        return Err(refused);
    }
    journal.commit()?;

    record_history(editor, track, action, beats, next.clone(), options, row_bpms);

    Ok(GridOutcome {
        state: GridStateDto { locked: editor.is_locked(track) || database_locked(location, track)?, ..editor.state_for(track, &next) },
        bpm_changed,
        written: true,
        beats: wire_beats(&next),
    })
}

fn row_tempo(location: &rbl_db::LibraryLocation, track: &str) -> AppResult<u32> {
    let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).map_err(write_error)?;
    db.connection().query_row("SELECT COALESCE(BPM,0) FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [track], |r| r.get(0))
        .map_err(|e| AppError::internal(e.to_string()))
}

fn save_tempo(state: &AppState, track: &str, bpm: u32, analysis_changed: bool, before: u32, path: &str) -> AppResult<()> {
    state.write(|w| if analysis_changed { w.save_grid_revision_if_current(track, bpm, before, path) } else { w.set_bpm_metadata(track, before, path, bpm).map(|_| ()) }).map_err(write_error)
}

/// The Info panel's BPM field: the grid editor's Set Tempo over the whole
/// track, through the same durable file/row transaction and recorded in the
/// same per-track history, so the grid panel's Undo puts the grid and
/// `djmdContent.BPM` back together and earlier grid edits stay undoable
/// (#505; rekordbox's `djplay::BeatGridAdjustment::ApplyNewBpm` is undoable
/// too, 7.2.19 symbols).
pub(crate) fn set_tempo(state: &AppState, editor: &GridEditor, track: &str, value: &str) -> AppResult<()> {
    let _edit_guard = state.edit_gate.lock();
    set_tempo_inner(state, editor, track, value)
}

fn set_tempo_inner(state: &AppState, editor: &GridEditor, track: &str, value: &str) -> AppResult<()> {
    let bpm: f64 = value.trim().parse().map_err(|_| AppError::new(ErrorKind::Malformed, "Enter 0 or a BPM from 40 to 499."))?;
    if !bpm.is_finite() || (bpm != 0.0 && !(40.0..=499.0).contains(&bpm)) {
        return Err(AppError::new(ErrorKind::Malformed, "Enter 0 or a BPM from 40 to 499."));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "validated 40..=499 above")]
    let bpm_x100 = (bpm * 100.0).round() as u16;
    let _files = state.analysis_write.lock();
    let location = state.location()?;
    if let Some(reason) = rbl_db::write_refusal_reason(location.is_real_install,
        rbl_db::test_mode(), rbl_db::is_rekordbox_running()) {
        return Err(AppError::new(ErrorKind::ReadOnly, reason));
    }
    if editor.is_locked(track) || database_locked(&location, track)? { return Err(AppError::new(ErrorKind::ReadOnly, "The beat grid is locked.")); }
    crate::file_journal::recover(state.backup_dir(), &location)?;
    let library = state.library()?;
    let row = library.row_of(track).ok_or_else(|| AppError::new(ErrorKind::NotFound, "That track is no longer in the library."))?;
    let relative = state.analysis_path_of(&library, row as usize, track);
    let mut set_bpm = |bpm_x100: u32, analysis_changed: bool, before: u32| save_tempo(state, track, bpm_x100, analysis_changed, before, &relative);
    if relative.is_empty() {
        // No analysis, so no grid and nothing for the grid history.
        return set_bpm(u32::from(bpm_x100), false, row_tempo(&location, track)?);
    }
    let (_, beats) = read_dat(&rbl_anlz::resolve(&location.share_root, &relative))?;
    if bpm_x100 != 0 && rbl_anlz::grid::is_dynamic_from(&beats, None) { return Err(AppError::new(ErrorKind::Malformed, "Use the grid BPM field to confirm replacing tempo changes.")); }
    let options = GridOptions {
        registered: state.registered_analysis(track),
        metadata_bpm: Some(u32::from(bpm_x100)),
        duration_ms: library.length_sec.get(row as usize).map(|s| s.saturating_mul(1000)),
        ..GridOptions::default()
    };
    let edit = GridAction::Edit { edit: GridEdit::Tempo { bpm_x100, anchor_ms: 0 }, from_ms: None };
    let outcome = apply_options(editor, &library, &location, track, edit, &options, &mut set_bpm)?;
    if !outcome.written && bpm_x100 != 0 {
        // The grid already stands at that tempo: nothing to record, but the
        // row still takes the typed value, as it did before #505.
        set_bpm(u32::from(bpm_x100), false, row_tempo(&location, track)?)?;
    }
    Ok(())
}

/// A Devices tree track's grid: its analysis files on the stick and its
/// row in the library it was browsed in (#660).
struct DeviceGrid {
    mount: PathBuf,
    format: rbl_export::device_library::Format,
    track: u32,
    dat: PathBuf,
    ext: PathBuf,
    ext_present: bool,
}

impl DeviceGrid {
    fn of(assets: &crate::device_library::DeviceAssets) -> AppResult<Self> {
        let Some((format, track)) = assets.library else {
            return Err(AppError::new(ErrorKind::NotFound, "That track's device library is not known. Open it again from the device."));
        };
        let Some(dat) = assets.analysis_dat() else {
            return Err(AppError::new(ErrorKind::NotFound, "That track has not been analysed, so it has no grid to edit."));
        };
        let ext = rbl_anlz::sibling(&dat, "EXT");
        // Validate both companions before reading or backing up either one.
        // An EXT without a rewritten PQT2 never reaches GridWrite.files, so
        // the publication's later path check cannot protect these reads.
        let plain_file = |path: &Path| crate::device_library::plainly_under(&assets.mount, path)
            && std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file());
        let refused = || AppError::new(ErrorKind::Malformed, "This track's analysis companions must be plain files on the USB. Nothing was changed.");
        if !plain_file(&dat) { return Err(refused()); }
        let ext_present = match std::fs::symlink_metadata(&ext) {
            Ok(_) if plain_file(&ext) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            _ => return Err(refused()),
        };
        Ok(Self { mount: assets.mount.clone(), format, track, dat, ext, ext_present })
    }

    /// `path`, relative to the stick's root.
    fn relative(&self, path: &Path) -> AppResult<PathBuf> {
        path.strip_prefix(&self.mount).map(Path::to_path_buf)
            .map_err(|_| AppError::internal(format!("{} is not on the device", path.display())))
    }

    /// Where the backups of this stick's analysis files go, under the
    /// editor's backup directory: `devices/<volume>/<path on the stick>`,
    /// to which `GridEditor::back_up_device` adds a digest of the content.
    fn backup_key(&self, path: &Path) -> AppResult<String> {
        let volume = self.mount.file_name().map_or_else(|| "device".into(), |n| n.to_string_lossy().into_owned());
        Ok(format!("devices/{volume}/{}", self.relative(path)?.to_string_lossy()))
    }
}

/// The grid of a Devices tree track, as the panel sees it. Its lock is not
/// offered: that would be the stick's to keep.
fn device_read_lease(mount: &Path) -> AppResult<Option<std::fs::File>> {
    let lease = rbl_core::durable::read_lease(mount).map_err(|e| AppError::internal(e.to_string()))?;
    if rbl_export::recovery_pending(mount) {
        return Err(AppError::internal("This USB needs recovery before its grid can be read."));
    }
    Ok(lease)
}

fn device_history_key(track: &str, assets: &crate::device_library::DeviceAssets) -> String {
    let source = assets.library.map(|(format, id)| format!("{}:{id}", format.file_name())).unwrap_or_default();
    format!("{track}|{source}|{}", assets.analysis)
}

pub(crate) fn device_state(editor: &GridEditor, assets: &crate::device_library::DeviceAssets, track: &str) -> AppResult<GridStateDto> {
    let key = device_history_key(track, assets);
    let track = key.as_str();
    let _lease = device_read_lease(&assets.mount)?;
    let grid = DeviceGrid::of(assets)?;
    let (_, beats) = read_dat(&grid.dat)?;
    Ok(GridStateDto { locked: false, lockable: false, ..editor.state_for(track, &beats) })
}

/// Why a grid edit to a stick must be refused, if it must: rekordbox holds
/// a connected stick's libraries open and writes them itself, so only one
/// application may write the stick at a time, as for its playlists.
pub(crate) fn device_refusal(rekordbox_running: bool) -> Option<AppError> {
    rekordbox_running.then(|| AppError::new(ErrorKind::ReadOnly, "Quit rekordbox before editing a beat grid on this USB so only one application writes it."))
}

/// Applies one action to a Devices tree track's grid, on the stick: the
/// track's `.DAT` and `.EXT` there and, when the tempo changed, its row in
/// the stick's library, published together through the export journal
/// (`rbl_export::device_library::write_grid`). Before a file is rewritten,
/// its bytes are copied to the editor's backups unless this session wrote
/// them or a copy of the same bytes is already there. rekordbox does
/// the same for a device track's grid (see `GridWrite`).
pub(crate) fn apply_device(editor: &GridEditor, assets: &crate::device_library::DeviceAssets, track: &str,
    action: GridAction, options: &GridOptions, rekordbox_running: bool) -> AppResult<GridOutcome> {
    if let Some(refused) = device_refusal(rekordbox_running) {
        return Err(refused);
    }
    let _editing = editor.editing.lock();
    let key = device_history_key(track, assets);
    let track = key.as_str();
    let lease = rbl_export::device_library::grid_write_lease(&assets.mount).map_err(|e| crate::device_library::device_error(&e))?;
    let (format, id) = assets.library.ok_or_else(|| AppError::new(ErrorKind::NotFound, "That track's device library is not known. Open it again from the device."))?;
    let current = rbl_export::device_library::read_grid_track(&assets.mount, format, id).map_err(|e| crate::device_library::device_error(&e))?;
    if current.path != assets.audio || current.analysis_path != assets.analysis {
        return Err(AppError::new(ErrorKind::Malformed, "That device row changed. Reload it before editing the grid."));
    }
    let grid = DeviceGrid::of(assets)?;
    let (parsed, beats, original) = read_dat_snapshot(&grid.dat)?;
    let unchanged = |beats: &[Beat]| GridOutcome {
        state: GridStateDto { locked: false, lockable: false, ..editor.state_for(track, beats) },
        bpm_changed: false,
        written: false,
        beats: wire_beats(beats),
    };
    let Some(next) = plan_next(editor, track, &beats, action, options, options.duration_ms, false)? else {
        return Ok(unchanged(&beats));
    };
    editor.back_up_device(&grid.dat, &grid.backup_key(&grid.dat)?)?;
    let mut files = vec![rbl_export::device_library::GridFile {
        path: grid.relative(&grid.dat)?, original, replacement: parsed.with_beat_grid(&next),
    }];
    if grid.ext_present {
        editor.back_up_device(&grid.ext, &grid.backup_key(&grid.ext)?)?;
        let original = std::fs::read(&grid.ext).map_err(|e| AppError::internal(e.to_string()))?;
        let ext = rbl_anlz::parse(&original).map_err(|e| AppError::internal(e.to_string()))?;
        if let Some(replacement) = ext.with_extended_grid_edit(&parsed.beat_grid().unwrap_or_default(), &next, parsed.grid_offset().unwrap_or(0)) {
            files.push(rbl_export::device_library::GridFile { path: grid.relative(&grid.ext)?, original, replacement });
        }
    }
    let (old_bpm, new_bpm) = (u32::from(tempo_x100(&beats)), u32::from(tempo_x100(&next)));
    let bpm_changed = new_bpm != old_bpm;
    let written: Vec<(PathBuf, blake3::Hash)> = files.iter().map(|file| (grid.mount.join(&file.path), blake3::hash(&file.replacement))).collect();
    let write = rbl_export::device_library::GridWrite { audio: assets.audio.clone(), track: grid.track, files, bpm_x100: bpm_changed.then_some(new_bpm) };
    // Hand off to the writer lease; GridFile's original bytes close this gap.
    drop(lease);
    rbl_export::device_library::write_grid(&grid.mount, grid.format, &write).map_err(|e| crate::device_library::device_error(&e))?;
    {
        let mut known = editor.device_written.lock();
        known.retain(|(path, _)| !written.iter().any(|(file, _)| file == path));
        known.extend(written);
    }
    record_history(editor, track, action, beats, next.clone(), options, None);
    Ok(GridOutcome {
        state: GridStateDto { locked: false, lockable: false, ..editor.state_for(track, &next) },
        bpm_changed,
        written: true,
        beats: wire_beats(&next),
    })
}

fn wire_beats(beats: &[Beat]) -> Vec<(u32, u8)> {
    beats.iter().map(|beat| (beat.time_ms, u8::try_from(beat.beat_number).unwrap_or(0))).collect()
}

/// After a written edit: the deck's metronome takes the new grid, and every
/// deck showing the track refetches it (the track's id, inside the event
/// cap).
fn follow_grid<R: tauri::Runtime>(app: &tauri::AppHandle<R>, player: &crate::player::Player, deck: Option<String>, outcome: &GridOutcome, track: &str) {
    // The deck's metronome plays the grid it was given on load; give it this one.
    if let (Some(deck), Some(engine)) = (deck, player.opened()) {
        let grid: Vec<(u32, bool)> = outcome.beats.iter().map(|&(ms, number)| (ms, number == 1)).collect();
        let deck = crate::player::deck_of(&deck);
        if player.loaded_tracks.lock().get(&deck).is_some_and(|loaded| loaded == track) {
            engine.set_metronome_grid(deck, &grid);
        }
    }
    let _ = tauri::Emitter::emit(app, "grid:changed", track);
}

/// Runs one action under `blocking` (a `spawn_blocking` thread: the files
/// and the row are disk work), then tells the interface and the deck what
/// changed.
#[allow(clippy::too_many_arguments, reason = "three managed states, the app, and what the command was given")]
async fn run<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    player: State<'_, Arc<crate::player::Player>>,
    editor: State<'_, Arc<GridEditor>>,
    name: &'static str,
    track: String,
    action: GridAction,
    deck: Option<String>,
    mut options: GridOptions,
) -> AppResult<GridStateDto> {
    // Bind every command to the loaded Devices row, even if another view
    // has since registered the same audio path from a different database.
    if let Some(source) = options.device_source.clone() {
        let editor = Arc::clone(&editor);
        let changed = track.clone();
        let outcome = blocking(name, move || {
            let assets = crate::device_library::grid_assets(&source, &track)?;
            apply_device(&editor, &assets, &track, action, &options, rbl_db::is_rekordbox_app_running())
        }).await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                if matches!(action, GridAction::Undo | GridAction::Redo) { let _ = tauri::Emitter::emit(&app, "grid:changed", &changed); }
                return Err(error);
            }
        };
        if outcome.written { follow_grid(&app, &player, deck, &outcome, &changed); }
        if outcome.bpm_changed { let _ = tauri::Emitter::emit(&app, "devices:changed", ()); }
        return Ok(outcome.state);
    }
    if state.device_assets(&track).is_some() {
        return Err(AppError::new(ErrorKind::Malformed, "Reload this track from its device library before editing its grid."));
    }
    let library = state.library()?;
    let location = state.location()?;
    options.registered = state.registered_analysis(&track);
    let state = Arc::clone(&state);
    let editor = Arc::clone(&editor);
    let outcome = {
        let state = Arc::clone(&state);
        let track = track.clone();
        blocking(name, move || {
            let _edit_guard = state.edit_gate.lock();
            let _files_guard = state.analysis_write.lock();
            let relative = files_of(&library, &location.share_root, &track, options.registered.as_deref())?.relative;
            let mut set_bpm = |bpm_x100: u32, changed: bool, before: u32| save_tempo(&state, &track, bpm_x100, changed, before, &relative);
            apply_options(&editor, &library, &location, &track, action, &options, &mut set_bpm)
        })
        .await
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(refused) => {
            // A refused Undo or Redo may have dropped an entry that no
            // longer fits the grid; the panel refetches, so its buttons say so.
            if matches!(action, GridAction::Undo | GridAction::Redo) {
                let _ = tauri::Emitter::emit(&app, "grid:changed", &track);
            }
            return Err(refused);
        }
    };
    if !outcome.written {
        return Ok(outcome.state);
    }
    follow_grid(&app, &player, deck, &outcome, &track);
    // A changed tempo is a changed column, so the library is re-read as it
    // is after any other track edit.
    if outcome.bpm_changed {
        reload(app, state).await?;
    }
    Ok(outcome.state)
}

/// The grid as the panel shows it: tempo, beat count, undo, redo, lock.
#[tauri::command]
pub async fn grid_state(
    state: State<'_, Arc<AppState>>,
    editor: State<'_, Arc<GridEditor>>,
    track: String,
    device_source: Option<crate::dto::DeviceTrackSourceDto>,
) -> AppResult<GridStateDto> {
    if let Some(source) = device_source {
        let editor = Arc::clone(&editor);
        return blocking("grid_state", move || {
            let assets = crate::device_library::grid_assets(&source, &track)?;
            device_state(&editor, &assets, &track)
        }).await;
    }
    if state.device_assets(&track).is_some() {
        return Err(AppError::new(ErrorKind::Malformed, "Reload this track from its device library before editing its grid."));
    }
    let library = state.library()?;
    let location = state.location()?;
    let registered = state.registered_analysis(&track);
    let editor = Arc::clone(&editor);
    blocking("grid_state", move || {
        let mut result = state_with(&editor, &library, &location.share_root, &track, registered.as_deref())?;
        result.locked |= database_locked(&location, &track)?;
        Ok(result)
    }).await
}

/// One grid edit. `from_ms` names the beat from which it applies — the scope
/// point, or the playhead for the from-here buttons — and `deck` the deck
/// the track is loaded on, so its metronome follows.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "a command takes its states and its arguments flat")]
pub async fn grid_edit<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    player: State<'_, Arc<crate::player::Player>>,
    editor: State<'_, Arc<GridEditor>>,
    track: String,
    edit: GridEdit,
    from_ms: Option<u32>,
    deck: Option<String>,
    options: Option<GridOptions>,
) -> AppResult<GridStateDto> {
    run(app, state, player, editor, "grid_edit", track, GridAction::Edit { edit, from_ms }, deck, options.unwrap_or_default()).await
}

#[tauri::command]
pub async fn grid_undo<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    player: State<'_, Arc<crate::player::Player>>,
    editor: State<'_, Arc<GridEditor>>,
    track: String,
    deck: Option<String>,
    device_source: Option<crate::dto::DeviceTrackSourceDto>,
) -> AppResult<GridStateDto> {
    run(app, state, player, editor, "grid_undo", track, GridAction::Undo, deck, GridOptions { device_source, ..GridOptions::default() }).await
}

#[tauri::command]
pub async fn grid_redo<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    player: State<'_, Arc<crate::player::Player>>,
    editor: State<'_, Arc<GridEditor>>,
    track: String,
    deck: Option<String>,
    device_source: Option<crate::dto::DeviceTrackSourceDto>,
) -> AppResult<GridStateDto> {
    run(app, state, player, editor, "grid_redo", track, GridAction::Redo, deck, GridOptions { device_source, ..GridOptions::default() }).await
}

/// Locks or unlocks a track's grid. Nothing in the library changes: see the
/// module docs for where the lock lives.
#[tauri::command]
pub async fn grid_lock<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    editor: State<'_, Arc<GridEditor>>,
    track: String,
    on: bool,
) -> AppResult<GridStateDto> {
    if state.device_assets(&track).is_some() {
        return Err(AppError::new(ErrorKind::ReadOnly, "Analysis Lock is set on a track in the collection, not on a device."));
    }
    let library = state.library()?;
    let location = state.location()?;
    let registered = state.registered_analysis(&track);
    let editor = Arc::clone(&editor);
    let state = Arc::clone(&state);
    let changed = track.clone();
    let result = blocking("grid_lock", move || {
        let _gate = state.edit_gate.lock();
        state.write(|writer| writer.set_analysis_lock(&track, on)).map_err(write_error)?;
        editor.set_locked(&track, false)?;
        let mut result = state_with(&editor, &library, &location.share_root, &track, registered.as_deref())?;
        result.locked = database_locked(&location, &track)?;
        Ok(result)
    }).await?;
    let _ = tauri::Emitter::emit(&app, "grid:changed", &changed);
    Ok(result)
}

#[cfg(test)]
mod tests {
    // Every test builds its own library and share tree in a tempdir, the way
    // the cue tests do; `RBXPORT_TEST` cannot reach a fixture.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use rbl_anlz::AnlzBuilder;
    use rbl_db::fixture::{self, track_id, Shape};
    use rbl_db::{Library as Db, OpenMode};

    const RELATIVE: &str = "/PIONEER/USBANLZ/ab1/cd2/ANLZ0000.DAT";

    struct Fixture {
        dir: tempfile::TempDir,
        location: rbl_db::LibraryLocation,
        library: Library,
        editor: GridEditor,
        /// Every BPM the action asked the row to take, in order.
        bpms: Vec<u32>,
    }

    /// Eight beats at 120 BPM from 500 ms, numbered from the first.
    fn grid() -> Vec<Beat> {
        (0..8)
            .map(|i| Beat { beat_number: (i % 4) + 1, tempo_x100: 12_000, time_ms: 500 + u32::from(i) * 500 })
            .collect()
    }

    fn open() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let location = fixture::build(dir.path(), Shape::default()).expect("build the fixture");
        fixture::set_analysis_path(&location, 1, RELATIVE).unwrap();
        let dat = rbl_anlz::resolve(&location.share_root, RELATIVE);
        std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
        let mut file = AnlzBuilder::new();
        file.path("/x.mp3").beat_grid(&grid()).vbr_table_zero().cue_lists(false);
        std::fs::write(&dat, file.finish()).unwrap();
        // An `.EXT` with a filled extended grid, as rekordbox writes one.
        let mut ext = AnlzBuilder::new();
        ext.path("/x.mp3").cue_lists(true);
        let mut header = vec![0_u8; 44];
        header[4..8].copy_from_slice(&0x0100_0002_u32.to_be_bytes());
        ext.raw(rbl_core::FourCc::new(b"PQT2"), header, vec![0, 1, 0, 2, 0, 3]);
        std::fs::write(dat.with_extension("EXT"), ext.finish()).unwrap();

        let db = Db::open(location.clone(), OpenMode::ReadOnly).expect("read-only");
        let (library, _) = rbl_index::load(&db).expect("index");
        let editor = GridEditor::at(&dir.path().join("app"));
        Fixture { dir, location, library, editor, bpms: Vec::new() }
    }

    impl Fixture {
        fn track() -> String {
            track_id(1)
        }

        fn run(&mut self, action: GridAction) -> AppResult<GridOutcome> {
            let track = Self::track();
            let bpms = &mut self.bpms;
            let mut set_bpm = |bpm: u32| {
                bpms.push(bpm);
                Ok(())
            };
            apply(&self.editor, &self.library, &self.location, &track, action, &mut set_bpm)
        }

        fn edit(&mut self, edit: GridEdit) -> GridOutcome {
            self.run(GridAction::Edit { edit, from_ms: None }).unwrap()
        }

        fn dat(&self) -> rbl_anlz::Anlz {
            rbl_anlz::Anlz::read(&rbl_anlz::resolve(&self.location.share_root, RELATIVE)).unwrap()
        }

        fn ext(&self) -> rbl_anlz::Anlz {
            rbl_anlz::Anlz::read(&rbl_anlz::resolve(&self.location.share_root, RELATIVE).with_extension("EXT")).unwrap()
        }

        fn times(&self) -> Vec<u32> {
            self.dat().beat_grid().unwrap().iter().map(|b| b.time_ms).collect()
        }
    }

    fn app_state(f: &Fixture) -> AppState {
        let db = Db::open(f.location.clone(), OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(f.dir.path().join("app/backups"));
        state.set_library(library, false, db.schema().db_version, 0, f.location.clone());
        crate::backups::create(&state).unwrap();
        state
    }

    #[test]
    fn typed_bpm_keeps_the_row_and_both_analysis_files_consistent() {
        let f = open();
        let state = app_state(&f);
        set_tempo(&state, &f.editor, &Fixture::track(), "150").unwrap();
        let beats = f.dat().beat_grid().unwrap();
        assert!(beats.iter().any(|b| b.time_ms == 500), "the anchor remains, with one preceding beat filled");
        assert_eq!(beats[2].time_ms, 900);
        assert!(beats.iter().all(|b| b.tempo_x100 == 15_000));
        assert_eq!(f.ext().section(b"PQT2").unwrap().payload, [] as [u8; 0]);
        let bpm: u32 = state.read_db(|db| Ok(db.connection().query_row(
            "SELECT BPM FROM djmdContent WHERE ID=?1", [Fixture::track()], |r| r.get(0))?)).unwrap();
        assert_eq!(bpm, 15_000);
        set_tempo(&state, &f.editor, &track_id(2), "128.5").unwrap();
        let bpm: u32 = state.read_db(|db| Ok(db.connection().query_row(
            "SELECT BPM FROM djmdContent WHERE ID=?1", [track_id(2)], |r| r.get(0))?)).unwrap();
        assert_eq!(bpm, 12_850, "a track without analysis still gets its BPM");
    }

    #[test]
    fn a_refused_typed_bpm_restores_dat_and_ext() {
        let f = open();
        let state = app_state(&f);
        let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
        let before_dat = std::fs::read(&dat).unwrap();
        let before_ext = std::fs::read(dat.with_extension("EXT")).unwrap();
        let db = Db::open(f.location.clone(), OpenMode::ReadWrite).unwrap();
        db.connection().execute_batch("CREATE TRIGGER refuse_bpm BEFORE UPDATE OF BPM ON djmdContent BEGIN SELECT RAISE(ABORT, 'refused'); END;").unwrap();
        drop(db);
        assert!(set_tempo(&state, &f.editor, &Fixture::track(), "150").is_err());
        assert_eq!(std::fs::read(&dat).unwrap(), before_dat);
        assert_eq!(std::fs::read(dat.with_extension("EXT")).unwrap(), before_ext);
        for invalid in ["NaN", "inf", "0", "401", "not a tempo"] {
            assert!(set_tempo(&state, &f.editor, &Fixture::track(), invalid).is_err());
        }
        assert_eq!(std::fs::read(&dat).unwrap(), before_dat);
    }

    #[test]
    fn accepted_tap_run_shares_one_undo_transaction() {
        let f = open();
        let original = f.dat().beat_grid().unwrap();
        let options = GridOptions { transaction: Some("tap-test-run".into()), duration_ms: Some(4500), ..GridOptions::default() };
        let mut save = |_bpm, _changed, _before| Ok(());
        for bpm in [120.0, 121.25] {
            apply_options(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Edit {
                edit: GridEdit::Tap { bpm, anchor_ms: 1600 }, from_ms: None,
            }, &options, &mut save).unwrap();
        }
        assert_eq!(f.editor.histories.lock()[&Fixture::track()].undo.len(), 1);
        let last_tap = f.dat().beat_grid().unwrap();
        apply(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Undo, &mut |bpm| save(bpm, true, 0)).unwrap();
        assert_eq!(f.dat().beat_grid().unwrap(), original);
        // Redo lands on the run's last tap, not its first.
        apply(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Redo, &mut |bpm| save(bpm, true, 0)).unwrap();
        assert_eq!(f.dat().beat_grid().unwrap(), last_tap);
    }

    /// One grid action as the commands run it: the row written through the
    /// app's writer, not the fixture's recorder.
    fn through_writer(f: &Fixture, state: &AppState, action: GridAction) -> AppResult<GridOutcome> {
        // Held as `run` holds it, so the writer leaves this journal alone.
        let _files = state.analysis_write.lock();
        let mut set_bpm = |bpm: u32, changed: bool, before: u32| save_tempo(state, &Fixture::track(), bpm, changed, before, RELATIVE);
        apply_options(&f.editor, &f.library, &f.location, &Fixture::track(), action, &GridOptions::default(), &mut set_bpm)
    }

    fn row_bpm(state: &AppState) -> u32 {
        state.read_db(|db| Ok(db.connection().query_row(
            "SELECT BPM FROM djmdContent WHERE ID=?1", [Fixture::track()], |r| r.get(0))?)).unwrap()
    }

    #[test]
    fn zero_bpm_is_undoable_metadata_and_never_a_zero_grid() {
        let f = open();
        fixture::set_tempo(&f.location, 1, 60_000).unwrap();
        let state = app_state(&f);
        let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
        let before_dat = std::fs::read(&dat).unwrap();
        let before_ext = std::fs::read(dat.with_extension("EXT")).unwrap();
        let old_bpm = row_bpm(&state);
        let revision = || state.read_db(|db| Ok(db.connection().query_row("SELECT AnalysisUpdated FROM djmdContent WHERE ID=?1", [Fixture::track()], |r| r.get::<_, Option<String>>(0))?)).unwrap();
        let old_revision = revision();
        set_tempo(&state, &f.editor, &Fixture::track(), "0.00").unwrap();
        assert_eq!(row_bpm(&state), 0);
        assert_eq!(revision(), old_revision);
        assert_eq!(std::fs::read(&dat).unwrap(), before_dat);
        assert_eq!(std::fs::read(dat.with_extension("EXT")).unwrap(), before_ext);
        assert!(f.dat().beat_grid().unwrap().iter().all(|b| b.tempo_x100 > 0));
        assert_eq!(f.editor.state_for(&Fixture::track(), &grid()).bpm_x100, 12_000);
        assert!(!f.editor.backup_dir.exists(), "metadata does not back up analysis");
        through_writer(&f, &state, GridAction::Undo).unwrap();
        assert_eq!(row_bpm(&state), old_bpm);
        through_writer(&f, &state, GridAction::Redo).unwrap();
        assert_eq!(row_bpm(&state), 0);
        assert_eq!(std::fs::read(&dat).unwrap(), before_dat);
        assert_eq!(std::fs::read(dat.with_extension("EXT")).unwrap(), before_ext);
        assert_eq!(revision(), old_revision);

        // A later positive tempo edits the grid; Undo restores the zero
        // marker and the original grid together, not the grid's positive BPM.
        set_tempo(&state, &f.editor, &Fixture::track(), "150").unwrap();
        assert_eq!(row_bpm(&state), 15_000);
        through_writer(&f, &state, GridAction::Undo).unwrap();
        assert_eq!(row_bpm(&state), 0);
        assert_eq!(f.dat().beat_grid().unwrap(), grid());
        through_writer(&f, &state, GridAction::Redo).unwrap();
        assert_eq!(row_bpm(&state), 15_000);
    }

    #[test]
    fn zero_marker_to_the_existing_grid_tempo_remains_undoable_without_files() {
        let f = open();
        let state = app_state(&f);
        set_tempo(&state, &f.editor, &Fixture::track(), "0").unwrap();
        let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
        let before = std::fs::read(&dat).unwrap();
        set_tempo(&state, &f.editor, &Fixture::track(), "120").unwrap();
        assert_eq!(row_bpm(&state), 12_000);
        through_writer(&f, &state, GridAction::Undo).unwrap();
        assert_eq!(row_bpm(&state), 0);
        assert_eq!(std::fs::read(&dat).unwrap(), before);
    }

    #[test]
    fn zero_marker_refuses_external_row_changes_and_keeps_unrelated_tracks() {
        let f = open();
        let state = app_state(&f);
        let other = state.read_db(|db| Ok(db.connection().query_row("SELECT BPM FROM djmdContent WHERE ID=?1", [track_id(2)], |r| r.get::<_, u32>(0))?)).unwrap();
        set_tempo(&state, &f.editor, &Fixture::track(), "0").unwrap();
        fixture::set_tempo(&f.location, 1, 17_500).unwrap();
        assert_eq!(through_writer(&f, &state, GridAction::Undo).unwrap_err().kind, ErrorKind::Conflict);
        assert_eq!(row_bpm(&state), 17_500);
        assert_eq!(state.read_db(|db| Ok(db.connection().query_row("SELECT BPM FROM djmdContent WHERE ID=?1", [track_id(2)], |r| r.get::<_, u32>(0))?)).unwrap(), other);
    }

    #[test]
    fn zero_and_positive_bpm_validation_is_bounded_before_any_write() {
        let f = open();
        let state = app_state(&f);
        let before = row_bpm(&state);
        for value in ["-1", "NaN", "inf", "1e999", "655.36", "500", "0.01", "", "120junk"] {
            assert!(set_tempo(&state, &f.editor, &Fixture::track(), value).is_err(), "{value}");
            assert_eq!(row_bpm(&state), before);
            assert!(!f.editor.has_history(&Fixture::track()));
        }
        for value in ["40", "499", "0"] {
            set_tempo(&state, &f.editor, &Fixture::track(), value).unwrap();
        }
        assert_eq!(row_bpm(&state), 0);
        assert!(f.dat().beat_grid().unwrap().iter().all(|b| b.tempo_x100 > 0));
    }

    #[test]
    fn zero_bpm_works_without_analysis_and_preserves_a_dynamic_grid() {
        let f = open();
        let state = app_state(&f);
        set_tempo(&state, &f.editor, &track_id(2), "0").unwrap();
        assert_eq!(state.read_db(|db| Ok(db.connection().query_row("SELECT BPM FROM djmdContent WHERE ID=?1", [track_id(2)], |r| r.get::<_, u32>(0))?)).unwrap(), 0);
        assert!(!f.editor.has_history(&track_id(2)));
        let mut dynamic = grid();
        dynamic[4].tempo_x100 = 13_000;
        written_elsewhere(&f, &dynamic);
        let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
        let bytes = std::fs::read(&dat).unwrap();
        set_tempo(&state, &f.editor, &Fixture::track(), "0").unwrap();
        assert_eq!(std::fs::read(&dat).unwrap(), bytes);
        assert_eq!(f.dat().beat_grid().unwrap(), dynamic);
    }

    #[test]
    fn zero_bpm_refuses_stale_analysis_and_failed_writes_without_history_or_file_changes() {
        let f = open();
        let state = app_state(&f);
        let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
        let bytes = std::fs::read(&dat).unwrap();
        let before = row_bpm(&state);
        fixture::set_analysis_path(&f.location, 1, "/PIONEER/USBANLZ/other/ANLZ0000.DAT").unwrap();
        assert!(set_tempo(&state, &f.editor, &Fixture::track(), "0").is_err());
        assert_eq!(row_bpm(&state), before);
        assert!(f.editor.histories.lock()[&Fixture::track()].undo.is_empty());
        fixture::set_analysis_path(&f.location, 1, RELATIVE).unwrap();
        let db = Db::open(f.location.clone(), OpenMode::ReadWrite).unwrap();
        db.connection().execute_batch("CREATE TRIGGER refuse_bpm BEFORE UPDATE OF BPM ON djmdContent BEGIN SELECT RAISE(ABORT, 'refused'); END;").unwrap();
        drop(db);
        assert!(set_tempo(&state, &f.editor, &Fixture::track(), "0").is_err());
        assert_eq!(row_bpm(&state), before);
        assert!(f.editor.histories.lock()[&Fixture::track()].undo.is_empty());
        assert_eq!(std::fs::read(&dat).unwrap(), bytes);
        assert!(!f.editor.backup_dir.exists());
    }

    #[test]
    fn zero_history_refuses_a_different_analysis_even_with_identical_grid_bytes() {
        let mut f = open();
        let state = app_state(&f);
        set_tempo(&state, &f.editor, &Fixture::track(), "0").unwrap();
        let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
        let bytes = std::fs::read(&dat).unwrap();
        let relative = "/PIONEER/USBANLZ/replaced/ANLZ0000.DAT";
        let replacement = rbl_anlz::resolve(&f.location.share_root, relative);
        std::fs::create_dir_all(replacement.parent().unwrap()).unwrap();
        std::fs::write(&replacement, &bytes).unwrap();
        fixture::set_analysis_path(&f.location, 1, relative).unwrap();
        let db = Db::open(f.location.clone(), OpenMode::ReadOnly).unwrap();
        f.library = rbl_index::load(&db).unwrap().0;
        let mut save = |bpm, changed, before| save_tempo(&state, &Fixture::track(), bpm, changed, before, relative);
        let outcome = apply_options(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Undo, &GridOptions::default(), &mut save);
        assert_eq!(outcome.unwrap_err().kind, ErrorKind::Conflict);
        assert_eq!(row_bpm(&state), 0);
        assert_eq!(std::fs::read(&replacement).unwrap(), bytes);
        assert!(!f.editor.backup_dir.exists());
    }

    #[test]
    fn a_refused_grid_write_cannot_claim_an_external_matching_bpm_as_its_commit() {
        for elsewhere in [RELATIVE, "/PIONEER/USBANLZ/elsewhere/ANLZ0000.DAT"] {
            let f = open();
            let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
            let ext = dat.with_extension("EXT");
            let old_dat = std::fs::read(&dat).unwrap();
            let old_ext = std::fs::read(&ext).unwrap();
            let db = Db::open(f.location.clone(), OpenMode::ReadWrite).unwrap();
            let (old_usn, old_bpm): (i64, u32) = db.connection().query_row("SELECT rb_local_usn, BPM FROM djmdContent WHERE ID=?1", [Fixture::track()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            let mut refused = |bpm, _changed, _before| {
                db.connection().execute("UPDATE djmdContent SET BPM=?2, AnalysisDataPath=?3, rb_local_usn=rb_local_usn+1 WHERE ID=?1", rusqlite::params![Fixture::track(), bpm, elsewhere]).unwrap();
                Err(AppError::new(ErrorKind::Conflict, "The planned row no longer matches."))
            };
            let result = apply_options(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Edit {
                edit: GridEdit::Tempo { bpm_x100: 15_000, anchor_ms: 0 }, from_ms: None,
            }, &GridOptions::default(), &mut refused);
            assert!(result.is_err());
            let pending = f.editor.journal_root.join("analysis-journal");
            assert_eq!(std::fs::read_dir(&pending).unwrap().count(), 1, "the outside row is not proof that this grid committed");
            assert!(f.editor.histories.lock()[&Fixture::track()].undo.is_empty());
            let row: (u32, String, i64) = db.connection().query_row("SELECT BPM, AnalysisDataPath, rb_local_usn FROM djmdContent WHERE ID=?1", [Fixture::track()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
            assert_eq!(row, (15_000, elsewhere.to_owned(), old_usn + 1), "preserve the outside row");
            // The journal retained its original images. Restoring the exact
            // uncommitted row lets recovery roll both files back, not retain
            // the rejected edit's published images as a completed operation.
            db.connection().execute("UPDATE djmdContent SET BPM=?2, AnalysisDataPath=?3, rb_local_usn=?4 WHERE ID=?1", rusqlite::params![Fixture::track(), old_bpm, RELATIVE, old_usn]).unwrap();
            crate::file_journal::recover(&f.editor.journal_root, &f.location).unwrap();
            assert_eq!(std::fs::read(&dat).unwrap(), old_dat);
            assert_eq!(std::fs::read(&ext).unwrap(), old_ext);
        }
    }

    #[test]
    fn a_real_grid_commit_reported_as_an_error_keeps_its_files() {
        let f = open();
        let state = app_state(&f);
        let _files = state.analysis_write.lock();
        let mut save_then_error = |bpm, changed, before| {
            save_tempo(&state, &Fixture::track(), bpm, changed, before, RELATIVE)?;
            Err(AppError::internal("Failure after the database commit."))
        };
        let result = apply_options(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Edit {
            edit: GridEdit::Tempo { bpm_x100: 15_000, anchor_ms: 0 }, from_ms: None,
        }, &GridOptions::default(), &mut save_then_error);
        assert!(result.is_err());
        assert_eq!(row_bpm(&state), 15_000);
        assert!(f.dat().beat_grid().unwrap().iter().all(|beat| beat.tempo_x100 == 15_000));
        assert_eq!(std::fs::read_dir(f.editor.journal_root.join("analysis-journal")).unwrap().count(), 0);
    }

    #[test]
    fn a_typed_bpm_is_one_grid_undo_step_and_keeps_the_earlier_ones() {
        // #505: the Info panel's BPM is recorded as the grid editor's Set
        // Tempo. Undo puts the grid and the row back together, Redo puts the
        // typed tempo back, and the grid edit made before it stays undoable.
        let f = open();
        let state = app_state(&f);
        through_writer(&f, &state, GridAction::Edit { edit: GridEdit::Nudge { ms: 20 }, from_ms: None }).unwrap();
        let nudged = f.dat().beat_grid().unwrap();
        assert_eq!(row_bpm(&state), 12_000);

        set_tempo(&state, &f.editor, &Fixture::track(), "150").unwrap();
        let typed = f.dat().beat_grid().unwrap();
        assert!(typed.iter().all(|b| b.tempo_x100 == 15_000));
        assert_eq!(row_bpm(&state), 15_000);
        let after_typing = f.editor.state_for(&Fixture::track(), &typed);
        assert_eq!((after_typing.can_undo, after_typing.undo_label, after_typing.can_redo), (true, Some("Set Tempo"), false));
        assert_eq!(f.editor.histories.lock()[&Fixture::track()].undo.len(), 2, "the nudge's entry survived the typed BPM");

        let undone = through_writer(&f, &state, GridAction::Undo).unwrap();
        assert_eq!(f.dat().beat_grid().unwrap(), nudged, "the grid is the one before the typed BPM");
        assert_eq!(row_bpm(&state), 12_000, "and so is the row");
        assert_eq!((undone.state.undo_label, undone.state.redo_label), (Some("Shift Beat Grid Right"), Some("Set Tempo")));

        through_writer(&f, &state, GridAction::Redo).unwrap();
        assert_eq!(f.dat().beat_grid().unwrap(), typed);
        assert_eq!(row_bpm(&state), 15_000);

        through_writer(&f, &state, GridAction::Undo).unwrap();
        through_writer(&f, &state, GridAction::Undo).unwrap();
        assert_eq!(f.dat().beat_grid().unwrap(), grid(), "the earlier nudge undid too");
        assert_eq!(row_bpm(&state), 12_000);
        assert!(!f.editor.state_for(&Fixture::track(), &grid()).can_undo);
    }

    #[test]
    fn a_typed_bpm_after_an_undo_ends_the_redo_branch() {
        let mut f = open();
        f.edit(GridEdit::Double);
        f.run(GridAction::Undo).unwrap();
        let state = app_state(&f);
        set_tempo(&state, &f.editor, &Fixture::track(), "125").unwrap();
        let now = f.dat().beat_grid().unwrap();
        let after = f.editor.state_for(&Fixture::track(), &now);
        assert_eq!((after.can_redo, after.undo_label), (false, Some("Set Tempo")));
    }

    #[test]
    fn a_refused_typed_bpm_records_nothing() {
        let mut f = open();
        f.edit(GridEdit::Nudge { ms: 10 });
        let state = app_state(&f);
        let db = Db::open(f.location.clone(), OpenMode::ReadWrite).unwrap();
        db.connection().execute_batch("CREATE TRIGGER refuse_bpm BEFORE UPDATE OF BPM ON djmdContent BEGIN SELECT RAISE(ABORT, 'refused'); END;").unwrap();
        drop(db);
        assert!(set_tempo(&state, &f.editor, &Fixture::track(), "150").is_err());
        let history = &f.editor.histories.lock()[&Fixture::track()];
        assert_eq!(history.undo.len(), 1);
        assert_eq!(history.undo[0].label, "Shift Beat Grid Right");
    }

    /// Writes `beats` into the fixture's analysis file the way another
    /// program would: outside this editor and its history.
    fn written_elsewhere(f: &Fixture, beats: &[Beat]) {
        let dat = rbl_anlz::resolve(&f.location.share_root, RELATIVE);
        std::fs::write(&dat, f.dat().with_beat_grid(beats)).unwrap();
    }

    #[test]
    fn an_undo_over_a_grid_changed_elsewhere_is_refused_and_keeps_the_newer_grid() {
        // #506's policy on the grid history: a grid written since the entry
        // (rekordbox, say, while RBXport stayed open) is kept, and the
        // Undo says why it did not happen.
        let mut f = open();
        f.edit(GridEdit::Double);
        let elsewhere: Vec<Beat> = grid().into_iter().map(|b| Beat { time_ms: b.time_ms + 7, ..b }).collect();
        written_elsewhere(&f, &elsewhere);
        let refused = f.run(GridAction::Undo).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::Conflict);
        assert_eq!(refused.message, "Can't undo Double Tempo: the track's beat grid has changed since. The newer change was kept.");
        assert_eq!(f.dat().beat_grid().unwrap(), elsewhere, "the newer grid stays");
        assert_eq!(f.bpms.len(), 1, "the row was not written either");
        let now = f.editor.state_for(&Fixture::track(), &elsewhere);
        assert!(!now.can_undo && !now.can_redo, "the conflicting entry was dropped");
    }

    #[test]
    fn a_refused_undo_drops_only_its_entry_and_the_older_ones_still_undo() {
        // A grid put back elsewhere to the one before the newest edit: that
        // edit's Undo is refused and dropped, the one before it still fits.
        let mut f = open();
        f.edit(GridEdit::Nudge { ms: 10 });
        let nudged = f.dat().beat_grid().unwrap();
        f.edit(GridEdit::Double);
        written_elsewhere(&f, &nudged);
        assert_eq!(f.run(GridAction::Undo).unwrap_err().kind, ErrorKind::Conflict);
        let left = f.editor.state_for(&Fixture::track(), &nudged);
        assert_eq!((left.undo_label, left.can_redo), (Some("Shift Beat Grid Right"), false));
        f.run(GridAction::Undo).unwrap();
        assert_eq!(f.dat().beat_grid().unwrap(), grid());
    }

    #[test]
    fn a_redo_over_a_grid_changed_elsewhere_is_refused_and_other_tracks_keep_theirs() {
        let mut f = open();
        f.edit(GridEdit::Halve);
        f.run(GridAction::Undo).unwrap();
        f.editor.histories.lock().entry("other".into()).or_default().undo.push(HistoryEntry { before: grid(), after: grid(), label: "Set Tempo", row_bpms: None });
        let elsewhere: Vec<Beat> = grid().into_iter().map(|b| Beat { time_ms: b.time_ms + 3, ..b }).collect();
        written_elsewhere(&f, &elsewhere);
        let refused = f.run(GridAction::Redo).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::Conflict);
        assert_eq!(refused.message, "Can't redo Halve Tempo: the track's beat grid has changed since. The newer change was kept.");
        assert_eq!(f.dat().beat_grid().unwrap(), elsewhere);
        assert!(f.editor.state_for("other", &grid()).can_undo, "an unrelated track's history is preserved");
    }

    #[test]
    fn the_edit_wire_shape_is_a_tagged_kind() {
        let edit: GridEdit = serde_json::from_str(r#"{"kind":"nudge","ms":-3}"#).unwrap();
        assert_eq!(edit, GridEdit::Nudge { ms: -3 });
        let edit: GridEdit = serde_json::from_str(r#"{"kind":"tempo","bpmX100":12800,"anchorMs":500}"#).unwrap();
        assert_eq!(edit, GridEdit::Tempo { bpm_x100: 12_800, anchor_ms: 500 });
        assert!(serde_json::from_str::<GridEdit>(r#"{"kind":"reset"}"#).is_err());
        // Times are unsigned: the panel clamps a playhead before the track's
        // start to zero, because Tauri refuses this with a bare string (#107).
        assert!(serde_json::from_str::<GridEdit>(r#"{"kind":"downbeat","timeMs":-120}"#).is_err());
        assert_eq!(Edit::from(GridEdit::Halve), Edit::Halve);
    }

    /// 200 BPM from 100 ms for ten beats, then a tempo change to 100 BPM:
    /// the grid in the #107 recording.
    fn with_tempo_change(f: &Fixture) {
        let mut beats: Vec<Beat> = (0..10_u16).map(|i| Beat { beat_number: (i % 4) + 1, tempo_x100: 20_000, time_ms: 100 + u32::from(i) * 300 }).collect();
        beats.extend((0..10_u16).map(|i| Beat { beat_number: ((10 + i) % 4) + 1, tempo_x100: 10_000, time_ms: 3100 + u32::from(i) * 600 }));
        let mut file = AnlzBuilder::new();
        file.path("/x.mp3").beat_grid(&beats).vbr_table_zero().cue_lists(false);
        std::fs::write(rbl_anlz::resolve(&f.location.share_root, RELATIVE), file.finish()).unwrap();
    }

    fn confirmed(f: &Fixture, edit: GridEdit, from_ms: Option<u32>, allow_dynamic: bool) -> AppResult<GridOutcome> {
        let options = GridOptions { allow_dynamic, duration_ms: Some(9100), ..GridOptions::default() };
        apply_options(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Edit { edit, from_ms }, &options, &mut |_, _, _| Ok(()))
    }

    #[test]
    fn a_confirmed_tempo_edit_over_tempo_changes_replaces_them() {
        // #107 rows 2 and 3: rekordbox asks, then deletes the tempo changes in
        // scope. Unconfirmed, the backend refuses with a reason; confirmed, the
        // whole track (or everything from the scope point) takes one tempo.
        let f = open();
        with_tempo_change(&f);
        let refused = confirmed(&f, GridEdit::Stretch { by_ms: -1, time_ms: 400 }, None, false).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::Malformed);
        assert_ne!(refused.message, "");

        confirmed(&f, GridEdit::Stretch { by_ms: -1, time_ms: 400 }, None, true).unwrap();
        let beats = f.dat().beat_grid().unwrap();
        assert!(beats.iter().all(|b| b.tempo_x100 == beats[0].tempo_x100), "no tempo change survives");
        assert_eq!(beats[0].time_ms, 100);

        with_tempo_change(&f);
        confirmed(&f, GridEdit::Tempo { bpm_x100: 12_000, anchor_ms: 0 }, None, true).unwrap();
        assert!(f.dat().beat_grid().unwrap().iter().all(|b| b.tempo_x100 == 12_000));

        // From a beat inside the first section: the head before it stays.
        with_tempo_change(&f);
        confirmed(&f, GridEdit::Tempo { bpm_x100: 15_000, anchor_ms: 1000 }, Some(1000), true).unwrap();
        let beats = f.dat().beat_grid().unwrap();
        assert!(beats.iter().filter(|b| b.time_ms < 1000).all(|b| b.tempo_x100 == 20_000));
        assert!(beats.iter().filter(|b| b.time_ms >= 1000).all(|b| b.tempo_x100 == 15_000));
    }

    #[test]
    fn a_downbeat_at_the_start_puts_beat_one_at_zero() {
        // What the panel sends for "set 1st beat" with the playhead before
        // the track: rekordbox puts the bar's first beat at the very start
        // (as reported in #107).
        let mut f = open();
        f.edit(GridEdit::Downbeat { time_ms: 0 });
        let beats = f.dat().beat_grid().unwrap();
        assert_eq!((beats[0].time_ms, beats[0].beat_number), (0, 1));
        assert_eq!(beats[1].time_ms, 500);
    }

    #[test]
    fn a_nudge_rewrites_the_grid_empties_the_extended_grid_and_leaves_the_row() {
        let mut f = open();
        let before_ext = f.ext();
        assert_ne!(before_ext.section(b"PQT2").unwrap().payload, [] as [u8; 0]);

        let outcome = f.edit(GridEdit::Nudge { ms: 20 });
        assert!(outcome.written);
        assert!(!outcome.bpm_changed);
        assert_eq!(f.bpms, vec![12000], "phase edits also update the analysis revision");
        assert_eq!(&f.times()[..9], &[20,520,1020,1520,2020,2520,3020,3520,4020]);
        assert_eq!(outcome.beats[0], (20, 4));
        assert!(outcome.state.can_undo);
        assert_eq!(outcome.state.bpm_x100, 12000);

        // Only the grid changed in the `.DAT`, and only `PQT2` in the `.EXT`.
        let dat = f.dat();
        assert_eq!(dat.path().as_deref(), Some("/x.mp3"));
        assert_eq!(dat.sections.len(), 5);
        let ext = f.ext();
        assert_eq!(ext.section(b"PQT2").unwrap().payload, [] as [u8; 0]);
        assert_eq!(ext.sections.len(), before_ext.sections.len());
        assert_eq!(ext.sections[0], before_ext.sections[0]);
    }

    #[test]
    fn a_tempo_change_sets_the_row_and_undo_sets_it_back() {
        let mut f = open();
        let outcome = f.edit(GridEdit::Stretch { by_ms: -2, time_ms: 1000 });
        assert!(outcome.bpm_changed);
        assert_eq!(f.bpms, vec![12_048]);
        assert_eq!(outcome.state.bpm_x100, 12_048);

        let undone = f.run(GridAction::Undo).unwrap();
        assert_eq!(f.bpms, vec![12_048, 12_000]);
        assert_eq!(f.times(), (0..8).map(|i| 500 + i * 500).collect::<Vec<_>>());
        assert_eq!(undone.state, GridStateDto { bpm_x100: 12_000, beats: 8, can_undo: false, can_redo: true, undo_label: None, redo_label: Some("Adjust Tempo"), locked: false, lockable: true });

        let redone = f.run(GridAction::Redo).unwrap();
        assert_eq!(f.bpms, vec![12_048, 12_000, 12_048]);
        assert!(redone.state.can_undo);
        assert_eq!(redone.state.undo_label, Some("Adjust Tempo"));
        assert_eq!(redone.state.redo_label, None);
        assert!(!redone.state.can_redo);

        // A new edit forgets what could have been redone.
        f.run(GridAction::Undo).unwrap();
        assert!(f.editor.state_for(&Fixture::track(), &grid()).can_redo);
        let edited = f.edit(GridEdit::Nudge { ms: 1 });
        assert!(!edited.state.can_redo, "a fresh edit ends the redo stack");
        assert_eq!(edited.state.undo_label, Some("Shift Beat Grid Right"));
        assert_eq!(edited.state.redo_label, None);
        let err = f.run(GridAction::Redo).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
    }

    #[test]
    fn nothing_to_undo_is_said_not_written() {
        let mut f = open();
        let err = f.run(GridAction::Undo).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert_eq!(f.times()[0], 500);
    }

    #[test]
    fn an_edit_that_changes_nothing_writes_nothing() {
        let mut f = open();
        f.edit(GridEdit::Nudge { ms: 0 });
        let stamp = std::fs::metadata(rbl_anlz::resolve(&f.location.share_root, RELATIVE)).unwrap().modified().unwrap();
        let outcome = f.edit(GridEdit::Nudge { ms: 0 });
        assert!(!outcome.written);
        assert!(outcome.state.can_undo);
        let after = std::fs::metadata(rbl_anlz::resolve(&f.location.share_root, RELATIVE)).unwrap().modified().unwrap();
        assert_eq!(stamp, after);
    }

    #[test]
    fn an_edit_from_a_point_keeps_the_head() {
        let mut f = open();
        f.run(GridAction::Edit { edit: GridEdit::Nudge { ms: 100 }, from_ms: Some(2500) }).unwrap();
        assert_eq!(&f.times()[..9], &[0,500,1000,1500,2000,2600,3100,3600,4100]);
    }

    #[test]
    fn a_refused_undo_keeps_the_history_and_both_file_images() {
        let mut f = open();
        f.edit(GridEdit::Double);
        let before_dat = f.dat().beat_grid().unwrap();
        let before_ext = std::fs::read(rbl_anlz::resolve(&f.location.share_root, RELATIVE).with_extension("EXT")).unwrap();
        let mut refuse = |_bpm: u32| Err(AppError::new(ErrorKind::ReadOnly, "no"));
        assert!(apply(&f.editor, &f.library, &f.location, &Fixture::track(), GridAction::Undo, &mut refuse).is_err());
        assert_eq!(f.dat().beat_grid().unwrap(), before_dat);
        assert_eq!(std::fs::read(rbl_anlz::resolve(&f.location.share_root, RELATIVE).with_extension("EXT")).unwrap(), before_ext);
        assert!(f.editor.state_for(&Fixture::track(), &before_dat).can_undo);
        assert!(!f.editor.state_for(&Fixture::track(), &before_dat).can_redo);
        f.run(GridAction::Undo).unwrap();
        assert_eq!(f.dat().beat_grid().unwrap(), grid());
    }

    #[test]
    fn a_row_that_refuses_the_tempo_gets_its_grid_back() {
        let mut f = open();
        let mut refuse = |_bpm: u32| Err(AppError::new(ErrorKind::ReadOnly, "no"));
        let err = apply(
            &f.editor,
            &f.library,
            &f.location,
            &Fixture::track(),
            GridAction::Edit { edit: GridEdit::Double, from_ms: None },
            &mut refuse,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::ReadOnly);
        assert_eq!(f.times().len(), 8, "the doubled grid was put back");
        assert!(!f.run(GridAction::Undo).is_ok_and(|o| o.written), "nothing to undo either");
    }

    #[test]
    fn the_lock_refuses_edits_and_outlives_the_editor() {
        let mut f = open();
        f.editor.set_locked(&Fixture::track(), true).unwrap();
        let err = f.edit_result(GridEdit::Halve).unwrap_err();
        assert_eq!(err.kind, ErrorKind::ReadOnly);
        assert_eq!(f.times().len(), 8);
        assert!(state_of(&f.editor, &f.library, &f.location.share_root, &Fixture::track()).unwrap().locked);

        let again = GridEditor::at(&f.dir.path().join("app"));
        assert!(again.is_locked(&Fixture::track()));
        again.set_locked(&Fixture::track(), false).unwrap();
        assert!(!GridEditor::at(&f.dir.path().join("app")).is_locked(&Fixture::track()));
        assert!(!again.is_locked("other"));
    }

    impl Fixture {
        fn edit_result(&mut self, edit: GridEdit) -> AppResult<GridOutcome> {
            self.run(GridAction::Edit { edit, from_ms: None })
        }
    }

    #[test]
    fn the_first_rewrite_backs_the_files_up_and_later_ones_do_not_touch_the_copy() {
        let mut f = open();
        f.edit(GridEdit::Nudge { ms: 5 });
        let copy = f.dir.path().join("app/backups/analysis/PIONEER/USBANLZ/ab1/cd2/ANLZ0000.DAT");
        let kept = rbl_anlz::Anlz::read(&copy).unwrap().beat_grid().unwrap();
        assert_eq!(kept[0].time_ms, 500, "the copy is the grid before any edit");
        assert!(copy.with_extension("EXT").is_file());
        f.edit(GridEdit::Nudge { ms: 5 });
        // A second session finds the copy already there and leaves it.
        let later = GridEditor::at(&f.dir.path().join("app"));
        let mut none = |_bpm: u32| Ok(());
        apply(&later, &f.library, &f.location, &Fixture::track(), GridAction::Edit { edit: GridEdit::Nudge { ms: 5 }, from_ms: None }, &mut none).unwrap();
        assert_eq!(rbl_anlz::Anlz::read(&copy).unwrap().beat_grid().unwrap()[0].time_ms, 500);
        assert_eq!(f.times()[0], 15);
    }

    #[test]
    fn an_edit_past_the_history_cap_drops_the_oldest_grid_so_the_first_one_cannot_be_undone_to() {
        let mut f = open();
        // One edit more than the cap fits: each pushes the grid it replaced,
        // and the push past the cap takes the front of the stack off.
        let edits = HISTORY_CAP + 1;
        for _ in 0..edits {
            f.edit(GridEdit::Nudge { ms: 1 });
        }
        let cap = u32::try_from(HISTORY_CAP).unwrap();
        assert_eq!(f.times()[0], cap + 1);
        assert_eq!(f.editor.histories.lock()[&Fixture::track()].undo.len(), HISTORY_CAP, "the stack stops at the cap");

        // Undoing as far as the stack goes lands on the grid after the first
        // edit, not on the grid the track started with: the entry holding it
        // was the one dropped, and nothing else keeps it. The backup on disk
        // is what the original grid survives in.
        for _ in 0..HISTORY_CAP {
            f.run(GridAction::Undo).unwrap();
        }
        assert_eq!(f.times()[0], 1, "one edit in, not back at 500");
        let err = f.run(GridAction::Undo).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert!(!f.editor.state_for(&Fixture::track(), &grid()).can_undo);

        // Everything undone is still redoable: the cap is on the undo stack
        // after a push, and a redo puts grids back without being capped.
        assert_eq!(f.editor.histories.lock()[&Fixture::track()].redo.len(), HISTORY_CAP);
        for _ in 0..HISTORY_CAP {
            f.run(GridAction::Redo).unwrap();
        }
        assert_eq!(f.times()[0], cap + 1);
        assert_eq!(f.editor.histories.lock()[&Fixture::track()].undo.len(), HISTORY_CAP);
        assert!(f.bpms.iter().all(|b| *b == 12000));
    }

    #[test]
    fn a_track_without_analysis_has_no_grid_to_edit() {
        let mut f = open();
        let mut none = |_bpm: u32| Ok(());
        let err = apply(&f.editor, &f.library, &f.location, &track_id(2), GridAction::Edit { edit: GridEdit::Double, from_ms: None }, &mut none).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        let err = state_of(&f.editor, &f.library, &f.location.share_root, "no-such-track").unwrap_err();
        assert_eq!(err.kind, ErrorKind::NotFound);
        assert!(f.run(GridAction::Redo).is_err());
    }

    // A Devices tree track (#660): its grid is edited on the stick, in a
    // tempdir standing for one; no library is opened.
    mod device {
        use super::*;
        use rbl_export::device_library::{self as stick, Format};

        struct Stick {
            _src: tempfile::TempDir,
            dest: tempfile::TempDir,
            editor: GridEditor,
            _app: tempfile::TempDir,
        }

        impl Stick {
            fn assets(&self, format: Format) -> crate::device_library::DeviceAssets {
                let track = stick::read(self.dest.path(), format).unwrap().track(1).unwrap().clone();
                crate::device_library::DeviceAssets {
                    mount: self.dest.path().to_path_buf(),
                    audio: track.path,
                    analysis: track.analysis_path,
                    artwork: String::new(),
                    library: Some((format, 1)),
                }
            }

            fn run(&self, format: Format, action: GridAction) -> AppResult<GridOutcome> {
                apply_device(&self.editor, &self.assets(format), "file:track-1", action, &GridOptions::default(), false)
            }

            fn tempo(&self, format: Format) -> u32 {
                stick::read(self.dest.path(), format).unwrap().track(1).unwrap().bpm_x100
            }

            fn beats(&self, format: Format) -> Vec<Beat> {
                let dat = self.assets(format).analysis_dat().unwrap();
                rbl_anlz::Anlz::read(&dat).unwrap().beat_grid().unwrap()
            }
        }

        fn exported() -> Stick {
            let (src, dest, app) = (tempfile::tempdir().unwrap(), crate::test_stick::stick(), tempfile::tempdir().unwrap());
            let audio = src.path().join("source-1.mp3");
            std::fs::write(&audio, vec![1_u8; 2048]).unwrap();
            let track = rbl_export::SourceTrack {
                id: 1,
                source_path: audio,
                title: "Track 1".into(),
                bpm_x100: 12_000,
                duration_sec: 300,
                analysis: vec![("DAT".into(), AnlzBuilder::new().path("/x.mp3").beat_grid(&grid()).finish())],
                ..Default::default()
            };
            rbl_export::export(dest.path(), &[track], &[]).unwrap();
            Stick { _src: src, dest, editor: GridEditor::at(&app.path().join("app")), _app: app }
        }

        #[test]
        fn a_devices_tree_track_has_a_grid_to_edit_and_no_lock() {
            let s = exported();
            let state = device_state(&s.editor, &s.assets(Format::DeviceLibrary), "file:track-1").unwrap();
            assert_eq!((state.bpm_x100, state.beats, state.locked, state.lockable), (12_000, 8, false, false));
        }

        #[test]
        fn a_shift_moves_the_grid_on_the_stick_and_leaves_its_libraries() {
            for format in [Format::DeviceLibrary, Format::OneLibrary] {
                let s = exported();
                let libraries = |s: &Stick| [Format::DeviceLibrary, Format::OneLibrary].map(|f| std::fs::read(s.dest.path().join("PIONEER/rekordbox").join(f.file_name())).unwrap());
                let before = libraries(&s);
                let first = s.beats(format)[0].time_ms;
                let done = s.run(format, GridAction::Edit { edit: GridEdit::Nudge { ms: 5 }, from_ms: None }).unwrap();
                assert!(done.written && !done.bpm_changed);
                // The beat at `first` is 5 ms later; the grid may reach back
                // to the start of the track with one more beat.
                let after: Vec<u32> = s.beats(format).iter().map(|b| b.time_ms).collect();
                assert!(after.contains(&(first + 5)) && !after.contains(&first), "{format:?}: {after:?}");
                assert_eq!(libraries(&s), before, "{format:?}: a shift changes no row");
                assert!(done.state.can_undo);
                // The first rewrite kept the file as it was, in the app's backups.
                let backups = s.editor.backup_dir.join("devices");
                assert!(backups.is_dir(), "{format:?}");
            }
        }

        #[test]
        fn a_new_tempo_reaches_the_rows_and_undo_puts_everything_back() {
            for format in [Format::DeviceLibrary, Format::OneLibrary] {
                let s = exported();
                let original = s.beats(format);
                let done = s.run(format, GridAction::Edit { edit: GridEdit::Double, from_ms: None }).unwrap();
                assert!(done.bpm_changed);
                assert_eq!(done.state.bpm_x100, 24_000);
                assert_eq!((s.tempo(Format::DeviceLibrary), s.tempo(Format::OneLibrary)), (24_000, 24_000), "{format:?}");

                let undone = s.run(format, GridAction::Undo).unwrap();
                assert_eq!(undone.state.bpm_x100, 12_000);
                assert_eq!(s.beats(format), original, "{format:?}");
                assert_eq!((s.tempo(Format::DeviceLibrary), s.tempo(Format::OneLibrary)), (12_000, 12_000), "{format:?}");
                let check = rbl_export::verify(s.dest.path()).unwrap();
                assert!(check.is_ok(), "{format:?}: {:?}", check.errors);
            }
        }

        /// A stick mounted as `NO NAME` under `parent`, holding one track
        /// whose grid starts at `first_ms`.
        fn exported_named(parent: &Path, src: &Path, first_ms: u32) -> PathBuf {
            let mount = parent.join("NO NAME");
            let audio = src.join("source-1.mp3");
            std::fs::write(&audio, vec![1_u8; 2048]).unwrap();
            let beats: Vec<Beat> = grid().into_iter().map(|b| Beat { time_ms: b.time_ms - 500 + first_ms, ..b }).collect();
            let track = rbl_export::SourceTrack {
                id: 1,
                source_path: audio,
                title: "Track 1".into(),
                bpm_x100: 12_000,
                duration_sec: 300,
                analysis: vec![("DAT".into(), AnlzBuilder::new().path("/x.mp3").beat_grid(&beats).finish())],
                ..Default::default()
            };
            rbl_export::export(&mount, &[track], &[]).unwrap();
            mount
        }

        fn assets_at(mount: &Path) -> crate::device_library::DeviceAssets {
            let track = stick::read(mount, Format::DeviceLibrary).unwrap().track(1).unwrap().clone();
            crate::device_library::DeviceAssets {
                mount: mount.to_path_buf(),
                audio: track.path,
                analysis: track.analysis_path,
                artwork: String::new(),
                library: Some((Format::DeviceLibrary, 1)),
            }
        }

        fn nudge(editor: &GridEditor, mount: &Path) {
            let done = apply_device(editor, &assets_at(mount), "file:track-1",
                GridAction::Edit { edit: GridEdit::Nudge { ms: 5 }, from_ms: None }, &GridOptions::default(), false).unwrap();
            assert!(done.written);
        }

        /// Every backup's bytes, under the editor's device backups.
        fn backed_up(editor: &GridEditor) -> Vec<Vec<u8>> {
            fn walk(dir: &Path, out: &mut Vec<Vec<u8>>) {
                for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                    let path = entry.path();
                    if path.is_dir() { walk(&path, out) } else { out.push(std::fs::read(&path).unwrap()) }
                }
            }
            let mut out = Vec::new();
            walk(&editor.backup_dir.join("devices"), &mut out);
            out
        }

        fn dat_of(mount: &Path) -> Vec<u8> {
            std::fs::read(assets_at(mount).analysis_dat().unwrap()).unwrap()
        }

        #[test]
        fn two_sticks_with_one_volume_name_each_keep_their_original() {
            // Two sticks both named `NO NAME`, each with the same track at
            // the same analysis path but a different grid, edited in turn by
            // two sessions sharing one backup directory.
            let (src, app) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            let (one, two) = (crate::test_stick::stick(), crate::test_stick::stick());
            let (a, b) = (exported_named(one.path(), src.path(), 500), exported_named(two.path(), src.path(), 520));
            assert_eq!(assets_at(&a).analysis, assets_at(&b).analysis, "the same analysis path on each stick");
            let (original_a, original_b) = (dat_of(&a), dat_of(&b));
            assert_ne!(original_a, original_b);

            let first = GridEditor::at(&app.path().join("app"));
            nudge(&first, &a);
            nudge(&first, &a);
            let second = GridEditor::at(&app.path().join("app"));
            nudge(&second, &b);

            let copies = backed_up(&second);
            assert!(copies.contains(&original_a), "stick one's original was backed up");
            assert!(copies.contains(&original_b), "stick two's original was backed up");
            // The second nudge on stick one rewrote this session's own bytes,
            // which were not copied again.
            assert_eq!(copies.len(), 2, "one copy per stick");
        }

        #[test]
        fn a_stick_swapped_in_at_the_same_mount_keeps_its_original() {
            // One session edits a stick, which is ejected; another stick with
            // the same name is mounted at the same place and edited too.
            let (src, app, parent) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), crate::test_stick::stick());
            let editor = GridEditor::at(&app.path().join("app"));
            let mount = exported_named(parent.path(), src.path(), 500);
            let original_a = dat_of(&mount);
            nudge(&editor, &mount);

            std::fs::remove_dir_all(&mount).unwrap();
            let again = exported_named(parent.path(), src.path(), 520);
            assert_eq!(again, mount);
            let original_b = dat_of(&mount);
            nudge(&editor, &mount);

            let copies = backed_up(&editor);
            assert!(copies.contains(&original_a) && copies.contains(&original_b));
        }

        #[test]
        fn a_loaded_device_source_keeps_its_analysis_and_undo_when_the_audio_is_shared() {
            let s = exported();
            let source = |format: &str| crate::dto::DeviceTrackSourceDto { path: s.dest.path().to_string_lossy().into_owned(), format: format.into(), track_id: 1 };
            let original_assets = s.assets(Format::DeviceLibrary);
            let track = rbl_index::folder::loose_id(&original_assets.audio_file().unwrap());
            let original_dat = original_assets.analysis_dat().unwrap();
            let alternate = s.dest.path().join("PIONEER/USBANLZ/alternate/ANLZ0000.DAT");
            std::fs::create_dir_all(alternate.parent().unwrap()).unwrap();
            let shifted: Vec<Beat> = grid().into_iter().map(|b| Beat { time_ms: b.time_ms + 100, ..b }).collect();
            std::fs::write(&alternate, AnlzBuilder::new().path("/x.mp3").beat_grid(&shifted).finish()).unwrap();
            let conn = rbl_onelibrary::playlists::open_read_write(&s.dest.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
            conn.execute("UPDATE content SET analysisDataFilePath='/PIONEER/USBANLZ/alternate/ANLZ0000.DAT' WHERE content_id=1", []).unwrap();
            drop(conn);
            let legacy = crate::device_library::grid_assets(&source("deviceLibrary"), &track).unwrap();
            let one = crate::device_library::grid_assets(&source("oneLibrary"), &track).unwrap();
            assert_eq!(legacy.analysis_dat().unwrap(), original_dat);
            assert_eq!(one.analysis_dat().unwrap(), alternate);
            let other_before = std::fs::read(&alternate).unwrap();
            apply_device(&s.editor, &legacy, &track, GridAction::Edit { edit: GridEdit::Nudge { ms: 5 }, from_ms: None }, &GridOptions::default(), false).unwrap();
            assert_eq!(std::fs::read(&alternate).unwrap(), other_before);
            assert!(device_state(&s.editor, &legacy, &track).unwrap().can_undo);
            assert!(!device_state(&s.editor, &one, &track).unwrap().can_undo);
            assert!(apply_device(&s.editor, &one, &track, GridAction::Undo, &GridOptions::default(), false).is_err());
            assert_eq!(std::fs::read(&alternate).unwrap(), other_before);
            let forged = crate::dto::DeviceTrackSourceDto { track_id: 99, ..source("deviceLibrary") };
            assert!(crate::device_library::grid_assets(&forged, &track).is_err());
            assert!(crate::device_library::grid_assets(&source("deviceLibrary"), "file:other-audio").is_err());
        }

        #[cfg(unix)]
        #[test]
        fn a_write_plan_refuses_a_dangling_lock_link_without_creating_an_outside_file() {
            let s = exported();
            let assets = s.assets(Format::DeviceLibrary);
            let dat = assets.analysis_dat().unwrap();
            let original = std::fs::read(&dat).unwrap();
            let databases = [Format::DeviceLibrary, Format::OneLibrary].map(|format| std::fs::read(s.dest.path().join("PIONEER/rekordbox").join(format.file_name())).unwrap());
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("must-not-be-created.lock");
            let lock = s.dest.path().join(".rbxport-write.lock");
            std::fs::remove_file(&lock).unwrap();
            std::os::unix::fs::symlink(&target, &lock).unwrap();
            let result = apply_device(&s.editor, &assets, "file:track-1", GridAction::Edit { edit: GridEdit::Nudge { ms: 5 }, from_ms: None }, &GridOptions::default(), false);
            assert!(!target.exists(), "a lock link must not create a file outside the device");
            assert_eq!(backed_up(&s.editor), Vec::<Vec<u8>>::new());
            assert!(result.is_err());
            assert_eq!(std::fs::read(dat).unwrap(), original);
            assert_eq!([Format::DeviceLibrary, Format::OneLibrary].map(|format| std::fs::read(s.dest.path().join("PIONEER/rekordbox").join(format.file_name())).unwrap()), databases);
            assert!(s.editor.histories.lock().is_empty());
        }

        #[test]
        fn duplicate_browsed_ids_refuse_before_analysis_backup_or_history() {
            let (src, dest, app) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            let tracks: Vec<_> = (1..=2_u64).map(|id| {
                let audio = src.path().join(format!("{id}.mp3"));
                std::fs::write(&audio, [u8::try_from(id).unwrap(); 128]).unwrap();
                rbl_export::SourceTrack { id, source_path: audio, bpm_x100: 12_000, duration_sec: 20,
                    analysis: vec![("DAT".into(), AnlzBuilder::new().path("/audio.mp3").beat_grid(&grid()).finish())], ..Default::default() }
            }).collect();
            rbl_export::export(dest.path(), &tracks, &[]).unwrap();
            let path = dest.path().join("PIONEER/rekordbox/export.pdb");
            let mut bytes = std::fs::read(&path).unwrap();
            let at = {
                let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
                pdb.rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap()).into_iter()
                    .find(|row| pdb.u4_at(*row, 0x48) == 2).unwrap().offset + 0x48
            };
            bytes[at..at + 4].copy_from_slice(&1_u32.to_le_bytes());
            std::fs::write(path, bytes).unwrap();
            let library = stick::read(dest.path(), Format::DeviceLibrary).unwrap();
            let selected = library.track(1).unwrap();
            let assets = crate::device_library::DeviceAssets { mount: dest.path().to_path_buf(), audio: selected.path.clone(), analysis: selected.analysis_path.clone(), artwork: String::new(), library: Some((Format::DeviceLibrary, 1)) };
            let data: Vec<_> = library.tracks.iter().map(|row| {
                let path = dest.path().join(row.analysis_path.trim_start_matches('/'));
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            }).collect();
            let databases = [Format::DeviceLibrary, Format::OneLibrary].map(|format| std::fs::read(dest.path().join("PIONEER/rekordbox").join(format.file_name())).unwrap());
            let editor = GridEditor::at(app.path());
            let result = apply_device(&editor, &assets, "file:loaded", GridAction::Edit { edit: GridEdit::Double, from_ms: None }, &GridOptions::default(), false);
            assert!(backed_up(&editor).is_empty(), "ambiguous source must refuse before backing up analysis");
            assert!(result.is_err());
            for (path, bytes) in data { assert_eq!(std::fs::read(path).unwrap(), bytes); }
            assert_eq!([Format::DeviceLibrary, Format::OneLibrary].map(|format| std::fs::read(dest.path().join("PIONEER/rekordbox").join(format.file_name())).unwrap()), databases);
            assert!(editor.histories.lock().is_empty());
            assert!(editor.device_written.lock().is_empty());
        }

        #[cfg(unix)]
        #[test]
        fn a_linked_ext_is_refused_before_reading_or_backing_up_any_analysis() {
            for valid_analysis in [false, true] {
                let s = exported();
                let assets = s.assets(Format::DeviceLibrary);
                let dat = assets.analysis_dat().unwrap();
                let ext = rbl_anlz::sibling(&dat, "EXT");
                let before = std::fs::read(&dat).unwrap();
                let databases = [Format::DeviceLibrary, Format::OneLibrary].map(|format| std::fs::read(s.dest.path().join("PIONEER/rekordbox").join(format.file_name())).unwrap());
                let outside_root = tempfile::tempdir().unwrap();
                let outside = outside_root.path().join("outside-private.EXT");
                // A valid file with no PQT2 needs no EXT rewrite, so checking
                // only the files sent to the publication is too late.
                let private = if valid_analysis {
                    AnlzBuilder::new().path("/outside-private-sentinel.mp3").finish()
                } else { b"outside private sentinel, not analysis".to_vec() };
                std::fs::write(&outside, &private).unwrap();
                if std::fs::symlink_metadata(&ext).is_ok() { std::fs::remove_file(&ext).unwrap(); }
                std::os::unix::fs::symlink(&outside, &ext).unwrap();
                let result = apply_device(&s.editor, &assets, "file:track-1",
                    GridAction::Edit { edit: GridEdit::Nudge { ms: 5 }, from_ms: None }, &GridOptions::default(), false);
                let copies = backed_up(&s.editor);
                assert!(!copies.contains(&private), "outside bytes must never be copied to app backups");
                assert!(copies.is_empty(), "reject companions before backing up even the valid DAT");
                assert!(result.is_err());
                assert_eq!(std::fs::read(&outside).unwrap(), private);
                assert_eq!(std::fs::read(&dat).unwrap(), before);
                assert_eq!([Format::DeviceLibrary, Format::OneLibrary].map(|format| std::fs::read(s.dest.path().join("PIONEER/rekordbox").join(format.file_name())).unwrap()), databases);
                assert!(std::fs::symlink_metadata(&ext).unwrap().file_type().is_symlink());
                assert!(s.editor.histories.lock().is_empty());
                assert!(s.editor.device_written.lock().is_empty());
            }
        }

        #[test]
        fn a_stick_edit_is_refused_while_rekordbox_runs() {
            let s = exported();
            let before = std::fs::read(s.assets(Format::DeviceLibrary).analysis_dat().unwrap()).unwrap();
            let refused = apply_device(&s.editor, &s.assets(Format::DeviceLibrary), "file:track-1",
                GridAction::Edit { edit: GridEdit::Nudge { ms: 5 }, from_ms: None }, &GridOptions::default(), true).unwrap_err();
            assert_eq!(refused.kind, ErrorKind::ReadOnly);
            assert_eq!(std::fs::read(s.assets(Format::DeviceLibrary).analysis_dat().unwrap()).unwrap(), before);
        }

        #[test]
        fn a_track_without_its_library_is_not_edited() {
            let s = exported();
            let assets = crate::device_library::DeviceAssets { library: None, ..s.assets(Format::DeviceLibrary) };
            let err = apply_device(&s.editor, &assets, "file:track-1", GridAction::Edit { edit: GridEdit::Double, from_ms: None }, &GridOptions::default(), false).unwrap_err();
            assert_eq!(err.kind, ErrorKind::NotFound);
        }
    }
}
