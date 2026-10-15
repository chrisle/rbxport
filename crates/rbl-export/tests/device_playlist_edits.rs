//! A track added on the stick to a playlist a sync put there, and what the
//! next syncs do with it (#429).
//!
//! rekordbox keeps the added entry while the library's copy of the playlist
//! is unchanged, and rewrites the playlist from the library once that copy
//! changes; the added track itself stays on the stick, out of the playlist
//! [OBS rekordbox 7.2.14.0323 for Windows, chris-win11 2026-10-12, scratch
//! library and blank USB drive, #309: `docs/reference/usb-export-db.md`
//! "What a later sync does to a playlist edited on the device"]:
//!
//! | Step | Library `P` | Stick `P` |
//! | --- | --- | --- |
//! | 1. Sync | House 1, House 2 | House 1, House 2 |
//! | 2. Devices tree: add Techno 1 to the stick's `P` | unchanged | House 1, House 2, Techno 1 |
//! | 3. Sync, library unchanged | House 1, House 2 | House 1, House 2, Techno 1 |
//! | 4. Library: add House 3; sync | House 1, House 2, House 3 | House 1, House 2, House 3 |
//!
//! The tests here run those steps against a temporary stick. Step 2 is
//! what rekordbox does there: it exports Techno 1 to the stick, as a track
//! of its own, and appends it to `P` in both libraries. Nothing touches a
//! real library or device.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use rbl_export::device_library::{self, Edit, Format};
use rbl_export::snapshot::Snapshot;
use rbl_export::{Manifest, SourcePlaylist, SourceTrack};

const DB_ID: u64 = 429;
const P: u64 = 4290;
const Q: u64 = 4291;

fn track(dir: &Path, id: u64, title: &str) -> SourceTrack {
    let path = dir.join(format!("source-429-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: title.into(),
        artist: "TRIODE".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

/// The scratch library of #309: House 1, House 2, Techno 1, House 3.
fn library(dir: &Path) -> Vec<SourceTrack> {
    vec![track(dir, 1, "House 1"), track(dir, 2, "House 2"), track(dir, 3, "Techno 1"), track(dir, 4, "House 3")]
}

fn playlist(id: u64, name: &str, entries: &[usize]) -> SourcePlaylist {
    SourcePlaylist { id, name: name.into(), track_indices: entries.to_vec(), ..Default::default() }
}

/// A Sync Manager sync of `playlists`, whose tracks are `tracks`.
fn sync(dest: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist]) -> rbl_export::Result<rbl_export::ExportReport> {
    let tree = playlists.iter().map(|p| rbl_export::sync_record::SyncNode { id: p.id, parent: 0, attribute: 0 }).collect();
    let sync = rbl_export::SyncSource { db_id: DB_ID, tree, ..Default::default() };
    rbl_export::export_full(dest, tracks, playlists, &[], None, Some(&sync), &mut |_| {})
}

/// Step 1, the sync of `P` = House 1, House 2.
fn first_sync(lib: &[SourceTrack], dest: &Path) {
    sync(dest, &lib[..2], &[playlist(P, "P", &[0, 1])]).unwrap();
}

/// Puts Techno 1 on the stick as another writer's track, as rekordbox's
/// Devices tree exports it: on the stick, in no playlist, and not in
/// rbxport's record of what it exported.
fn rekordbox_exports_techno(lib: &[SourceTrack], dest: &Path) -> u32 {
    let record = std::fs::read(Manifest::path(dest)).unwrap();
    sync(dest, &lib[..3], &[playlist(P, "P", &[0, 1])]).unwrap();
    std::fs::write(Manifest::path(dest), record).unwrap();
    device_id(dest, "Techno 1")
}

fn device_id(dest: &Path, title: &str) -> u32 {
    let library = device_library::read(dest, Format::OneLibrary).unwrap();
    library.tracks.iter().find(|t| t.title == title).unwrap_or_else(|| panic!("{title} is not on the stick")).id
}

fn playlist_id(dest: &Path, format: Format, name: &str) -> u32 {
    device_library::read(dest, format).unwrap().nodes.iter().find(|n| n.name == name).unwrap().id
}

/// Appends `track` to the stick's playlist `name` in `formats`.
fn add_on_stick(dest: &Path, formats: &[Format], name: &str, track: u32) {
    for format in formats {
        let id = playlist_id(dest, *format, name);
        device_library::apply(dest, *format, &Edit::Add { playlist: id, tracks: vec![track], duplicates: false }).unwrap();
    }
}

/// The titles in the stick's playlist `name`, from each library.
fn on_stick(dest: &Path, name: &str) -> [Vec<String>; 2] {
    [Format::DeviceLibrary, Format::OneLibrary].map(|format| {
        let library = device_library::read(dest, format).unwrap();
        let node = library.nodes.iter().find(|n| n.name == name).unwrap_or_else(|| panic!("no {name} in {format:?}"));
        node.tracks.iter().map(|t| library.track(*t).unwrap().title.clone()).collect()
    })
}

fn titles(names: &[&str]) -> [Vec<String>; 2] {
    let list: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    [list.clone(), list]
}

/// Techno 1's row in both libraries, and its audio file, are still there.
fn techno_still_on_stick(dest: &Path, step: &str) {
    let snapshot = Snapshot::read(dest).unwrap();
    for (format, library) in [("Device Library", &snapshot.legacy), ("OneLibrary", &snapshot.one)] {
        let row = library.as_ref().unwrap().tracks.iter().find(|t| t.title == "Techno 1");
        let row = row.unwrap_or_else(|| panic!("{step}: Techno 1 left the {format}"));
        assert!(dest.join(row.path.trim_start_matches('/')).is_file(), "{step}: Techno 1's audio left the stick");
    }
    assert!(rbl_export::verify_databases(dest).unwrap().errors.is_empty(), "{step}");
}

#[test]
fn a_track_added_on_the_stick_stays_until_the_library_playlist_changes() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = library(src.path());
    first_sync(&lib, dest.path());
    let techno = rekordbox_exports_techno(&lib, dest.path());
    add_on_stick(dest.path(), &[Format::DeviceLibrary, Format::OneLibrary], "P", techno);
    assert_eq!(on_stick(dest.path(), "P"), titles(&["House 1", "House 2", "Techno 1"]), "step 2");

    // Step 3, twice: the library's P is unchanged, so the stick's stays.
    for step in ["step 3", "step 3, again"] {
        sync(dest.path(), &lib[..2], &[playlist(P, "P", &[0, 1])]).unwrap_or_else(|e| panic!("{step}: {e}"));
        assert_eq!(on_stick(dest.path(), "P"), titles(&["House 1", "House 2", "Techno 1"]), "{step}");
        assert_eq!(device_id(dest.path(), "Techno 1"), techno, "{step}: Techno 1 keeps its id");
        techno_still_on_stick(dest.path(), step);
    }

    // Step 4: House 3 added to the library's P. The stick's P becomes the
    // library's; Techno 1 leaves it but stays on the stick.
    let selection = [lib[0].clone(), lib[1].clone(), lib[3].clone()];
    for step in ["step 4", "step 4, again"] {
        sync(dest.path(), &selection, &[playlist(P, "P", &[0, 1, 2])]).unwrap_or_else(|e| panic!("{step}: {e}"));
        assert_eq!(on_stick(dest.path(), "P"), titles(&["House 1", "House 2", "House 3"]), "{step}");
        techno_still_on_stick(dest.path(), step);
    }
}

/// The same steps on a stick an older build last synced: its record does
/// not say what the library's playlists held. That build wrote each
/// playlist as the library had it, so the stick's copy at that sync stands
/// in for the library's.
#[test]
fn a_record_from_an_older_build_still_tells_an_unchanged_library_playlist() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = library(src.path());
    first_sync(&lib, dest.path());
    let techno = rekordbox_exports_techno(&lib, dest.path());
    let path = Manifest::path(dest.path());
    let mut record: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut stripped = 0;
    for entry in record["playlists"].as_array_mut().unwrap() {
        stripped += usize::from(entry.as_object_mut().unwrap().remove("library_entries").is_some());
    }
    assert_eq!(stripped, 1, "the record names P's library entries");
    std::fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    add_on_stick(dest.path(), &[Format::DeviceLibrary, Format::OneLibrary], "P", techno);

    sync(dest.path(), &lib[..2], &[playlist(P, "P", &[0, 1])]).unwrap();
    assert_eq!(on_stick(dest.path(), "P"), titles(&["House 1", "House 2", "Techno 1"]));
    let selection = [lib[0].clone(), lib[1].clone(), lib[3].clone()];
    sync(dest.path(), &selection, &[playlist(P, "P", &[0, 1, 2])]).unwrap();
    assert_eq!(on_stick(dest.path(), "P"), titles(&["House 1", "House 2", "House 3"]));
    techno_still_on_stick(dest.path(), "library changed");
}

/// The added track can be one this app exported for another playlist. It
/// stays where the stick put it while the library's P is unchanged, and
/// stays in its own playlist once P is rewritten.
#[test]
fn a_track_of_another_synced_playlist_added_on_the_stick_is_kept_too() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = library(src.path());
    let selection = [playlist(P, "P", &[0, 1]), playlist(Q, "Q", &[2])];
    sync(dest.path(), &lib[..3], &selection).unwrap();
    add_on_stick(dest.path(), &[Format::DeviceLibrary, Format::OneLibrary], "P", device_id(dest.path(), "Techno 1"));

    sync(dest.path(), &lib[..3], &selection).unwrap();
    assert_eq!(on_stick(dest.path(), "P"), titles(&["House 1", "House 2", "Techno 1"]));
    assert_eq!(on_stick(dest.path(), "Q"), titles(&["Techno 1"]));

    let changed = [lib[0].clone(), lib[1].clone(), lib[2].clone(), lib[3].clone()];
    sync(dest.path(), &changed, &[playlist(P, "P", &[0, 1, 3]), playlist(Q, "Q", &[2])]).unwrap();
    assert_eq!(on_stick(dest.path(), "P"), titles(&["House 1", "House 2", "House 3"]));
    assert_eq!(on_stick(dest.path(), "Q"), titles(&["Techno 1"]));
}

/// Only an addition was observed. A library entry removed on the stick is
/// still a device edit the sync will not overwrite unasked.
#[test]
fn a_library_entry_removed_on_the_stick_still_stops_the_sync() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = library(src.path());
    first_sync(&lib, dest.path());
    let house = device_id(dest.path(), "House 1");
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let id = playlist_id(dest.path(), format, "P");
        device_library::apply(dest.path(), format, &Edit::Remove { playlist: id, tracks: vec![house] }).unwrap();
    }
    let error = sync(dest.path(), &lib[..2], &[playlist(P, "P", &[0, 1])]).unwrap_err().to_string();
    assert!(error.contains("playlist 'P' changed on the USB"), "{error}");
    assert_eq!(on_stick(dest.path(), "P"), titles(&["House 2"]), "nothing was written");
}

/// Only an addition made in both libraries was observed. One made in one
/// of them (a player saves its edits to the library it reads) would leave
/// the two disagreeing about P, and the sync writes one P to both.
#[test]
fn a_track_added_to_one_library_only_still_stops_the_sync() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = library(src.path());
    first_sync(&lib, dest.path());
    let techno = rekordbox_exports_techno(&lib, dest.path());
    add_on_stick(dest.path(), &[Format::OneLibrary], "P", techno);
    let error = sync(dest.path(), &lib[..2], &[playlist(P, "P", &[0, 1])]).unwrap_err().to_string();
    assert!(error.contains("playlist 'P' changed on the USB"), "{error}");
    let [legacy, one] = on_stick(dest.path(), "P");
    assert_eq!((legacy.len(), one.len()), (2, 3), "nothing was written");
}
