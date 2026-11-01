//! A record of every disk the OS listed and what was decided about it.
//!
//! [`list`](crate::list) only returns the volumes worth offering, so a drive
//! that was skipped, or that was never listed, leaves no trace. This reports
//! the whole set, with the reason for each decision, so a log can answer
//! "was my drive seen, and why was it (not) offered" without changing which
//! drives are offered. It reads only what the OS already reported: no
//! partition scheme, no disk I/O.

use std::path::{Path, PathBuf};

/// One disk as the OS reported it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskEntry {
    pub mount_point: PathBuf,
    pub name: String,
    /// The filesystem name as reported, before any friendlier naming.
    pub file_system: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub removable: bool,
}

/// A disk with the decision made about it. The mount point and name have the
/// home directory replaced by `~`, so the record is safe to put in a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskDecision {
    pub mount_point: String,
    pub name: String,
    pub file_system: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub removable: bool,
    pub offered: bool,
    /// Why it was offered or skipped.
    pub reason: &'static str,
}

impl DiskDecision {
    /// One line for the log. Free space is left out of
    /// [`Self::same_as`] but is useful when read.
    #[must_use]
    pub fn record(&self) -> String {
        format!(
            "mount={:?} name={:?} fs={:?} total_bytes={} free_bytes={} removable={} offered={} reason={}",
            self.mount_point,
            self.name,
            self.file_system,
            self.total_bytes,
            self.free_bytes,
            self.removable,
            self.offered,
            self.reason
        )
    }

    /// Whether two decisions are the same apart from free space, which moves
    /// with every write and would otherwise log on each call.
    #[must_use]
    pub fn same_as(&self, other: &Self) -> bool {
        Self { free_bytes: 0, ..self.clone() } == Self { free_bytes: 0, ..other.clone() }
    }
}

/// Decides each disk and describes it. `home` is redacted from the output.
#[must_use]
pub fn decide(disks: &[DiskEntry], home: Option<&Path>) -> Vec<DiskDecision> {
    let mut decisions: Vec<DiskDecision> = disks
        .iter()
        .map(|disk| {
            let (offered, reason) = match crate::offer_reason(&disk.mount_point, disk.removable) {
                Ok(reason) => (true, reason),
                Err(reason) => (false, reason),
            };
            DiskDecision {
                mount_point: redact(&disk.mount_point.to_string_lossy(), home),
                name: redact(&disk.name, home),
                file_system: disk.file_system.clone(),
                total_bytes: disk.total_bytes,
                free_bytes: disk.free_bytes,
                removable: disk.removable,
                offered,
                reason,
            }
        })
        .collect();
    decisions.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    decisions
}

/// Whether the set of decisions differs from the last one seen.
#[must_use]
pub fn changed(previous: Option<&[DiskDecision]>, current: &[DiskDecision]) -> bool {
    match previous {
        None => true,
        Some(previous) => {
            previous.len() != current.len() || previous.iter().zip(current).any(|(a, b)| !a.same_as(b))
        }
    }
}

/// Writes every decision to the log.
pub fn log(decisions: &[DiskDecision]) {
    if decisions.is_empty() {
        tracing::info!("the OS listed no disks");
    }
    for decision in decisions {
        tracing::info!(disk = %decision.record(), "disk listed");
    }
}

/// Replaces the home directory with `~`.
fn redact(text: &str, home: Option<&Path>) -> String {
    match home.map(|home| home.to_string_lossy()) {
        Some(home) if !home.is_empty() && home != "/" => text.replace(home.as_ref(), "~"),
        _ => text.to_owned(),
    }
}

/// What the OS currently says about the disk holding `path`: the one whose
/// mount point is its longest ancestor. For a failure log, where the volume
/// the write went to is the useful fact.
#[must_use]
pub fn disk_holding(path: &Path) -> Option<DiskDecision> {
    let disks = system_disks();
    let best = disks
        .iter()
        .filter(|disk| path.starts_with(&disk.mount_point))
        .max_by_key(|disk| disk.mount_point.components().count())?;
    decide(std::slice::from_ref(best), dirs::home_dir().as_deref()).pop()
}

/// The disks as the OS lists them now.
#[must_use]
pub fn system_disks() -> Vec<DiskEntry> {
    entries_from(&sysinfo::Disks::new_with_refreshed_list())
}

pub(crate) fn entries_from(disks: &sysinfo::Disks) -> Vec<DiskEntry> {
    disks
        .list()
        .iter()
        .map(|disk| DiskEntry {
            mount_point: disk.mount_point().to_owned(),
            name: disk.name().to_string_lossy().into_owned(),
            file_system: disk.file_system().to_string_lossy().into_owned(),
            total_bytes: disk.total_space(),
            free_bytes: disk.available_space(),
            removable: disk.is_removable(),
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn disk(mount: &str, name: &str, fs: &str, total: u64, removable: bool) -> DiskEntry {
        DiskEntry {
            mount_point: PathBuf::from(mount),
            name: name.into(),
            file_system: fs.into(),
            total_bytes: total,
            free_bytes: total / 2,
            removable,
        }
    }

    const TB: u64 = 1_000_000_000_000;

    fn fixture() -> Vec<DiskEntry> {
        vec![
            disk("/Volumes/EXFAT", "EXFAT", "exfat", TB, false),
            disk("/Volumes/APFS", "APFS", "apfs", TB, false),
            disk("/Volumes/NTFS", "NTFS", "ntfs", TB, false),
            disk("/Volumes/HFS", "HFS", "hfs", 500_000_000_000, true),
            disk("/Volumes/FAT", "FAT", "msdos", 32_000_000_000, true),
            disk("/System/Volumes/Data", "Macintosh HD - Data", "apfs", TB, false),
            disk("/", "Macintosh HD", "apfs", TB, false),
            disk("/mnt/odd", "odd", "ext4", TB, false),
        ]
    }

    fn find<'a>(all: &'a [DiskDecision], mount: &str) -> &'a DiskDecision {
        all.iter().find(|d| d.mount_point == mount).unwrap_or_else(|| panic!("no decision for {mount}"))
    }

    #[test]
    fn every_listed_disk_gets_a_decision_and_a_reason() {
        let all = decide(&fixture(), None);
        assert_eq!(all.len(), 8);
        for (mount, fs, offered, reason) in [
            ("/Volumes/EXFAT", "exfat", true, "mounted under /Volumes"),
            ("/Volumes/APFS", "apfs", true, "mounted under /Volumes"),
            ("/Volumes/NTFS", "ntfs", true, "mounted under /Volumes"),
            ("/Volumes/HFS", "hfs", true, "reported removable"),
            ("/Volumes/FAT", "msdos", true, "reported removable"),
            ("/System/Volumes/Data", "apfs", false, "system or boot volume"),
            ("/", "apfs", false, "system or boot volume"),
            ("/mnt/odd", "ext4", true, "mounted under a Linux mount directory"),
        ] {
            let d = find(&all, mount);
            assert_eq!((d.file_system.as_str(), d.offered, d.reason), (fs, offered, reason), "{mount}");
        }
    }

    #[test]
    fn a_non_removable_disk_outside_any_mount_directory_is_skipped_with_that_reason() {
        let all = decide(&[disk("/data", "data", "ext4", TB, false), disk("C:\\", "OS", "NTFS", TB, false)], None);
        assert!(!all[0].offered && all[0].reason == "not removable and not under a mount directory");
        assert!(!find(&all, "C:\\").offered);
        assert_eq!(find(&all, "C:\\").reason, "system or boot volume");
    }

    #[test]
    fn the_decision_agrees_with_what_is_offered() {
        for d in fixture() {
            let decided = decide(std::slice::from_ref(&d), None).remove(0);
            assert_eq!(decided.offered, crate::is_offerable(&d.mount_point, d.removable), "{:?}", d.mount_point);
        }
    }

    #[test]
    fn the_record_carries_the_filesystem_and_size_and_no_home_path() {
        let home = Path::new("/Users/dj");
        let all = decide(&[disk("/Users/dj/mnt/SSD", "dj's SSD", "exfat", TB, false)], Some(home));
        let record = all[0].record();
        assert!(record.contains("fs=\"exfat\""), "{record}");
        assert!(record.contains("total_bytes=1000000000000"), "{record}");
        assert!(record.contains("free_bytes=500000000000"), "{record}");
        assert!(record.contains("removable=false") && record.contains("offered="), "{record}");
        assert!(!record.contains("/Users/dj"), "{record}");
        assert!(record.contains("~/mnt/SSD"), "{record}");
    }

    #[test]
    fn only_a_change_in_the_set_is_worth_logging() {
        let first = decide(&fixture(), None);
        assert!(changed(None, &first));
        assert!(!changed(Some(&first), &first));

        // Free space moves with every write and is not a change.
        let mut used = fixture();
        used[0].free_bytes -= 1;
        assert!(!changed(Some(&first), &decide(&used, None)));

        // A drive arriving, or reporting another filesystem, is.
        let mut arrived = fixture();
        arrived.push(disk("/Volumes/NEW", "NEW", "exfat", TB, false));
        assert!(changed(Some(&first), &decide(&arrived, None)));
        let mut renamed_fs = fixture();
        renamed_fs[0].file_system = "ntfs".into();
        assert!(changed(Some(&first), &decide(&renamed_fs, None)));
    }
}
