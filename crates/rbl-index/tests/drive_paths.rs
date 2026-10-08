#![allow(clippy::unwrap_used)]
//! A library kept on an external drive stores track paths under the drive
//! as it was mounted when the library was made (`BaseDBDrive`); rekordbox
//! reads them under the drive's current mount. See `rbl_db::DriveMapping`.
use rbl_db::{fixture, Library, OpenMode};

fn set_drives(location: &rbl_db::LibraryLocation, base: &str, current: &str) {
    let writer = rbl_db::write::Writer::open(location.clone(), location.share_root.join("../backups")).unwrap();
    writer
        .library()
        .connection()
        .execute("UPDATE djmdProperty SET BaseDBDrive = ?1, CurrentDBDrive = ?2", [base, current])
        .unwrap();
}

/// The `CurrentDBDrive` rekordbox reads a fixture library in `folder`
/// through: off `/Volumes` on macOS and Linux the stored one; on Windows
/// the folder's drive letter (`getDrivePathFromFilePath` gives `C:/`).
fn current_drive_for(folder: &std::path::Path, stored: &str) -> String {
    if cfg!(windows) {
        format!("{}/", &folder.to_string_lossy()[..2])
    } else {
        stored.to_owned()
    }
}

#[test]
fn a_drive_library_reads_track_paths_under_the_drives_current_mount() {
    let root = tempfile::tempdir().unwrap();
    // The library was made on "Music"; the drive now mounts as "Music 1".
    // The fixture folder is not under /Volumes, so rekordbox's
    // getDrivePathFromFilePath gives "/" and the stored CurrentDBDrive
    // stands (the /Volumes cases are rbl-db's `current_drive` unit tests).
    let location = fixture::build(&root.path().join("PIONEER").join("Master"), fixture::Shape::default()).unwrap();
    fixture::point_at_audio(&location, 0, "/Volumes/Music/Tracks/a.mp3", 300).unwrap();
    fixture::point_at_audio(&location, 1, "/Users/dj/Music/b.mp3", 300).unwrap();
    set_drives(&location, "/Volumes/Music/", "/Volumes/Music 1/");

    let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let expected_drive = current_drive_for(root.path(), "/Volumes/Music 1/");
    let (index, _) = rbl_index::load(&db).unwrap();
    let path_of = |i: usize| {
        let id: u64 = fixture::track_id(i).parse().unwrap();
        let row = index.ids.iter().position(|x| *x == id).unwrap();
        index.folder_path.get(row).to_owned()
    };
    assert_eq!(path_of(0), format!("{expected_drive}Tracks/a.mp3"));
    assert_eq!(path_of(1), "/Users/dj/Music/b.mp3", "a path off the base drive is left alone");

    // The stored column is untouched: this is a read-side resolution.
    let stored: String = db
        .connection()
        .query_row("SELECT FolderPath FROM djmdContent WHERE ID = ?1", [fixture::track_id(0)], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, "/Volumes/Music/Tracks/a.mp3");
    // The Info panel's Location reads the same way.
    assert_eq!(db.track_details(&fixture::track_id(0)).unwrap().unwrap().path, format!("{expected_drive}Tracks/a.mp3"));
}

#[test]
fn a_library_without_drive_values_reads_paths_as_stored() {
    let root = tempfile::tempdir().unwrap();
    let location = fixture::build(root.path(), fixture::Shape::default()).unwrap();
    fixture::point_at_audio(&location, 0, "/Volumes/Music/Tracks/a.mp3", 300).unwrap();
    let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
    assert!(db.drive_mapping().is_none());

    // Off /Volumes: the stored CurrentDBDrive is what rekordbox uses.
    set_drives(&location, "/Volumes/Music/", "/Volumes/Music 1/");
    let db = Library::open(location, OpenMode::ReadOnly).unwrap();
    let mapping = db.drive_mapping().unwrap();
    let expected_drive = current_drive_for(root.path(), "/Volumes/Music 1/");
    assert_eq!(mapping.apply("/volumes/music/x.mp3"), format!("{expected_drive}x.mp3"), "the prefix compares ignoring case");
    assert_eq!(mapping.apply("/Volumes/Music 2/x.mp3"), "/Volumes/Music 2/x.mp3");
}
