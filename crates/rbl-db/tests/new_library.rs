//! A library made from nothing takes the first things anyone does to one:
//! a track added, analysed, and put in a playlist.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_db::new_library::{create, plan_at, use_existing_at};
use rbl_db::write::{AnalysisRegistration, Writer};
use rbl_db::{detect_from, Library, OpenMode};

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
    let options = root.path().join("rekordboxAgent/storage/options.json");
    let plan = plan_at(&options, &root.path().join("rekordbox")).unwrap().unwrap();
    create(&plan).unwrap();

    let mut location = detect_from(&options).unwrap();
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

#[test]
fn an_existing_external_library_can_be_selected_without_changing_it() {
    let root = tempfile::tempdir().unwrap();
    let original_options = root.path().join("source/options.json");
    let external = root.path().join("mounted-drive/rekordbox");
    let made = create(&plan_at(&original_options, &external).unwrap().unwrap()).unwrap();
    let before = std::fs::read(&made.master_db).unwrap();
    std::fs::remove_file(&original_options).unwrap();

    let installed_options = root.path().join("installed-agent/options.json");
    let selected = use_existing_at(&installed_options, &made.master_db).unwrap();

    assert_eq!(selected.master_db, made.master_db);
    assert_eq!(selected.share_root, external.join("share"));
    assert_eq!(std::fs::read(&selected.master_db).unwrap(), before);
    assert_eq!(detect_from(&installed_options).unwrap().master_db, selected.master_db);
    Library::open(selected, OpenMode::ReadOnly).unwrap();
}

#[test]
fn selecting_an_invalid_database_does_not_persist_it() {
    let root = tempfile::tempdir().unwrap();
    let options = root.path().join("installed-agent/options.json");
    let invalid = root.path().join("mounted-drive/master.db");
    std::fs::create_dir_all(invalid.parent().unwrap()).unwrap();
    std::fs::write(&invalid, b"not a rekordbox database").unwrap();

    assert!(use_existing_at(&options, &invalid).is_err());
    assert!(!options.exists());
}

#[test]
fn selecting_a_library_never_replaces_existing_agent_options() {
    let root = tempfile::tempdir().unwrap();
    let first_options = root.path().join("first/options.json");
    let first = create(&plan_at(&first_options, &root.path().join("first/library")).unwrap().unwrap()).unwrap();
    let second_options = root.path().join("second/options.json");
    let second = create(&plan_at(&second_options, &root.path().join("second/library")).unwrap().unwrap()).unwrap();
    let before = std::fs::read(&first_options).unwrap();

    assert!(use_existing_at(&first_options, &second.master_db).is_err());
    assert_eq!(std::fs::read(&first_options).unwrap(), before);
    assert_eq!(detect_from(&first_options).unwrap().master_db, first.master_db);
}
