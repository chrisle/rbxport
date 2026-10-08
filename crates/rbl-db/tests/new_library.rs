//! A library made from nothing takes the first things anyone does to one:
//! a track added, analysed, and put in a playlist.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use rbl_db::locate::{locate_with, switch_with, use_default_with, Located, Sources, MASTER_DB_DIRECTORY};
use rbl_db::new_library::{create, plan_with};
use rbl_db::write::{AnalysisRegistration, Writer};
use rbl_db::{Library, LibraryLocation, OpenMode};

/// One second of silence as a 16-bit mono WAV.
fn write_wav(path: &std::path::Path) {
    let rate = 44_100_u32;
    let data_len = rate * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(44 + data_len as usize, 0);
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_new_library_takes_a_track_its_analysis_and_a_playlist() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(root.path());
    let plan = plan_with(&sources).unwrap().unwrap();
    create(&plan).unwrap();

    let mut location = found(&sources);
    // A temp directory is not the user's install; the test gate allows it.
    location.is_real_install = false;
    let backups = tempfile::tempdir().unwrap();
    let mut writer = Writer::open(location.clone(), backups.path()).unwrap();

    let audio = root.path().join("Track.wav");
    write_wav(&audio);
    let track = writer.import_file(&audio).unwrap();

    let dat = writer.analysis_data_path_for(&track).unwrap();
    let on_disk = location.share_root.join(dat.trim_start_matches('/'));
    std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
    std::fs::write(&on_disk, b"PMAI").unwrap();
    let registered = writer
        .register_analysis(&track, &AnalysisRegistration { bpm_x100: 12_800, key: None, analysis_data_path: &dat })
        .unwrap();
    assert_eq!(registered.rows, 1);

    let playlist = writer.create_playlist("New", "root").unwrap();
    writer.add_tracks(&playlist, std::slice::from_ref(&track)).unwrap();
    drop(writer);

    let db = Library::open(location, OpenMode::ReadOnly).unwrap();
    let (bpm, dat_path): (i64, String) = db
        .connection()
        .query_row("SELECT BPM, AnalysisDataPath FROM djmdContent WHERE ID = ?1", [&track], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((bpm, dat_path.as_str()), (12_800, dat.as_str()));
    let members: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM djmdSongPlaylist WHERE PlaylistID = ?1", [&playlist], |r| r.get(0))
        .unwrap();
    assert_eq!(members, 1);
}

fn found(sources: &Sources) -> LibraryLocation {
    match locate_with(sources).unwrap() {
        Located::Found { location, .. } => location,
        other => panic!("expected a library, got {other:?}"),
    }
}

/// A library rekordbox's way, in `dir` on a pretend drive.
fn library_on_drive(root: &Path, dir: &Path) -> PathBuf {
    let mut maker = Sources::under(&root.join("maker"));
    maker.default_dir = dir.to_path_buf();
    create(&plan_with(&maker).unwrap().unwrap()).unwrap().master_db
}

fn master_db_directory(sources: &Sources) -> Option<String> {
    let text = std::fs::read_to_string(sources.rekordbox_settings.as_deref()?).ok()?;
    rbl_core::paths::setting_value(&text, MASTER_DB_DIRECTORY)
}

#[test]
fn switching_to_a_drive_library_changes_rekordbox_s_setting_and_not_the_library() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(&root.path().join("machine"));
    rekordbox_ran(&sources);
    let rekordbox = create(&plan_with(&sources).unwrap().unwrap()).unwrap().master_db;
    let drive_dir = root.path().join("Volumes/B/PIONEER/Master");
    let drive = library_on_drive(root.path(), &drive_dir);
    let before = std::fs::read(&drive).unwrap();
    assert_eq!(found(&sources).master_db, rekordbox);

    let switched = switch_with(&sources, &drive).unwrap();
    assert_eq!(switched.master_db, drive);
    assert_eq!(switched.share_root, drive_dir.join("share"));
    assert_eq!(std::fs::read(&drive).unwrap(), before, "the library is only read");
    assert_eq!(master_db_directory(&sources).as_deref(), drive_dir.to_str());
    assert!(!sources.agent_options.as_deref().unwrap().exists(), "options.json is rekordbox's to write");
    assert_eq!(found(&sources).master_db, drive, "the next start opens it");

    // And back to the default, as choosing the default drive does.
    switch_with(&sources, &rekordbox).unwrap();
    assert_eq!(found(&sources).master_db, rekordbox);
}

#[test]
fn switching_to_a_database_that_is_not_a_library_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(&root.path().join("machine"));
    rekordbox_ran(&sources);
    let settings = std::fs::read(sources.rekordbox_settings.as_deref().unwrap()).unwrap();
    let invalid = root.path().join("Volumes/X/PIONEER/Master/master.db");
    std::fs::create_dir_all(invalid.parent().unwrap()).unwrap();
    std::fs::write(&invalid, b"not a rekordbox database").unwrap();

    assert!(switch_with(&sources, &invalid).is_err());
    assert_eq!(std::fs::read(sources.rekordbox_settings.as_deref().unwrap()).unwrap(), settings);
}

#[test]
fn without_rekordbox_an_empty_default_library_made_here_gives_way_to_a_drive_library() {
    // Issue #49: a machine with no rekordbox where an earlier build made an
    // empty library in the default folder, while the real one is on a drive.
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(&root.path().join("machine"));
    let made = create(&plan_with(&sources).unwrap().unwrap()).unwrap().master_db;
    assert_eq!(found(&sources).master_db, made);
    let drive = library_on_drive(root.path(), &root.path().join("media/ryan/T7/PIONEER/Master"));

    switch_with(&sources, &drive).unwrap();
    assert_eq!(found(&sources).master_db, drive);
    let settings = std::fs::read_to_string(sources.rekordbox_settings.as_deref().unwrap()).unwrap();
    assert!(settings.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n<PROPERTIES>\n"));
}

#[test]
fn a_missing_drive_is_unavailable_until_the_default_is_chosen_and_nothing_is_made_on_it() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(&root.path().join("machine"));
    let drive_dir = root.path().join("Volumes/Gone/PIONEER/Master");
    let drive = library_on_drive(root.path(), &drive_dir);
    switch_with(&sources, &drive).unwrap();
    std::fs::remove_dir_all(root.path().join("Volumes")).unwrap();

    assert!(matches!(locate_with(&sources).unwrap(), Located::Unavailable { .. }));
    assert_eq!(plan_with(&sources).unwrap(), None, "no library is offered on the missing drive");

    use_default_with(&sources).unwrap();
    assert_eq!(master_db_directory(&sources).as_deref(), sources.default_dir.to_str());
    assert!(!root.path().join("Volumes").exists());
    let plan = plan_with(&sources).unwrap().expect("the default folder is offered once chosen");
    assert_eq!(plan.master_db, sources.default_master_db());
}

/// rekordbox has run on the machine with nothing set: its settings file is
/// there without a `masterDbDirectory`.
fn rekordbox_ran(sources: &Sources) {
    let file = sources.rekordbox_settings.as_deref().unwrap();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n<PROPERTIES>\n  <VALUE name=\"ColorType\" val=\"3\"/>\n</PROPERTIES>\n").unwrap();
}
