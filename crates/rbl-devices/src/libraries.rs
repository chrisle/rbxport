//! rekordbox libraries kept on a connected drive.
//!
//! rekordbox keeps a library on another drive in `PIONEER/Master` at the
//! drive's root, or `.PIONEER/Master` on an HFS-formatted drive
//! (`getMasterDbDirectoryPath`, with `getDriveFileSystemType` answering 4 for
//! a filesystem label starting `HFS`). Its Preferences › Advanced › Database
//! management Drive list offers exactly the drives that already hold a
//! `master.db` there [OBS macOS 7.2.11, static analysis]. This looks in the
//! same two places on every mounted volume, and only looks: nothing is
//! opened, so a drive's library is not touched until one is chosen.
//!
//! Both folders are checked on every drive rather than the one rekordbox
//! would pick for its filesystem, so a drive moved between machines is still
//! found; a drive holding both lists both [ASSUME: rare enough not to need
//! rekordbox's filesystem rule to settle it].

use std::path::{Path, PathBuf};

/// Where on a drive rekordbox keeps a library, relative to its root.
pub const MASTER_DIRS: [&str; 2] = ["PIONEER/Master", ".PIONEER/Master"];

/// A library found on a drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveLibrary {
    /// The drive's name, as the export panel shows it.
    pub name: String,
    /// Where the drive is mounted.
    pub volume: PathBuf,
    /// Its `master.db`.
    pub master_db: PathBuf,
}

/// The libraries on the drives mounted now, in drive order.
#[must_use]
pub fn discover() -> Vec<DriveLibrary> {
    let mut found = discover_in(&volumes());
    // A Windows drive letter carries no name of its own; its label is in the
    // disk list. That list is refreshed only when a library is on such a
    // drive, as one refresh can take seconds while a card reader wakes.
    if found.iter().any(|library| crate::is_drive_root(&library.volume.to_string_lossy())) {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let labels: Vec<(PathBuf, String)> = disks
            .list()
            .iter()
            .map(|disk| (disk.mount_point().to_owned(), disk.name().to_string_lossy().into_owned()))
            .collect();
        label(&mut found, &labels);
    }
    found
}

/// Names each library on a drive root after the drive's label, where the
/// label is known and not empty.
fn label(found: &mut [DriveLibrary], labels: &[(PathBuf, String)]) {
    for library in found {
        if let Some((_, name)) = labels.iter().find(|(mount, name)| *mount == library.volume && !name.is_empty()) {
            library.name = crate::display_name(&library.volume, name);
        }
    }
}

/// The libraries on the given volumes, each named after its mount point's
/// folder, or the drive itself (`E:\`) for a drive root.
#[must_use]
pub fn discover_in(volumes: &[PathBuf]) -> Vec<DriveLibrary> {
    let mut found: Vec<DriveLibrary> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for volume in volumes {
        for master_db in on_volume(volume) {
            // `/Volumes/Macintosh HD` and the like are links to a volume
            // that is listed already.
            let identity = master_db.canonicalize().unwrap_or_else(|_| master_db.clone());
            if seen.contains(&identity) {
                continue;
            }
            seen.push(identity);
            found.push(DriveLibrary { name: crate::display_name(volume, ""), volume: volume.clone(), master_db });
        }
    }
    found
}

/// The `master.db` files at a volume's rekordbox library folders.
#[must_use]
pub fn on_volume(volume: &Path) -> Vec<PathBuf> {
    MASTER_DIRS
        .iter()
        .map(|dir| volume.join(dir).join("master.db"))
        .filter(|master_db| master_db.is_file())
        .collect()
}

/// Every place a drive may be mounted: what the mount watcher sees, and on
/// Linux the folders in the desktop mount locations as well. The disk list
/// behind the watcher skips network mounts (`cifs`, `nfs`) and anything it
/// cannot `statvfs` [OBS sysinfo 0.32 source], and a library on a share
/// mounted under `/mnt` is still one rekordbox users keep. A hung network
/// mount can make a look here slow; callers run it off the interface thread.
fn volumes() -> Vec<PathBuf> {
    let mut volumes = crate::mounts::mounts();
    if std::env::var_os(crate::FAKE_VOLUMES).is_none() {
        volumes.extend(linux_mount_folders());
    }
    volumes.sort();
    volumes.dedup();
    volumes
}

#[cfg(target_os = "linux")]
fn linux_mount_folders() -> Vec<PathBuf> {
    let user = std::env::var_os("USER")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().and_then(|home| home.file_name().map(PathBuf::from)));
    let mut parents = vec![PathBuf::from("/media"), PathBuf::from("/mnt")];
    if let Some(user) = user {
        parents.push(Path::new("/media").join(&user));
        parents.push(Path::new("/run/media").join(&user));
    }
    parents.iter().flat_map(|parent| subfolders(parent)).collect()
}

#[cfg(not(target_os = "linux"))]
fn linux_mount_folders() -> Vec<PathBuf> {
    Vec::new()
}

/// The folders directly in `parent`; nothing when it cannot be read.
#[cfg(any(target_os = "linux", test))]
fn subfolders(parent: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else { return Vec::new() };
    entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn library(volume: &Path, dir: &str) -> PathBuf {
        let master_db = volume.join(dir).join("master.db");
        std::fs::create_dir_all(master_db.parent().unwrap()).unwrap();
        std::fs::write(&master_db, b"a library").unwrap();
        master_db
    }

    #[test]
    fn libraries_in_pioneer_and_hidden_pioneer_folders_are_found() {
        let root = tempfile::tempdir().unwrap();
        let usb = root.path().join("USB A");
        let hfs = root.path().join("DJ SSD");
        let empty = root.path().join("Photos");
        std::fs::create_dir_all(&empty).unwrap();
        let on_usb = library(&usb, "PIONEER/Master");
        let on_hfs = library(&hfs, ".PIONEER/Master");

        let found = discover_in(&[hfs.clone(), empty, usb.clone()]);
        assert_eq!(
            found,
            vec![
                DriveLibrary { name: "DJ SSD".into(), volume: hfs, master_db: on_hfs },
                DriveLibrary { name: "USB A".into(), volume: usb, master_db: on_usb },
            ]
        );
    }

    #[test]
    fn an_export_or_a_stray_folder_is_not_a_library() {
        let root = tempfile::tempdir().unwrap();
        let stick = root.path().join("STICK");
        // A USB export holds `PIONEER/rekordbox/export.pdb`, not a library.
        std::fs::create_dir_all(stick.join("PIONEER/rekordbox")).unwrap();
        std::fs::write(stick.join("PIONEER/rekordbox/export.pdb"), b"pdb").unwrap();
        // A folder named master.db is not a database file.
        std::fs::create_dir_all(stick.join(".PIONEER/Master/master.db")).unwrap();
        assert_eq!(discover_in(&[stick]), []);
    }

    #[test]
    fn a_drive_root_is_named_after_its_label_or_its_letter() {
        let found = |volume: &str| DriveLibrary {
            name: crate::display_name(Path::new(volume), ""),
            volume: PathBuf::from(volume),
            master_db: Path::new(volume).join("PIONEER/Master/master.db"),
        };
        let mut libraries = [found("E:\\"), found("F:\\"), found("G:\\")];
        assert_eq!(libraries[0].name, "E:\\", "unlabelled, the drive is named by its letter");
        label(
            &mut libraries,
            &[(PathBuf::from("E:\\"), "DJ SSD".into()), (PathBuf::from("F:\\"), String::new())],
        );
        let names: Vec<&str> = libraries.iter().map(|library| library.name.as_str()).collect();
        assert_eq!(names, ["DJ SSD", "F:\\", "G:\\"]);
    }

    #[test]
    fn a_volume_listed_twice_is_found_once() {
        let root = tempfile::tempdir().unwrap();
        let usb = root.path().join("USB A");
        library(&usb, "PIONEER/Master");
        assert_eq!(discover_in(&[usb.clone(), usb]).len(), 1);
    }

    #[test]
    fn a_volume_that_cannot_be_read_holds_nothing() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(discover_in(&[root.path().join("gone")]), []);
        assert_eq!(subfolders(&root.path().join("gone")), [] as [PathBuf; 0]);
    }
}
