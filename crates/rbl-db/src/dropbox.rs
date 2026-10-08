//! This machine's Dropbox folder, found the way rekordbox finds it.
//!
//! [OBS rekordbox 7.2.19 macOS arm64, static] `DropBox::localPublicPath`
//! @0x10102cdc0 returns a cached folder, or `checklocalPublicPath`
//! @0x10102ce14 (to 0x10102d14c), which:
//!
//! 1. reads the `DropboxSharingPath` setting from `rekordbox3.settings`;
//! 2. if it is empty or names nothing that exists (`juce::File::exists`),
//!    takes `localPersonalPath`, else `localBusinessPath`;
//! 3. if it exists but `rb::FileHelper::isSymlink` @0x10240e79c holds (the
//!    real path, `juce_getRealPath`, is absolute and is not the same file
//!    name), takes `localBusinessPath`, else `localPersonalPath`, else
//!    nothing;
//! 4. keeps the result only if it is an absolute path, then caches it and
//!    writes it back to `DropboxSharingPath` (`setStringValue` @0x10102d0cc).
//!
//! `localPersonalPath` / `localBusinessPath` (@0x10102d478 / @0x10102d2b8)
//! call `get_local_public_path` @0x10102d654, which parses
//! `<home>/.dropbox/info.json` (`getSpecialLocation(userHomeDirectory)`) and
//! returns `personal.path` / `business.path` when it is an absolute path.
//! That file is the Dropbox desktop app's own record of its folders.
//!
//! rbxport finds the same folder but never writes it back: rekordbox's
//! settings stay rekordbox's. [ASSUME] Step 4 also replaces the result with
//! the path the file system reports for its file id when the two name the
//! same file under different letter case (`getFileIdentifier`,
//! `getFilePathFromID`); that is not modelled, as every comparison made
//! with the folder ignores case. [UNKNOWN] What the Windows build reads:
//! only the macOS binary was read, and the same `<home>/.dropbox/info.json`
//! is used on every platform here.

use std::path::{Path, PathBuf};

/// The Dropbox desktop app's folders as `~/.dropbox/info.json` records
/// them: absolute paths only, as `get_local_public_path` keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropboxInfo {
    pub personal: Option<String>,
    pub business: Option<String>,
}

impl DropboxInfo {
    /// Parses the text of `info.json`. Anything unreadable is "no folder".
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
            return Self::default();
        };
        let path = |account: &str| {
            json.get(account)
                .and_then(|a| a.get("path"))
                .and_then(serde_json::Value::as_str)
                .filter(|p| Path::new(p).is_absolute())
                .map(str::to_owned)
        };
        Self { personal: path("personal"), business: path("business") }
    }

    /// Reads `<home>/.dropbox/info.json`; no file is no folder.
    #[must_use]
    pub fn read(home: &Path) -> Self {
        std::fs::read_to_string(info_json(home)).map(|t| Self::parse(&t)).unwrap_or_default()
    }
}

/// `<home>/.dropbox/info.json`.
#[must_use]
pub fn info_json(home: &Path) -> PathBuf {
    home.join(".dropbox").join("info.json")
}

/// `DropBox::checklocalPublicPath` without the write-back: the folder
/// rekordbox uses for Dropbox given the `DropboxSharingPath` setting and
/// Dropbox's own record. `None` when there is none.
#[must_use]
pub fn check_local_public_path(setting: &str, info: &DropboxInfo) -> Option<String> {
    let personal = info.personal.as_deref().filter(|p| !p.is_empty());
    let business = info.business.as_deref().filter(|p| !p.is_empty());
    let chosen = if setting.is_empty() || !Path::new(setting).exists() {
        personal.or(business)
    } else if is_symlink(Path::new(setting)) {
        business.or(personal)
    } else {
        Some(setting)
    };
    chosen.filter(|p| Path::new(p).is_absolute()).map(str::to_owned)
}

/// `rb::FileHelper::isSymlink`: the path's real path is absolute and names
/// another file than the path itself. juce compares file names ignoring
/// case on macOS and Windows, and drops a trailing separator.
fn is_symlink(path: &Path) -> bool {
    let Ok(real) = std::fs::canonicalize(path) else {
        return false;
    };
    if !real.is_absolute() {
        return false;
    }
    let trim = |p: &Path| p.to_string_lossy().trim_end_matches(['/', '\\']).to_owned();
    let (real, path) = (trim(&real), trim(path));
    if cfg!(any(target_os = "macos", windows)) {
        real.to_lowercase() != path.to_lowercase()
    } else {
        real != path
    }
}

/// This machine's Dropbox folder as rekordbox would use it: from
/// `rekordbox3.settings` and `~/.dropbox/info.json`. Not cached.
#[must_use]
pub fn local_public_path() -> Option<String> {
    let setting = rbl_core::paths::rekordbox_setting("DropboxSharingPath").unwrap_or_default();
    let info = dirs::home_dir().map(|home| DropboxInfo::read(&home)).unwrap_or_default();
    check_local_public_path(&setting, &info)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn info(personal: Option<&Path>, business: Option<&Path>) -> DropboxInfo {
        let s = |p: Option<&Path>| p.map(|p| p.to_string_lossy().into_owned());
        DropboxInfo { personal: s(personal), business: s(business) }
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn info_json_gives_absolute_personal_and_business_paths() {
        let text = r#"{"personal": {"path": "/Users/dj/Library/CloudStorage/Dropbox", "host": 1},
                       "business": {"path": "relative/Team"}}"#;
        let parsed = DropboxInfo::parse(text);
        assert_eq!(parsed.personal.as_deref(), Some("/Users/dj/Library/CloudStorage/Dropbox"));
        assert_eq!(parsed.business, None, "a relative path is not a folder");
        assert_eq!(DropboxInfo::parse("not json"), DropboxInfo::default());
        assert_eq!(DropboxInfo::parse(r#"{"personal": {}}"#), DropboxInfo::default());
    }

    #[test]
    fn info_json_is_read_from_the_home_folder() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(DropboxInfo::read(home.path()), DropboxInfo::default());
        std::fs::create_dir_all(home.path().join(".dropbox")).unwrap();
        std::fs::write(info_json(home.path()), r#"{"business": {"path": "/Volumes/Team Dropbox"}}"#).unwrap();
        assert_eq!(DropboxInfo::read(home.path()).business.as_deref(), Some("/Volumes/Team Dropbox"));
    }

    #[test]
    fn an_existing_plain_setting_wins() {
        let dir = tempfile::tempdir().unwrap();
        let setting = dir.path().canonicalize().unwrap();
        let other = info(Some(Path::new("/elsewhere/personal")), Some(Path::new("/elsewhere/business")));
        assert_eq!(check_local_public_path(&s(&setting), &other), Some(s(&setting)));
    }

    #[test]
    fn an_empty_or_missing_setting_falls_back_to_personal_then_business() {
        let both = info(Some(Path::new("/p/Dropbox")), Some(Path::new("/b/Dropbox (Team)")));
        let business_only = info(None, Some(Path::new("/b/Dropbox (Team)")));
        // The reporter's likely case: rekordbox never stored a folder.
        assert_eq!(check_local_public_path("", &both).as_deref(), Some("/p/Dropbox"));
        assert_eq!(check_local_public_path("", &business_only).as_deref(), Some("/b/Dropbox (Team)"));
        // A stale setting is treated like none.
        let gone = "/definitely/not/a/folder/Dropbox";
        assert_eq!(check_local_public_path(gone, &both).as_deref(), Some("/p/Dropbox"));
        // No setting and no Dropbox app: no folder.
        assert_eq!(check_local_public_path("", &DropboxInfo::default()), None);
        assert_eq!(check_local_public_path(gone, &DropboxInfo::default()), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_setting_is_detected_again_business_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let real = root.join("CloudStorage/Dropbox");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.join("Dropbox");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let both = info(Some(&real), Some(Path::new("/b/Team")));
        assert_eq!(check_local_public_path(&s(&link), &both).as_deref(), Some("/b/Team"));
        assert_eq!(check_local_public_path(&s(&link), &info(Some(&real), None)), Some(s(&real)));
        // A symlink and no Dropbox record: rekordbox clears the folder.
        assert_eq!(check_local_public_path(&s(&link), &DropboxInfo::default()), None);
        // A folder under a symlinked parent counts too: its real path differs.
        let inner = link.join("rekordbox");
        std::fs::create_dir_all(&inner).unwrap();
        assert!(is_symlink(&inner));
        assert!(!is_symlink(&real));
    }
}
