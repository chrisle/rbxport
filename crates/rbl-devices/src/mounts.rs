//! Noticing a stick being plugged in or pulled out.
//!
//! macOS has `DiskArbitration` and Windows `WM_DEVICECHANGE`, but both are
//! reached through foreign calls the workspace forbids as `unsafe`. What both
//! platforms offer safely is a cheap place to look: every non-boot volume on
//! macOS is an entry under `/Volumes`, and on Windows a mounted drive is a
//! letter that answers to `exists`. Reading either costs microseconds, so
//! looking every couple of seconds is far inside the idle budget, and a stick
//! is seen within that long of arriving whether or not the window has focus.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often the watcher looks. Two seconds is quicker than a person reaches
/// for the panel after plugging a stick in, and 26 stats or one `readdir` at
/// that rate is nothing.
pub const INTERVAL: Duration = Duration::from_secs(2);

/// The thread sleeps in slices this long so a stop is prompt.
const SLICE: Duration = Duration::from_millis(250);

/// Where macOS mounts everything that is not the boot volume.
#[cfg(target_os = "macos")]
const VOLUMES: &str = "/Volumes";

/// A running mount watcher; dropping it stops the thread.
pub struct MountWatcher {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MountWatcher {
    /// Starts looking, and calls `changed` whenever the set of mounted
    /// volumes differs from the last look. The first look is the baseline:
    /// nothing is reported for what was already there.
    pub fn start<F>(interval: Duration, mut changed: F) -> Self
    where
        F: FnMut() + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut last = mounts();
                let mut looked = Instant::now();
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(SLICE.min(interval));
                    if looked.elapsed() < interval {
                        continue;
                    }
                    looked = Instant::now();
                    let now = mounts();
                    if now != last {
                        last = now;
                        changed();
                    }
                }
            })
        };
        Self { stop, thread: Some(thread) }
    }

    /// Stops the thread and waits for it.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

impl Drop for MountWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The mounted volumes, as a sorted list so two looks compare directly.
///
/// With [`crate::FAKE_VOLUMES`] set the fakes are the answer, so `pnpm dev`
/// and the tests see a stable set.
#[must_use]
pub fn mounts() -> Vec<PathBuf> {
    if let Some(fake) = std::env::var_os(crate::FAKE_VOLUMES) {
        let mut fakes: Vec<PathBuf> = fake
            .to_string_lossy()
            .split(':')
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .collect();
        fakes.sort();
        return fakes;
    }
    platform_mounts()
}

#[cfg(target_os = "macos")]
fn platform_mounts() -> Vec<PathBuf> {
    entries_of(Path::new(VOLUMES))
}

/// Linux mount points are chosen by the desktop service and are not confined
/// to one directory (`/media`, `/run/media`, or a manually chosen `/mnt`).
/// The disk list already knows which of those are usable export volumes, so
/// use it for the snapshot rather than watching a macOS-only directory.
#[cfg(target_os = "linux")]
fn platform_mounts() -> Vec<PathBuf> {
    super::devices_from(&sysinfo::Disks::new_with_refreshed_list())
        .into_iter()
        .map(|device| device.mount_point)
        .collect()
}

/// The entries of a volumes directory, sorted. Split out so a test can point
/// it at a directory of its own.
fn entries_of(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).collect();
    found.sort();
    found
}

#[cfg(windows)]
fn platform_mounts() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|letter| PathBuf::from(format!("{}:\\", char::from(letter))))
        .filter(|root| root.exists())
        .collect()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform_mounts() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_look_lists_the_volumes_in_a_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("ZED")).unwrap();
        std::fs::create_dir(dir.path().join("ALPHA")).unwrap();
        let looked = entries_of(dir.path());
        assert_eq!(looked, vec![dir.path().join("ALPHA"), dir.path().join("ZED")]);
        // Somewhere that is not there lists nothing rather than failing: a
        // Mac with no volume mounted has no `/Volumes` entries either.
        assert!(entries_of(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn the_watcher_reports_only_a_change_and_stops_when_dropped() {
        // Driven through the same comparison the thread makes, without the
        // real `/Volumes`: a change is a differing snapshot, nothing more.
        let dir = tempfile::tempdir().unwrap();
        let before = entries_of(dir.path());
        std::fs::create_dir(dir.path().join("DJ STICK")).unwrap();
        let after = entries_of(dir.path());
        assert_ne!(before, after);
        assert_eq!(after, entries_of(dir.path()));

        let watcher = MountWatcher::start(Duration::from_millis(50), || {});
        std::thread::sleep(Duration::from_millis(120));
        watcher.stop();
    }
}
