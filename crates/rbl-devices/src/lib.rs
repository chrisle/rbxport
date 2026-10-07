//! Removable media the app can export to, and what is already on it.
//!
//! Two jobs, kept apart because they cost very different amounts:
//!
//! - [`list`] enumerates volumes. It touches only what the OS already knows,
//!   so it is cheap enough to run whenever the panel is opened.
//! - [`inspect`] reads a stick to see what export it holds. That is disk I/O
//!   over USB, so it happens per device, on request, never in a loop.
//!
//! A third, [`MountWatcher`], notices a volume arriving or leaving so the
//! panel can refresh itself without waiting for focus or a click.
//!
//! [`libraries`] finds rekordbox libraries kept on a connected drive, where
//! rekordbox's own Database management looks for them.

pub mod settings;
pub mod explorer;
pub mod mounts;
pub mod eject;
pub mod libraries;

pub use mounts::MountWatcher;

use std::path::{Path, PathBuf};

/// Set to a `:`-separated list of directories to use those instead of the real
/// volumes. Tests and `pnpm dev` use it; nothing else should.
pub const FAKE_VOLUMES: &str = "RB_LITE_FAKE_VOLUMES";

/// A volume an export could be written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// What to show: the volume's name, e.g. `DJ STICK`.
    pub name: String,
    pub mount_point: PathBuf,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub file_system: String,
    /// Whether the OS calls it removable. External SSDs often say no, so this
    /// is shown, not used to decide what to list.
    pub removable: bool,
    /// A name for the medium that a rename does not change; see [`volume_id`].
    pub volume_id: String,
}

impl Device {
    /// Bytes in use, for a capacity bar.
    #[must_use]
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }
}

/// What a stick already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceExport {
    pub tracks: usize,
    pub playlists: usize,
    /// True when we wrote it, which is what makes a sync incremental. A stick
    /// rekordbox wrote is exported to in full.
    pub ours: bool,
    /// When our own export last ran, empty when this is not one of ours.
    pub written: String,
}

/// Lists the volumes worth offering as an export destination.
#[must_use]
pub fn list() -> Vec<Device> {
    if let Some(fake) = std::env::var_os(FAKE_VOLUMES) {
        return fake
            .to_string_lossy()
            .split(':')
            .filter(|path| !path.is_empty())
            .map(|path| fake_device(Path::new(path)))
            .collect();
    }

    devices_from(&sysinfo::Disks::new_with_refreshed_list())
}

/// The offerable volumes among what one refresh of the disk list found.
///
/// Split from [`list`] so the Explorer can name the system volume from the
/// same refresh: one costs tens of milliseconds, and up to seconds while a
/// card reader wakes [OBS].
fn devices_from(disks: &sysinfo::Disks) -> Vec<Device> {
    let mut devices: Vec<Device> = disks
        .list()
        .iter()
        .filter(|disk| is_offerable(disk.mount_point(), disk.is_removable()))
        .map(|disk| Device {
            name: display_name(disk.mount_point(), &disk.name().to_string_lossy()),
            mount_point: disk.mount_point().to_owned(),
            total_bytes: disk.total_space(),
            free_bytes: disk.available_space(),
            file_system: filesystem_name(disk.mount_point(), &disk.file_system().to_string_lossy()),
            removable: disk.is_removable(),
            volume_id: volume_id(disk.mount_point()),
        })
        .collect();
    // The same volume can be reported twice when it is mounted more than once.
    devices.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    devices.dedup_by(|a, b| a.mount_point == b.mount_point);
    devices
}

/// A directory standing in for a stick, with the free space of whatever it
/// actually sits on left unknown.
fn fake_device(path: &Path) -> Device {
    Device {
        name: display_name(path, "Volume"),
        mount_point: path.to_owned(),
        total_bytes: 0,
        free_bytes: 0,
        file_system: String::new(),
        removable: true,
        volume_id: volume_id(path),
    }
}

/// What still names a stick after the user renames it.
///
/// On macOS a volume's mount point is its name, so renaming `USB A` to
/// `USB B` moves it to `/Volumes/USB B` and every path the app holds for it
/// goes stale. The filesystem underneath does not move: its device number
/// (`st_dev`) is the same before and after, so that is the identity, and a
/// device list taken after the rename can be matched to the one before it.
/// It is not stable across an unplug, which is right — a stick that was
/// pulled and pushed back in is looked at afresh. On Windows a rename
/// changes only the label; the drive letter stays and is the identity.
#[must_use]
pub fn volume_id(mount_point: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(mount_point) {
            return format!("dev:{}", meta.dev());
        }
    }
    format!("path:{}", mount_point.to_string_lossy())
}

/// Whether a volume should be offered as somewhere to export to.
///
/// `is_removable` alone is not enough: an external SSD, which is what a lot of
/// people actually carry, reports false on every platform. The mount point is
/// the better signal — macOS puts every non-boot volume under `/Volumes`,
/// Linux desktop mount services use `/media` or `/run/media`, and on Windows
/// anything that is not the system drive is a separate letter.
fn is_offerable(mount_point: &Path, removable: bool) -> bool {
    if removable {
        return true;
    }
    let text = mount_point.to_string_lossy();
    if text == "/" || text.starts_with("/System") || text.starts_with("/private") {
        return false;
    }
    text.starts_with("/Volumes/")
        || is_linux_mount_point(&text)
        || (is_drive_root(&text) && !text.starts_with('C'))
}

/// The paths used by the common Linux desktop mount services. `/mnt` is also
/// conventional for a volume mounted explicitly by its owner.
fn is_linux_mount_point(path: &str) -> bool {
    path.starts_with("/media/") || path.starts_with("/run/media/") || path.starts_with("/mnt/")
}

/// The name to show. A mount point's last component is the volume name on
/// macOS; on Windows it is a bare drive letter, so the disk's own name is
/// better there.
fn display_name(mount_point: &Path, fallback: &str) -> String {
    let text = mount_point.to_string_lossy();
    // Recognised by shape rather than by `Path`, which only treats a backslash
    // as a separator on Windows — so a Mac reading a recorded Windows mount
    // point would otherwise call the volume `E:\`.
    let name = if is_drive_root(&text) {
        None
    } else {
        mount_point.file_name().map(|n| n.to_string_lossy().into_owned())
    };
    name.filter(|name| !name.is_empty()).unwrap_or_else(|| {
        if fallback.is_empty() { text.into_owned() } else { fallback.to_owned() }
    })
}

/// `C:`, `E:\` and the like.
fn is_drive_root(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(
        (chars.next(), chars.next(), chars.next(), chars.next()),
        (Some(c), Some(':'), None | Some('\\' | '/'), None) if c.is_ascii_alphabetic()
    )
}

/// Reads what is already exported to a volume.
///
/// Returns `None` when the volume holds no export at all. Our own manifest is
/// preferred over the database: it is a few kilobytes rather than a few
/// megabytes, and it is the thing that says whether a sync can be incremental.
#[must_use]
pub fn inspect(mount_point: &Path) -> Option<DeviceExport> {
    // Discovery is read-only. Recover pending publications only during an explicit operation.
    // `PIONEER` or `.PIONEER`: rekordbox 7 can write the export hidden.
    let pdb = settings::export_root(mount_point).join("rekordbox/export.pdb");
    if !pdb.is_file() {
        return None;
    }

    if let Some(manifest) = rbl_export::Manifest::load(mount_point) {
        // Playlists are not in the manifest — it exists to decide what to
        // copy — so they still come from the database.
        return Some(DeviceExport {
            tracks: manifest.tracks.len(),
            playlists: count_playlists(&pdb),
            ours: true,
            written: manifest.written,
        });
    }

    let bytes = std::fs::read(&pdb).ok()?;
    let parsed = rbl_pdb::Pdb::parse(&bytes).ok()?;
    Some(DeviceExport {
        tracks: parsed
            .table(rbl_pdb::PageType::Tracks)
            .map_or(0, |table| parsed.track_rows(table).len()),
        playlists: parsed
            .table(rbl_pdb::PageType::PlaylistTree)
            .map_or(0, |table| parsed.playlist_nodes(table).len()),
        ours: false,
        written: String::new(),
    })
}

fn count_playlists(pdb: &Path) -> usize {
    let Ok(bytes) = std::fs::read(pdb) else { return 0 };
    let Ok(parsed) = rbl_pdb::Pdb::parse(&bytes) else { return 0 };
    parsed
        .table(rbl_pdb::PageType::PlaylistTree)
        .map_or(0, |table| parsed.playlist_nodes(table).len())
}

fn filesystem_name(path: &Path, raw: &str) -> String {
    #[cfg(target_os = "macos")]
    if matches!(raw.to_ascii_lowercase().as_str(), "msdos" | "fat") {
        if let Some(fat) = volume_type(path).as_deref().and_then(fat_variant) {
            return fat.to_owned();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
    raw.to_owned()
}

/// `FAT32` from Disk Arbitration's `MS-DOS (FAT32)`: which FAT a stick is,
/// where the mount itself only says `msdos`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn fat_variant(volume_type: &str) -> Option<&str> {
    volume_type.strip_prefix("MS-DOS (")?.strip_suffix(')')
}

/// The volume's type as Disk Utility names it, such as `MS-DOS (FAT32)`.
///
/// Asked of Disk Arbitration directly, which answers in well under a
/// millisecond. `diskutil info` gives the same name but takes 100 to 200 ms
/// a stick, and the Sync Manager waits for the device list before it opens.
#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "Disk Arbitration is a C framework; each object it returns is held by a CFRetained and released on drop"
)]
fn volume_type(path: &Path) -> Option<String> {
    use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType, CFURL};
    use objc2_disk_arbitration::{kDADiskDescriptionVolumeTypeKey, DADisk, DASession};

    let url = CFURL::from_directory_path(path)?;
    // SAFETY: the default allocator; the session is released when dropped.
    let session = unsafe { DASession::new(None) }?;
    // SAFETY: a live session and a file URL; returns a new, owned disk.
    let disk = unsafe { DADisk::from_volume_path(None, &session, &url) }?;
    // SAFETY: a live disk; returns a new, owned dictionary.
    let description = unsafe { disk.description() }?;
    // SAFETY: Disk Arbitration's description is keyed by CFString constants,
    // and its values are CF objects of whatever type each key documents.
    let description: CFRetained<CFDictionary<CFString, CFType>> =
        unsafe { CFRetained::cast_unchecked(description) };
    // SAFETY: an immutable CFString constant the framework exports.
    let key = unsafe { kDADiskDescriptionVolumeTypeKey };
    let value = description.get(key)?.downcast::<CFString>().ok()?;
    Some(value.to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn fat_variant_is_read_from_the_volume_type() {
        assert_eq!(fat_variant("MS-DOS (FAT32)"), Some("FAT32"));
        assert_eq!(fat_variant("MS-DOS (FAT16)"), Some("FAT16"));
        assert_eq!(fat_variant("ExFAT"), None);
        assert_eq!(fat_variant("Mac OS Extended (Journaled)"), None);
    }

    #[test]
    fn the_boot_volume_is_never_an_export_destination() {
        assert!(!is_offerable(Path::new("/"), false));
        assert!(!is_offerable(Path::new("/System/Volumes/Data"), false));
        assert!(!is_offerable(Path::new("/private/var/vm"), false));
        assert!(!is_offerable(Path::new("C:\\"), false));
    }

    #[test]
    fn an_external_volume_is_offered_even_when_it_says_it_is_not_removable() {
        // Which is what an external SSD reports, and people do carry those.
        assert!(is_offerable(Path::new("/Volumes/SAMSUNG T7"), false));
        assert!(is_offerable(Path::new("/media/chris/SAMSUNG T7"), false));
        assert!(is_offerable(Path::new("/run/media/chris/SAMSUNG T7"), false));
        assert!(is_offerable(Path::new("/mnt/SAMSUNG T7"), false));
        assert!(is_offerable(Path::new("E:\\"), false));
        // And anything the OS does call removable, wherever it is mounted.
        assert!(is_offerable(Path::new("/mnt/stick"), true));
    }

    #[test]
    fn the_name_shown_is_the_volume_name_where_there_is_one() {
        assert_eq!(display_name(Path::new("/Volumes/DJ STICK"), "disk4s1"), "DJ STICK");
        // A bare drive letter has no last component worth showing.
        assert_eq!(display_name(Path::new("E:\\"), "PIONEER"), "PIONEER");
        assert_eq!(display_name(Path::new("/"), ""), "/");
    }

    #[test]
    fn a_volume_with_nothing_on_it_holds_no_export() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(inspect(dir.path()), None);
    }

    #[test]
    fn inspecting_a_device_does_not_recover_a_pending_export() {
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join(".rbxport-publication");
        std::fs::create_dir(&journal).unwrap();
        std::fs::write(journal.join("publication.json"), br#"[{"path":"track.wav","present":true}]"#).unwrap();
        std::fs::write(journal.join("track.wav"), b"staged audio").unwrap();
        assert_eq!(inspect(dir.path()), None);
        assert!(!dir.path().join("track.wav").exists());
        assert!(journal.join("track.wav").exists());
        assert!(!dir.path().join(".rbxport-write.lock").exists());
    }

    #[test]
    fn fake_volumes_stand_in_for_real_ones() {
        let dir = tempfile::tempdir().unwrap();
        let stick = dir.path().join("DJ STICK");
        std::fs::create_dir_all(&stick).unwrap();
        // A process-wide write; this is the only test in the crate that reads
        // the variable, so nothing else can see it half-set.
        std::env::set_var(FAKE_VOLUMES, &stick);
        let devices = list();
        std::env::remove_var(FAKE_VOLUMES);

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "DJ STICK");
        assert_eq!(devices[0].mount_point, stick);
    }

    #[test]
    fn used_space_never_goes_negative() {
        let device = Device {
            name: "x".into(),
            mount_point: PathBuf::new(),
            total_bytes: 100,
            free_bytes: 400,
            file_system: String::new(),
            volume_id: String::new(),
            removable: true,
        };
        assert_eq!(device.used_bytes(), 0);
    }
}
