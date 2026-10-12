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
//! That working path is checked again when the file is used. [OBS rekordbox
//! 7.2.19 macOS arm64, static] `DatabaseMediator::checkAndUpdateCloudFilePath`
//! @0x100c48904 (called when a deck loads a track,
//! `UiPlayer::handleMessageDragAndDrop`, and by export and the Missing File
//! Manager) acts on every cloud-shared track. With `CLSSyncMethod` not 0
//! (its default is 1) it keeps the working path when that is the local copy
//! and the file is there; else it takes the local copy (`OrgFolderPath`, or
//! for a shared desktop app track the service's folder + `FolderPath`) when
//! that file is there; else the cloud copy when
//! `DatabaseMediator::existAsFileOnCloud` @0x101e6ad9c finds it:
//! `sharePath(ServiceID)` up to its last `/rekordbox`, then `/rekordbox` when
//! `FolderPath` starts with `/Contents` (ignoring case), then `FolderPath`.
//! Otherwise the working path stands and the track is missing.
//! `BrowseBasicView::updateMissingStatus` @0x100321010 does the same for
//! this machine's own cloud-shared tracks when a row is selected, and
//! updates the cached path, so the Location column shows the copy found.
//! This is how rekordbox plays a track whose upload moved the file into the
//! Dropbox folder (issue #340): `OrgFolderPath` is gone, and the cloud copy,
//! whose name rekordbox lowercased, is the path `FolderPath` already holds.
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
//! [`crate::resolve_folder_path`] unless its local copy is on this machine.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

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

    /// The path rekordbox reads and shows for a track: the working path
    /// it sets when it reads the row, then for a cloud-shared track the
    /// copy it finds on this machine when the file is used (see the module
    /// docs). Looks at the file system only for cloud-shared tracks.
    #[must_use]
    pub fn resolve(&self, row: &StoredPath) -> String {
        let working = self.working_path(row);
        self.found_cloud_track_file(row, &working, Path::is_file).unwrap_or(working)
    }

    /// The path the browser's Location column and the Info panel show: for
    /// this machine's own cloud-shared track the file [`Self::resolve`]
    /// opens (its local copy, the path the reporter of issue #176 saw
    /// rekordbox show, or the Dropbox copy once that is gone, issue #340),
    /// else the stored path through the drive substitution, as before. A
    /// cloud track from another device keeps its `/contents_` path here
    /// rather than the Dropbox copy [`Self::resolve`] opens (see the module
    /// docs).
    #[must_use]
    pub fn location(&self, row: &StoredPath) -> String {
        if self.own_local_copy(row).is_some() {
            return self.resolve(row);
        }
        self.drive.as_ref().map_or_else(|| row.folder_path.clone(), |d| d.apply(&row.folder_path).into_owned())
    }

    /// The working path `set_content_data` and
    /// `modify_shared_relPath_to_absPath` give a row, before any file is
    /// looked at.
    fn working_path(&self, row: &StoredPath) -> String {
        if let Some(local) = self.own_local_copy(row) {
            return local.to_owned();
        }
        let path = self.drive.as_ref().map_or(Cow::Borrowed(row.folder_path.as_str()), |d| d.apply(&row.folder_path));
        let cloud_root = self.dropbox.as_ref().map(|d| PathBuf::from(d).join("rekordbox"));
        crate::resolve_folder_path(&path, cloud_root.as_deref())
    }

    /// `DatabaseMediator::checkAndUpdateCloudFilePath` @0x100c48904 with
    /// `CLSSyncMethod` 1: the file a cloud-shared track is played from when
    /// that is not `working`, or `None` to keep `working`. `is_file` is
    /// juce `File::existsAsFile`.
    fn found_cloud_track_file(
        &self,
        row: &StoredPath,
        working: &str,
        is_file: impl Fn(&Path) -> bool,
    ) -> Option<String> {
        if row.content_link & CLOUD_SHARED == 0 {
            return None;
        }
        let local = self.local_copy(row);
        if local == working && is_file(Path::new(working)) {
            return None;
        }
        if is_file(Path::new(&local)) {
            return Some(local);
        }
        let cloud = self.cloud_copy(row)?;
        (cloud != working && is_file(Path::new(&cloud))).then_some(cloud)
    }

    /// The local copy `checkAndUpdateCloudFilePath` and
    /// `updateMissingStatus` look for first: `OrgFolderPath`, or for a
    /// shared desktop app track not on service 3 the service's folder,
    /// then `/rekordbox` when `FolderPath` starts with `/contents_`
    /// (ignoring case), then `FolderPath`.
    ///
    /// [ASSUME] Other services' folders are not known here, so for them
    /// the local copy stays `OrgFolderPath`, as rekordbox leaves it when
    /// the service has no folder.
    fn local_copy(&self, row: &StoredPath) -> String {
        if row.service_id != SERVICE_GOOGLE_DRIVE && self.is_shared_desktop_app_track(row) {
            if let Some(root) = self.dropbox.as_deref().filter(|_| row.service_id == SERVICE_DROPBOX) {
                let sub = if starts_with_ignore_case(&row.folder_path, "/contents_") { "/rekordbox" } else { "" };
                return format!("{root}{sub}{}", row.folder_path);
            }
        }
        row.org_folder_path.clone()
    }

    /// `DatabaseMediator::existAsFileOnCloud` @0x101e6ad9c's path for a
    /// Dropbox track: the Dropbox folder (`sharePath` up to its last
    /// `/rekordbox`), then `/rekordbox` when the stored `FolderPath`
    /// starts with `/Contents` (ignoring case), then `FolderPath`.
    /// [UNKNOWN] Other services' folders; `None` for them.
    fn cloud_copy(&self, row: &StoredPath) -> Option<String> {
        if row.service_id != SERVICE_DROPBOX {
            return None;
        }
        let root = self.dropbox.as_deref()?;
        let sub = if starts_with_ignore_case(&row.folder_path, "/Contents") { "/rekordbox" } else { "" };
        Some(format!("{root}{sub}{}", row.folder_path))
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
#[allow(clippy::unwrap_used)]
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

    /// A Dropbox folder and a music folder under a temporary directory,
    /// with `files` (paths relative to it) created empty.
    fn machine(files: &[&str]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"").unwrap();
        }
        let root = dir.path().to_str().unwrap().to_owned();
        (dir, root)
    }

    #[test]
    fn an_own_cloud_track_whose_local_copy_is_gone_plays_the_dropbox_copy() {
        // Issue #340: the upload moved the file into the Dropbox folder
        // under a lowercased name; OrgFolderPath still names where it was
        // imported from.
        let (_dir, root) = machine(&["Dropbox/rekordbox/contents_777182204/artist/unknownalbum/track.wav"]);
        let resolver = paths(Some(&format!("{root}/Dropbox")));
        let row = shared(
            "/contents_777182204/artist/unknownalbum/track.wav",
            &format!("{root}/Music/Tracks/To Be Added/Track.wav"),
            SERVICE_DROPBOX,
            OWN,
        );
        let dropbox_copy = format!("{root}/Dropbox/rekordbox/contents_777182204/artist/unknownalbum/track.wav");
        assert_eq!(resolver.resolve(&row), dropbox_copy);
        assert_eq!(resolver.location(&row), dropbox_copy, "the Location column follows the copy found");
    }

    #[test]
    fn an_own_cloud_track_keeps_its_local_copy_while_it_is_there() {
        // Issue #176: both copies exist, rekordbox shows OrgFolderPath.
        let (_dir, root) = machine(&["Music/a.mp3", "Dropbox/rekordbox/contents_1/x/a.mp3"]);
        let resolver = paths(Some(&format!("{root}/Dropbox")));
        let row = shared("/contents_1/x/a.mp3", &format!("{root}/Music/a.mp3"), SERVICE_DROPBOX, OWN);
        assert_eq!(resolver.resolve(&row), row.org_folder_path);
        assert_eq!(resolver.location(&row), row.org_folder_path);
    }

    #[test]
    fn an_own_cloud_track_with_neither_copy_stays_missing_at_its_local_copy() {
        let (_dir, root) = machine(&[]);
        let resolver = paths(Some(&format!("{root}/Dropbox")));
        let row = shared("/contents_1/x/a.mp3", &format!("{root}/Music/a.mp3"), SERVICE_DROPBOX, OWN);
        assert_eq!(resolver.resolve(&row), row.org_folder_path);
    }

    #[test]
    fn another_devices_cloud_track_plays_a_local_copy_found_on_this_machine() {
        // checkAndUpdateCloudFilePath does not compare DeviceID with
        // CLSSyncMethod 1: OrgFolderPath wins whenever that file is here.
        let (_dir, root) = machine(&["Music/a.mp3", "Dropbox/rekordbox/contents_1/x/a.mp3"]);
        let resolver = paths(Some(&format!("{root}/Dropbox")));
        let row = shared("/contents_1/x/a.mp3", &format!("{root}/Music/a.mp3"), SERVICE_DROPBOX, "other-device");
        assert_eq!(resolver.resolve(&row), row.org_folder_path);
        assert_eq!(resolver.location(&row), "/contents_1/x/a.mp3", "Location is unchanged for another device's track");
        let gone = shared("/contents_1/x/a.mp3", &format!("{root}/Elsewhere/a.mp3"), SERVICE_DROPBOX, "other-device");
        assert_eq!(resolver.resolve(&gone), format!("{root}/Dropbox/rekordbox/contents_1/x/a.mp3"));
    }

    #[test]
    fn the_file_check_follows_check_and_update_cloud_file_path() {
        let resolver = paths(Some("/D"));
        let own = shared("/Contents_1/a.mp3", "/M/a.mp3", SERVICE_DROPBOX, OWN);
        let found = |hit: &'static str| move |p: &Path| p == Path::new(hit);
        // The local copy is the working path and is there: kept.
        assert_eq!(resolver.found_cloud_track_file(&own, "/M/a.mp3", found("/M/a.mp3")), None);
        // existAsFileOnCloud adds /rekordbox for /Contents, ignoring case.
        assert_eq!(
            resolver.found_cloud_track_file(&own, "/M/a.mp3", found("/D/rekordbox/Contents_1/a.mp3")),
            Some("/D/rekordbox/Contents_1/a.mp3".to_owned())
        );
        // A cloud copy that is already the working path changes nothing.
        let other = shared("/contents_1/a.mp3", "/M/a.mp3", SERVICE_DROPBOX, "other");
        assert_eq!(
            resolver.found_cloud_track_file(&other, "/D/rekordbox/contents_1/a.mp3", found("/D/rekordbox/contents_1/a.mp3")),
            None
        );
        // No cloud flag, or no Dropbox folder for a Dropbox track: nothing found.
        let mut plain = own.clone();
        plain.content_link = 0;
        assert_eq!(resolver.found_cloud_track_file(&plain, "/x", |_| true), None);
        let no_dropbox = paths(None);
        assert_eq!(no_dropbox.found_cloud_track_file(&own, "/M/a.mp3", found("/rekordbox/Contents_1/a.mp3")), None);
        // A shared desktop app track looks for its service-folder copy
        // first, not OrgFolderPath.
        let desktop = shared("/contents_1/a.mp3", "/D/Music/a.mp3", SERVICE_DROPBOX, OWN);
        assert_eq!(
            resolver.found_cloud_track_file(&desktop, "/elsewhere", |p: &Path| p == Path::new("/D/rekordbox/contents_1/a.mp3")),
            Some("/D/rekordbox/contents_1/a.mp3".to_owned())
        );
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
