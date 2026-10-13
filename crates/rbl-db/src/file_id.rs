//! `djmdContent.rb_file_id`: the file system's own number for a track's file.
//!
//! [OBS rekordbox 7.2.19 macOS arm64, static, issue #201] rekordbox fills the
//! column with `juce::File::getFileIdentifier` of the track's file when it
//! imports one (`db::DatabaseMediator::importFile`, written by
//! `db::AppSyncDBController::insertTrack` @0x100a58200) and again when it
//! points a track at another file (`modifyContentFilePath`
//! @0x100a69280..0x100a6929c), as a decimal string. Auto Relocate reads it
//! back to find a file that moved on the same volume
//! (`common::getFilePathFromID` @0x101e42610).
//!
//! `getFileIdentifier` is the inode number on macOS and Linux and the NTFS
//! file index on Windows (`nFileIndexHigh << 32 | nFileIndexLow`), 0 when the
//! file cannot be read. [ASSUME] JUCE's own source; rekordbox links JUCE.

use std::path::Path;

/// `juce::File::getFileIdentifier` of `path`: 0 when it cannot be read.
pub fn of(path: &Path) -> u64 {
    platform(path).unwrap_or(0)
}

/// The value rekordbox stores: the identifier as a signed 64-bit decimal,
/// as `juce::String (long long)` writes it.
pub fn column(path: &Path) -> String {
    i64::from_ne_bytes(of(path).to_ne_bytes()).to_string()
}

/// A stored `rb_file_id` read back as rekordbox does
/// (`juce::String::getLargeIntValue`): the leading number, 0 for anything
/// else.
pub fn parse(stored: &str) -> u64 {
    let text = stored.trim_start();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    let Ok(value) = digits[..end].parse::<i64>() else { return 0 };
    let value = if negative { -value } else { value };
    u64::from_ne_bytes(value.to_ne_bytes())
}

#[cfg(unix)]
fn platform(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    // perf-ok: one `stat` per file imported or relocated.
    std::fs::metadata(path).ok().map(|meta| meta.ino())
}

#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "the NTFS file index is only reachable through GetFileInformationByHandle; the handle is owned by a File"
)]
fn platform(path: &Path) -> Option<u64> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
    };
    // Opened for reading with backup semantics, as JUCE opens it, so a
    // folder works too; the handle closes when `file` drops.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `file` owns a valid handle for this call and `info` is a
    // properly sized, writable structure.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut info) };
    (ok != 0).then(|| (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow))
}

#[cfg(not(any(unix, windows)))]
fn platform(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_file_keeps_its_identifier_when_it_moves_on_the_same_volume() {
        let dir = tempfile::tempdir().unwrap();
        let before = dir.path().join("a.mp3");
        std::fs::write(&before, b"x").unwrap();
        let id = of(&before);
        assert_ne!(id, 0);
        std::fs::create_dir_all(dir.path().join("deeper")).unwrap();
        let after = dir.path().join("deeper/a.mp3");
        std::fs::rename(&before, &after).unwrap();
        assert_eq!(of(&after), id);
        assert_eq!(column(&after), id.to_string());
    }

    #[test]
    fn nothing_there_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(of(&dir.path().join("gone.mp3")), 0);
        assert_eq!(column(&dir.path().join("gone.mp3")), "0");
    }

    #[test]
    fn a_stored_value_reads_back_as_rekordbox_reads_it() {
        assert_eq!(parse("80430616"), 80_430_616);
        assert_eq!(parse(" 12abc"), 12);
        assert_eq!(parse(""), 0);
        assert_eq!(parse("x"), 0);
        assert_eq!(parse("-1"), u64::MAX);
    }
}
