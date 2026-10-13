//! The USB stick a test exports to.
//!
//! By default a temporary directory on the local disk. With
//! `RBX_USB_STICK=/Volumes/<name>`, a temporary directory on that stick
//! instead, so each test that uses [`stick`] writes through the stick's own
//! filesystem and driver. The stick must hold a `.rbxport-disposable` file at
//! its root, which says it may be written to; without it the test refuses.
//! Each directory is named `.rbxport-test-*` and removed when the test ends.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::path::PathBuf;

/// Names the stick to export to, as its mount point.
pub const STICK_ENV: &str = "RBX_USB_STICK";
/// The file on a real stick that says it may be written to.
pub const DISPOSABLE_MARKER: &str = ".rbxport-disposable";

/// A fresh, empty directory standing for a USB stick.
pub fn stick() -> tempfile::TempDir {
    let Some(mount) = std::env::var_os(STICK_ENV).map(PathBuf::from) else {
        return tempfile::tempdir().unwrap();
    };
    assert!(
        mount.join(DISPOSABLE_MARKER).is_file(),
        "refusing to write to {}: it has no {DISPOSABLE_MARKER}. Create that file on the stick to confirm \
         rbxport's tests may write there.",
        mount.display()
    );
    tempfile::Builder::new()
        .prefix(".rbxport-test-")
        .tempdir_in(&mount)
        .unwrap_or_else(|e| panic!("could not make a test directory on {}: {e}", mount.display()))
}

/// Whether `name` is a `._` sidecar, where macOS keeps a file's or folder's
/// extended attributes on FAT32 and exFAT. It is the OS's, not the export's,
/// and appears on one stick and not another, so listings leave it out.
pub fn is_os_metadata(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with("._")
}
