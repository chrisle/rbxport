//! RBXport backup archives. RBXport writes them; RBXport Restore reads them
//! and puts them back as the rekordbox library.
//!
//! An archive is a standard ZIP holding:
//!
//! - `manifest.json`, the [`manifest::Manifest`]: which library it came from
//!   and which parts it holds. What a restore relies on.
//! - `summary.json`, the [`summary::Summary`]: counts and sizes, so a person
//!   can check it is the right backup. Backups made before it existed lack it.
//! - `master.db`, and `master.db-wal` in some older backups.
//! - `analysis/…`, a copy of `PIONEER/USBANLZ`.
//! - `artwork/…`, a copy of `PIONEER/Artwork`.
//! - Whichever of [`LIBRARY_FILES`] were beside `master.db`.
//!
//! A restore ignores any other entry.
//!
//! Both programs share the restore journal in [`state_dir`]: a restore
//! interrupted in either one is finished or rolled back by the next of them
//! to start, and RBXport will not read a library while one is pending.

pub mod archive;
pub mod journal;
pub mod manifest;
pub mod restore;
pub mod sizes;
pub mod summary;

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Why the backup cannot be used or restored, worded for the person restoring.
    #[error("{0}")]
    Refused(String),
    #[error("Restore stopped.")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Database(#[from] rbl_db::DbError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn refused(message: impl Into<String>) -> Error {
    Error::Refused(message.into())
}

/// Library selections kept beside `master.db`, outside the SQL database:
/// Sync Manager's playlist choices and the Automix playlist.
pub const LIBRARY_FILES: [&str; 2] = ["masterPlaylists6.xml", "automixPlaylist6.xml"];

/// `PIONEER/USBANLZ`: beat grids, waveforms, cues, phrases and vocals.
pub fn analysis_dir(location: &rbl_db::LibraryLocation) -> PathBuf {
    location.share_root.join("PIONEER/USBANLZ")
}

/// `PIONEER/Artwork`: album art and its thumbnails.
pub fn artwork_dir(location: &rbl_db::LibraryLocation) -> PathBuf {
    location.share_root.join("PIONEER/Artwork")
}

/// `master.db-wal` from `master.db`, and so on.
pub fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

/// Names a directory to use instead of [`state_dir`]'s default, so a test
/// run never shares recovery state with the installed apps.
pub const STATE_DIR_ENV: &str = "RBXPORT_STATE_DIR";

/// RBXport's local backup state: the restore journal, the journal of
/// analysis edits, and the chosen backup folder. It stays on this machine
/// even when backups are saved to another drive.
pub fn state_dir() -> PathBuf {
    if let Some(chosen) = std::env::var_os(STATE_DIR_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(chosen);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("rbxport-triode/backups")
}

/// Where RBXport's Default backup folder setting is saved, as a JSON string.
pub const DESTINATION_FILE: &str = "backup-destination.json";

/// The folder RBXport saves new backups in: the Default backup folder chosen
/// in Preferences › Backups, or [`state_dir`] itself when none was chosen.
pub fn default_destination(state_dir: &Path) -> PathBuf {
    std::fs::read(state_dir.join(DESTINATION_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PathBuf>(&bytes).ok())
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| state_dir.to_path_buf())
}

/// Refuses to change the installed library while rekordbox runs.
pub fn writable(location: &rbl_db::LibraryLocation) -> Result<()> {
    match rbl_db::write_refusal_reason(
        location.is_real_install,
        rbl_db::test_mode(),
        rbl_db::is_rekordbox_running(),
    ) {
        Some(reason) => Err(refused(reason)),
        None => Ok(()),
    }
}

/// True while RBXport has an analysis edit it has not finished. RBXport
/// completes or undoes it the next time it opens the library, and a restore
/// underneath it would be overwritten by that recovery.
pub fn analysis_edit_pending(state_dir: &Path) -> bool {
    std::fs::read_dir(state_dir.join("analysis-journal"))
        .is_ok_and(|mut entries| entries.next().is_some())
}

#[cfg(any(test, feature = "testing"))]
#[allow(clippy::unwrap_used, clippy::missing_panics_doc)]
pub mod testing {
    //! A fixture library and archives laid out the way RBXport writes them.
    use std::{fs, io::Write, path::Path};
    use zip::{write::SimpleFileOptions, ZipWriter};

    pub fn library(dir: &Path) -> rbl_db::LibraryLocation {
        rbl_db::fixture::build(dir, rbl_db::fixture::Shape::default()).unwrap()
    }

    pub fn rating(location: &rbl_db::LibraryLocation) -> u8 {
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        db.connection()
            .query_row("SELECT Rating FROM djmdContent WHERE ID=?1", [rbl_db::fixture::track_id(1)], |r| r.get(0))
            .unwrap()
    }

    pub fn set_rating(location: &rbl_db::LibraryLocation, rating: u8) {
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
        db.connection()
            .execute("UPDATE djmdContent SET Rating=?1 WHERE ID=?2", rusqlite::params![rating, rbl_db::fixture::track_id(1)])
            .unwrap();
    }

    /// An ANLZ file: the file header, then one section per tag.
    pub fn anlz(tags: &[&[u8; 4]]) -> Vec<u8> {
        let mut bytes = b"PMAI".to_vec();
        bytes.extend(12u32.to_be_bytes());
        bytes.extend(u32::try_from(12 + tags.len() * 16).unwrap().to_be_bytes());
        for tag in tags {
            bytes.extend(*tag);
            bytes.extend(12u32.to_be_bytes());
            bytes.extend(16u32.to_be_bytes());
            bytes.extend([7; 4]);
        }
        bytes
    }

    /// Archives the library at `location` as RBXport does: a vacuumed
    /// database, both trees, the library files present and the manifest.
    pub fn archive(location: &rbl_db::LibraryLocation, to: &Path, summary: Option<&crate::summary::Summary>) {
        let stage = tempfile::tempdir().unwrap();
        let db = stage.path().join("master.db");
        let snapshot = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        snapshot.connection().execute("VACUUM main INTO ?1", [db.to_string_lossy().as_ref()]).unwrap();
        drop(snapshot);
        let mut zip = ZipWriter::new(fs::File::create(to).unwrap());
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut add = |name: &str, bytes: &[u8]| {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        };
        add("master.db", &fs::read(&db).unwrap());
        let root = location.master_db.parent().unwrap();
        let library_files: Vec<String> = crate::LIBRARY_FILES
            .into_iter()
            .filter(|name| root.join(name).exists())
            .map(str::to_owned)
            .collect();
        for name in &library_files {
            add(name, &fs::read(root.join(name)).unwrap());
        }
        for (tree, prefix) in [(crate::analysis_dir(location), "analysis"), (crate::artwork_dir(location), "artwork")] {
            zip.add_directory(format!("{prefix}/"), SimpleFileOptions::default()).unwrap();
            let mut pending = vec![tree.clone()];
            while let Some(directory) = pending.pop() {
                let Ok(entries) = fs::read_dir(&directory) else { continue };
                for entry in entries {
                    let path = entry.unwrap().path();
                    let relative = path.strip_prefix(&tree).unwrap().to_string_lossy().replace('\\', "/");
                    if path.is_dir() {
                        zip.add_directory(format!("{prefix}/{relative}/"), SimpleFileOptions::default()).unwrap();
                        pending.push(path);
                    } else {
                        zip.start_file(format!("{prefix}/{relative}"), options).unwrap();
                        zip.write_all(&fs::read(&path).unwrap()).unwrap();
                    }
                }
            }
        }
        let manifest = crate::manifest::Manifest {
            version: crate::manifest::VERSION,
            library: location.master_db.clone(),
            created_at: 1_790_000_000_000,
            bytes: 0,
            includes_artwork: true,
            library_files,
        };
        zip.start_file(crate::manifest::NAME, options).unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
        if let Some(summary) = summary {
            zip.start_file(crate::summary::NAME, options).unwrap();
            zip.write_all(&serde_json::to_vec(summary).unwrap()).unwrap();
        }
        zip.finish().unwrap();
    }
}
