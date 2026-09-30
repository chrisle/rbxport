//! Durable replacement through a unique sibling, with data flushed before
//! rename and the containing directory flushed afterwards on Unix.
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub fn sync_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Err(error) = std::fs::File::open(path)?.sync_all() {
        // Some removable filesystems mounted by macOS, notably exFAT, allow
        // the file and rename flushes above but reject fsync on a directory
        // with ENOTSUP (os error 45). The directory flush is an extra
        // durability guarantee, not a reason to report a completed USB
        // database write as failed. Keep every other error fatal.
        if !directory_sync_unsupported(&error) {
            return Err(error);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(unix)]
fn directory_sync_unsupported(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::Unsupported
        || cfg!(target_os = "macos") && error.raw_os_error() == Some(45)
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
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    sync_dir(parent)
}

/// Flush a complete staged file before atomically replacing the destination.
/// Both paths must be on the same filesystem, and database handles closed.
pub fn replace(staged: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(staged)?
        .sync_all()?;
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
    temp.as_file().sync_all()?;
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
}

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
            Self::finish(root, &journal)?;
        }
        Ok(Self {
            root: root.to_owned(),
            journal,
            stage: tempfile::tempdir_in(root)?,
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

    /// Missing staged files mean deletion. Paths are relative to root.
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
                    .open(&image)?
                    .sync_all()?;
            }
            let target = self.root.join(path);
            let had_target = target.try_exists()? && std::fs::metadata(&target)?.is_file();
            entries.push(PublicationEntry {
                path: path.clone(),
                present,
                had_target: Some(had_target),
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
        std::fs::rename(self.stage.path(), &self.journal)?;
        sync_dir(&self.root)?;
        Self::finish(&self.root, &self.journal)
    }

    pub fn recover(root: &Path, name: &str) -> std::io::Result<()> {
        let journal = root.join(name);
        if journal.try_exists()? {
            let _lock = lock(root)?;
            if journal.try_exists()? {
                Self::finish(root, &journal)?;
            }
        }
        Ok(())
    }

    fn finish(root: &Path, journal: &Path) -> std::io::Result<()> {
        let entries: Vec<PublicationEntry> =
            serde_json::from_slice(&std::fs::read(journal.join("publication.json"))?)?;
        validate_paths(
            &entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
        )?;
        let incomplete = journal.join(".incomplete");
        if incomplete.try_exists()? {
            return Err(std::io::Error::other(format!(
                "Publication image was lost for {}; recovery data was retained",
                String::from_utf8_lossy(&std::fs::read(incomplete)?)
            )));
        }
        check_external_changes(root, journal, &entries)?;
        #[cfg(unix)]
        let root_handle = std::fs::File::open(root)?;
        // The previous generation stays inside the journal until every new
        // image is in place. All moves are within one filesystem, so a large
        // audio file is published with atomic renames instead of being copied
        // over the USB a second time. Every intermediate state is recognizable
        // and recovery always rolls forward.
        for entry in entries {
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
                    std::fs::rename(&previous, &target)?;
                    sync_dir(target.parent().unwrap_or(root))?;
                }
                continue;
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
                        std::fs::rename(&target, &previous)?;
                        sync_dir(target.parent().unwrap_or(root))?;
                    }
                    if let Some(parent) = target.parent() {
                        create_dir_all(parent)?;
                    }
                    std::fs::rename(&image, &target)?;
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
                std::fs::rename(&target, &previous)?;
                sync_dir(target.parent().unwrap_or(root))?;
            }
        }
        // Remove the commit intent atomically BEFORE deleting its images.
        // A crash during cleanup must never replay a missing image as deletion.
        let discarded = tempfile::tempdir_in(root)?;
        std::fs::rename(journal, discarded.path().join("completed"))?;
        if let Err(error) = sync_dir(root) {
            // Until retirement is durable, a crash may resurrect the journal.
            // Keep its images instead of deleting them during TempDir::drop.
            let _retained = discarded.keep();
            return Err(error);
        }
        Ok(())
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
    let published_at = std::fs::metadata(journal.join("publication.json"))?
        .modified()?
        .checked_add(Duration::from_secs(2))
        .ok_or_else(|| std::io::Error::other("Invalid publication timestamp"))?;
    for entry in entries {
        if is_appledouble(&entry.path) {
            continue;
        }
        let target = root.join(&entry.path);
        if !target.try_exists()? || std::fs::metadata(&target)?.modified()? <= published_at {
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
            return Err(std::io::Error::other(format!(
                "Publication conflict at {}; the device changed after sync failed and recovery data was retained",
                entry.path.display()
            )));
        }
    }
    Ok(())
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
        .open(root.join(".rbxport-write.lock"))?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn only_an_unsupported_directory_flush_is_optional() {
        assert!(directory_sync_unsupported(&std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "directory fsync is unsupported",
        )));
        assert!(!directory_sync_unsupported(&std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        )));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_enotsup_from_directory_flush_is_optional() {
        assert!(directory_sync_unsupported(
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
            },
            PublicationEntry {
                path: "Contents/._01".into(),
                present: true,
                had_target: None,
            },
            PublicationEntry {
                path: "Contents/01/._missing.wav".into(),
                present: true,
                had_target: None,
            },
            PublicationEntry {
                path: "PIONEER/rekordbox/exportLibrary.db".into(),
                present: true,
                had_target: None,
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
                },
                PublicationEntry {
                    path: db.into(),
                    present: true,
                    had_target: None,
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
}
