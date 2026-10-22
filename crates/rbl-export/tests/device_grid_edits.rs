//! A beat grid edited on a Devices tree track, written to the stick (#660).
//!
//! rekordbox saves the grid to the track's analysis files on the device and
//! a changed tempo to the track's row in the library it was browsed from
//! (`device_library::GridWrite`). Every stick here is written by the export
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
fn a_new_tempo_goes_to_the_analysis_file_and_to_every_row_naming_it() {
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
        // The other library names the same analysis file, so its row moves
        // with the grid rather than disagreeing with it.
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
