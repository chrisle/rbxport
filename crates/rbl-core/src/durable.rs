//! Durable replacement through a unique sibling, with data flushed before
//! rename and the containing directory flushed afterwards on Unix.
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Err(error) = flush(&std::fs::File::open(path)?) {
        // Some removable filesystems mounted by macOS, notably exFAT, allow
        // the file and rename flushes above but reject fsync on a directory
        // with ENOTSUP (os error 45). The directory flush is an extra
        // durability guarantee, not a reason to report a completed USB
        // database write as failed. Keep every other error fatal.
        if !unsupported(&error) {
            return Err(error);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Flushes a file's data and metadata to the device, as
/// [`std::fs::File::sync_all`] does, on any filesystem a DJ stick uses.
///
/// On macOS `sync_all` is `fcntl(F_FULLFSYNC)` and nothing else, and Apple
/// documents `F_FULLFSYNC` as implemented on HFS, MS-DOS (FAT), UDF and APFS
/// only [OBS `man 2 fcntl`, macOS 26.6.2]: exFAT is not among them, and
/// where the call is not implemented it fails with ENOTSUP (os error 45).
/// SQLite falls back to `fsync` there [OBS `unixSync`/`full_fsync` in the
/// bundled `sqlite3.c`: "failure indicates that FULLFSYNC isn't supported
/// for this file system"], and so does this. The data still reaches the
/// device; only the drive-cache barrier is lost. Every other error stays.
pub fn flush(file: &std::fs::File) -> std::io::Result<()> {
    flush_with(file, std::fs::File::sync_all)
}

/// [`flush`] with the full flush supplied, so a test can stand in for a
/// filesystem that lacks it.
fn flush_with(file: &std::fs::File, full: impl FnOnce(&std::fs::File) -> std::io::Result<()>) -> std::io::Result<()> {
    match full(file) {
        #[cfg(target_vendor = "apple")]
        Err(error) if unsupported(&error) => rustix::fs::fsync(file).map_err(std::io::Error::from),
        other => other,
    }
}

/// Whether the filesystem rejected an operation it does not implement
/// (`ENOTSUP`/`EOPNOTSUPP`, or `ENOSYS`), as opposed to failing it.
#[must_use]
pub fn unsupported(error: &std::io::Error) -> bool {
    // macOS: ENOTSUP 45, EOPNOTSUPP 102. Linux: EOPNOTSUPP/ENOTSUP 95.
    const CODES: &[i32] = if cfg!(target_vendor = "apple") { &[45, 102] } else if cfg!(target_os = "linux") { &[95] } else { &[] };
    error.kind() == std::io::ErrorKind::Unsupported
        || error.raw_os_error().is_some_and(|code| CODES.contains(&code))
}

/// Moves a temporary file to `to`, refusing to replace an existing `to`.
///
/// The exclusive rename (`renameatx_np(RENAME_EXCL)` on macOS) is not
/// available everywhere a DJ stick is mounted: the macOS kernel `msdosfs`
/// driver has no `vnop_renamex`, so a FAT32 stick on a Mac that still uses
/// it answers `ENOTSUP` (os error 45), and exFAT does on macOS 26 as well
/// [OBS 2026-10-08, `hdiutil` exFAT image]. The usual emulation, a hard
/// link, is missing on both. Where the filesystem cannot do it, check that
/// `to` is absent and rename. The caller must own the directory (a private
/// staging directory under the export's write lock), so nothing can create
/// `to` in between.
pub fn persist_new(temp: tempfile::TempPath, to: &Path) -> std::io::Result<()> {
    persist_new_with(temp, to, |temp, to| temp.persist_noclobber(to))
}

/// [`persist_new`] with the exclusive rename supplied, so a test can stand
/// in for a filesystem that lacks it.
fn persist_new_with(
    temp: tempfile::TempPath,
    to: &Path,
    exclusive: impl FnOnce(tempfile::TempPath, &Path) -> Result<(), tempfile::PathPersistError>,
) -> std::io::Result<()> {
    match exclusive(temp, to) {
        Ok(()) => Ok(()),
        Err(failed) if unsupported(&failed.error) => {
            if to.symlink_metadata().is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} already exists", to.display()),
                ));
            }
            failed.path.persist(to).map_err(|e| e.error)
        }
        Err(failed) => Err(failed.error),
    }
}

/// `path` with every component in Unicode NFC, the form an export writes
/// its names in. Components that are not UTF-8 are kept as they are.
#[must_use]
pub fn nfc_path(path: &Path) -> PathBuf {
    use unicode_normalization::UnicodeNormalization;
    path.components()
        .map(|component| {
            let raw = component.as_os_str();
            raw.to_str().map_or_else(|| raw.to_owned(), |text| text.nfc().collect::<String>().into())
        })
        .collect()
}

/// `path` with its last component in the other Unicode forms (NFC, NFD)
/// that differ from the one given.
///
/// macOS's FAT32 and exFAT drivers (`FSKit`, macOS 26) list every name in
/// NFD whatever form it was stored in, and look a name up in any form, yet
/// rename (FAT32) or delete (exFAT) only under the stored form of the last
/// component [OBS 2026-10-08, `hdiutil` FAT32 and exFAT images: a file
/// created as NFC `Kesä.mp3` is listed as NFD, `stat` and `open` accept
/// both, `rename` from the NFD name fails with ENOENT on FAT32 and `unlink`
/// of the NFD name fails with ENOENT on exFAT]. Directory components in the
/// middle of a path resolve in either form.
fn name_forms(path: &Path) -> Vec<PathBuf> {
    use unicode_normalization::UnicodeNormalization;
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    let mut forms: Vec<PathBuf> = Vec::new();
    for form in [name.nfc().collect::<String>(), name.nfd().collect::<String>()] {
        if form != name && !forms.iter().any(|f| f.file_name().and_then(|n| n.to_str()) == Some(form.as_str())) {
            forms.push(path.with_file_name(form));
        }
    }
    forms
}

/// Runs `operation` on `path`, and when the name is not found although
/// something answers to it, on the other Unicode forms of its last
/// component; see [`name_forms`].
fn in_stored_form(path: &Path, operation: impl Fn(&Path) -> std::io::Result<()>) -> std::io::Result<()> {
    let mut error = match operation(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => error,
        other => return other,
    };
    if path.symlink_metadata().is_err() {
        return Err(error);
    }
    for alternate in name_forms(path) {
        match operation(&alternate) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => error = e,
            other => return other,
        }
    }
    Err(error)
}

/// [`std::fs::rename`], finding `from` under the form its name was stored
/// in when it was given in another; see [`name_forms`]. `to` is created
/// under the name given.
pub fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    in_stored_form(from, |from| std::fs::rename(from, to))
}

/// Deletes a directory tree, as `std::fs::remove_dir_all` would, on a
/// filesystem that lists names in a form it will not delete them under
/// (exFAT on macOS; see [`name_forms`]). An entry that goes away on its
/// own, as an `AppleDouble` `._` companion does with its file, is not an
/// error. A missing `path` is not an error either.
pub fn remove_tree(path: &Path) -> std::io::Result<()> {
    fn contents(dir: &Path) -> std::io::Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            other => other?,
        };
        for entry in entries {
            let child = entry?.path();
            let Ok(meta) = child.symlink_metadata() else { continue };
            if meta.is_dir() {
                contents(&child)?;
                gone(in_stored_form(&child, |p| std::fs::remove_dir(p)), &child)?;
            } else {
                gone(in_stored_form(&child, |p| std::fs::remove_file(p)), &child)?;
            }
        }
        Ok(())
    }
    fn gone(result: std::io::Result<()>, path: &Path) -> std::io::Result<()> {
        match result {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && path.symlink_metadata().is_err() => Ok(()),
            other => other,
        }
    }
    contents(path)?;
    gone(in_stored_form(path, |p| std::fs::remove_dir(p)), path)
}

/// An I/O error that says what was being done to which file.
fn at<'a>(action: &'static str, path: &'a Path) -> impl FnOnce(std::io::Error) -> std::io::Error + 'a {
    move |error| std::io::Error::new(error.kind(), format!("Could not {action} {}: {error}", path.display()))
}

pub fn create_dir_all(path: impl AsRef<Path>) -> std::io::Result<()> {
    let path = path.as_ref();
    let mut missing = Vec::new();
    let mut at = path;
    while !at.exists() {
        missing.push(at.to_path_buf());
        let Some(parent) = at.parent().filter(|p| !p.as_os_str().is_empty()) else {
            break;
        };
        at = parent;
    }
    std::fs::create_dir_all(path)?;
    for dir in missing.iter().rev() {
        sync_dir(
            dir.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new(".")),
        )?;
    }
    Ok(())
}

pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    flush(temp.as_file())?;
    temp.persist(path).map_err(|e| e.error)?;
    sync_dir(parent)
}

/// Flush a complete staged file before atomically replacing the destination.
/// Both paths must be on the same filesystem, and database handles closed.
pub fn replace(staged: &Path, target: &Path) -> std::io::Result<()> {
    flush(&std::fs::OpenOptions::new().write(true).open(staged)?)?;
    std::fs::rename(staged, target)?;
    sync_dir(
        target
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
}

/// Copy without truncating the destination, then durably publish it.
pub fn copy(source: &Path, target: &Path) -> std::io::Result<u64> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let bytes = std::io::copy(&mut std::fs::File::open(source)?, &mut temp)?;
    flush(temp.as_file())?;
    temp.persist(target).map_err(|e| e.error)?;
    sync_dir(parent)?;
    Ok(bytes)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PublicationEntry {
    path: std::path::PathBuf,
    present: bool,
    #[serde(default)]
    had_target: Option<bool>,
    /// Whether `path` is the name to publish under, exactly. Journals
    /// written before this field took their paths from the staging
    /// directory's listing, which macOS gives in NFD on FAT32 and exFAT
    /// while the export names its files in NFC; those are published under
    /// the form their image was stored in. See [`name_forms`].
    #[serde(default)]
    exact: bool,
}

/// A journal that cannot be replayed because something else wrote to the
/// device after the publication stopped: a file it was about to replace or
/// remove now holds neither the new image nor the file it set aside.
/// rekordbox does this on its own when it sees a stick, rewriting
/// `export.pdb` and `exportLibrary.db` and creating `exportLibrary.db-wal`
/// [OBS 2026-10-09, rekordbox 7.2.14 on Windows 11, #122]. See
/// [`Publication::set_aside`] for leaving such a journal behind.
#[derive(Debug)]
pub struct DeviceChanged {
    pub path: PathBuf,
}

impl std::fmt::Display for DeviceChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Publication conflict at {}; the device changed after sync failed and recovery data was retained",
            self.path.display()
        )
    }
}

impl std::error::Error for DeviceChanged {}

/// Whether `error` is a [`DeviceChanged`].
#[must_use]
pub fn is_device_changed(error: &std::io::Error) -> bool {
    error.get_ref().is_some_and(<dyn std::error::Error + Send + Sync>::is::<DeviceChanged>)
}

/// What a staging directory's name starts with, so a stage left behind by
/// an export that could not clean up after itself is recognizably ours.
const STAGE_PREFIX: &str = ".rbxport-staging-";
/// What a finished journal is renamed to before its images are deleted.
const RETIRED_PREFIX: &str = ".rbxport-retired-";

/// Durable commit intent for a set of files. Recovery rolls publication forward
/// using retained images, so recovery itself can be interrupted repeatedly.
/// Callers must serialize access and recover before exposing the file set.
pub struct Publication {
    root: std::path::PathBuf,
    journal: std::path::PathBuf,
    stage: tempfile::TempDir,
    _lock: std::fs::File,
    #[cfg(unix)]
    root_handle: std::fs::File,
}

impl Publication {
    pub fn new(root: &Path, name: &str) -> std::io::Result<Self> {
        create_dir_all(root)?;
        let lock = lock(root)?;
        let journal = root.join(name);
        if journal.try_exists()? {
            Self::finish(root, &journal, true)?;
        }
        // Under the exclusive lock nothing else is using a stage, so any
        // left here belonged to an export that stopped or could not delete
        // it, and would otherwise hold a copy of the audio on the device.
        remove_leftovers(root);
        Ok(Self {
            root: root.to_owned(),
            journal,
            stage: tempfile::Builder::new()
                .prefix(STAGE_PREFIX)
                .tempdir_in(root)
                .map_err(at("create a staging folder in", root))?,
            _lock: lock,
            #[cfg(unix)]
            root_handle: std::fs::File::open(root)?,
        })
    }

    pub fn check_root(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = self.root_handle.metadata()?;
            let current = std::fs::metadata(&self.root)?;
            if held.dev() != current.dev() || held.ino() != current.ino() {
                return Err(std::io::Error::other(
                    "The destination volume changed during publication",
                ));
            }
        }
        Ok(())
    }

    pub fn stage(&self) -> &Path {
        self.stage.path()
    }

    /// Missing staged files mean deletion. Paths are relative to root, and
    /// are the names the files are published under.
    pub fn commit(&self, paths: &[std::path::PathBuf]) -> std::io::Result<()> {
        self.check_root()?;
        validate_paths(paths)?;
        let mut entries = Vec::with_capacity(paths.len());
        let mut directories = std::collections::BTreeSet::new();
        for path in paths {
            // macOS creates AppleDouble companions on removable filesystems.
            // They are metadata, not part of the exported library, and can
            // disappear independently while publication is in progress.
            if is_appledouble(path) {
                continue;
            }
            let image = self.stage.path().join(path);
            let present = image.try_exists()?;
            if present {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&image)
                    .and_then(|file| flush(&file))
                    .map_err(at("flush", &image))?;
            }
            let target = self.root.join(path);
            let had_target = target.try_exists()? && std::fs::metadata(&target)?.is_file();
            entries.push(PublicationEntry {
                path: path.clone(),
                present,
                had_target: Some(had_target),
                exact: true,
            });
            let mut parent = image.parent();
            while let Some(dir) = parent.filter(|p| p.starts_with(self.stage.path())) {
                if dir.try_exists()? {
                    directories.insert(dir.to_path_buf());
                }
                parent = dir.parent();
            }
        }
        // Flush each directory once, children before parents.
        for directory in directories.iter().rev() {
            sync_dir(directory)?;
        }
        write(
            &self.stage.path().join("publication.json"),
            &serde_json::to_vec(&entries)?,
        )?;
        std::fs::rename(self.stage.path(), &self.journal).map_err(at("start publishing to", &self.root))?;
        sync_dir(&self.root)?;
        // This run wrote the journal and holds the lock: nothing else can
        // have changed the device since, and a file that looks newer only
        // has a clock ahead of this one (FAT keeps local time with no zone).
        Self::finish(&self.root, &self.journal, false)
    }

    pub fn recover(root: &Path, name: &str) -> std::io::Result<()> {
        let journal = root.join(name);
        if journal.try_exists()? {
            let _lock = lock(root)?;
            if journal.try_exists()? {
                Self::finish(root, &journal, true)?;
            }
        }
        Ok(())
    }

    /// Settles an interrupted publication that [`DeviceChanged`] stops from
    /// being replayed, so the device can be written again, leaving the
    /// device's files from one generation.
    ///
    /// Publication runs in the journal's order, so the last file the other
    /// writer changed tells which generation it saw. If that file had
    /// already been published, the writer saw the new one: the rest of the
    /// publication is finished. If it had not, the writer saw the earlier
    /// one: the publication is rolled back, every file it replaced or
    /// removed put back and every file it created where there was none
    /// removed. Either way the other writer's files are left as it wrote
    /// them. A SQLite `-wal` or `-shm` belongs to the database beside it, so
    /// it is left only when that database is the other writer's, and is
    /// otherwise moved out of the way.
    ///
    /// The journal's record is kept as `keep/publication.json`, and whatever
    /// the settling moves off the device under `keep/` in the device's own
    /// tree: the device's earlier versions under `keep/previous`, and other
    /// writers' files that had to move under `keep/theirs`. `keep` must be
    /// on the same file system as `root`. Returns the paths left as the
    /// other writer wrote them, or `None` when there was no journal.
    #[allow(clippy::too_many_lines, reason = "one settling pass over the journal")]
    pub fn set_aside(root: &Path, name: &str, keep: &Path) -> std::io::Result<Option<Vec<PathBuf>>> {
        let journal = root.join(name);
        if !journal.try_exists()? {
            return Ok(None);
        }
        let _lock = lock(root)?;
        if !journal.try_exists()? {
            return Ok(None);
        }
        let record = journal.join("publication.json");
        let bytes = std::fs::read(&record).map_err(at("read the interrupted publication record", &record))?;
        let entries: Vec<PublicationEntry> = serde_json::from_slice::<Vec<PublicationEntry>>(&bytes)?
            .into_iter()
            .filter(|entry| !is_appledouble(&entry.path))
            .map(|entry| PublicationEntry { path: publish_as(&journal, &entry), exact: true, ..entry })
            .collect();
        create_dir_all(keep).map_err(at("create the folder", keep))?;
        write(&keep.join("publication.json"), &bytes)?;
        let since = published_at(&journal)?;
        let changed: Vec<bool> = entries
            .iter()
            .map(|entry| written_since(&root.join(&entry.path), since))
            .collect::<std::io::Result<_>>()?;
        let base_of = |entry: &PublicationEntry| sqlite_companion_of(&entry.path).and_then(|base| entries.iter().position(|e| e.path == base));
        let published = |entry: &PublicationEntry| !journal.join(&entry.path).try_exists().unwrap_or(true);
        let forward = entries
            .iter()
            .zip(&changed)
            .rfind(|(entry, changed)| **changed && entry.present && base_of(entry).is_none())
            .is_none_or(|(entry, _)| published(entry));
        // What stays as the other writer left it.
        let theirs: Vec<bool> = entries
            .iter()
            .zip(&changed)
            .map(|(entry, this)| *this && base_of(entry).is_none_or(|base| changed[base]))
            .collect();
        let move_away = |path: &Path, under: &str| -> std::io::Result<()> {
            let to = keep.join(under).join(path);
            if let Some(parent) = to.parent() {
                create_dir_all(parent)?;
            }
            rename(&root.join(path), &to).map_err(at("move aside", &root.join(path)))
        };
        let kept_paths: Vec<PathBuf> = entries.iter().zip(&theirs).filter(|(_, t)| **t).map(|(e, _)| e.path.clone()).collect();
        // The device's earlier version of what stays theirs is kept.
        for path in &kept_paths {
            let previous = journal.join(".previous").join(path);
            if previous.try_exists()? {
                let to = keep.join("previous").join(path);
                if let Some(parent) = to.parent() {
                    create_dir_all(parent)?;
                }
                rename(&previous, &to).map_err(at("keep", &previous))?;
            }
        }
        // A companion whose database is not the other writer's.
        for (entry, (changed, theirs)) in entries.iter().zip(changed.iter().zip(&theirs)) {
            if *changed && !*theirs {
                move_away(&entry.path, "theirs")?;
            }
        }
        if forward {
            // Finish the rest as a run's own publication would.
            let rest: Vec<PublicationEntry> = entries
                .iter()
                .zip(&theirs)
                .filter(|(_, theirs)| !**theirs)
                .map(|(entry, _)| PublicationEntry { path: entry.path.clone(), present: entry.present, had_target: entry.had_target, exact: true })
                .collect();
            write(&record, &serde_json::to_vec(&rest)?)?;
            Self::finish(root, &journal, false)?;
        } else {
            for (entry, theirs) in entries.iter().zip(&theirs).rev() {
                if *theirs {
                    continue;
                }
                let target = root.join(&entry.path);
                let previous = journal.join(".previous").join(&entry.path);
                if previous.try_exists()? {
                    if let Some(parent) = target.parent() {
                        create_dir_all(parent)?;
                    }
                    rename(&previous, &target).map_err(at("put back", &target))?;
                    sync_dir(target.parent().unwrap_or(root))?;
                } else if entry.present && entry.had_target == Some(false) && published(entry) && target.try_exists()? {
                    // Published where the device had nothing.
                    in_stored_form(&target, |p| std::fs::remove_file(p)).map_err(at("remove", &target))?;
                }
            }
            // As `finish` retires a journal: out of the way first, then deleted.
            let discarded = tempfile::Builder::new().prefix(RETIRED_PREFIX).tempdir_in(root)?;
            std::fs::rename(&journal, discarded.path().join("set-aside"))
                .map_err(at("set aside the interrupted publication on", root))?;
            sync_dir(root)?;
            let _ = remove_tree(discarded.path());
        }
        sync_dir(keep)?;
        Ok(Some(kept_paths))
    }

    #[allow(clippy::too_many_lines, reason = "one ordered roll-forward over the journal")]
    /// `replay`: the journal was left by a run that stopped, so the device
    /// may have been written to since; see [`check_external_changes`].
    fn finish(root: &Path, journal: &Path, replay: bool) -> std::io::Result<()> {
        let record = journal.join("publication.json");
        let entries: Vec<PublicationEntry> = serde_json::from_slice(
            &std::fs::read(&record).map_err(at("read the interrupted publication record", &record))?,
        )?;
        validate_paths(
            &entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
        )?;
        let entries: Vec<PublicationEntry> = entries
            .into_iter()
            .map(|entry| PublicationEntry { path: publish_as(journal, &entry), exact: true, ..entry })
            .collect();
        let incomplete = journal.join(".incomplete");
        if incomplete.try_exists()? {
            return Err(std::io::Error::other(format!(
                "Publication image was lost for {}; recovery data was retained",
                String::from_utf8_lossy(&std::fs::read(incomplete)?)
            )));
        }
        if replay {
            check_external_changes(root, journal, &entries)?;
        }
        #[cfg(unix)]
        let root_handle = std::fs::File::open(root)?;
        // The previous generation stays inside the journal until every new
        // image is in place. All moves are within one filesystem, so a large
        // audio file is published with atomic renames instead of being copied
        // over the USB a second time. Every intermediate state is recognizable
        // and recovery always rolls forward.
        let publish = |entry: &PublicationEntry| -> std::io::Result<()> {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let held = root_handle.metadata()?;
                let current = std::fs::metadata(root)?;
                if held.dev() != current.dev() || held.ino() != current.ino() {
                    return Err(std::io::Error::other(
                        "Destination volume changed during publication",
                    ));
                }
            }
            let image = journal.join(&entry.path);
            let target = root.join(&entry.path);
            let previous = journal.join(".previous").join(&entry.path);
            if is_appledouble(&entry.path) {
                // Older journals included these transient files. Preserve an
                // existing target; if it was moved aside before a crash,
                // restore it. Never let a sidecar block real library recovery.
                if !target.try_exists()? && previous.try_exists()? {
                    if let Some(parent) = target.parent() {
                        create_dir_all(parent)?;
                    }
                    rename(&previous, &target).map_err(at("restore", &target))?;
                    sync_dir(target.parent().unwrap_or(root))?;
                }
                return Ok(());
            }
            if entry.present {
                if image.try_exists()? {
                    if entry.had_target == Some(true)
                        && !target.try_exists()?
                        && !previous.try_exists()?
                    {
                        return Err(std::io::Error::other(format!(
                            "Publication target vanished at {}; recovery data was retained",
                            entry.path.display()
                        )));
                    }
                    if target.try_exists()? {
                        if target.is_dir() {
                            return Err(std::io::Error::other(format!(
                                "Publication target is a directory: {}",
                                entry.path.display()
                            )));
                        }
                        if previous.try_exists()? {
                            return Err(std::io::Error::other(format!(
                                "Publication conflict at {}; recovery data was retained",
                                entry.path.display()
                            )));
                        }
                        if entry.had_target == Some(false) {
                            return Err(std::io::Error::other(format!(
                                "Publication target appeared at {}; recovery data was retained",
                                entry.path.display()
                            )));
                        }
                        if let Some(parent) = previous.parent() {
                            create_dir_all(parent)?;
                        }
                        rename(&target, &previous).map_err(at("set aside the previous", &target))?;
                        sync_dir(target.parent().unwrap_or(root))?;
                    }
                    if let Some(parent) = target.parent() {
                        create_dir_all(parent).map_err(at("create the folder", parent))?;
                    }
                    rename(&image, &target).map_err(at("publish", &target))?;
                    sync_dir(target.parent().unwrap_or(root))?;
                } else if !target.try_exists()?
                    || (entry.had_target == Some(true) && !previous.try_exists()?)
                {
                    // A lost new image must never silently become a deletion.
                    // Mark this journal before restoring the old file, so a
                    // later recovery cannot mistake that old file for a
                    // successfully published new image.
                    write(&incomplete, entry.path.to_string_lossy().as_bytes())?;
                    if previous.try_exists()? && !target.try_exists()? {
                        if let Some(parent) = target.parent() {
                            create_dir_all(parent)?;
                        }
                        copy(&previous, &target)?;
                    }
                    return Err(std::io::Error::other(format!(
                        "Missing publication image {}; the previous file was restored",
                        entry.path.display()
                    )));
                }
            } else if target.try_exists()? {
                if entry.had_target == Some(false) {
                    return Err(std::io::Error::other(format!(
                        "Publication target appeared at {}; recovery data was retained",
                        entry.path.display()
                    )));
                }
                if previous.try_exists()? {
                    return Err(std::io::Error::other(format!(
                        "Publication conflict at {}; recovery data was retained",
                        entry.path.display()
                    )));
                }
                if let Some(parent) = previous.parent() {
                    create_dir_all(parent)?;
                }
                rename(&target, &previous).map_err(at("remove", &target))?;
                sync_dir(target.parent().unwrap_or(root))?;
            }
            Ok(())
        };
        for entry in &entries {
            publish(entry).map_err(|error| {
                // Every failure names the file it was publishing.
                if error.to_string().contains(&*entry.path.to_string_lossy()) {
                    error
                } else {
                    std::io::Error::new(error.kind(), format!("Could not publish {}: {error}", entry.path.display()))
                }
            })?;
        }
        // Remove the commit intent atomically BEFORE deleting its images.
        // A crash during cleanup must never replay a missing image as deletion.
        let discarded = tempfile::Builder::new().prefix(RETIRED_PREFIX).tempdir_in(root)?;
        std::fs::rename(journal, discarded.path().join("completed"))
            .map_err(at("finish publishing to", root))?;
        if let Err(error) = sync_dir(root) {
            // Until retirement is durable, a crash may resurrect the journal.
            // Keep its images instead of deleting them during TempDir::drop.
            let _retained = discarded.keep();
            return Err(error);
        }
        // `TempDir`'s own cleanup cannot delete names exFAT lists in another
        // form; whatever this leaves, the next publication clears.
        let _ = remove_tree(discarded.path());
        Ok(())
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        // A stage that was not committed; see `remove_tree` for why
        // `TempDir`'s own cleanup is not enough on a stick.
        let _ = remove_tree(self.stage.path());
    }
}

/// The name a journal entry is published under. A journal written before
/// entries were exact carries the staging directory's listing, which on a
/// FAT32 or exFAT stick mounted by macOS is NFD although the image was
/// written in NFC; publishing under the listed form would name the file
/// differently from both databases, and renaming from it fails. Take the
/// NFC form whenever the image or the set-aside file answers to it, which
/// on those drivers it always does, and the listed form otherwise.
fn publish_as(journal: &Path, entry: &PublicationEntry) -> PathBuf {
    if entry.exact {
        return entry.path.clone();
    }
    let nfc = nfc_path(&entry.path);
    let answers = |path: &Path| journal.join(path).exists() || journal.join(".previous").join(path).exists();
    if nfc != entry.path && answers(&nfc) { nfc } else { entry.path.clone() }
}

/// The database a SQLite `-wal` or `-shm` file belongs to.
fn sqlite_companion_of(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let base = name.strip_suffix("-wal").or_else(|| name.strip_suffix("-shm"))?;
    Some(path.with_file_name(base))
}

/// Stages and retired journals a previous run left at `root`. Best effort:
/// a leftover that will not go is no reason to refuse this export.
fn remove_leftovers(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let ours = name.to_str().is_some_and(|n| n.starts_with(STAGE_PREFIX) || n.starts_with(RETIRED_PREFIX));
        if ours && entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = remove_tree(&entry.path());
        }
    }
}

fn is_appledouble(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("._"))
}

/// Check all real paths before replay so a late conflict cannot cause another
/// partially applied pass over a device that has changed since publication.
fn check_external_changes(
    root: &Path,
    journal: &Path,
    entries: &[PublicationEntry],
) -> std::io::Result<()> {
    let since = published_at(journal)?;
    for entry in entries {
        if is_appledouble(&entry.path) {
            continue;
        }
        let target = root.join(&entry.path);
        if !written_since(&target, since)? {
            continue;
        }
        let image = journal.join(&entry.path);
        let previous = journal.join(".previous").join(&entry.path);
        let expected = if entry.present && image.try_exists()? {
            Some(image.as_path())
        } else if previous.try_exists()? {
            Some(previous.as_path())
        } else {
            None
        };
        if !expected.is_some_and(|path| same_file_contents(&target, path).unwrap_or(false)) {
            return Err(std::io::Error::other(DeviceChanged { path: entry.path.clone() }));
        }
    }
    Ok(())
}

/// When a journal's publication began, with two seconds' grace for the
/// coarsest file system clock (FAT).
fn published_at(journal: &Path) -> std::io::Result<std::time::SystemTime> {
    std::fs::metadata(journal.join("publication.json"))?
        .modified()?
        .checked_add(Duration::from_secs(2))
        .ok_or_else(|| std::io::Error::other("Invalid publication timestamp"))
}

/// Whether `target` was written after `since`: by another writer, since a
/// publication's own images keep the time they were staged.
fn written_since(target: &Path, since: std::time::SystemTime) -> std::io::Result<bool> {
    if !target.try_exists().map_err(at("check", target))? {
        return Ok(false);
    }
    Ok(std::fs::metadata(target).and_then(|m| m.modified()).map_err(at("check", target))? > since)
}

fn same_file_contents(left: &Path, right: &Path) -> std::io::Result<bool> {
    let mut left = std::fs::File::open(left)?;
    let mut right = std::fs::File::open(right)?;
    if left.metadata()?.len() != right.metadata()?.len() {
        return Ok(false);
    }
    let mut left_buffer = [0; 16 * 1024];
    let mut right_buffer = [0; 16 * 1024];
    loop {
        let count = left.read(&mut left_buffer)?;
        if count == 0 {
            return Ok(true);
        }
        right.read_exact(&mut right_buffer[..count])?;
        if left_buffer[..count] != right_buffer[..count] {
            return Ok(false);
        }
    }
}

fn validate_paths(paths: &[std::path::PathBuf]) -> std::io::Result<()> {
    if paths.iter().any(|p| {
        p.as_os_str().is_empty()
            || p.components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
    }) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "publication paths must be relative",
        ));
    }
    Ok(())
}

/// Hold after recovery while inspecting/importing a device. Export publishers
/// use the matching exclusive lock for the whole operation.
pub fn read_lock(root: &Path) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".rbxport-write.lock"))?;
    fs2::FileExt::lock_shared(&file)?;
    Ok(file)
}

fn lock(root: &Path) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".rbxport-write.lock"))
        .map_err(at("open the write lock on", root))?;
    fs2::FileExt::lock_exclusive(&file).map_err(at("lock", root))?;
    Ok(file)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn only_an_unsupported_directory_flush_is_optional() {
        assert!(unsupported(&std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "directory fsync is unsupported",
        )));
        assert!(!unsupported(&std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        )));
    }

    /// A filesystem without `F_FULLFSYNC` (exFAT on macOS) answers ENOTSUP;
    /// the file is flushed with `fsync` instead of failing the write (#283).
    #[cfg(target_vendor = "apple")]
    #[test]
    fn a_file_flush_the_filesystem_lacks_falls_back_to_fsync() {
        let dir = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(dir.path().join("exportLibrary.db")).unwrap();
        let mut asked = false;
        flush_with(&file, |_| {
            asked = true;
            Err(std::io::Error::from_raw_os_error(45))
        })
        .unwrap();
        assert!(asked);
        flush_with(&file, |_| Err(std::io::Error::from_raw_os_error(102))).unwrap();
    }

    #[test]
    fn a_file_flush_that_fails_otherwise_still_fails() {
        let dir = tempfile::tempdir().unwrap();
        let file = std::fs::File::create(dir.path().join("exportLibrary.db")).unwrap();
        // EIO: the device failed the write; that is never papered over.
        let error = flush_with(&file, |_| Err(std::io::Error::from_raw_os_error(5))).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
        flush(&file).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_enotsup_from_directory_flush_is_optional() {
        assert!(unsupported(
            &std::io::Error::from_raw_os_error(45)
        ));
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write(path, bytes).unwrap();
    }
    fn mark_newer(path: &Path) {
        let times = std::fs::FileTimes::new()
            .set_modified(std::time::SystemTime::now() + Duration::from_secs(10));
        std::fs::File::open(path).unwrap().set_times(times).unwrap();
    }
    #[test]
    fn failed_publication_replays_retained_images_and_deletions() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("first"), b"old").unwrap();
        write(&root.path().join("obsolete"), b"old").unwrap();
        // Force failure after publishing the first file.
        std::fs::create_dir(root.path().join("second")).unwrap();
        let publication = Publication::new(root.path(), ".journal").unwrap();
        write(&publication.stage().join("first"), b"new first").unwrap();
        write(&publication.stage().join("second"), b"new second").unwrap();
        assert!(publication
            .commit(&["first".into(), "second".into(), "obsolete".into()])
            .is_err());
        drop(publication);
        assert_eq!(
            std::fs::read(root.path().join("first")).unwrap(),
            b"new first"
        );
        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert!(root.path().join(".journal/.previous/first").is_file());
        std::fs::remove_dir(root.path().join("second")).unwrap();
        Publication::recover(root.path(), ".journal").unwrap();
        Publication::recover(root.path(), ".journal").unwrap();
        assert_eq!(
            std::fs::read(root.path().join("second")).unwrap(),
            b"new second"
        );
        assert!(!root.path().join("obsolete").exists());
        assert!(!root.path().join(".journal").exists());
    }
    #[test]
    fn abandoned_staging_never_modifies_published_files() {
        let root = tempfile::tempdir().unwrap();
        write(&root.path().join("db"), b"old").unwrap();
        let publication = Publication::new(root.path(), ".journal").unwrap();
        write(&publication.stage().join("db"), b"new").unwrap();
        drop(publication);
        Publication::recover(root.path(), ".journal").unwrap();
        assert_eq!(std::fs::read(root.path().join("db")).unwrap(), b"old");
    }

    #[test]
    fn publication_ignores_appledouble_companions() {
        let root = tempfile::tempdir().unwrap();
        let publication = Publication::new(root.path(), ".journal").unwrap();
        write_file(
            &publication
                .stage()
                .join("PIONEER/rekordbox/exportLibrary.db"),
            b"new db",
        );
        write_file(
            &publication
                .stage()
                .join("PIONEER/rekordbox/._exportLibrary.db"),
            b"metadata",
        );
        publication
            .commit(&[
                "PIONEER/rekordbox/exportLibrary.db".into(),
                "PIONEER/rekordbox/._exportLibrary.db".into(),
            ])
            .unwrap();
        assert_eq!(
            std::fs::read(root.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap(),
            b"new db"
        );
        assert!(!root
            .path()
            .join("PIONEER/rekordbox/._exportLibrary.db")
            .exists());
    }

    #[test]
    fn old_sidecar_entries_do_not_block_real_database_recovery() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        write_file(
            &root.path().join("PIONEER/._rekordbox"),
            b"current metadata",
        );
        write_file(&journal.join("PIONEER/._rekordbox"), b"staged metadata");
        write_file(
            &journal.join(".previous/PIONEER/._rekordbox"),
            b"old metadata",
        );
        write_file(
            &journal.join(".previous/Contents/._01"),
            b"restore metadata",
        );
        write_file(
            &root.path().join("PIONEER/rekordbox/exportLibrary.db"),
            b"old db",
        );
        write_file(
            &journal.join("PIONEER/rekordbox/exportLibrary.db"),
            b"new db",
        );
        let entries = [
            PublicationEntry {
                path: "PIONEER/._rekordbox".into(),
                present: true,
                had_target: None,
                exact: false,
            },
            PublicationEntry {
                path: "Contents/._01".into(),
                present: true,
                had_target: None,
                exact: false,
            },
            PublicationEntry {
                path: "Contents/01/._missing.wav".into(),
                present: true,
                had_target: None,
                exact: false,
            },
            PublicationEntry {
                path: "PIONEER/rekordbox/exportLibrary.db".into(),
                present: true,
                had_target: None,
                exact: false,
            },
        ];
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&entries).unwrap(),
        );

        Publication::recover(root.path(), ".journal").unwrap();

        assert_eq!(
            std::fs::read(root.path().join("PIONEER/._rekordbox")).unwrap(),
            b"current metadata"
        );
        assert_eq!(
            std::fs::read(root.path().join("Contents/._01")).unwrap(),
            b"restore metadata"
        );
        assert_eq!(
            std::fs::read(root.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap(),
            b"new db"
        );
        assert!(!journal.exists());
    }

    #[test]
    fn missing_real_database_image_still_retains_recovery_data() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let path = "PIONEER/rekordbox/exportLibrary.db";
        write_file(&journal.join(".previous").join(path), b"old db");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry {
                path: path.into(),
                present: true,
                had_target: None,
                exact: false,
            }])
            .unwrap(),
        );

        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert_eq!(std::fs::read(root.path().join(path)).unwrap(), b"old db");
        assert_eq!(
            std::fs::read(journal.join(".previous").join(path)).unwrap(),
            b"old db"
        );
        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert!(journal.exists());
    }

    #[test]
    fn interrupted_replacement_finishes_from_retained_image() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let path = "PIONEER/rekordbox/exportLibrary.db";
        write_file(&journal.join(path), b"new db");
        write_file(&journal.join(".previous").join(path), b"old db");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry {
                path: path.into(),
                present: true,
                had_target: None,
                exact: false,
            }])
            .unwrap(),
        );

        Publication::recover(root.path(), ".journal").unwrap();
        Publication::recover(root.path(), ".journal").unwrap();
        assert_eq!(std::fs::read(root.path().join(path)).unwrap(), b"new db");
        assert!(!journal.exists());
    }

    #[test]
    fn conflicting_real_target_keeps_both_versions_for_recovery() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let path = "PIONEER/rekordbox/exportLibrary.db";
        write_file(&root.path().join(path), b"external db");
        write_file(&journal.join(path), b"new db");
        write_file(&journal.join(".previous").join(path), b"old db");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry {
                path: path.into(),
                present: true,
                had_target: None,
                exact: false,
            }])
            .unwrap(),
        );

        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert_eq!(
            std::fs::read(root.path().join(path)).unwrap(),
            b"external db"
        );
        assert_eq!(std::fs::read(journal.join(path)).unwrap(), b"new db");
        assert_eq!(
            std::fs::read(journal.join(".previous").join(path)).unwrap(),
            b"old db"
        );
    }

    #[test]
    fn a_journal_the_device_changed_since_is_rolled_back_around_the_change() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let db = "PIONEER/rekordbox/exportLibrary.db";
        // Another writer rewrote the database after the stop.
        write_file(&root.path().join(db), b"rekordbox rewrote it");
        write_file(&journal.join(db), b"staged db");
        write_file(&journal.join(".previous").join(db), b"the device's database");
        // Already replaced before the stop: the device's own file is in
        // `.previous`, the new one in place.
        write_file(&root.path().join("PIONEER/first.pdb"), b"new first");
        write_file(&journal.join(".previous/PIONEER/first.pdb"), b"the device's own");
        // Published where the device had nothing.
        write_file(&root.path().join("Contents/new.mp3"), b"new audio");
        // Not reached: still an image, the device's file untouched.
        write_file(&root.path().join("PIONEER/later.pdb"), b"the device's later");
        write_file(&journal.join("PIONEER/later.pdb"), b"staged later");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[
                PublicationEntry { path: "Contents/new.mp3".into(), present: true, had_target: Some(false), exact: true },
                PublicationEntry { path: "PIONEER/first.pdb".into(), present: true, had_target: Some(true), exact: true },
                PublicationEntry { path: "PIONEER/later.pdb".into(), present: true, had_target: Some(true), exact: true },
                PublicationEntry { path: db.into(), present: true, had_target: Some(true), exact: true },
            ])
            .unwrap(),
        );
        mark_newer(&root.path().join(db));

        let error = Publication::recover(root.path(), ".journal").unwrap_err();
        assert!(is_device_changed(&error), "{error}");
        assert!(!is_device_changed(&std::io::Error::other("something else")));

        let keep = root.path().join("kept");
        let theirs = Publication::set_aside(root.path(), ".journal", &keep).unwrap().unwrap();
        assert_eq!(theirs, [PathBuf::from(db)]);
        assert!(!journal.exists());
        let read = |p: &str| std::fs::read(root.path().join(p)).unwrap();
        // The other writer's change stays; the device's version of it is kept.
        assert_eq!(read(db), b"rekordbox rewrote it");
        assert_eq!(std::fs::read(keep.join("previous").join(db)).unwrap(), b"the device's database");
        // Everything else is as it was before the publication.
        assert_eq!(read("PIONEER/first.pdb"), b"the device's own");
        assert_eq!(read("PIONEER/later.pdb"), b"the device's later");
        assert!(!root.path().join("Contents/new.mp3").exists());
        assert!(keep.join("publication.json").is_file());
        // The device is usable again, and a second call finds nothing to do.
        Publication::recover(root.path(), ".journal").unwrap();
        assert!(Publication::set_aside(root.path(), ".journal", &keep).unwrap().is_none());
        assert!(std::fs::read_dir(root.path()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(RETIRED_PREFIX)));
    }

    #[test]
    fn a_journal_whose_published_database_was_rewritten_is_finished() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let db = "PIONEER/rekordbox/exportLibrary.db";
        let wal = "PIONEER/rekordbox/exportLibrary.db-wal";
        // Published before the stop, then rewritten, and opened in WAL mode.
        write_file(&root.path().join(db), b"rewritten new database");
        write_file(&journal.join(".previous").join(db), b"the device's database");
        write_file(&root.path().join(wal), b"their wal");
        // Published before the stop and left alone since.
        write_file(&root.path().join("PIONEER/first.pdb"), b"new first");
        write_file(&journal.join(".previous/PIONEER/first.pdb"), b"the device's own");
        // Not reached.
        write_file(&root.path().join("PIONEER/sync"), b"earlier sync record");
        write_file(&journal.join("PIONEER/sync"), b"new sync record");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[
                PublicationEntry { path: wal.into(), present: false, had_target: Some(false), exact: true },
                PublicationEntry { path: "PIONEER/first.pdb".into(), present: true, had_target: Some(true), exact: true },
                PublicationEntry { path: db.into(), present: true, had_target: Some(true), exact: true },
                PublicationEntry { path: "PIONEER/sync".into(), present: true, had_target: Some(true), exact: true },
            ])
            .unwrap(),
        );
        mark_newer(&root.path().join(db));
        mark_newer(&root.path().join(wal));
        assert!(is_device_changed(&Publication::recover(root.path(), ".journal").unwrap_err()));

        let keep = root.path().join("kept");
        let theirs = Publication::set_aside(root.path(), ".journal", &keep).unwrap().unwrap();
        assert_eq!(theirs, [PathBuf::from(wal), PathBuf::from(db)]);
        let read = |p: &str| std::fs::read(root.path().join(p)).unwrap();
        // The writer saw the new generation, so the rest of it is published,
        // and its database stays with the WAL beside it.
        assert_eq!(read("PIONEER/sync"), b"new sync record");
        assert_eq!(read("PIONEER/first.pdb"), b"new first");
        assert_eq!(read(db), b"rewritten new database");
        assert_eq!(read(wal), b"their wal");
        assert_eq!(std::fs::read(keep.join("previous").join(db)).unwrap(), b"the device's database");
        assert!(!journal.exists());
    }

    #[test]
    fn a_wal_beside_a_database_that_is_put_back_is_moved_away() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let db = "PIONEER/rekordbox/exportLibrary.db";
        let wal = "PIONEER/rekordbox/exportLibrary.db-wal";
        // Not reached: the database is the device's, the new one an image.
        write_file(&root.path().join(db), b"the device's database");
        write_file(&journal.join(db), b"new database");
        // Another file changed by the other writer, not yet published.
        write_file(&root.path().join("PIONEER/other.pdb"), b"theirs");
        write_file(&journal.join("PIONEER/other.pdb"), b"new other");
        // And a WAL appeared, though the database it would belong to is not
        // the other writer's.
        write_file(&root.path().join(wal), b"a wal");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[
                PublicationEntry { path: wal.into(), present: false, had_target: Some(false), exact: true },
                PublicationEntry { path: "PIONEER/other.pdb".into(), present: true, had_target: Some(true), exact: true },
                PublicationEntry { path: db.into(), present: true, had_target: Some(true), exact: true },
            ])
            .unwrap(),
        );
        mark_newer(&root.path().join("PIONEER/other.pdb"));
        mark_newer(&root.path().join(wal));

        let keep = root.path().join("kept");
        let theirs = Publication::set_aside(root.path(), ".journal", &keep).unwrap().unwrap();
        assert_eq!(theirs, [PathBuf::from("PIONEER/other.pdb")]);
        assert_eq!(std::fs::read(root.path().join(db)).unwrap(), b"the device's database");
        assert!(!root.path().join(wal).exists(), "a WAL is not left beside a database it may not belong to");
        assert_eq!(std::fs::read(keep.join("theirs").join(wal)).unwrap(), b"a wal");
        assert!(!journal.exists());
    }

    #[test]
    fn newer_external_database_edit_stops_before_replaying_other_files() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let db = "PIONEER/rekordbox/exportLibrary.db";
        write_file(&root.path().join(db), b"external edit");
        write_file(&journal.join(db), b"staged db");
        write_file(&root.path().join("PIONEER/first.pdb"), b"old first");
        write_file(&journal.join("PIONEER/first.pdb"), b"new first");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[
                PublicationEntry {
                    path: "PIONEER/first.pdb".into(),
                    present: true,
                    had_target: None,
                    exact: false,
                },
                PublicationEntry {
                    path: db.into(),
                    present: true,
                    had_target: None,
                    exact: false,
                },
            ])
            .unwrap(),
        );
        mark_newer(&root.path().join(db));

        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert_eq!(
            std::fs::read(root.path().join(db)).unwrap(),
            b"external edit"
        );
        assert_eq!(
            std::fs::read(root.path().join("PIONEER/first.pdb")).unwrap(),
            b"old first"
        );
        assert!(journal.exists());
    }

    #[test]
    fn newer_timestamp_with_identical_bytes_can_recover() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let path = "PIONEER/rekordbox/exportLibrary.db";
        write_file(&root.path().join(path), b"staged db");
        write_file(&journal.join(path), b"staged db");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry {
                path: path.into(),
                present: true,
                had_target: None,
                exact: false,
            }])
            .unwrap(),
        );
        mark_newer(&root.path().join(path));

        Publication::recover(root.path(), ".journal").unwrap();
        assert_eq!(std::fs::read(root.path().join(path)).unwrap(), b"staged db");
        assert!(!journal.exists());
    }

    #[test]
    fn recreated_database_wal_is_not_deleted_during_recovery() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let path = "PIONEER/rekordbox/exportLibrary.db-wal";
        write_file(&root.path().join(path), b"new sqlite writes");
        write_file(&journal.join(".previous").join(path), b"old sqlite writes");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry {
                path: path.into(),
                present: false,
                had_target: None,
                exact: false,
            }])
            .unwrap(),
        );
        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert_eq!(
            std::fs::read(root.path().join(path)).unwrap(),
            b"new sqlite writes"
        );
        assert_eq!(
            std::fs::read(journal.join(".previous").join(path)).unwrap(),
            b"old sqlite writes"
        );
    }

    #[test]
    fn vanished_image_cannot_masquerade_as_an_old_database() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let path = "PIONEER/rekordbox/exportLibrary.db";
        write_file(&root.path().join(path), b"old db");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry {
                path: path.into(),
                present: true,
                had_target: Some(true),
                exact: false,
            }])
            .unwrap(),
        );

        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert_eq!(std::fs::read(root.path().join(path)).unwrap(), b"old db");
        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert!(journal.join(".incomplete").exists());
    }

    #[test]
    fn newly_appeared_real_target_is_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let path = "PIONEER/rekordbox/exportLibrary.db";
        write_file(&root.path().join(path), b"external db");
        write_file(&journal.join(path), b"staged db");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry {
                path: path.into(),
                present: true,
                had_target: Some(false),
                exact: false,
            }])
            .unwrap(),
        );

        assert!(Publication::recover(root.path(), ".journal").is_err());
        assert_eq!(
            std::fs::read(root.path().join(path)).unwrap(),
            b"external db"
        );
        assert_eq!(std::fs::read(journal.join(path)).unwrap(), b"staged db");
    }

    #[test]
    fn filesystems_without_an_operation_are_told_apart_from_failures() {
        // ENOTSUP from renameatx_np(RENAME_EXCL) on a macOS FAT32/exFAT
        // stick, which made exportLibrary.db unwritable (#122).
        let code = if cfg!(target_vendor = "apple") { 45 } else { 95 };
        assert!(unsupported(&std::io::Error::from_raw_os_error(code)));
        assert!(!unsupported(&std::io::Error::from_raw_os_error(2)));
        assert!(!unsupported(&std::io::Error::from_raw_os_error(13)));
    }

    #[test]
    fn persisting_a_new_file_never_replaces_one() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("exportLibrary.db");
        let temp = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        std::fs::write(&temp, b"new").unwrap();
        persist_new(temp, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        let again = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        assert!(persist_new(again, &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    /// A filesystem without the exclusive rename (exFAT on macOS 26, FAT32
    /// under the kernel `msdosfs` driver): the file is still published, and
    /// still never over an existing one.
    #[test]
    fn persisting_falls_back_where_the_exclusive_rename_is_unsupported() {
        let code = if cfg!(target_vendor = "apple") { 45 } else { 95 };
        let unsupported_here = |temp: tempfile::TempPath, _: &Path| {
            Err(tempfile::PathPersistError { error: std::io::Error::from_raw_os_error(code), path: temp })
        };
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("exportLibrary.db");
        let temp = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        std::fs::write(&temp, b"new").unwrap();
        persist_new_with(temp, &target, unsupported_here).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");

        let temp = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        std::fs::write(&temp, b"other").unwrap();
        let staged = temp.to_path_buf();
        let error = persist_new_with(temp, &target, unsupported_here).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!staged.exists(), "the refused temporary file is cleaned up");

        // Any other failure is reported as it is.
        let temp = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        let denied = |temp: tempfile::TempPath, _: &Path| {
            Err(tempfile::PathPersistError { error: std::io::Error::from_raw_os_error(13), path: temp })
        };
        let error = persist_new_with(temp, &root.path().join("other.db"), denied).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(13));
        assert!(!root.path().join("other.db").exists());
    }

    #[test]
    fn paths_are_brought_to_nfc_component_by_component() {
        assert_eq!(
            nfc_path(Path::new("Contents/Bjo\u{308}rk/Kesa\u{308}.mp3")),
            PathBuf::from("Contents/Bj\u{f6}rk/Kes\u{e4}.mp3")
        );
        assert_eq!(nfc_path(Path::new("PIONEER/rekordbox/export.pdb")), PathBuf::from("PIONEER/rekordbox/export.pdb"));
    }

    /// A journal an earlier version wrote on a FAT32 stick mounted by macOS:
    /// its paths are the stage's listing, in NFD, while the images were
    /// written in NFC. Renaming from the listed name failed with "No such
    /// file or directory", and every later sync and import failed the same
    /// way trying to recover it (#161). It must publish, under the NFC
    /// name both databases use.
    #[test]
    fn a_journal_of_listed_nfd_names_publishes_under_the_nfc_names() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let nfc = "Contents/Bj\u{f6}rk/Album/Kes\u{e4}.mp3";
        let nfd = "Contents/Bjo\u{308}rk/Album/Kesa\u{308}.mp3";
        write_file(&journal.join(nfc), b"audio");
        write_file(&journal.join("PIONEER/rekordbox/export.pdb"), b"pdb");
        write_file(
            &journal.join("publication.json"),
            // As the earlier version wrote it: no `exact` field.
            &serde_json::to_vec(&serde_json::json!([
                {"path": nfd, "present": true, "had_target": false},
                {"path": "PIONEER/rekordbox/export.pdb", "present": true, "had_target": false},
            ]))
            .unwrap(),
        );
        assert_ne!(nfc, nfd);

        Publication::recover(root.path(), ".journal").unwrap();

        assert!(!journal.exists());
        let album = root.path().join("Contents/Bj\u{f6}rk/Album");
        let listed: Vec<String> = std::fs::read_dir(&album)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| !n.starts_with("._"))
            .collect();
        // The temporary directory keeps the form a name was given in, so
        // its listing shows the form the file was published under.
        assert_eq!(listed, vec!["Kes\u{e4}.mp3".to_owned()]);
        assert_eq!(std::fs::read(root.path().join(nfc)).unwrap(), b"audio");
        assert_eq!(std::fs::read(root.path().join("PIONEER/rekordbox/export.pdb")).unwrap(), b"pdb");
    }

    #[test]
    fn a_journal_entry_with_an_exact_path_is_published_as_written() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        let nfd = "Contents/Kesa\u{308}.mp3";
        write_file(&journal.join(nfd), b"audio");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry { path: nfd.into(), present: true, had_target: Some(false), exact: true }]).unwrap(),
        );
        Publication::recover(root.path(), ".journal").unwrap();
        assert_eq!(std::fs::read(root.path().join(nfd)).unwrap(), b"audio");
    }

    #[test]
    fn an_abandoned_stage_is_deleted_and_a_stale_one_cleared() {
        let root = tempfile::tempdir().unwrap();
        let publication = Publication::new(root.path(), ".journal").unwrap();
        write_file(&publication.stage().join("Contents/a.mp3"), b"audio");
        let stage = publication.stage().to_owned();
        assert!(stage.file_name().unwrap().to_str().unwrap().starts_with(STAGE_PREFIX));
        drop(publication);
        assert!(!stage.exists());

        // What an export that could not delete its stage leaves behind
        // (exFAT on macOS lists names it will not delete): the next
        // publication on the device clears it, and nothing else.
        let left_behind = root.path().join(format!("{STAGE_PREFIX}old"));
        write_file(&left_behind.join("Contents/b.mp3"), b"audio");
        let retired = root.path().join(format!("{RETIRED_PREFIX}old"));
        write_file(&retired.join("completed/publication.json"), b"[]");
        write_file(&root.path().join(".tmpOther/keep"), b"someone else's");
        let publication = Publication::new(root.path(), ".journal").unwrap();
        assert!(!left_behind.exists());
        assert!(!retired.exists());
        assert!(root.path().join(".tmpOther/keep").exists());
        drop(publication);
    }

    #[test]
    fn a_committed_publication_leaves_no_stage_or_journal() {
        let root = tempfile::tempdir().unwrap();
        let publication = Publication::new(root.path(), ".journal").unwrap();
        write_file(&publication.stage().join("PIONEER/rekordbox/export.pdb"), b"pdb");
        publication.commit(&["PIONEER/rekordbox/export.pdb".into()]).unwrap();
        drop(publication);
        let left: Vec<String> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(left.iter().all(|n| n == "PIONEER" || n == ".rbxport-write.lock"), "{left:?}");
    }

    #[test]
    fn a_failed_rename_names_the_file() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join(".journal");
        write_file(&journal.join("Contents/a.mp3"), b"audio");
        // A plain file where the folder must go.
        write_file(&root.path().join("Contents"), b"not a folder");
        write_file(
            &journal.join("publication.json"),
            &serde_json::to_vec(&[PublicationEntry { path: "Contents/a.mp3".into(), present: true, had_target: Some(false), exact: true }]).unwrap(),
        );
        let error = Publication::recover(root.path(), ".journal").unwrap_err();
        assert!(error.to_string().contains("Contents"), "{error}");
    }
}
