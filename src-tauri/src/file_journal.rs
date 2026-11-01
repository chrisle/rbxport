//! Recovery journal for an edit spanning analysis files and a library row.
//! File images are durable before publication. After a crash the row's USN
//! decides whether to finish publication or restore every original file.
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::error::{AppError, AppResult};

#[derive(Serialize, Deserialize)]
struct FileImage { path: PathBuf, existed: bool }
#[derive(Serialize, Deserialize)]
struct Manifest {
    library: PathBuf,
    track: String,
    before_usn: i64,
    analysis_update_before: i64,
    bpm_after: u32,
    path_after: Option<String>,
    changes_row: bool,
    committed: bool,
    files: Vec<FileImage>,
}

pub struct FileJournal { dir: PathBuf, manifest: Manifest }

fn error(e: impl std::fmt::Display) -> AppError { AppError::internal(format!("Analysis recovery: {e}")) }

fn row(db: &rbl_db::Library, track: &str) -> AppResult<(i64, u32, String, i64)> {
    db.connection().query_row("SELECT COALESCE(rb_local_usn, 0), COALESCE(BPM, 0), COALESCE(AnalysisDataPath, ''), CAST(COALESCE(AnalysisUpdated, '0') AS INTEGER) FROM djmdContent WHERE ID = ?1 AND rb_local_deleted = 0",
        [track], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(error)
}

impl FileJournal {
    pub fn prepare(root: &Path, location: &rbl_db::LibraryLocation, track: &str,
        bpm_after: u32, path_after: Option<String>, changes_row: bool,
        files: &[(PathBuf, Vec<u8>)]) -> AppResult<Self> {
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).map_err(error)?;
        Self::prepare_with(&db, root, location, track, bpm_after, path_after, changes_row, files)
    }

    /// [`Self::prepare`] on a library handle the caller already holds. Opening
    /// the library derives its key again, about 60 ms, so a loop preparing a
    /// journal per track reads the row on the app's shared handle (#794).
    #[allow(clippy::too_many_arguments, reason = "prepare's arguments plus the open handle")]
    pub fn prepare_with(db: &rbl_db::Library, root: &Path, location: &rbl_db::LibraryLocation, track: &str,
        bpm_after: u32, path_after: Option<String>, changes_row: bool,
        files: &[(PathBuf, Vec<u8>)]) -> AppResult<Self> {
        let (before_usn, _, _, analysis_update_before) = row(db, track)?;
        let dir = root.join("analysis-journal").join(uuid::Uuid::new_v4().to_string());
        crate::durable::create_dir_all(&dir).map_err(error)?;
        crate::durable::sync_dir(root).map_err(error)?;
        let mut images = Vec::new();
        for (index, (path, after)) in files.iter().enumerate() {
            let before = match std::fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(error(e)),
            };
            if let Some(bytes) = &before { crate::durable::write(&dir.join(format!("{index}.before")), bytes).map_err(error)?; }
            crate::durable::write(&dir.join(format!("{index}.after")), after).map_err(error)?;
            images.push(FileImage { path: path.clone(), existed: before.is_some() });
        }
        let journal = Self { dir, manifest: Manifest {
            library: location.master_db.clone(), track: track.into(), before_usn, analysis_update_before, bpm_after,
            path_after, changes_row, committed: false, files: images,
        }};
        journal.persist()?;
        crate::durable::sync_dir(journal.dir.parent().unwrap_or(root)).map_err(error)?;
        Ok(journal)
    }

    fn persist(&self) -> AppResult<()> {
        crate::durable::write(&self.dir.join("manifest.json"), &serde_json::to_vec(&self.manifest).map_err(error)?).map_err(error)
    }

    pub fn publish(&self) -> AppResult<()> { self.install(true, false) }

    fn install(&self, after: bool, check_external: bool) -> AppResult<()> {
        // Verify the whole set before changing anything during recovery.
        if check_external {
            for (index, file) in self.manifest.files.iter().enumerate() {
                let current = match std::fs::read(&file.path) {
                    Ok(bytes) => Some(bytes),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(error(e)),
                };
                let before = if file.existed { Some(std::fs::read(self.dir.join(format!("{index}.before"))).map_err(error)?) } else { None };
                let next = std::fs::read(self.dir.join(format!("{index}.after"))).map_err(error)?;
                if current != before && current.as_deref() != Some(next.as_slice()) {
                    return Err(error(format!("{} changed outside this edit; recovery files remain in {}", file.path.display(), self.dir.display())));
                }
            }
        }
        for (index, file) in self.manifest.files.iter().enumerate() {
            if !after && !file.existed {
                match std::fs::remove_file(&file.path) {
                    Ok(()) => crate::durable::sync_dir(file.path.parent().unwrap_or(Path::new("."))).map_err(error)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                    Err(e) => return Err(error(e)),
                }
            } else {
                let suffix = if after { "after" } else { "before" };
                let bytes = std::fs::read(self.dir.join(format!("{index}.{suffix}"))).map_err(error)?;
                if let Some(parent) = file.path.parent() { crate::durable::create_dir_all(parent).map_err(error)?; }
                crate::durable::write(&file.path, &bytes).map_err(error)?;
            }
        }
        Ok(())
    }

    fn remove(&self) -> AppResult<()> {
        std::fs::remove_file(self.dir.join("manifest.json")).map_err(error)?;
        crate::durable::sync_dir(&self.dir).map_err(error)?;
        std::fs::remove_dir_all(&self.dir).map_err(error)?;
        crate::durable::sync_dir(self.dir.parent().unwrap_or(Path::new("."))).map_err(error)
    }

    pub fn rollback(&self) -> AppResult<()> { self.install(false, false)?; self.remove() }

    /// A failed database call can have committed before a later error. Resolve
    /// from the persisted row rather than blindly restoring the old files.
    pub fn reconcile(self, location: &rbl_db::LibraryLocation) -> AppResult<()> {
        let root = self.dir.parent().and_then(Path::parent).ok_or_else(|| error("invalid journal path"))?;
        recover(root, location)
    }


    pub fn commit(mut self) -> AppResult<()> {
        self.manifest.committed = true;
        self.persist()?;
        if let Err(e) = self.remove() { tracing::warn!(error = %e, "committed analysis journal will be cleaned up on restart"); }
        Ok(())
    }
}

pub fn recover(root: &Path, location: &rbl_db::LibraryLocation) -> AppResult<()> {
    let directory = root.join("analysis-journal");
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(error(e)),
    };
    for entry in entries {
        let dir = entry.map_err(error)?.path();
        let bytes = match std::fs::read(dir.join("manifest.json")) {
            Ok(bytes) => bytes,
            // A crash while staging cannot have published any files.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(error(e)),
        };
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(error)?;
        if manifest.library != location.master_db { continue; }
        if location.is_real_install && rbl_db::is_rekordbox_running() {
            return Err(error("Quit rekordbox so an interrupted analysis edit can be recovered."));
        }
        let journal = FileJournal { dir, manifest };
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).map_err(error)?;
        let (usn, bpm, path, analysis_updated) = row(&db, &journal.manifest.track)?;
        let changed = usn != journal.manifest.before_usn;
        let expected = bpm == journal.manifest.bpm_after && journal.manifest.path_after.as_ref().is_none_or(|p| *p == path && analysis_updated == journal.manifest.analysis_update_before + 1);
        if !journal.manifest.committed && journal.manifest.changes_row && changed && !expected {
            return Err(error(format!("the library row changed outside the interrupted edit; journal retained at {}", journal.dir.display())));
        }
        let committed = journal.manifest.committed || (journal.manifest.changes_row && changed && expected);
        journal.install(committed, true)?;
        journal.remove()?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, rbl_db::LibraryLocation, PathBuf, String) {
        let root = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(root.path(), rbl_db::fixture::Shape::default()).unwrap();
        let backup = root.path().join("backups");
        let track = rbl_db::fixture::track_id(0);
        (root, location, backup, track)
    }
    #[test]
    fn crash_before_database_commit_restores_both_files_and_removes_new_files() {
        let (root, location, backup, track) = fixture();
        let dat = root.path().join("DAT"); let ext = root.path().join("EXT"); let extra = root.path().join("2EX");
        std::fs::write(&dat, b"old dat").unwrap(); std::fs::write(&ext, b"old ext").unwrap();
        let journal = FileJournal::prepare(&backup, &location, &track, 12345, None, true,
            &[(dat.clone(), b"new dat".to_vec()), (ext.clone(), b"new ext".to_vec()), (extra.clone(), b"new extra".to_vec())]).unwrap();
        journal.publish().unwrap(); drop(journal);
        recover(&backup, &location).unwrap();
        assert_eq!(std::fs::read(dat).unwrap(), b"old dat");
        assert_eq!(std::fs::read(ext).unwrap(), b"old ext"); assert!(!extra.exists());
        recover(&backup, &location).unwrap();
    }
    #[test]
    fn crash_after_database_commit_finishes_the_file_publication() {
        let (root, location, backup, track) = fixture();
        let dat = root.path().join("DAT"); std::fs::write(&dat, b"old").unwrap();
        let journal = FileJournal::prepare(&backup, &location, &track, 12345, None, true,
            &[(dat.clone(), b"new".to_vec())]).unwrap();
        let mut writer = rbl_db::write::Writer::open(location.clone(), &backup).unwrap();
        writer.set_bpm_x100(&track, 12345).unwrap(); drop(writer); drop(journal);
        recover(&backup, &location).unwrap();
        assert_eq!(std::fs::read(dat).unwrap(), b"new");
    }
    #[test]
    fn a_reported_error_after_database_commit_keeps_the_committed_files() {
        let (root, location, backup, track) = fixture();
        let dat = root.path().join("DAT");
        std::fs::write(&dat, b"old").unwrap();
        let journal = FileJournal::prepare(&backup, &location, &track, 12345, None, true,
            &[(dat.clone(), b"new".to_vec())]).unwrap();
        journal.publish().unwrap();
        let mut writer = rbl_db::write::Writer::open(location.clone(), &backup).unwrap();
        writer.set_bpm_x100(&track, 12345).unwrap();
        drop(writer);
        journal.reconcile(&location).unwrap();
        assert_eq!(std::fs::read(dat).unwrap(), b"new");
    }
    #[test]
    fn an_external_file_edit_is_preserved_and_the_journal_is_retained() {
        let (root, location, backup, track) = fixture();
        let dat = root.path().join("DAT"); std::fs::write(&dat, b"old").unwrap();
        let journal = FileJournal::prepare(&backup, &location, &track, 12345, None, true,
            &[(dat.clone(), b"new".to_vec())]).unwrap();
        let dir = journal.dir.clone(); journal.publish().unwrap(); drop(journal);
        std::fs::write(&dat, b"external").unwrap();
        assert!(recover(&backup, &location).is_err());
        assert_eq!(std::fs::read(dat).unwrap(), b"external"); assert!(dir.exists());
    }
}
