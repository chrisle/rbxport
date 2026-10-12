//! Playlists a stick holds in one of its two libraries only, the first time
//! it is synced from here (#353).
//!
//! rekordbox keeps a stick's two libraries apart: an edit made under one of
//! them changes that one alone, and each numbers its own playlists, so one
//! id can name a different playlist in each [DOC: rekordbox FAQ "Device
//! Library Plus"; OBS static, rekordbox 7.2.19: its export finds a playlist's
//! place in the other library by name, `SearchPlaylistPositionForSameDevDB::
//! search` @0x1010dd828]. A player saves a playlist, such as its Tag List,
//! in the library it reads and only there [DOC: AlphaTheta "USB export
//! guide" 251007 p.4]. Such a playlist is the stick's own; a sync carries it
//! and is not refused.
//!
//! Every stick here is written by the export pipeline into a temporary
//! directory and edited through `device_library`, one library at a time, as
//! rekordbox's Devices tree and a player do; nothing touches a real device.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use rbl_export::device_library::{self, Edit, Format};
use rbl_export::snapshot::Snapshot;
use rbl_export::{SourcePlaylist, SourceTrack};

const DB_ID: u64 = 353;

fn track(dir: &Path, id: u64) -> SourceTrack {
    let path = dir.join(format!("source-353-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: format!("Track {id}"),
        artist: "TRIODE".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

fn sync_to(dest: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist]) -> rbl_export::Result<rbl_export::ExportReport> {
    // The library's tree names "Set", so the stick's record ties it to its
    // device playlist, as rekordbox's own record does.
    let tree = vec![rbl_export::sync_record::SyncNode { id: 10, parent: 0, attribute: 0 }];
    let sync = rbl_export::SyncSource { db_id: DB_ID, tree, ..Default::default() };
    rbl_export::export_full(dest, tracks, playlists, &[], None, Some(&sync), &mut |_| {})
}

/// Tracks 1 to 4 in the library's playlist "Set", synced to `dest`, with
/// rbxport's own record of the sync removed: a stick never synced from here.
fn stick(src: &Path, dest: &Path) -> (Vec<SourceTrack>, Vec<SourcePlaylist>) {
    let tracks: Vec<SourceTrack> = (1..=4).map(|id| track(src, id)).collect();
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    sync_to(dest, &tracks, &playlists).unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest)).unwrap();
    (tracks, playlists)
}

/// A playlist made under one library only, holding `tracks` (device ids).
fn create(dest: &Path, format: Format, name: &str, tracks: Vec<u32>) -> u32 {
    let id = device_library::apply(dest, format, &Edit::Create { parent: 0, name: name.into(), folder: false }).unwrap().id;
    device_library::apply(dest, format, &Edit::Add { playlist: id, tracks }).unwrap();
    id
}

/// Every playlist of one library, by name, with its tracks' titles.
fn playlists(dest: &Path, format: Format) -> Vec<(String, Vec<String>)> {
    let library = device_library::read(dest, format).unwrap();
    let mut out: Vec<(String, Vec<String>)> = library
        .nodes
        .iter()
        .map(|n| (n.name.clone(), n.tracks.iter().map(|t| library.track(*t).unwrap().title.clone()).collect()))
        .collect();
    out.sort();
    out
}

fn device_ids(dest: &Path) -> Vec<u32> {
    device_library::read(dest, Format::DeviceLibrary).unwrap().tracks.iter().map(|t| t.id).collect()
}

fn titles(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

#[test]
fn a_tag_list_in_one_library_whose_id_the_other_gives_another_playlist_does_not_stop_the_sync() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (tracks, selection) = stick(src.path(), dest.path());
    let ids = device_ids(dest.path());
    let legacy = create(dest.path(), Format::DeviceLibrary, "TAG LIST 367", vec![ids[0], ids[1]]);
    let one = create(dest.path(), Format::OneLibrary, "TAG LIST 368", vec![ids[2]]);
    assert_eq!(legacy, one, "each library numbers its own playlists, so both take the same id");

    let report = sync_to(dest.path(), &tracks, &selection).unwrap();
    assert_eq!(report.tracks, tracks.len());

    let expected = vec![
        ("Set".to_owned(), titles(&["Track 1", "Track 2", "Track 3", "Track 4"])),
        ("TAG LIST 367".to_owned(), titles(&["Track 1", "Track 2"])),
        ("TAG LIST 368".to_owned(), titles(&["Track 3"])),
    ];
    assert_eq!(playlists(dest.path(), Format::DeviceLibrary), expected, "export.pdb keeps its Tag List");
    assert_eq!(playlists(dest.path(), Format::OneLibrary), expected, "exportLibrary.db keeps its playlist");
    let snapshot = Snapshot::read(dest.path()).unwrap();
    assert_eq!(snapshot.legacy, snapshot.one);
    assert!(rbl_export::verify_databases(dest.path()).unwrap().errors.is_empty());

    // Recorded as the stick's own: the next sync keeps both again.
    sync_to(dest.path(), &tracks, &selection).unwrap();
    assert_eq!(playlists(dest.path(), Format::DeviceLibrary), expected);
    assert_eq!(playlists(dest.path(), Format::OneLibrary), expected);
}

#[test]
fn a_playlist_both_libraries_hold_under_different_ids_is_carried_once() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (tracks, selection) = stick(src.path(), dest.path());
    let ids = device_ids(dest.path());
    // Made in the opposite order under each library: each id names "A" in
    // one and "B" in the other.
    let a = create(dest.path(), Format::DeviceLibrary, "A", vec![ids[0]]);
    create(dest.path(), Format::DeviceLibrary, "B", vec![ids[1]]);
    let b = create(dest.path(), Format::OneLibrary, "B", vec![ids[1]]);
    create(dest.path(), Format::OneLibrary, "A", vec![ids[0]]);
    assert_eq!(a, b);

    sync_to(dest.path(), &tracks, &selection).unwrap();
    let expected = vec![
        ("A".to_owned(), titles(&["Track 1"])),
        ("B".to_owned(), titles(&["Track 2"])),
        ("Set".to_owned(), titles(&["Track 1", "Track 2", "Track 3", "Track 4"])),
    ];
    assert_eq!(playlists(dest.path(), Format::DeviceLibrary), expected);
    assert_eq!(playlists(dest.path(), Format::OneLibrary), expected);
}

#[test]
fn one_playlist_the_two_libraries_fill_differently_still_stops_the_first_sync() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (tracks, selection) = stick(src.path(), dest.path());
    let ids = device_ids(dest.path());
    create(dest.path(), Format::DeviceLibrary, "TAG LIST 367", vec![ids[0], ids[1]]);
    create(dest.path(), Format::OneLibrary, "TAG LIST 367", vec![ids[2]]);

    let error = sync_to(dest.path(), &tracks, &selection).unwrap_err().to_string();
    assert_eq!(
        error,
        "USB sync conflict: The USB's two libraries, export.pdb and exportLibrary.db, differ on playlist 'TAG LIST 367', \
         so this first sync stopped before changing anything. Delete that playlist from the USB in rekordbox, then sync again."
    );
}


#[test]
fn a_later_sync_keeps_a_tag_list_whose_id_the_other_library_gives_another_playlist() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let tracks: Vec<SourceTrack> = (1..=4).map(|id| track(src.path(), id)).collect();
    let selection = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    // Synced from here before: the stick has a record of it.
    sync_to(dest.path(), &tracks, &selection).unwrap();
    let ids = device_ids(dest.path());
    create(dest.path(), Format::DeviceLibrary, "TAG LIST 367", vec![ids[0], ids[1]]);
    create(dest.path(), Format::OneLibrary, "TAG LIST 368", vec![ids[2]]);

    sync_to(dest.path(), &tracks, &selection).unwrap();
    let expected = vec![
        ("Set".to_owned(), titles(&["Track 1", "Track 2", "Track 3", "Track 4"])),
        ("TAG LIST 367".to_owned(), titles(&["Track 1", "Track 2"])),
        ("TAG LIST 368".to_owned(), titles(&["Track 3"])),
    ];
    assert_eq!(playlists(dest.path(), Format::DeviceLibrary), expected, "export.pdb's Tag List is not lost");
    assert_eq!(playlists(dest.path(), Format::OneLibrary), expected);
}
