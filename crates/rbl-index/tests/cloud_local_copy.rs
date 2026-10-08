#![allow(clippy::unwrap_used)]
//! A track shared through rekordbox's Cloud Library Sync stores a cloud path
//! in `FolderPath` and its file on the uploading machine in `OrgFolderPath`.
//! On that machine rekordbox opens and shows `OrgFolderPath` (issue #176).
//! See `rbl_db::track_path`.
use rbl_db::{fixture, Library, OpenMode};

const OWN: &str = "1a2b3c4d";
const CLOUD: &str = "/contents_1739239895/coredata/wonderland/14659475 coredata.mp3";
const LOCAL: &str = "/Volumes/Music Libs/Beatport Music/Coredata - Wonderland/14659475 Coredata - Wonderland(Original Mix).mp3";

fn share(location: &rbl_db::LibraryLocation, index: usize, service: i64, device: &str) {
    let writer = rbl_db::write::Writer::open(location.clone(), location.share_root.join("../backups")).unwrap();
    writer
        .library()
        .connection()
        .execute(
            "UPDATE djmdContent SET FolderPath = ?1, OrgFolderPath = ?2, ContentLink = ?3, ServiceID = ?4, DeviceID = ?5 WHERE ID = ?6",
            rusqlite::params![CLOUD, LOCAL, rbl_db::track_path::CLOUD_SHARED, service, device, fixture::track_id(index)],
        )
        .unwrap();
    writer.library().connection().execute("UPDATE djmdProperty SET DeviceID = ?1", [OWN]).unwrap();
}

#[test]
fn an_own_cloud_track_opens_and_shows_its_local_copy() {
    let root = tempfile::tempdir().unwrap();
    let location = fixture::build(root.path(), fixture::Shape::default()).unwrap();
    // Service 3 takes OrgFolderPath without consulting this machine's cloud
    // folder, so the result does not depend on the test machine's settings.
    share(&location, 0, rbl_db::track_path::SERVICE_GOOGLE_DRIVE, OWN);
    // Uploaded from another machine: never its OrgFolderPath.
    share(&location, 1, rbl_db::track_path::SERVICE_GOOGLE_DRIVE, "another-device");
    // Dropbox: OrgFolderPath when this machine has a Dropbox folder for
    // rekordbox, as there the shared-desktop test passes; with none,
    // rekordbox's share path is empty and the cloud path stands.
    share(&location, 2, rbl_db::track_path::SERVICE_DROPBOX, OWN);

    let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let (index, _) = rbl_index::load(&db).unwrap();
    let path_of = |i: usize| {
        let id: u64 = fixture::track_id(i).parse().unwrap();
        let row = index.ids.iter().position(|x| *x == id).unwrap();
        index.folder_path.get(row).to_owned()
    };
    assert_eq!(path_of(0), LOCAL);
    assert_ne!(path_of(1), LOCAL);
    let dropbox_expected = if rbl_db::dropbox_sharing_path().is_some() { LOCAL.to_owned() } else { CLOUD.to_owned() };
    assert_eq!(path_of(2), dropbox_expected);

    // The Info panel's Location reads the same way, and the stored column
    // is untouched.
    assert_eq!(db.track_details(&fixture::track_id(0)).unwrap().unwrap().path, LOCAL);
    let stored: String = db
        .connection()
        .query_row("SELECT FolderPath FROM djmdContent WHERE ID = ?1", [fixture::track_id(0)], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, CLOUD);
}
