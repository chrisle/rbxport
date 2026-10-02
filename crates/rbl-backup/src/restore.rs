//! Putting a backup back as the library, all of it or only some parts.
//!
//! The chosen parts are unpacked beside what they replace, under names the
//! journal recognises, and checked: every entry against its checksum and the
//! database with SQLite's own check. Only then is the journal saved and each
//! part renamed into place, so an interruption at any point leaves either
//! the library as it was or, after recovery, the restored one.
use crate::{
    archive::{self, Entry},
    journal::{self, Journal, Swap},
    manifest::Manifest,
    refused, sidecar,
    sizes::BackupSizes,
    Error, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// Which parts of a backup to put back: the choices a person ticks.
#[allow(clippy::struct_excessive_bools, reason = "one independent choice per part, as the restore screen offers them")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parts {
    /// `master.db`: tracks, playlists, My Tag, ratings, cues and history.
    pub database: bool,
    /// `PIONEER/USBANLZ`.
    pub analysis: bool,
    /// `PIONEER/Artwork`.
    pub artwork: bool,
    /// Sync Manager's selections and the Automix playlist.
    pub library_files: bool,
}

impl Parts {
    pub const ALL: Self = Self { database: true, analysis: true, artwork: true, library_files: true };

    pub fn is_empty(self) -> bool {
        !(self.database || self.analysis || self.artwork || self.library_files)
    }

    /// Bytes these parts unpack to, by a backup's measured sizes.
    pub fn bytes(self, sizes: &BackupSizes) -> u64 {
        let analysis = sizes.waveforms + sizes.cues + sizes.beat_grids + sizes.phrases + sizes.vocals + sizes.other;
        (if self.database { sizes.database } else { 0 })
            + (if self.analysis { analysis } else { 0 })
            + (if self.artwork { sizes.artwork } else { 0 })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Preparing,
    Unpacking,
    Validating,
    Replacing,
}

pub struct Request<'a> {
    pub archive: &'a Path,
    pub parts: Parts,
    pub location: &'a rbl_db::LibraryLocation,
    /// Where the journal is kept: [`crate::state_dir`].
    pub state_dir: &'a Path,
    /// The backup's sizes, when already known, for progress and the free
    /// space check. Otherwise its summary's, or the whole archive's.
    pub sizes: Option<&'a BackupSizes>,
}

/// The phase, bytes unpacked so far, bytes expected, and the item being
/// worked on. Returning an error stops the restore, until files are being
/// replaced; from then on it runs to the end.
pub type Progress<'a> = dyn FnMut(Phase, u64, u64, &str) -> Result<()> + 'a;

/// Restores `request.parts` of a backup. `guard` is asked before anything
/// is written and again just before the library's files are replaced; an
/// error from it stops the restore with nothing changed.
pub fn restore(request: &Request<'_>, guard: &dyn Fn() -> Result<()>, progress: &mut Progress<'_>) -> Result<()> {
    let (location, state_dir) = (request.location, request.state_dir);
    if request.parts.is_empty() {
        return Err(refused("Choose what to restore."));
    }
    progress(Phase::Preparing, 0, 0, "Checking the backup")?;
    crate::writable(location)?;
    guard()?;
    if crate::analysis_edit_pending(state_dir) {
        return Err(refused("RBXport has an analysis edit it has not finished. Open RBXport once so it can finish it, quit it, then restore."));
    }
    journal::recover(state_dir, location)?;
    journal::remove_orphans(state_dir, location)?;
    let manifest = Manifest::read(request.archive)?;
    if !manifest.belongs_to(location) {
        return Err(refused("This backup belongs to another rekordbox library."));
    }
    let zip = archive::open(request.archive)?;
    let mut plan = Plan::new(&archive::names(&zip), &manifest, request.parts, location)?;
    let summary = match request.sizes {
        Some(_) => None,
        None => crate::summary::Summary::read(request.archive).ok().flatten(),
    };
    plan.bytes = match request.sizes.or(summary.as_ref().map(|summary| &summary.sizes)) {
        Some(sizes) => request.parts.bytes(sizes),
        None => zip.decompressed_size().and_then(|bytes| u64::try_from(bytes).ok()).unwrap_or(0),
    };
    drop(zip);
    plan.check_space(location)?;
    let result = (|| {
        let unpacked = plan.unpack(request.archive, progress)?;
        if let Some((database, wal)) = plan.database {
            progress(Phase::Validating, unpacked, unpacked, "Checking database · master.db")?;
            validate_database(&plan.stages[database].swap.staged, &plan.stages[wal].swap.staged, location)?;
        }
        progress(Phase::Replacing, unpacked, unpacked, "Replacing library files")?;
        crate::writable(location)?;
        guard()?;
        plan.replace(state_dir, location)
    })();
    if let Err(error) = result {
        if journal::pending(state_dir) {
            if let Err(recovery) = journal::recover(state_dir, location) {
                return Err(refused(format!(
                    "{error} The library could not be put back as it was ({recovery}). Open RBXport Restore again to finish recovering it."
                )));
            }
        } else {
            for stage in &plan.stages {
                let _ = journal::remove(&stage.swap.staged);
            }
        }
        return Err(error);
    }
    Ok(())
}

/// One target of the restore. Its staged path is unpacked into; a target
/// with nothing staged is set aside and not replaced, as a stale WAL is.
struct Stage {
    swap: Swap,
}

struct Unpack {
    index: usize,
    path: PathBuf,
    label: String,
}

#[derive(Default)]
struct Plan {
    stages: Vec<Stage>,
    /// Staged folders to create, parents before children.
    directories: BTreeSet<PathBuf>,
    files: Vec<Unpack>,
    /// Expected, for progress: entry sizes are only read as they unpack.
    bytes: u64,
    /// The database's and its WAL's stages.
    database: Option<(usize, usize)>,
}

impl Plan {
    /// Plans from entry names alone; reading every entry's header first
    /// would cost a pass over the whole archive.
    fn new(names: &[String], manifest: &Manifest, parts: Parts, location: &rbl_db::LibraryLocation) -> Result<Self> {
        let mut database = None;
        let mut wal = None;
        let mut library_files = Vec::new();
        let mut analysis = Vec::new();
        let mut artwork = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let kind = archive::classify(name).ok_or_else(|| refused("The backup has an unsafe entry."))?;
            let is_dir = name.ends_with('/');
            match kind {
                Entry::Database => database = Some(index),
                Entry::DatabaseWal => wal = Some(index),
                Entry::LibraryFile(file) => library_files.push((file, index)),
                Entry::Analysis(relative) => analysis.push((relative, index, is_dir)),
                Entry::Artwork(relative) => artwork.push((relative, index, is_dir)),
                Entry::Other => {}
            }
        }
        let mut plan = Self::default();
        if parts.database {
            let index = database.ok_or_else(|| refused("This backup has no library database."))?;
            let db = plan.single(&location.master_db, Some(index), "Database · master.db")?;
            let wal = plan.single(&sidecar(&location.master_db, "-wal"), wal, "Database · master.db-wal")?;
            plan.single(&sidecar(&location.master_db, "-shm"), None, "")?;
            plan.database = Some((db, wal));
        }
        // A part the backup does not hold is left as it is, as with the
        // artwork of a backup made before artwork was included.
        if parts.analysis && !analysis.is_empty() {
            plan.tree(&crate::analysis_dir(location), &analysis, "Analysis files · USBANLZ")?;
        }
        if parts.artwork && manifest.includes_artwork && !artwork.is_empty() {
            plan.tree(&crate::artwork_dir(location), &artwork, "Artwork · Artwork")?;
        }
        if parts.library_files {
            let root = location.master_db.parent().ok_or_else(|| refused("Invalid database path"))?;
            for name in &manifest.library_files {
                let &(_, index) = library_files
                    .iter()
                    .find(|(file, _)| file == name)
                    .ok_or_else(|| refused("A backup library settings file is missing."))?;
                plan.single(&root.join(name), Some(index), &format!("Library settings · {name}"))?;
            }
        }
        if plan.stages.is_empty() {
            return Err(refused("This backup holds none of the parts chosen."));
        }
        // In archive order, which is the order it is read in.
        plan.files.sort_by_key(|file| file.index);
        Ok(plan)
    }

    fn single(&mut self, target: &Path, index: Option<usize>, label: &str) -> Result<usize> {
        let swap = journal::swap_for(target)?;
        if let Some(index) = index {
            self.files.push(Unpack { index, path: swap.staged.clone(), label: label.to_owned() });
        }
        self.stages.push(Stage { swap });
        Ok(self.stages.len() - 1)
    }

    fn tree(&mut self, target: &Path, entries: &[(PathBuf, usize, bool)], label: &str) -> Result<()> {
        let swap = journal::swap_for(target)?;
        self.directories.insert(swap.staged.clone());
        for (relative, index, is_dir) in entries {
            if relative.as_os_str().is_empty() {
                continue;
            }
            let path = swap.staged.join(relative);
            if *is_dir {
                self.directories.insert(path);
                continue;
            }
            let mut parent = path.parent();
            while let Some(directory) = parent.filter(|directory| directory.starts_with(&swap.staged)) {
                if !self.directories.insert(directory.to_path_buf()) {
                    break;
                }
                parent = directory.parent();
            }
            let shown = relative.to_string_lossy().replace('\\', "/");
            self.files.push(Unpack { index: *index, path, label: format!("{label}/{shown}") });
        }
        self.stages.push(Stage { swap });
        Ok(())
    }

    /// A friendly refusal ahead of unpacking. Running out of space midway
    /// would still fail safely, with nothing replaced.
    fn check_space(&self, location: &rbl_db::LibraryLocation) -> Result<()> {
        let Some(root) = location.master_db.parent() else { return Ok(()) };
        let Ok(free) = fs2::available_space(root) else { return Ok(()) };
        let needed = self.bytes + 64 * 1024 * 1024;
        if needed > free {
            return Err(refused(format!(
                "There is not enough free disk space to restore. It needs {}, and {} is free.",
                gigabytes(needed),
                gigabytes(free)
            )));
        }
        Ok(())
    }

    /// Unpacks every planned entry and makes it durable. Returns the bytes
    /// unpacked.
    fn unpack(&self, path: &Path, progress: &mut Progress<'_>) -> Result<u64> {
        for directory in &self.directories {
            fs::create_dir_all(directory)?;
        }
        progress(Phase::Unpacking, 0, self.bytes, self.files.first().map_or("", |file| file.label.as_str()))?;
        let indexes: Vec<usize> = self.files.iter().map(|file| file.index).collect();
        let mut done = 0;
        archive::read_each(
            path,
            &indexes,
            &|position, meta, contents, report| {
                if archive::is_symlink(meta.unix_mode) {
                    return Err(refused("The backup has an unsafe entry."));
                }
                let file = &self.files[position];
                let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&file.path)?;
                archive::copy(contents, &mut output, report).map_err(|error| damaged(error, &file.label))?;
                output.sync_all()?;
                Ok(())
            },
            &mut |position, bytes| {
                done += bytes;
                progress(Phase::Unpacking, done, self.bytes.max(done), &self.files[position].label)
            },
        )?;
        // Children must be durable before their parents, and all of it
        // before the journal can publish any of it.
        for directory in self.directories.iter().rev() {
            rbl_core::durable::sync_dir(directory)?;
        }
        let mut parents: Vec<_> = self.stages.iter().filter_map(|stage| stage.swap.staged.parent()).collect();
        parents.sort();
        parents.dedup();
        for parent in parents {
            rbl_core::durable::sync_dir(parent)?;
        }
        Ok(done)
    }

    fn replace(&self, state_dir: &Path, location: &rbl_db::LibraryLocation) -> Result<()> {
        let mut journal = Journal {
            library: location.master_db.clone(),
            committed: false,
            swaps: self
                .stages
                .iter()
                .map(|stage| Swap { existed: stage.swap.target.exists(), ..stage.swap.clone() })
                .collect(),
        };
        journal::save(state_dir, &journal)?;
        for swap in &journal.swaps {
            if swap.existed {
                fs::rename(&swap.target, &swap.previous)?;
            }
            if swap.staged.exists() {
                fs::rename(&swap.staged, &swap.target)?;
            }
            rbl_core::durable::sync_dir(swap.target.parent().ok_or_else(|| refused("Invalid restore path"))?)?;
        }
        journal.committed = true;
        journal::save(state_dir, &journal)?;
        journal::recover(state_dir, location)
    }
}

/// A checksum or inflate failure names the entry; other errors pass through.
fn damaged(error: Error, label: &str) -> Error {
    match error {
        Error::Io(e) if matches!(e.kind(), std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput | std::io::ErrorKind::UnexpectedEof) => {
            refused(format!("The backup is damaged and cannot be restored: {label} did not unpack correctly."))
        }
        other => other,
    }
}

/// The staged database must open with this library's key and pass SQLite's
/// own check before it can replace anything.
fn validate_database(database: &Path, wal: &Path, location: &rbl_db::LibraryLocation) -> Result<()> {
    let check_wal = sidecar(database, "-wal");
    let valid = (|| -> Result<()> {
        if wal.exists() {
            fs::copy(wal, &check_wal)?;
        }
        let mut copy = location.clone();
        copy.master_db = database.to_path_buf();
        copy.is_real_install = false;
        let db = rbl_db::Library::open(copy, rbl_db::OpenMode::ReadOnly)
            .map_err(|_| refused("The backup's library database could not be opened. It may be damaged, or belong to another rekordbox installation."))?;
        let check: String = db.connection().query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(refused("The backup's library database is damaged."));
        }
        Ok(())
    })();
    journal::remove(&check_wal)?;
    journal::remove(&sidecar(database, "-shm"))?;
    valid
}

#[allow(clippy::cast_precision_loss, reason = "a size shown to one decimal place")]
fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1024_f64.powi(3))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::testing;
    use std::io::{Read, Write};

    struct Fixture {
        dir: tempfile::TempDir,
        location: rbl_db::LibraryLocation,
        archive: PathBuf,
        grid: PathBuf,
        art: PathBuf,
        selections: PathBuf,
    }

    fn state(fixture: &Fixture) -> PathBuf {
        fixture.dir.path().join("state")
    }

    /// A library with a rating of 4 and one of each kind of file, archived,
    /// then every part changed.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let location = testing::library(dir.path());
        testing::set_rating(&location, 4);
        let grid = crate::analysis_dir(&location).join("P001/0001/ANLZ0000.DAT");
        fs::create_dir_all(grid.parent().unwrap()).unwrap();
        fs::write(&grid, b"original grid").unwrap();
        fs::create_dir_all(crate::analysis_dir(&location).join("empty/nested")).unwrap();
        let art = crate::artwork_dir(&location).join("abc/artwork_m.jpg");
        fs::create_dir_all(art.parent().unwrap()).unwrap();
        fs::write(&art, b"original art").unwrap();
        let selections = location.master_db.parent().unwrap().join("masterPlaylists6.xml");
        fs::write(&selections, b"original selections").unwrap();
        let archive = dir.path().join("backup.zip");
        testing::archive(&location, &archive, None);
        testing::set_rating(&location, 1);
        fs::write(&grid, b"edited grid").unwrap();
        fs::write(crate::analysis_dir(&location).join("later.DAT"), b"new analysis").unwrap();
        fs::write(&art, b"edited art").unwrap();
        fs::write(&selections, b"edited selections").unwrap();
        Fixture { dir, location, archive, grid, art, selections }
    }

    fn run(fixture: &Fixture, parts: Parts) -> Result<Vec<Phase>> {
        let mut phases = Vec::new();
        restore(
            &Request { archive: &fixture.archive, parts, location: &fixture.location, state_dir: &state(fixture), sizes: None },
            &|| Ok(()),
            &mut |phase, done, total, _| {
                assert!(done <= total);
                if phases.last() != Some(&phase) {
                    phases.push(phase);
                }
                Ok(())
            },
        )?;
        Ok(phases)
    }

    fn leftovers(fixture: &Fixture) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for parent in [fixture.location.master_db.parent().unwrap().to_path_buf(), crate::analysis_dir(&fixture.location).parent().unwrap().to_path_buf()] {
            for entry in fs::read_dir(parent).unwrap() {
                let path = entry.unwrap().path();
                if path.file_name().unwrap().to_string_lossy().starts_with(journal::PREFIX) {
                    found.push(path);
                }
            }
        }
        found
    }

    #[test]
    fn a_full_restore_puts_every_part_back_exactly() {
        let fixture = fixture();
        let phases = run(&fixture, Parts::ALL).unwrap();
        assert_eq!(phases, [Phase::Preparing, Phase::Unpacking, Phase::Validating, Phase::Replacing]);
        assert_eq!(testing::rating(&fixture.location), 4);
        assert_eq!(fs::read(&fixture.grid).unwrap(), b"original grid");
        assert!(!crate::analysis_dir(&fixture.location).join("later.DAT").exists());
        assert!(crate::analysis_dir(&fixture.location).join("empty/nested").is_dir());
        assert_eq!(fs::read(&fixture.art).unwrap(), b"original art");
        assert_eq!(fs::read(&fixture.selections).unwrap(), b"original selections");
        assert!(!journal::pending(&state(&fixture)));
        assert_eq!(leftovers(&fixture), [] as [std::path::PathBuf; 0]);
    }

    #[test]
    fn each_part_restores_alone_and_leaves_the_others_as_they_are() {
        let fixture = fixture();
        run(&fixture, Parts { analysis: true, ..Parts::default() }).unwrap();
        assert_eq!(fs::read(&fixture.grid).unwrap(), b"original grid");
        assert_eq!(testing::rating(&fixture.location), 1);
        assert_eq!(fs::read(&fixture.art).unwrap(), b"edited art");
        assert_eq!(fs::read(&fixture.selections).unwrap(), b"edited selections");

        run(&fixture, Parts { database: true, ..Parts::default() }).unwrap();
        assert_eq!(testing::rating(&fixture.location), 4);
        assert_eq!(fs::read(&fixture.art).unwrap(), b"edited art");

        run(&fixture, Parts { artwork: true, ..Parts::default() }).unwrap();
        assert_eq!(fs::read(&fixture.art).unwrap(), b"original art");
        assert_eq!(fs::read(&fixture.selections).unwrap(), b"edited selections");

        run(&fixture, Parts { library_files: true, ..Parts::default() }).unwrap();
        assert_eq!(fs::read(&fixture.selections).unwrap(), b"original selections");
        assert_eq!(leftovers(&fixture), [] as [std::path::PathBuf; 0]);
    }

    #[test]
    fn restoring_the_database_replaces_a_stale_wal_and_shm() {
        let fixture = fixture();
        let wal = sidecar(&fixture.location.master_db, "-wal");
        let shm = sidecar(&fixture.location.master_db, "-shm");
        fs::write(&shm, b"stale").unwrap();
        run(&fixture, Parts { database: true, ..Parts::default() }).unwrap();
        assert!(!wal.exists() && !shm.exists());
        assert_eq!(testing::rating(&fixture.location), 4);
    }

    #[test]
    fn a_backup_of_another_library_changes_nothing() {
        let fixture = fixture();
        let other_dir = tempfile::tempdir().unwrap();
        let other = testing::library(other_dir.path());
        let archive = other_dir.path().join("other.zip");
        testing::archive(&other, &archive, None);
        let result = restore(
            &Request { archive: &archive, parts: Parts::ALL, location: &fixture.location, state_dir: &state(&fixture), sizes: None },
            &|| Ok(()),
            &mut |_, _, _, _| Ok(()),
        );
        assert!(matches!(result, Err(Error::Refused(message)) if message.contains("another rekordbox library")));
        assert_eq!(testing::rating(&fixture.location), 1);
    }

    #[test]
    fn stopping_or_a_refusing_guard_leaves_the_library_and_no_staged_files() {
        let fixture = fixture();
        let result = restore(
            &Request { archive: &fixture.archive, parts: Parts::ALL, location: &fixture.location, state_dir: &state(&fixture), sizes: None },
            &|| Ok(()),
            &mut |phase, done, _, _| if phase == Phase::Unpacking && done > 0 { Err(Error::Cancelled) } else { Ok(()) },
        );
        assert!(matches!(result, Err(Error::Cancelled)));
        let calls = std::cell::Cell::new(0);
        let result = restore(
            &Request { archive: &fixture.archive, parts: Parts::ALL, location: &fixture.location, state_dir: &state(&fixture), sizes: None },
            &|| {
                calls.set(calls.get() + 1);
                if calls.get() > 1 { Err(refused("Quit RBXport before restoring a backup.")) } else { Ok(()) }
            },
            &mut |_, _, _, _| Ok(()),
        );
        assert!(matches!(result, Err(Error::Refused(message)) if message.starts_with("Quit RBXport")));
        assert_eq!(calls.get(), 2);
        assert_eq!(testing::rating(&fixture.location), 1);
        assert_eq!(fs::read(&fixture.grid).unwrap(), b"edited grid");
        assert_eq!(leftovers(&fixture), [] as [std::path::PathBuf; 0]);
        assert!(!journal::pending(&state(&fixture)));
    }

    #[test]
    fn a_damaged_entry_is_caught_before_anything_is_replaced() {
        use std::io::{Seek, SeekFrom};
        let fixture = fixture();
        let start = {
            let mut zip = archive::open(&fixture.archive).unwrap();
            let index = (0..zip.len()).find(|&i| zip.by_index_raw(i).unwrap().name() == "master.db").unwrap();
            let entry = zip.by_index_raw(index).unwrap();
            entry.data_start() + entry.compressed_size() / 2
        };
        let mut file = fs::OpenOptions::new().read(true).write(true).open(&fixture.archive).unwrap();
        file.seek(SeekFrom::Start(start)).unwrap();
        let mut byte = [0];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::Start(start)).unwrap();
        file.write_all(&[byte[0] ^ 0xff]).unwrap();
        drop(file);
        assert!(run(&fixture, Parts::ALL).is_err());
        assert_eq!(testing::rating(&fixture.location), 1);
        assert_eq!(fs::read(&fixture.grid).unwrap(), b"edited grid");
        assert_eq!(leftovers(&fixture), [] as [std::path::PathBuf; 0]);
    }

    #[test]
    fn unsafe_entries_missing_parts_and_pending_edits_are_refused() {
        let fixture = fixture();
        let bad = fixture.dir.path().join("bad.zip");
        fs::copy(&fixture.archive, &bad).unwrap();
        {
            let mut zip = zip::ZipWriter::new_append(fs::OpenOptions::new().read(true).write(true).open(&bad).unwrap()).unwrap();
            zip.start_file("analysis/../../escape", zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"bad").unwrap();
            zip.finish().unwrap();
        }
        let state_dir = state(&fixture);
        let request = Request { archive: &bad, parts: Parts::ALL, location: &fixture.location, state_dir: &state_dir, sizes: None };
        assert!(restore(&request, &|| Ok(()), &mut |_, _, _, _| Ok(())).is_err());
        assert!(!fixture.dir.path().join("escape").exists());
        assert!(run(&fixture, Parts::default()).is_err());
        let edit = state(&fixture).join("analysis-journal/some-edit");
        fs::create_dir_all(&edit).unwrap();
        assert!(matches!(run(&fixture, Parts::ALL), Err(Error::Refused(message)) if message.contains("analysis edit")));
        fs::remove_dir(&edit).unwrap();
        run(&fixture, Parts::ALL).unwrap();
        assert_eq!(testing::rating(&fixture.location), 4);
    }
}
