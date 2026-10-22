//! A beat grid edited on a Devices tree track, written to the stick (#660).
//!
//! rekordbox saves the grid to the track's analysis files on the device and
//! a changed tempo to the track's row in both of the stick's libraries,
//! leaving the update counters alone (`device_library::write_grid`). Every stick here is written by the export
//! pipeline into a temporary directory, or one on the stick `RBX_USB_STICK`
//! names (see `common`); no real library or stick is touched.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use rbl_anlz::{Anlz, AnlzBuilder, Beat};
use rbl_export::device_library::{self, Format, GridWrite};
use rbl_export::{export, verify, SourcePlaylist, SourceTrack};

fn grid(first_ms: u32, tempo_x100: u16) -> Vec<Beat> {
    let step = 6_000_000 / u32::from(tempo_x100);
    (0..8_u32).map(|i| Beat { beat_number: (i % 4 + 1) as u16, tempo_x100, time_ms: first_ms + i * step }).collect()
}

fn track(dir: &Path, id: u64) -> SourceTrack {
    let path = dir.join(format!("source-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: format!("Track {id}"),
        artist: "TRIODE".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        analysis: vec![
            ("DAT".into(), AnlzBuilder::new().path("/x.mp3").beat_grid(&grid(0, 12_800)).finish()),
            ("EXT".into(), AnlzBuilder::new().path("/x.mp3").finish()),
        ],
        ..SourceTrack::default()
    }
}

fn stick(src: &Path, dest: &Path) {
    let tracks: Vec<SourceTrack> = (1..=2).map(|i| track(src, i)).collect();
    export(dest, &tracks, &[SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }]).unwrap();
}

fn db(dest: &Path, format: Format) -> Vec<u8> {
    std::fs::read(dest.join("PIONEER/rekordbox").join(format.file_name())).unwrap()
}

fn other(format: Format) -> Format {
    match format {
        Format::DeviceLibrary => Format::OneLibrary,
        Format::OneLibrary => Format::DeviceLibrary,
    }
}

/// The `.DAT` the library names for track `id`, volume-relative.
fn dat_of(dest: &Path, format: Format, id: u32) -> PathBuf {
    let library = device_library::read(dest, format).unwrap();
    PathBuf::from(library.track(id).unwrap().analysis_path.trim_start_matches('/'))
}

fn tempo_of(dest: &Path, format: Format, id: u32) -> u32 {
    device_library::read(dest, format).unwrap().track(id).unwrap().bpm_x100
}

#[test]
fn a_new_tempo_goes_to_the_analysis_file_and_to_the_track_in_both_libraries() {
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
        stick(src.path(), dest.path());
        let audio_path = dest.path().join(device_library::read(dest.path(), format).unwrap().track(1).unwrap().path.trim_start_matches('/'));
        let audio = std::fs::read(&audio_path).unwrap();
        let relative = dat_of(dest.path(), format, 1);
        let dat = dest.path().join(&relative);
        let next = grid(12, 12_600);
        let bytes = Anlz::read(&dat).unwrap().with_beat_grid(&next);

        device_library::write_grid(dest.path(), format, &GridWrite { track: 1, files: vec![(relative.clone(), bytes)], bpm_x100: Some(12_600) }).unwrap();

        assert_eq!(Anlz::read(&dat).unwrap().beat_grid().unwrap(), next, "{format:?}");
        assert_eq!(tempo_of(dest.path(), format, 1), 12_600, "{format:?}: the row takes the grid's tempo");
        assert_eq!(tempo_of(dest.path(), format, 2), 12_800, "{format:?}: no other row moves");
        // The same track in the other library (same master ids, same
        // analysis file) moves too, as rekordbox's same-device update does.
        assert_eq!(dat_of(dest.path(), other(format), 1), relative);
        assert_eq!(tempo_of(dest.path(), other(format), 1), 12_600, "{format:?}: the other library shares the grid");
        assert_eq!(tempo_of(dest.path(), other(format), 2), 12_800);
        assert_eq!(std::fs::read(&audio_path).unwrap(), audio);
        assert!(!dest.path().join(".rbxport-publication").exists(), "{format:?}: the publication finished");
        let check = verify(dest.path()).unwrap();
        assert!(check.is_ok(), "{format:?}: {:?}", check.errors);
    }
}

#[test]
fn a_grid_moved_at_the_same_tempo_leaves_both_libraries_byte_for_byte() {
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
        stick(src.path(), dest.path());
        let before = (db(dest.path(), Format::DeviceLibrary), db(dest.path(), Format::OneLibrary));
        let relative = dat_of(dest.path(), format, 2);
        let dat = dest.path().join(&relative);
        let next = grid(40, 12_800);
        let bytes = Anlz::read(&dat).unwrap().with_beat_grid(&next);

        device_library::write_grid(dest.path(), format, &GridWrite { track: 2, files: vec![(relative, bytes)], bpm_x100: None }).unwrap();

        assert_eq!(Anlz::read(&dat).unwrap().beat_grid().unwrap(), next);
        assert_eq!((db(dest.path(), Format::DeviceLibrary), db(dest.path(), Format::OneLibrary)), before, "{format:?}");
    }
}

#[test]
fn a_refused_grid_write_changes_nothing_on_the_stick() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    stick(src.path(), dest.path());
    let relative = dat_of(dest.path(), Format::DeviceLibrary, 1);
    let dat = dest.path().join(&relative);
    let original = std::fs::read(&dat).unwrap();
    let before = (db(dest.path(), Format::DeviceLibrary), db(dest.path(), Format::OneLibrary));
    let bytes = Anlz::read(&dat).unwrap().with_beat_grid(&grid(12, 12_600));
    let refused = [
        // A track this library does not hold.
        GridWrite { track: 99, files: vec![(relative.clone(), bytes.clone())], bpm_x100: Some(12_600) },
        // A path that leaves the stick.
        GridWrite { track: 1, files: vec![(PathBuf::from("../outside.DAT"), bytes.clone())], bpm_x100: None },
        // A file the stick does not have: a grid write never creates one.
        GridWrite { track: 1, files: vec![(PathBuf::from("PIONEER/USBANLZ/P000/00000000/ANLZ0000.DAT"), bytes.clone())], bpm_x100: None },
    ];
    for write in &refused {
        assert!(device_library::write_grid(dest.path(), Format::DeviceLibrary, write).is_err(), "{write:?}");
    }
    assert_eq!(std::fs::read(&dat).unwrap(), original);
    assert!(!dest.path().join("PIONEER/USBANLZ/P000").exists());
    assert_eq!((db(dest.path(), Format::DeviceLibrary), db(dest.path(), Format::OneLibrary)), before);
}

/// Every column of every `content` row in `exportLibrary.db`, as text.
fn content_rows(dest: &Path) -> Vec<Vec<String>> {
    let path = dest.join("PIONEER/rekordbox").join(Format::OneLibrary.file_name());
    let library = rbl_onelibrary::ExportLibrary::open_read_only(&path).unwrap();
    let conn = library.connection();
    let mut stmt = conn.prepare("SELECT * FROM content ORDER BY content_id").unwrap();
    let columns = stmt.column_count();
    let names: Vec<String> = stmt.column_names().iter().map(|n| (*n).to_owned()).collect();
    stmt.query_map([], |r| {
        Ok((0..columns).map(|i| {
            let value: rusqlite::types::Value = r.get(i).unwrap();
            format!("{}={value:?}", names[i])
        }).collect())
    }).unwrap().map(Result::unwrap).collect()
}

#[test]
fn a_new_tempo_leaves_every_update_counter_as_it_was() {
    // rekordbox bumps none of a device row's counters on a grid edit
    // (`updateTrackInfoUpdated` returns for a device database; its
    // `setBeatInfo` writes no device row), so only `bpmx100` moves.
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
        stick(src.path(), dest.path());
        let before = content_rows(dest.path());
        let relative = dat_of(dest.path(), format, 1);
        let bytes = Anlz::read(&dest.path().join(&relative)).unwrap().with_beat_grid(&grid(12, 12_600));

        device_library::write_grid(dest.path(), format, &GridWrite { track: 1, files: vec![(relative, bytes)], bpm_x100: Some(12_600) }).unwrap();

        let after = content_rows(dest.path());
        assert_eq!(after.len(), before.len());
        for (old, new) in before.iter().zip(&after) {
            for (a, b) in old.iter().zip(new) {
                if a != b {
                    assert!(a.starts_with("bpmx100=") && b == "bpmx100=Integer(12600)", "{format:?}: {a} became {b}");
                }
            }
        }
        assert_ne!(before, after, "{format:?}: the tempo did change");
    }
}

#[test]
fn the_other_library_s_row_is_the_one_with_the_same_master_ids() {
    // rekordbox finds the track in the stick's other library by its
    // masterDbId and masterContentId (`getContentIDBsByMasterIDs`), not by
    // its own id or its analysis file.
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    stick(src.path(), dest.path());
    let pdb_track = device_library::read(dest.path(), Format::DeviceLibrary).unwrap().track(1).unwrap().clone();
    assert_ne!((pdb_track.master_db_id, pdb_track.master_content_id), (0, 0), "the export says where track 1 came from");
    // Swap the two OneLibrary rows' master ids: track 1 there now claims to
    // be the other collection track, and track 2 claims to be this one.
    let path = dest.path().join("PIONEER/rekordbox").join(Format::OneLibrary.file_name());
    let conn = rbl_onelibrary::playlists::open_read_write(&path).unwrap();
    let ids: Vec<i64> = [1, 2].iter().map(|id| conn.query_row("SELECT masterContentId FROM content WHERE content_id = ?1", [id], |r| r.get(0)).unwrap()).collect();
    conn.execute("UPDATE content SET masterContentId = ?1 WHERE content_id = 1", [ids[1]]).unwrap();
    conn.execute("UPDATE content SET masterContentId = ?1 WHERE content_id = 2", [ids[0]]).unwrap();
    drop(conn);
    let one = device_library::read(dest.path(), Format::OneLibrary).unwrap();
    assert_eq!(one.track(2).unwrap().master_content_id, pdb_track.master_content_id);
    let relative = dat_of(dest.path(), Format::DeviceLibrary, 1);
    let bytes = Anlz::read(&dest.path().join(&relative)).unwrap().with_beat_grid(&grid(12, 12_600));

    device_library::write_grid(dest.path(), Format::DeviceLibrary, &GridWrite { track: 1, files: vec![(relative, bytes)], bpm_x100: Some(12_600) }).unwrap();

    assert_eq!(tempo_of(dest.path(), Format::DeviceLibrary, 1), 12_600);
    assert_eq!(tempo_of(dest.path(), Format::OneLibrary, 2), 12_600, "the row with track 1's master ids");
    assert_eq!(tempo_of(dest.path(), Format::OneLibrary, 1), 12_800, "the row with the same id but other master ids");
}
