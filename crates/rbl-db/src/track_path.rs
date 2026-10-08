//! Where a track's file is, read the way rekordbox reads it.
//!
//! A track shared through rekordbox's Cloud Library Sync keeps a cloud path
//! (`/contents_<id>/…`) in `FolderPath` and the file's place on the machine
//! that uploaded it in `OrgFolderPath`. On that machine rekordbox plays and
//! shows `OrgFolderPath`; elsewhere it looks in the cloud service's folder.
//!
//! [OBS rekordbox 7.2.19 macOS arm64, static]
//! `AppSyncDBController::set_content_data` @0x100a80318 copies `FolderPath`
//! into the track's working path (`RowDataTrack+0x98`) and through
//! `replaceDrivePath`, keeps `OrgFolderPath` (+0x488), `ContentLink`
//! (+0x400), `ServiceID` (+0x4b8) and `DeviceID` (+0x4c0), then calls
//! `set_cloud_track_cache` @0x100a44abc. That only acts when `ContentLink`
//! has bit `0x400000`: it marks the track as this machine's own when its
//! `DeviceID` is non-empty and equals `djmdProperty.DeviceID`
//! (`getDeviceUUID` @0x100a493f4, `select DeviceID from djmdProperty`), and
//! calls `rekordboxDBController::modify_shared_relPath_to_absPath`
//! @0x10052a73c. For the master library (`AppSyncDBController` passes type
//! 3) that sets the working path to `OrgFolderPath` when the track is this
//! machine's own and either not a "shared desktop app track"
//! (`RowDataTrack::isSharedDesktopAppTrack` @0x1004e0268) or on service 3.
//! The working path is what the browser's missing check
//! (`BrowseBasicView::updateMissingStatus` @0x100321010) opens, and the
//! Location the reporter of issue #176 saw [OBS reporter].
//!
//! `ServiceID` picks the cloud folder (`getCloudSharedPath` @0x100ebeb90,
//! jump table at 0x1037bbf98): 1 the library's `share` folder, 2
//! `DropBox::localPublicPath()` + `/rekordbox`, 3 Google Drive, 4
//! `OneDrive` (each + `/rekordbox`). The Dropbox folder is the
//! `DropboxSharingPath` setting only when that names a real, non-symlinked
//! folder; otherwise rekordbox takes the Dropbox app's own folder from
//! `~/.dropbox/info.json` ([`crate::dropbox`]).
//!
//! The Location column is not always the opened path:
//! `browse::ListViewer::getLocationString` @0x1003ec478 [OBS static] prints
//! the working path plainly for this machine's tracks, but for a track
//! from another device prefixes the sharing device's name
//! (`getSharingDeviceName`) or, for a cloud track, the service name with
//! the path from the service folder on. [`TrackPaths::location`] keeps the
//! stored path for those, as rbxport did before; [UNKNOWN] the exact text
//! rekordbox prints for them.
//!
//! [UNKNOWN] The `CLSSyncMethod` 0 branch (download-on-demand, which reads
//! `ExtInfo.ClsInfo.Download` and the "Moved from Cloud" folder) and the
//! cloud folders of services 1, 3 and 4 are not modelled; a track that is
//! not this machine's own keeps the Dropbox resolution of
//! [`crate::resolve_folder_path`].

use std::borrow::Cow;
use std::path::PathBuf;

use crate::DriveMapping;

/// `ContentLink` bit rekordbox reads as "shared through Cloud Library Sync"
/// (`set_cloud_track_cache` tests bit 6 of the byte at +0x402).
pub const CLOUD_SHARED: i64 = 0x40_0000;
/// `ServiceID` of a track kept in Dropbox.
pub const SERVICE_DROPBOX: i64 = 2;
/// `ServiceID` for which `modify_shared_relPath_to_absPath` uses
/// `OrgFolderPath` on the uploading machine without the shared-desktop test.
pub const SERVICE_GOOGLE_DRIVE: i64 = 3;

/// The `djmdContent` columns, in [`StoredPath::COLUMNS`] order, that place a
/// track's file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredPath {
    pub folder_path: String,
    pub org_folder_path: String,
    pub content_link: i64,
    pub service_id: i64,
    pub device_id: String,
}

impl StoredPath {
    /// The columns to select, in field order.
    pub const COLUMNS: &'static str = "FolderPath, OrgFolderPath, ContentLink, ServiceID, DeviceID";

    /// Reads [`Self::COLUMNS`] from the start of a row. Text and number
    /// columns are read whatever SQLite storage class holds them.
    pub fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Self::from_row_at(row, 0)
    }

    /// Reads [`Self::COLUMNS`] starting at column `first`.
    pub fn from_row_at(row: &rusqlite::Row<'_>, first: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            folder_path: text(row, first)?,
            org_folder_path: text(row, first + 1)?,
            content_link: number(row, first + 2)?,
            service_id: number(row, first + 3)?,
            device_id: text(row, first + 4)?,
        })
    }
}

fn text(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<String> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(idx)? {
        ValueRef::Null => String::new(),
        ValueRef::Text(v) | ValueRef::Blob(v) => String::from_utf8_lossy(v).into_owned(),
        ValueRef::Integer(v) => v.to_string(),
        ValueRef::Real(v) => v.to_string(),
    })
}

fn number(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<i64> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(idx)? {
        ValueRef::Integer(v) => v,
        ValueRef::Text(v) => std::str::from_utf8(v).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0),
        ValueRef::Null | ValueRef::Real(_) | ValueRef::Blob(_) => 0,
    })
}

/// What rekordbox resolves every track's path against: the library's drive
/// substitution, this machine's Dropbox folder and the library's own
/// device id. Built once per load, applied per row.
#[derive(Debug, Clone, Default)]
pub struct TrackPaths {
    drive: Option<DriveMapping>,
    dropbox: Option<String>,
    own_device: Option<String>,
}

impl TrackPaths {
    /// `dropbox` is this machine's Dropbox folder as rekordbox finds it
    /// ([`crate::dropbox::local_public_path`], without `/rekordbox`);
    /// `own_device` is `djmdProperty.DeviceID`.
    #[must_use]
    pub fn new(drive: Option<DriveMapping>, dropbox: Option<String>, own_device: Option<String>) -> Self {
        Self {
            drive,
            dropbox: dropbox.filter(|d| !d.is_empty()),
            own_device: own_device.filter(|d| !d.is_empty()),
        }
    }

    /// The path rekordbox reads and shows for a track.
    #[must_use]
    pub fn resolve(&self, row: &StoredPath) -> String {
        if let Some(local) = self.own_local_copy(row) {
            return local.to_owned();
        }
        let path = self.drive.as_ref().map_or(Cow::Borrowed(row.folder_path.as_str()), |d| d.apply(&row.folder_path));
        let cloud_root = self.dropbox.as_ref().map(|d| PathBuf::from(d).join("rekordbox"));
        crate::resolve_folder_path(&path, cloud_root.as_deref())
    }

    /// The path the browser's Location column and the Info panel show: the
    /// local copy of this machine's own cloud-shared track (the path the
    /// reporter of issue #176 saw rekordbox show), else the stored path
    /// through the drive substitution, as before. A cloud track from
    /// another device keeps its `/contents_` path here rather than the
    /// Dropbox copy [`Self::resolve`] opens (see the module docs).
    #[must_use]
    pub fn location(&self, row: &StoredPath) -> String {
        if let Some(local) = self.own_local_copy(row) {
            return local.to_owned();
        }
        self.drive.as_ref().map_or_else(|| row.folder_path.clone(), |d| d.apply(&row.folder_path).into_owned())
    }

    /// `OrgFolderPath` when rekordbox would read a cloud-shared track from
    /// it: the track was uploaded from this library's device and is not a
    /// shared desktop app track (or is on service 3).
    ///
    /// [ASSUME] An empty `OrgFolderPath` falls back to the cloud path:
    /// rekordbox would take the empty string, which no file answers to.
    fn own_local_copy<'a>(&self, row: &'a StoredPath) -> Option<&'a str> {
        if row.content_link & CLOUD_SHARED == 0 || row.org_folder_path.is_empty() {
            return None;
        }
        let own = self.own_device.as_deref()?;
        if row.device_id != own {
            return None;
        }
        if row.service_id != SERVICE_GOOGLE_DRIVE && self.is_shared_desktop_app_track(row) {
            return None;
        }
        Some(&row.org_folder_path)
    }

    /// `RowDataTrack::isSharedDesktopAppTrack` @0x1004e0268 for a
    /// cloud-shared track: `OrgFolderPath` lies in the service's folder
    /// (the share path up to its last `/rekordbox`); or `FolderPath` is not
    /// a `/contents_` path; or `OrgFolderPath` ends with `FolderPath` and is
    /// not under the "Moved from Cloud" download folder.
    ///
    /// For Dropbox the folder is this machine's Dropbox folder as rekordbox
    /// finds it; with none at all (no usable setting and no Dropbox app)
    /// rekordbox's share path is empty, and every path starts with it.
    /// [ASSUME] Other services' folders are not known here, so that test is
    /// skipped for them; and an `OrgFolderPath` that ends with `FolderPath`
    /// is taken not to be under the download folder ([UNKNOWN] its default,
    /// `getSpecialLocation(?)/PioneerDJ/Moved from Cloud`).
    fn is_shared_desktop_app_track(&self, row: &StoredPath) -> bool {
        let root = (row.service_id == SERVICE_DROPBOX).then(|| self.dropbox.as_deref().unwrap_or(""));
        if root.is_some_and(|root| starts_with_ignore_case(&row.org_folder_path, root)) {
            return true;
        }
        if !starts_with_ignore_case(&row.folder_path, "/contents_") {
            return true;
        }
        ends_with_ignore_case(&row.org_folder_path, &row.folder_path)
    }
}

/// juce `String::startsWithIgnoreCase`: true for an empty prefix.
fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    let mut chars = text.chars();
    prefix.chars().all(|want| chars.next().is_some_and(|got| same_ignoring_case(got, want)))
}

/// juce `String::endsWithIgnoreCase`.
fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    let mut chars = text.chars().rev();
    suffix.chars().rev().all(|want| chars.next().is_some_and(|got| same_ignoring_case(got, want)))
}

fn same_ignoring_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: &str = "own-device";

    fn shared(folder: &str, org: &str, service: i64, device: &str) -> StoredPath {
        StoredPath {
            folder_path: folder.to_owned(),
            org_folder_path: org.to_owned(),
            content_link: CLOUD_SHARED,
            service_id: service,
            device_id: device.to_owned(),
        }
    }

    fn paths(dropbox: Option<&str>) -> TrackPaths {
        TrackPaths::new(None, dropbox.map(str::to_owned), Some(OWN.to_owned()))
    }

    #[test]
    fn an_own_cloud_track_reads_its_local_copy() {
        // The shape issue #176 reports: rekordbox shows OrgFolderPath.
        let row = shared(
            "/contents_1739239895/coredata/wonderland/14659475 coredata.mp3",
            "/Volumes/Music Libs/Beatport Music/Coredata - Wonderland/14659475 Coredata - Wonderland(Original Mix).mp3",
            SERVICE_DROPBOX,
            OWN,
        );
        assert_eq!(paths(Some("/Users/dj/Dropbox")).resolve(&row), row.org_folder_path);
    }

    #[test]
    fn another_devices_cloud_track_reads_the_dropbox_copy() {
        let row = shared("/contents_1/a/b.mp3", "/Volumes/Other Mac/b.mp3", SERVICE_DROPBOX, "other-device");
        assert_eq!(paths(Some("/Users/dj/Dropbox")).resolve(&row), "/Users/dj/Dropbox/rekordbox/contents_1/a/b.mp3");
    }

    #[test]
    fn without_the_cloud_flag_or_a_device_the_stored_path_stands() {
        let mut row = shared("/contents_1/a/b.mp3", "/Volumes/M/b.mp3", SERVICE_DROPBOX, OWN);
        row.content_link = 0;
        assert_eq!(paths(None).resolve(&row), "/contents_1/a/b.mp3");
        let row = shared("/contents_1/a/b.mp3", "/Volumes/M/b.mp3", SERVICE_DROPBOX, "");
        assert_eq!(paths(None).resolve(&row), "/contents_1/a/b.mp3", "an empty DeviceID is never this machine's");
        let no_own = TrackPaths::new(None, None, None);
        let row = shared("/contents_1/a/b.mp3", "/Volumes/M/b.mp3", SERVICE_DROPBOX, OWN);
        assert_eq!(no_own.resolve(&row), "/contents_1/a/b.mp3");
    }

    #[test]
    fn a_shared_desktop_app_track_keeps_the_cloud_path() {
        let dropbox = paths(Some("/Users/dj/Dropbox"));
        // OrgFolderPath inside the Dropbox folder.
        let row = shared("/contents_1/a.mp3", "/users/DJ/dropbox/Music/a.mp3", SERVICE_DROPBOX, OWN);
        assert_eq!(dropbox.resolve(&row), "/Users/dj/Dropbox/rekordbox/contents_1/a.mp3");
        // FolderPath not a cloud path at all.
        let row = shared("/Volumes/M/a.mp3", "/Volumes/N/a.mp3", SERVICE_DROPBOX, OWN);
        assert_eq!(dropbox.resolve(&row), "/Volumes/M/a.mp3");
        // OrgFolderPath ending with the cloud path, ignoring case.
        let row = shared("/contents_1/a.mp3", "/Volumes/N/CONTENTS_1/A.MP3", SERVICE_DROPBOX, OWN);
        assert_eq!(dropbox.resolve(&row), "/Users/dj/Dropbox/rekordbox/contents_1/a.mp3");
        // With no Dropbox folder at all (no usable DropboxSharingPath and
        // no Dropbox app record, `crate::dropbox`) rekordbox's share path
        // is empty, which every path starts with. With the setting empty
        // but Dropbox installed, the folder is the app's, as above.
        let row = shared("/contents_1/a.mp3", "/Volumes/N/a.mp3", SERVICE_DROPBOX, OWN);
        assert_eq!(paths(None).resolve(&row), "/contents_1/a.mp3");
    }

    #[test]
    fn the_location_column_shows_the_local_copy_or_the_stored_path() {
        let drive = DriveMapping::new("/Volumes/Music/", "/Volumes/Music 1/");
        let resolver = TrackPaths::new(drive, Some("/D".into()), Some(OWN.into()));
        let own = shared("/contents_1/a.mp3", "/Volumes/Music/a.mp3", SERVICE_DROPBOX, OWN);
        assert_eq!(resolver.location(&own), "/Volumes/Music/a.mp3");
        // Another device's track: the stored cloud path, not the Dropbox
        // copy that is opened.
        let other = shared("/contents_1/a.mp3", "/Volumes/X/a.mp3", SERVICE_DROPBOX, "other");
        assert_eq!(resolver.location(&other), "/contents_1/a.mp3");
        assert_eq!(resolver.resolve(&other), "/D/rekordbox/contents_1/a.mp3");
        let plain = StoredPath { folder_path: "/Volumes/Music/b.mp3".into(), ..StoredPath::default() };
        assert_eq!(resolver.location(&plain), "/Volumes/Music 1/b.mp3");
    }

    #[test]
    fn service_3_skips_the_shared_desktop_test() {
        let row = shared("/Volumes/M/a.mp3", "/Volumes/N/a.mp3", SERVICE_GOOGLE_DRIVE, OWN);
        assert_eq!(paths(None).resolve(&row), "/Volumes/N/a.mp3");
    }

    #[test]
    fn an_empty_local_copy_falls_back_to_the_cloud_path() {
        let row = shared("/contents_1/a.mp3", "", SERVICE_DROPBOX, OWN);
        assert_eq!(paths(Some("/D")).resolve(&row), "/D/rekordbox/contents_1/a.mp3");
    }

    #[test]
    fn the_local_copy_skips_the_drive_substitution() {
        // rekordbox substitutes the drive in FolderPath, then replaces the
        // whole working path with OrgFolderPath as stored.
        let drive = DriveMapping::new("/Volumes/Music/", "/Volumes/Music 1/");
        let resolver = TrackPaths::new(drive, Some("/D".into()), Some(OWN.into()));
        let row = shared("/contents_1/a.mp3", "/Volumes/Music/a.mp3", SERVICE_DROPBOX, OWN);
        assert_eq!(resolver.resolve(&row), "/Volumes/Music/a.mp3");
        let plain = StoredPath { folder_path: "/Volumes/Music/b.mp3".into(), ..StoredPath::default() };
        assert_eq!(resolver.resolve(&plain), "/Volumes/Music 1/b.mp3");
    }

    #[test]
    fn juce_case_insensitive_affixes() {
        assert!(starts_with_ignore_case("/Contents_1/x", "/contents_"));
        assert!(starts_with_ignore_case("anything", ""));
        assert!(!starts_with_ignore_case("/con", "/contents_"));
        assert!(ends_with_ignore_case("/a/B/C.mp3", "/b/c.MP3"));
        assert!(!ends_with_ignore_case("c.mp3", "/b/c.mp3"));
    }
}
