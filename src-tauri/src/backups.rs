//! Explicit library snapshots. No work is done here on ordinary edits.
use crate::{
    dto::BackupDto,
    error::{AppError, AppResult},
    state::AppState,
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProgress {
    pub running: bool,
    pub phase: String,
    pub copied_bytes: u64,
    pub total_bytes: u64,
    pub error: Option<String>,
    pub path: Option<String>,
    pub current_item: Option<String>,
}

/// Reserve the job before spawning, so requests from two windows cannot queue duplicates.
pub fn start(state: std::sync::Arc<AppState>) -> AppResult<()> {
    {
        let mut progress = state.backup_progress.lock();
        if progress.running {
            return Err(error("A backup is already running."));
        }
        *progress = BackupProgress { running: true, phase: "preparing".into(), ..Default::default() };
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut last_detail = std::time::Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            create_with_progress(&state, &mut |phase, copied_bytes, total_bytes, item| {
                let mut progress = state.backup_progress.lock();
                if progress.phase == "stopping" {
                    return Err(AppError::new(crate::error::ErrorKind::Cancelled, "Backup stopped."));
                }
                if progress.phase != phase || progress.current_item.is_none() || last_detail.elapsed() >= std::time::Duration::from_secs(1) {
                    progress.current_item = Some(item.to_owned());
                    last_detail = std::time::Instant::now();
                }
                progress.phase = phase.into();
                progress.copied_bytes = copied_bytes;
                progress.total_bytes = total_bytes;
                Ok(())
            })
        })).unwrap_or_else(|_| Err(error("Backup worker stopped unexpectedly.")));
        let mut progress = state.backup_progress.lock();
        progress.running = false;
        progress.current_item = None;
        match result {
            Ok(path) => { progress.phase = "complete".into(); progress.path = Some(path); }
            Err(e) if e.kind == crate::error::ErrorKind::Cancelled => { progress.phase = "cancelled".into(); }
            Err(e) => { progress.phase = "failed".into(); progress.error = Some(failure_message(&e)); }
        }
    });
    Ok(())
}

/// The background job reports through `BackupProgress.error`, not a rejected
/// command, so carry the detail that `errorMessage` would otherwise append.
fn failure_message(error: &AppError) -> String {
    match error.detail.as_deref().map(str::trim) {
        Some(detail) if !detail.is_empty() && detail != error.message => format!("{} {detail}", error.message),
        _ => error.message.clone(),
    }
}

pub fn cancel(state: &AppState) {
    let mut progress = state.backup_progress.lock();
    if progress.running { progress.phase = "stopping".into(); }
}

fn tree_size(path: &Path, check: &mut dyn FnMut() -> AppResult<()>) -> AppResult<u64> {
    check()?;
    let meta = fs::symlink_metadata(path).map_err(error)?;
    if meta.file_type().is_symlink() { return Err(error("Symbolic links are not supported in backups.")); }
    if meta.is_file() { return Ok(meta.len()); }
    if !meta.is_dir() { return Err(error("Unsupported file in backup.")); }
    let mut bytes = 0;
    for entry in fs::read_dir(path).map_err(error)? {
        bytes += tree_size(&entry.map_err(error)?.path(), check)?;
    }
    Ok(bytes)
}

fn error(e: impl std::fmt::Display) -> AppError {
    AppError::internal(format!("Backup: {e}"))
}
fn millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn analysis(location: &rbl_db::LibraryLocation) -> PathBuf {
    rbl_backup::analysis_dir(location)
}
fn artwork(location: &rbl_db::LibraryLocation) -> PathBuf {
    rbl_backup::artwork_dir(location)
}
// Library selections live beside master.db, outside the SQL database.
pub use rbl_backup::LIBRARY_FILES;
use rbl_backup::manifest::Manifest;

fn remove(path: &Path) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            fs::remove_dir_all(path).map_err(error)
        }
        Ok(_) => fs::remove_file(path).map_err(error),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(error(e)),
    }
}
/// Prefix of an unfinished backup that was set aside for deletion. Like
/// `.partial-`, it is never listed, restored, or offered for deletion.
const DISCARD_PREFIX: &str = ".discard-";

/// Paths a background thread is deleting, so a sweep never starts a second
/// deletion of the same tree.
static DISCARDING: parking_lot::Mutex<std::collections::BTreeSet<PathBuf>> =
    parking_lot::const_mutex(std::collections::BTreeSet::new());

/// Held for writing by tests that need to see a tree before its deletion.
#[cfg(test)]
static HOLD_DISCARDS: parking_lot::RwLock<()> = parking_lot::const_rwlock(());

/// Renames each existing path to a fresh `.discard-` name in `root`. A rename
/// within one folder is a single directory operation however many files the
/// tree holds. A path that cannot be renamed is returned as it is, to be
/// deleted where it lies.
fn set_aside(root: &Path, paths: &[&Path]) -> Vec<PathBuf> {
    let mut doomed = Vec::new();
    for path in paths {
        if fs::symlink_metadata(path).is_err() {
            continue;
        }
        let aside = root.join(format!("{DISCARD_PREFIX}{}", uuid::Uuid::new_v4()));
        doomed.push(if fs::rename(path, &aside).is_ok() { aside } else { path.to_path_buf() });
    }
    doomed
}

/// Deletes `paths` on a background thread. Errors are only logged: whatever
/// is left keeps its hidden name and the next sweep tries again.
fn delete_in_background(paths: Vec<PathBuf>) {
    let paths: Vec<_> = {
        let mut busy = DISCARDING.lock();
        paths.into_iter().filter(|path| busy.insert(path.clone())).collect()
    };
    if paths.is_empty() {
        return;
    }
    let delete = |paths: &[PathBuf]| {
        #[cfg(test)]
        let _held = HOLD_DISCARDS.read();
        for path in paths {
            if let Err(e) = remove(path) {
                tracing::warn!(path = %path.display(), error = %e, "could not delete an unfinished backup");
            }
        }
        let mut busy = DISCARDING.lock();
        for path in paths {
            busy.remove(path);
        }
    };
    let owned = paths.clone();
    if std::thread::Builder::new()
        .name("backup-discard".into())
        .spawn(move || delete(&owned))
        .is_err()
    {
        delete(&paths);
    }
}

/// Stopping a backup used to delete its staging folder before the job could
/// report "cancelled". A large library stages one file per analysis file, and
/// deleting 150,000 of them took minutes on Windows with the edit gate held
/// (#472). Moving the unfinished parts aside is immediate; the files are then
/// deleted without holding anything.
fn discard(root: &Path, paths: &[&Path]) {
    delete_in_background(set_aside(root, paths));
}

/// Deletes unfinished backups set aside by an earlier run, for example one
/// that quit before its background deletion finished.
pub fn sweep_discarded(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return; };
    let doomed = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(DISCARD_PREFIX))
        .map(|entry| entry.path())
        .collect();
    delete_in_background(doomed);
}

fn checked(root: &Path, path: &Path) -> AppResult<PathBuf> {
    let root = root.canonicalize().map_err(error)?;
    let meta = fs::symlink_metadata(path).map_err(error)?;
    if meta.file_type().is_symlink() {
        return Err(error("Not a managed backup."));
    }
    let path = path.canonicalize().map_err(error)?;
    if path.parent() != Some(root.as_path()) {
        return Err(error("Not a managed backup."));
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !(name.starts_with("rbexport-") && meta.is_file() && is_zip(&path)
        || name.starts_with("rbxport-backup-") && meta.is_file() && is_zip(&path)
        || name.starts_with("library-") && (meta.is_dir() || (meta.is_file() && path.extension().is_some_and(|e| e == "zip")))
        || name.starts_with("master-")
            && path.extension().is_some_and(|e| e == "db")
            && meta.is_file())
    {
        return Err(error("Not a managed backup."));
    }
    Ok(path)
}
fn manifest(path: &Path, location: &rbl_db::LibraryLocation) -> AppResult<Manifest> {
    let saved = Manifest::read(path).map_err(error)?;
    if !saved.belongs_to(location) {
        return Err(error("This backup belongs to another library."));
    }
    Ok(saved)
}

fn is_zip(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

/// The Backups preference shows only snapshots created under the current
/// naming scheme. RBXport Restore restores older ones too.
fn is_listed_backup(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let Ok(meta) = fs::symlink_metadata(path) else { return false; };
    name.starts_with("rbexport-") && meta.is_file() && !meta.file_type().is_symlink() && is_zip(path)
}

/// Reads the backup folder without the edit gate. A backup holds that gate
/// from start to finish, minutes on a large library, and Library
/// Protection asks this list before it unlocks, so taking the gate here left
/// the switch greyed out for the whole backup (#436). Nothing here needs it:
/// a backup is written under a `.partial-` name and renamed into place
/// complete, and an entry that disappears while it is read is skipped.
pub fn list(state: &AppState) -> AppResult<Vec<BackupDto>> {
    let location = state.location()?;
    let mut result = Vec::new();
    let entries = match fs::read_dir(state.backup_destination()) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(e) => return Err(error(e)),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !is_listed_backup(&path) {
            continue;
        }
        let Ok(path) = checked(&state.backup_destination(), &path) else {
            continue;
        };
        let Ok(saved) = manifest(&path, &location) else {
            continue;
        };
        let created_at = saved.created_at;
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let bytes = meta.len();
        let includes_analysis = true;
        let includes_artwork = saved.includes_artwork;
        result.push(BackupDto {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: path.to_string_lossy().into_owned(),
            bytes,
            created_at,
            includes_analysis,
            includes_artwork,
        });
    }
    result.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    Ok(result)
}

pub fn create(state: &AppState) -> AppResult<String> {
    create_with_progress(state, &mut |_, _, _, _| Ok(()))
}

pub fn validate_destination(directory: &Path, location: &rbl_db::LibraryLocation) -> AppResult<()> {
    for source in [analysis(location), artwork(location)] {
        if source.canonicalize().is_ok_and(|source| directory.starts_with(source)) {
            return Err(error("Choose a backup folder outside the library's analysis and artwork folders."));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines, reason = "one linear backup pipeline; splitting it would hide the order writes happen in")]
fn create_with_progress(state: &AppState, progress: &mut dyn FnMut(&str, u64, u64, &str) -> AppResult<()>) -> AppResult<String> {
    let _gate = state.edit_gate.lock();
    let _files = state.analysis_write.lock();
    let location = state.location()?;
    if pending(state.backup_dir()) {
        state.with_closed_reader(|| recover(state.backup_dir(), &location))?;
    }
    crate::file_journal::recover(state.backup_dir(), &location)?;
    let tree = analysis(&location);
    let art = artwork(&location);
    let wal = rbl_backup::sidecar(&location.master_db, "-wal");
    let destination = state.backup_destination();
    let root = destination.as_path();
    crate::durable::create_dir_all(root).map_err(error)?;
    sweep_discarded(root);
    validate_destination(&root.canonicalize().map_err(error)?, &location)?;
    let created_at = millis();
    let id = uuid::Uuid::new_v4();
    let partial = root.join(format!(".partial-{id}"));
    let target = root.join(format!("rbexport-{}.zip", rbl_core::time::local_backup_stamp()));
    if target.try_exists().map_err(error)? {
        return Err(error("A backup for this minute already exists. Try again in the next minute."));
    }
    let archive = root.join(format!(".partial-{id}.zip"));
    let result = (|| {
        fs::create_dir(&partial).map_err(error)?;
        let mut refused = None;
        let plan = if tree.exists() {
            let prepared = crate::backup_copy::TreeCopyPlan::prepare(&tree, &partial.join("analysis"), &mut |_| {
                progress("preparing", 0, 0, "Scanning analysis files").map_err(|e| {
                    refused = Some(e);
                    std::io::Error::new(std::io::ErrorKind::Interrupted, "Backup stopped")
                })
            });
            if let Some(error) = refused { return Err(error); }
            Some(prepared.map_err(error)?)
        } else { None };
        let art_plan = if art.exists() {
            let prepared = crate::backup_copy::TreeCopyPlan::prepare(&art, &partial.join("artwork"), &mut |_| {
                progress("preparing", 0, 0, "Scanning artwork thumbnails").map_err(|e| {
                    refused = Some(e);
                    std::io::Error::new(std::io::ErrorKind::Interrupted, "Backup stopped")
                })
            });
            if let Some(error) = refused { return Err(error); }
            Some(prepared.map_err(error)?)
        } else { None };
        let mut check = || progress("preparing", 0, 0, "Measuring database files");
        let library_root = location.master_db.parent().ok_or_else(|| error("Invalid database path"))?;
        let library_files: Vec<_> = LIBRARY_FILES.into_iter().filter(|name| library_root.join(name).exists()).collect();
        let mut library_bytes = 0;
        for name in &library_files {
            library_bytes += tree_size(&library_root.join(name), &mut check)?;
        }
        let total = tree_size(&location.master_db, &mut check)?
            + library_bytes
            + if wal.exists() { tree_size(&wal, &mut check)? } else { 0 }
            + plan.as_ref().map_or(0, |plan| plan.bytes)
            + art_plan.as_ref().map_or(0, |plan| plan.bytes);
        progress("copying", 0, total, "Database · master.db")?;
        let mut copied = 0;
        let mut copied_file = |bytes, source: Option<&Path>| {
            copied += bytes;
            let item = match source {
                Some(path) if path.starts_with(&art) => format!("Artwork thumbnails · Artwork/{}", path.strip_prefix(&art).unwrap_or(path).to_string_lossy().replace('\\', "/")),
                Some(path) if path.starts_with(&tree) => format!("Analysis files · USBANLZ/{}", path.strip_prefix(&tree).unwrap_or(path).to_string_lossy().replace('\\', "/")),
                Some(path) => format!("Database · {}", path.file_name().unwrap_or_default().to_string_lossy()),
                None => "Analysis files".to_owned(),
            };
            progress("copying", copied, total, &item)
        };
        let snapshot = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).map_err(error)?;
        snapshot.connection().execute("VACUUM main INTO ?1", [partial.join("master.db").to_string_lossy().as_ref()]).map_err(error)?;
        let counts = rbl_backup::summary::count(snapshot.connection()).map_err(error)?;
        drop(snapshot);
        let mut bytes = fs::metadata(partial.join("master.db")).map_err(error)?.len();
        let database_bytes = bytes + library_bytes;
        // FlushFileBuffers needs GENERIC_WRITE on Windows; a read-only handle
        // fails with "Access is denied" (os error 5) on every backup.
        fs::OpenOptions::new().write(true).open(partial.join("master.db")).map_err(error)?.sync_all().map_err(error)?;
        copied_file(bytes, Some(&location.master_db))?;
        for (plan, directory) in [(plan, "analysis"), (art_plan, "artwork")] {
        if let Some(plan) = plan {
            let mut refused = None;
            let copied = plan.compress(&mut |bytes, source| {
                copied_file(bytes, source).map_err(|e| {
                    refused = Some(e);
                    std::io::Error::new(std::io::ErrorKind::Interrupted, "Backup stopped")
                })
            });
            if let Some(error) = refused { return Err(error); }
            bytes += copied.map_err(error)?;
        } else {
            fs::create_dir(partial.join(directory)).map_err(error)?;
        }
        }
        progress("validating", bytes, total, "Checking database · master.db")?;
        validate_database(&partial.join("master.db"), &location)?;
        remove(&partial.join("master.db-shm"))?;
        for name in &library_files {
            let source = library_root.join(name);
            let target = partial.join(name);
            let mut refused = None;
            let compressed = crate::backup_zip::compress_file(&source, &target, &mut |delta| {
                bytes += delta;
                progress("copying", bytes, total, &format!("Library settings · {name}")).map_err(|e| {
                    refused = Some(e);
                    std::io::Error::new(std::io::ErrorKind::Interrupted, "Backup stopped")
                })
            });
            if let Some(error) = refused { return Err(error); }
            compressed.map_err(error)?;
        }
        for name in ["master.db", "master.db-wal"] {
            let source = partial.join(name);
            if !source.exists() { continue; }
            let mut refused = None;
            let compressed = crate::backup_zip::compress_file(&source, &source, &mut |_| {
                progress("compressing", bytes, total, &format!("Compressing database · {name}")).map_err(|e| {
                    refused = Some(e);
                    std::io::Error::new(std::io::ErrorKind::Interrupted, "Backup stopped")
                })
            });
            if let Some(error) = refused { return Err(error); }
            compressed.map_err(error)?;
            remove(&source)?;
        }
        // What the backup holds, for RBXport Restore to show before anyone
        // restores it. The trees are as they were copied: the edit gate and
        // the analysis lock are still held.
        let mut measured = rbl_backup::sizes::Measurement::default();
        for (source, is_artwork) in [(&tree, false), (&art, true)] {
            let mut refused = None;
            let result = rbl_backup::sizes::measure_tree(source, is_artwork, &mut measured, &mut || {
                progress("validating", bytes, total, "Summarizing backup contents").map_err(|e| {
                    refused = Some(e);
                    std::io::Error::new(std::io::ErrorKind::Interrupted, "Backup stopped")
                })
            });
            if let Some(error) = refused { return Err(error); }
            result.map_err(error)?;
        }
        measured.sizes.database = database_bytes;
        let summary = rbl_backup::summary::Summary::new(created_at, Some(env!("CARGO_PKG_VERSION").to_owned()), measured, counts);
        crate::durable::write(
            &partial.join(rbl_backup::summary::NAME),
            &serde_json::to_vec(&summary).map_err(error)?,
        )
        .map_err(error)?;
        let saved = Manifest {
            version: rbl_backup::manifest::VERSION,
            library: location.master_db.clone(),
            created_at,
            bytes,
            includes_artwork: true,
            library_files: library_files.into_iter().map(str::to_owned).collect(),
        };
        crate::durable::write(
            &partial.join(rbl_backup::manifest::NAME),
            &serde_json::to_vec(&saved).map_err(error)?,
        )
        .map_err(error)?;
        crate::durable::write(
            &partial.join(crate::backup_restore_scripts::SHELL_NAME),
            crate::backup_restore_scripts::shell(&location.master_db).as_bytes(),
        )
        .map_err(error)?;
        crate::durable::write(
            &partial.join(crate::backup_restore_scripts::POWERSHELL_NAME),
            crate::backup_restore_scripts::powershell(&location.master_db).as_bytes(),
        )
        .map_err(error)?;
        progress("validating", bytes, total, "Finishing backup · manifest.json")?;
        let mut refused = None;
        let packed = crate::backup_zip::assemble(&partial, &archive, &mut || {
            progress("compressing", bytes, total, "Finishing ZIP archive").map_err(|e| {
                refused = Some(e);
                std::io::Error::new(std::io::ErrorKind::Interrupted, "Backup stopped")
            })
        });
        if let Some(error) = refused { return Err(error); }
        packed.map_err(error)?;
        // The archive holds everything now; the staged copies go in the background.
        discard(root, &[&partial]);
        // A backup folder on an external exFAT or FAT32 drive has no
        // exclusive rename on macOS; `persist_new` still never replaces one.
        rbl_core::durable::persist_new(tempfile::TempPath::try_from_path(&archive).map_err(error)?, &target).map_err(error)?;
        crate::durable::sync_dir(root).map_err(error)?;
        Ok(target.to_string_lossy().into_owned())
    })();
    if result.is_err() {
        discard(root, &[&partial, &archive]);
    }
    result
}
fn validate_database(path: &Path, location: &rbl_db::LibraryLocation) -> AppResult<()> {
    let mut copy = location.clone();
    copy.master_db = path.to_path_buf();
    copy.is_real_install = false;
    let db = rbl_db::Library::open(copy, rbl_db::OpenMode::ReadOnly).map_err(error)?;
    let check: String = db
        .connection()
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(error)?;
    if check != "ok" {
        return Err(error("The backup database is damaged."));
    }
    Ok(())
}

/// A restore RBXport Restore left unfinished must be rolled back or cleaned
/// up before the library is read; see [`rbl_backup::journal`].
pub fn pending(root: &Path) -> bool {
    rbl_backup::journal::pending(root)
}
/// A pending restore rolls back on restart; a committed one only needs cleanup.
pub fn recover(root: &Path, location: &rbl_db::LibraryLocation) -> AppResult<()> {
    rbl_backup::journal::recover(root, location).map_err(error)
}

pub fn delete(state: &AppState, path: &Path) -> AppResult<()> {
    let _gate = state.edit_gate.lock();
    let path = checked(&state.backup_destination(), path)?;
    if path.is_dir() || path.extension().is_some_and(|e| e == "zip") {
        manifest(&path, &state.location()?)?;
    } else {
        for suffix in ["-wal", "-shm"] {
            remove(&rbl_backup::sidecar(&path, suffix))?;
        }
    }
    remove(&path)?;
    crate::durable::sync_dir(&state.backup_destination()).map_err(error)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    fn fixture() -> (tempfile::TempDir, AppState, rbl_db::LibraryLocation) {
        let dir = tempfile::tempdir().unwrap();
        let location =
            rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location.clone());
        (dir, state, location)
    }

    /// Restores an RBXport archive the way RBXport Restore does, with
    /// RBXport's own recovery directory as the journal's home.
    fn restore(state: &AppState, location: &rbl_db::LibraryLocation, archive: &Path, parts: rbl_backup::restore::Parts) {
        state.drop_reader();
        rbl_backup::restore::restore(
            &rbl_backup::restore::Request { archive, parts, location, state_dir: state.backup_dir(), sizes: None },
            &|| Ok(()),
            &mut |_, _, _, _| Ok(()),
        )
        .unwrap();
    }

    #[test]
    fn editing_is_allowed_without_a_backup_and_after_the_last_is_deleted() {
        let (_dir, state, _location) = fixture();
        let track = rbl_db::fixture::track_id(1);
        state.write(|w| w.set_rating(&track, 4)).unwrap();
        assert_eq!(rating(&state), 4);
        let path = PathBuf::from(create(&state).unwrap());
        delete(&state, &path).unwrap();
        state.write(|w| w.set_rating(&track, 2)).unwrap();
        assert_eq!(rating(&state), 2);
    }

    /// Library Protection asks for this list before it unlocks. A running
    /// backup holds the edit gate until it ends, so the list must not wait
    /// on it, or the switch stays greyed out for the whole backup (#436).
    #[test]
    fn listing_does_not_wait_for_a_running_backup() {
        let (_dir, state, _location) = fixture();
        let saved = PathBuf::from(create(&state).unwrap());
        let listed = std::thread::scope(|scope| {
            let running_backup = state.edit_gate.lock();
            let (send, receive) = std::sync::mpsc::channel();
            let state = &state;
            scope.spawn(move || {
                let _ = send.send(list(state).map(|backups| backups.len()));
            });
            let listed = receive.recv_timeout(std::time::Duration::from_secs(10));
            drop(running_backup);
            listed
        });
        assert!(listed.is_ok(), "the list waited for the edit gate");
        assert_eq!(listed.unwrap().unwrap(), 1);
        assert!(saved.is_file());
    }

    #[test]
    fn backup_is_available_while_the_library_is_read_only() {
        let (dir, _state, location) = fixture();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let db_version = db.schema().db_version;
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("read-only-backups"));
        state.set_library(library, true, db_version, 0, location);

        let backup = PathBuf::from(create(&state).unwrap());
        assert!(backup.is_file());
        assert_eq!(list(&state).unwrap().len(), 1);
    }

    #[test]
    fn filenames_use_local_24_hour_time_and_never_overwrite_the_same_minute() {
        let (_dir, state, _) = fixture();
        let before = rbl_core::time::local_backup_stamp();
        let first = PathBuf::from(create(&state).unwrap());
        let after = rbl_core::time::local_backup_stamp();
        let name = first.file_name().unwrap().to_string_lossy();
        assert!(name == format!("rbexport-{before}.zip") || name == format!("rbexport-{after}.zip"));
        let bytes = fs::read(&first).unwrap();
        // Reserve the current minute, including if the first copy crossed a boundary.
        let reserved = state.backup_destination().join(format!("rbexport-{after}.zip"));
        if reserved != first { fs::copy(&first, &reserved).unwrap(); }
        assert!(create(&state).is_err());
        assert_eq!(fs::read(first).unwrap(), bytes);
        assert_eq!(fs::read(reserved).unwrap(), bytes);
    }

    #[test]
    fn archives_include_standalone_restore_scripts() {
        use std::io::Read;

        let (_dir, state, location) = fixture();
        let path = PathBuf::from(create(&state).unwrap());
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut shell = String::new();
        let shell_mode = {
            let mut entry = archive.by_name(crate::backup_restore_scripts::SHELL_NAME).unwrap();
            entry.read_to_string(&mut shell).unwrap();
            entry.unix_mode()
        };
        assert!(shell.starts_with("#!/usr/bin/env bash"));
        assert!(shell.contains(location.master_db.to_string_lossy().as_ref()));
        assert_eq!(shell_mode.unwrap() & 0o111, 0o111);
        let mut powershell = String::new();
        archive
            .by_name(crate::backup_restore_scripts::POWERSHELL_NAME)
            .unwrap()
            .read_to_string(&mut powershell)
            .unwrap();
        assert!(powershell.starts_with("param([string]$MasterDb"));
        assert!(powershell.contains("Get-Process -Name rekordbox, rekordboxAgent"));
    }

    #[test]
    fn archives_carry_a_summary_of_what_they_hold() {
        let (_dir, state, location) = fixture();
        {
            let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
            for (id, kind) in [("m1", 0), ("m2", 0), ("h1", 1), ("h2", 6)] {
                db.connection().execute(
                    "INSERT INTO djmdCue (ID, ContentID, InMsec, Kind, created_at, updated_at) VALUES (?1, ?2, 0, ?3, '', '')",
                    (id, rbl_db::fixture::track_id(2), kind),
                ).unwrap();
            }
        }
        let grid = analysis(&location).join("P001/0001/ANLZ0000.DAT");
        fs::create_dir_all(grid.parent().unwrap()).unwrap();
        let mut anlz = b"PMAI".to_vec();
        for word in [12u32, 40] { anlz.extend(word.to_be_bytes()); }
        anlz.extend(b"PQTZ");
        for word in [12u32, 28] { anlz.extend(word.to_be_bytes()); }
        anlz.extend([0; 16]);
        fs::write(&grid, &anlz).unwrap();
        let art = artwork(&location).join("abc/artwork_m.jpg");
        fs::create_dir_all(art.parent().unwrap()).unwrap();
        fs::write(&art, [1; 33]).unwrap();
        let path = PathBuf::from(create(&state).unwrap());
        let summary = rbl_backup::summary::Summary::read(&path).unwrap().unwrap();
        let shape = rbl_db::fixture::Shape::default();
        assert_eq!(summary.app_version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(summary.created_at, list(&state).unwrap()[0].created_at);
        assert_eq!(summary.counts.tracks, shape.tracks as u64);
        assert_eq!(summary.counts.playlists, shape.playlists as u64);
        assert_eq!((summary.counts.hot_cues, summary.counts.memory_cues), (2, 2));
        assert_eq!((summary.counts.analysis_files, summary.counts.artwork_files), (1, 1));
        assert_eq!((summary.sizes.beat_grids, summary.sizes.artwork, summary.sizes.other), (28, 33, 12));
        assert_eq!(summary.sizes.track_count, 40);
        let saved = Manifest::read(&path).unwrap();
        // Everything the manifest counted, less the 40-byte analysis file and the artwork.
        assert_eq!(summary.sizes.database, saved.bytes - 40 - 33);
    }

    #[test]
    fn list_shows_only_current_rbexport_archives() {
        let (_dir, state, _location) = fixture();
        let current = PathBuf::from(create(&state).unwrap());
        let root = state.backup_destination();
        fs::copy(&current, root.join("rbxport-backup-legacy.zip")).unwrap();
        fs::copy(&current, root.join("library-legacy.zip")).unwrap();
        fs::write(root.join("rbexport-not-a-backup.zip"), b"not a ZIP").unwrap();
        fs::write(root.join("notes.zip"), b"not a ZIP").unwrap();

        let listed = list(&state).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].path, current.canonicalize().unwrap().to_string_lossy());
        assert!(listed[0].name.starts_with("rbexport-"));
    }

    #[test]
    fn a_moved_and_renamed_archive_restores_and_is_not_deleted_from_its_new_folder() {
        let (dir, state, location) = fixture();
        let track = rbl_db::fixture::track_id(1);
        state.write(|w| w.set_rating(&track, 4)).unwrap();
        let original = PathBuf::from(create(&state).unwrap());
        let bytes = fs::read(&original).unwrap();
        let destination = dir.path().join("external drive");
        fs::create_dir(&destination).unwrap();
        let renamed = destination.join("My saved library.ZIP");
        fs::rename(&original, &renamed).unwrap();
        assert!(list(&state).unwrap().is_empty());
        state.write(|w| w.set_rating(&track, 1)).unwrap();
        restore(&state, &location, &renamed, rbl_backup::restore::Parts::ALL);
        assert_eq!(rating(&state), 4);
        assert_eq!(fs::read(&renamed).unwrap(), bytes);
        assert!(delete(&state, &renamed).is_err());
        assert!(renamed.exists());
    }

    #[test]
    fn another_librarys_backup_is_neither_listed_nor_deleted() {
        let (_dir, state, _location) = fixture();
        let (_other_dir, other, _) = fixture();
        let foreign = PathBuf::from(create(&other).unwrap());
        let copied = state.backup_destination().join(foreign.file_name().unwrap());
        fs::create_dir_all(state.backup_destination()).unwrap();
        fs::copy(&foreign, &copied).unwrap();
        assert!(list(&state).unwrap().is_empty());
        assert!(delete(&state, &copied).is_err());
        assert!(copied.exists());
    }

    #[test]
    fn default_destination_persists_and_only_new_backups_use_it() {
        let (dir, state, location) = fixture();
        let original = PathBuf::from(create(&state).unwrap());
        let destination = dir.path().join("new backup folder");
        fs::create_dir(&destination).unwrap();
        let canonical = destination.canonicalize().unwrap();
        state.set_backup_destination(&destination).unwrap();
        assert_eq!(state.backup_destination(), canonical);
        assert_eq!(rbl_backup::default_destination(state.backup_dir()), canonical, "RBXport Restore reads the same setting");
        assert!(original.exists());
        assert!(list(&state).unwrap().is_empty());
        let created = PathBuf::from(create(&state).unwrap());
        assert_eq!(created.parent(), Some(canonical.as_path()));
        assert_eq!(list(&state).unwrap().len(), 1);
        assert_ne!(state.backup_dir(), canonical);
        let restarted = AppState::with_backups(state.backup_dir());
        assert_eq!(restarted.backup_destination(), canonical);
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        restarted.set_library(library, false, db.schema().db_version, 0, location);
        assert_eq!(list(&restarted).unwrap()[0].path, created.to_string_lossy());
        delete(&restarted, &created).unwrap();
        assert!(original.exists());
    }

    #[test]
    fn invalid_or_busy_destination_changes_keep_the_previous_setting() {
        let (dir, state, location) = fixture();
        let previous = state.backup_destination();
        let file = dir.path().join("not a folder");
        fs::write(&file, b"keep").unwrap();
        assert!(state.set_backup_destination(&file).is_err());
        assert!(state.set_backup_destination(&dir.path().join("missing")).is_err());
        let recursive = analysis(&location).join("backups");
        fs::create_dir_all(&recursive).unwrap();
        assert!(state.set_backup_destination(&recursive).is_err());
        assert_eq!(state.backup_destination(), previous);
        state.backup_progress.lock().running = true;
        assert!(state.set_backup_destination(dir.path()).is_err());
        assert_eq!(state.backup_destination(), previous);
        assert_eq!(AppState::with_backups(state.backup_dir()).backup_destination(), previous);
    }

    #[test]
    fn progress_reports_bytes_and_cancellation_discards_only_the_partial_copy() {
        let (_dir, state, location) = fixture();
        let file = analysis(&location).join("test/ANLZ0000.DAT");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, vec![1; 3 * 1024 * 1024]).unwrap();
        let mut last = (0, 0);
        let mut items = std::collections::HashSet::new();
        let saved = create_with_progress(&state, &mut |phase, copied, total, item| {
            items.insert(item.to_owned());
            if phase == "copying" {
                assert!(copied >= last.0);
                assert!(copied <= total);
                last = (copied, total);
            }
            Ok(())
        }).unwrap();
        assert!(last.0 > 3 * 1024 * 1024);
        assert_eq!(last.0, last.1);
        assert!(items.contains("Database · master.db"));
        assert!(items.contains("Analysis files · USBANLZ/test/ANLZ0000.DAT"));
        assert!(items.contains("Checking database · master.db"));
        assert!(items.contains("Summarizing backup contents"));
        // Keep a prior snapshot while testing cancellation of a new one.
        let previous = state.backup_destination().join("library-previous.zip");
        fs::rename(&saved, &previous).unwrap();
        let before = rating(&state);
        let result = create_with_progress(&state, &mut |phase, copied, _, _| {
            if phase == "copying" && copied > 0 {
                Err(AppError::new(crate::error::ErrorKind::Cancelled, "Backup stopped."))
            } else { Ok(()) }
        });
        assert_eq!(result.unwrap_err().kind, crate::error::ErrorKind::Cancelled);
        assert_eq!(rating(&state), before);
        assert!(list(&state).unwrap().is_empty());
        assert!(fs::read_dir(state.backup_dir()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".partial-")));
        assert_eq!(fs::metadata(file).unwrap().len(), 3 * 1024 * 1024);
    }

    fn discarded(root: &Path) -> Vec<PathBuf> {
        fs::read_dir(root).unwrap().map(|e| e.unwrap().path())
            .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(DISCARD_PREFIX))
            .collect()
    }

    fn files_under(path: &Path) -> usize {
        if path.is_file() { return 1; }
        fs::read_dir(path).unwrap().map(|e| files_under(&e.unwrap().path())).sum()
    }

    fn wait_until_gone(path: &Path) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while path.exists() {
            assert!(std::time::Instant::now() < deadline, "{} was never deleted", path.display());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Stopping a backup of a large library took minutes because the staged
    /// copy, one file per analysis file, was deleted before the job could
    /// end (#472). The deletion is held back here, so the job can only have
    /// returned if it did not delete the staged files itself.
    #[test]
    fn a_stopped_backup_ends_before_its_staged_files_are_deleted() {
        let (_dir, state, location) = fixture();
        let tree = analysis(&location);
        for i in 0..400 {
            let file = tree.join(format!("P{:03}/{i:04}/ANLZ0000.DAT", i % 16));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, [1; 64]).unwrap();
        }
        let root = state.backup_destination();
        let held = HOLD_DISCARDS.write();
        let mut staged = std::collections::HashSet::new();
        let result = create_with_progress(&state, &mut |phase, _, _, item| {
            if phase == "copying" && item.starts_with("Analysis files ·") {
                staged.insert(item.to_owned());
                if staged.len() == 300 {
                    return Err(AppError::new(crate::error::ErrorKind::Cancelled, "Backup stopped."));
                }
            }
            Ok(())
        });
        assert_eq!(result.unwrap_err().kind, crate::error::ErrorKind::Cancelled);
        assert!(fs::read_dir(&root).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".partial-")));
        let aside = discarded(&root);
        assert_eq!(aside.len(), 1, "the staging folder is set aside, not deleted in place");
        // A file is reported just before its worker creates it.
        let workers = std::thread::available_parallelism().map_or(2, usize::from);
        assert!(files_under(&aside[0]) >= 300 - workers, "no staged file was deleted before the job ended");
        assert!(list(&state).unwrap().is_empty());
        drop(held);
        wait_until_gone(&aside[0]);
        assert_eq!(files_under(&tree), 400, "the library's own analysis files are untouched");
    }

    #[test]
    fn leftovers_from_an_earlier_run_are_swept_and_nothing_else_is() {
        let (_dir, state, _location) = fixture();
        let saved = PathBuf::from(create(&state).unwrap());
        let root = state.backup_destination();
        let leftover = root.join(format!("{DISCARD_PREFIX}earlier-run"));
        fs::create_dir_all(leftover.join("analysis/P001")).unwrap();
        fs::write(leftover.join("analysis/P001/ANLZ0000.DAT.zip"), b"staged").unwrap();
        let foreign = root.join(".partial-another-process");
        fs::create_dir(&foreign).unwrap();
        fs::write(root.join("notes.txt"), b"keep").unwrap();
        sweep_discarded(&root);
        wait_until_gone(&leftover);
        assert!(saved.is_file());
        assert!(foreign.is_dir());
        assert!(root.join("notes.txt").is_file());
    }

    #[test]
    fn a_completed_backup_leaves_no_staging_behind() {
        let (_dir, state, _location) = fixture();
        let saved = PathBuf::from(create(&state).unwrap());
        let root = state.backup_destination();
        for path in discarded(&root) {
            wait_until_gone(&path);
        }
        let names: Vec<_> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, [saved.file_name().unwrap()]);
    }

    #[test]
    fn a_failed_background_backup_reports_what_went_wrong() {
        let failed = error("Access is denied. (os error 5)");
        assert_eq!(failure_message(&failed), "Something went wrong inside rbxport. Backup: Access is denied. (os error 5)");
        let plain = AppError::new(crate::error::ErrorKind::NotFound, "The library could not be found.");
        assert_eq!(failure_message(&plain), "The library could not be found.");
    }

    #[test]
    fn background_job_rejects_duplicates_and_can_be_stopped_while_waiting() {
        let (_dir, state, _location) = fixture();
        let state = std::sync::Arc::new(state);
        let gate = state.edit_gate.lock();
        start(state.clone()).unwrap();
        assert!(state.backup_progress.lock().running);
        assert!(start(state.clone()).is_err());
        cancel(&state);
        assert_eq!(state.backup_progress.lock().phase, "stopping");
        drop(gate);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.backup_progress.lock().running {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(state.backup_progress.lock().phase, "cancelled");
        assert!(list(&state).unwrap().is_empty());
    }

    fn rating(state: &AppState) -> u8 {
        state
            .read_db(|db| {
                Ok(db.connection().query_row(
                    "SELECT Rating FROM djmdContent WHERE ID=?1",
                    [rbl_db::fixture::track_id(1)],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }
    #[test]
    fn artwork_vocals_and_library_selections_round_trip() {
        let (_dir, state, location) = fixture();
        let art = artwork(&location).join("abc/cover/artwork_m.jpg");
        fs::create_dir_all(art.parent().unwrap()).unwrap();
        fs::write(&art, b"thumbnail bytes").unwrap();
        let vocals = analysis(&location).join("abc/ANLZ0000.2EX");
        fs::create_dir_all(vocals.parent().unwrap()).unwrap();
        let mut anlz = b"PMAI".to_vec();
        for word in [12u32, 40] { anlz.extend(word.to_be_bytes()); }
        anlz.extend(b"PVDI");
        for word in [24u32, 28, 1024, 0x5622_0001, 4] { anlz.extend(word.to_be_bytes()); }
        anlz.extend([0, 2, 4, 1]);
        fs::write(&vocals, &anlz).unwrap();
        let root = location.master_db.parent().unwrap();
        for name in LIBRARY_FILES { fs::write(root.join(name), b"original selections").unwrap(); }
        let mut items = Vec::new();
        let snapshot = create_with_progress(&state, &mut |_, _, _, item| { items.push(item.to_owned()); Ok(()) }).unwrap();
        assert!(list(&state).unwrap()[0].includes_artwork);
        assert!(items.iter().any(|s| s.contains("Artwork thumbnails · Artwork/abc/cover/artwork_m.jpg")));
        fs::write(&art, b"changed artwork").unwrap();
        fs::write(&vocals, b"changed vocals").unwrap();
        for name in LIBRARY_FILES { fs::write(root.join(name), b"changed selections").unwrap(); }
        restore(&state, &location, Path::new(&snapshot), rbl_backup::restore::Parts::ALL);
        assert_eq!(fs::read(&art).unwrap(), b"thumbnail bytes");
        assert_eq!(fs::read(&vocals).unwrap(), anlz);
        assert_eq!(rbl_anlz::Anlz::read(&vocals).unwrap().vocals().unwrap(), &[0, 2, 4, 1]);
        for name in LIBRARY_FILES { assert_eq!(fs::read(root.join(name)).unwrap(), b"original selections"); }
    }

    #[test]
    fn backups_round_trip_database_analysis_and_wal_and_delete_only_the_snapshot() {
        let (_dir, state, location) = fixture();
        let track = rbl_db::fixture::track_id(1);
        // Keep a WAL connection alive to ensure committed WAL pages are included.
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
        db.connection()
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        db.connection().execute("UPDATE djmdContent SET Rating=3 WHERE ID=?1", [&track]).unwrap();
        let file = analysis(&location).join("test/ANLZ0000.DAT");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"original grid").unwrap();
        let music = location.share_root.join("music.mp3");
        fs::write(&music, b"music stays here").unwrap();
        let path = PathBuf::from(create(&state).unwrap());
        drop(db);
        let entries = list(&state).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].bytes > 0 && entries[0].created_at > 0 && entries[0].includes_analysis);
        assert_eq!(entries[0].bytes, fs::metadata(&path).unwrap().len(), "saved size is the compressed ZIP size");
        state.write(|w| w.set_rating(&track, 5)).unwrap();
        assert_eq!(rating(&state), 5);
        fs::write(&file, b"edited grid").unwrap();
        let extra = analysis(&location).join("later.DAT");
        fs::write(&extra, b"new analysis").unwrap();
        restore(&state, &location, &path, rbl_backup::restore::Parts::ALL);
        assert_eq!(rating(&state), 3);
        assert_eq!(fs::read(&file).unwrap(), b"original grid");
        assert!(!extra.exists());
        assert_eq!(fs::read(&music).unwrap(), b"music stays here");
        assert!(!pending(state.backup_dir()));
        // Reopening the app still sees the backup, rather than a memory-only list.
        assert_eq!(list(&state).unwrap().len(), 1);
        delete(&state, &path).unwrap();
        assert!(list(&state).unwrap().is_empty());
        assert_eq!(rating(&state), 3);
        assert!(file.exists());
        assert!(delete(&state, &location.master_db).is_err());
    }

    #[test]
    fn an_unfinished_restore_blocks_the_library_until_it_is_recovered() {
        let (_dir, state, location) = fixture();
        fs::create_dir_all(state.backup_dir()).unwrap();
        let journal = serde_json::json!({ "library": location.master_db, "committed": true, "swaps": [] });
        fs::write(state.backup_dir().join(rbl_backup::journal::NAME), journal.to_string()).unwrap();
        assert!(pending(state.backup_dir()));
        assert!(state.write(|w| w.set_rating(&rbl_db::fixture::track_id(1), 5)).is_err());
        recover(state.backup_dir(), &location).unwrap();
        state.write(|w| w.set_rating(&rbl_db::fixture::track_id(1), 5)).unwrap();
        assert_eq!(rating(&state), 5);
    }

    #[test]
    fn older_backup_formats_can_still_be_deleted() {
        let (_dir, state, location) = fixture();
        let database = rbl_db::write::Writer::open(location.clone(), state.backup_dir().to_path_buf()).unwrap().back_up_now().unwrap();
        assert!(list(&state).unwrap().is_empty());
        delete(&state, &database).unwrap();
        assert!(!database.exists());
        let folder = state.backup_destination().join("library-legacy-directory");
        fs::create_dir_all(&folder).unwrap();
        let manifest = Manifest { version: 1, library: location.master_db.clone(), created_at: 1, bytes: 0, includes_artwork: false, library_files: Vec::new() };
        fs::write(folder.join(rbl_backup::manifest::NAME), serde_json::to_vec(&manifest).unwrap()).unwrap();
        delete(&state, &folder).unwrap();
        assert!(!folder.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_backup_and_analysis_paths_are_refused() {
        let (_dir, state, location) = fixture();
        fs::create_dir_all(state.backup_dir()).unwrap();
        let alias = state.backup_dir().join("master-link.db");
        std::os::unix::fs::symlink(&location.master_db, &alias).unwrap();
        assert!(delete(&state, &alias).is_err());
        fs::create_dir_all(analysis(&location)).unwrap();
        std::os::unix::fs::symlink(&location.master_db, analysis(&location).join("escape"))
            .unwrap();
        assert!(create(&state).is_err());
        assert!(location.master_db.exists());
    }
}
