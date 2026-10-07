//! A library made from nothing takes the first things anyone does to one:
//! a track added, analysed, and put in a playlist.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use rbl_db::locate::{locate_with, Located, Origin, Sources};
use rbl_db::new_library::{create, plan_with, use_existing_with};
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

#[test]
fn an_existing_external_library_can_be_selected_without_changing_it() {
    let root = tempfile::tempdir().unwrap();
    let master_db = library_on_drive(root.path(), &root.path().join("media/ryan/T7/PIONEER/Master"));
    let before = std::fs::read(&master_db).unwrap();
    let sources = Sources::under(&root.path().join("machine"));

    let selected = use_existing_with(&sources, &master_db).unwrap();

    assert_eq!(selected.master_db, master_db);
    assert_eq!(selected.share_root, master_db.parent().unwrap().join("share"));
    assert_eq!(std::fs::read(&selected.master_db).unwrap(), before);
    assert!(!sources.agent_options.as_deref().unwrap().exists(), "rekordbox's options.json is not written");
    assert!(!sources.rekordbox_settings.as_deref().unwrap().exists(), "nor its settings");
    let next = locate_with(&sources).unwrap();
    let Located::Found { location, origin } = next else { panic!("not found: {next:?}") };
    assert_eq!((location.master_db, origin), (master_db, Origin::Rbxport));
}

#[test]
fn selecting_an_invalid_database_does_not_persist_it() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(&root.path().join("machine"));
    let invalid = root.path().join("mounted-drive/master.db");
    std::fs::create_dir_all(invalid.parent().unwrap()).unwrap();
    std::fs::write(&invalid, b"not a rekordbox database").unwrap();

    assert!(use_existing_with(&sources, &invalid).is_err());
    assert!(!sources.choice.as_deref().unwrap().exists());
}

#[test]
fn a_choice_that_rekordbox_s_own_library_would_override_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(&root.path().join("machine"));
    rekordbox_ran(&sources);
    let rekordbox = create(&plan_with(&sources).unwrap().unwrap()).unwrap().master_db;
    let other = library_on_drive(root.path(), &root.path().join("Volumes/B/PIONEER/Master"));

    let refused = use_existing_with(&sources, &other).unwrap_err().to_string();
    assert!(refused.contains("rekordbox is set to use"), "{refused}");
    assert!(!sources.choice.as_deref().unwrap().exists());
    assert_eq!(found(&sources).master_db, rekordbox);

    // Choosing rekordbox's own library is fine, and needs nothing saved.
    assert_eq!(use_existing_with(&sources, &rekordbox).unwrap().master_db, rekordbox);
    assert!(!sources.choice.as_deref().unwrap().exists());
}

#[test]
fn without_rekordbox_a_default_library_made_here_can_be_replaced_by_a_drive_library() {
    // Issue #49: a machine with no rekordbox where an earlier build made an
    // empty library in the default folder, while the real one is on a drive.
    let root = tempfile::tempdir().unwrap();
    let sources = Sources::under(&root.path().join("machine"));
    let made = create(&plan_with(&sources).unwrap().unwrap()).unwrap().master_db;
    assert_eq!(found(&sources).master_db, made);
    let drive = library_on_drive(root.path(), &root.path().join("media/ryan/T7/PIONEER/Master"));

    assert_eq!(use_existing_with(&sources, &drive).unwrap().master_db, drive);
    let next = locate_with(&sources).unwrap();
    let Located::Found { location, origin } = next else { panic!("not found: {next:?}") };
    assert_eq!((location.master_db, origin), (drive, Origin::Rbxport));
    assert!(!sources.rekordbox_settings.as_deref().unwrap().exists(), "rekordbox's settings are not written");
}

/// rekordbox has run on the machine with nothing set: its settings file is
/// there without a `masterDbDirectory`.
fn rekordbox_ran(sources: &Sources) {
    let file = sources.rekordbox_settings.as_deref().unwrap();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n<PROPERTIES>\n</PROPERTIES>\n").unwrap();
}
