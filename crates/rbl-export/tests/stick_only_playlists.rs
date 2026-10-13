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

mod common;

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
    if !tracks.is_empty() {
        device_library::apply(dest, format, &Edit::Add { playlist: id, tracks, duplicates: false }).unwrap();
    }
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
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
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

/// Where each id names one playlist in export.pdb and another in
/// exportLibrary.db, the same-named lists with the same tracks are one
/// playlist: carrying export.pdb's under a new id would make a second copy.
#[test]
fn a_playlist_both_libraries_hold_under_different_ids_is_carried_once() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
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
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
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
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
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

/// "Gig" in each library under its own id, filled differently: export.pdb
/// takes id 3 (a playlist made before it was deleted), exportLibrary.db id 2.
/// Two playlists that share a name, not one the libraries disagree on.
fn same_name_other_id_other_tracks(dest: &Path) {
    let ids = device_ids(dest);
    let pad = create(dest, Format::DeviceLibrary, "Pad", vec![]);
    let legacy = create(dest, Format::DeviceLibrary, "Gig", vec![ids[0], ids[1]]);
    device_library::apply(dest, Format::DeviceLibrary, &Edit::Delete { id: pad }).unwrap();
    let one = create(dest, Format::OneLibrary, "Gig", vec![ids[2]]);
    assert_ne!(legacy, one);
}

fn both_gigs() -> Vec<(String, Vec<String>)> {
    vec![
        ("Gig".to_owned(), titles(&["Track 1", "Track 2"])),
        ("Gig".to_owned(), titles(&["Track 3"])),
        ("Set".to_owned(), titles(&["Track 1", "Track 2", "Track 3", "Track 4"])),
    ]
}

#[test]
fn a_first_sync_keeps_two_same_named_playlists_under_other_ids_with_other_tracks() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, selection) = stick(src.path(), dest.path());
    same_name_other_id_other_tracks(dest.path());

    sync_to(dest.path(), &tracks, &selection).unwrap();
    assert_eq!(playlists(dest.path(), Format::DeviceLibrary), both_gigs(), "export.pdb keeps its Gig");
    assert_eq!(playlists(dest.path(), Format::OneLibrary), both_gigs(), "exportLibrary.db keeps its Gig");
    assert!(rbl_export::verify_databases(dest.path()).unwrap().errors.is_empty());

    sync_to(dest.path(), &tracks, &selection).unwrap();
    assert_eq!(playlists(dest.path(), Format::DeviceLibrary), both_gigs());
    assert_eq!(playlists(dest.path(), Format::OneLibrary), both_gigs());
}

#[test]
fn a_later_sync_keeps_two_same_named_playlists_under_other_ids_with_other_tracks() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let tracks: Vec<SourceTrack> = (1..=4).map(|id| track(src.path(), id)).collect();
    let selection = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    sync_to(dest.path(), &tracks, &selection).unwrap();
    same_name_other_id_other_tracks(dest.path());

    sync_to(dest.path(), &tracks, &selection).unwrap();
    assert_eq!(playlists(dest.path(), Format::DeviceLibrary), both_gigs(), "export.pdb's Gig is not lost");
    assert_eq!(playlists(dest.path(), Format::OneLibrary), both_gigs());
}

/// The same, where exportLibrary.db gives export.pdb's id to another
/// playlist: export.pdb's "Gig" is carried under a new id.
#[test]
fn a_same_named_playlist_with_other_tracks_whose_id_is_taken_is_carried_under_a_new_id() {
    for record in [false, true] {
        let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
        let (tracks, selection) = stick(src.path(), dest.path());
        if record {
            sync_to(dest.path(), &tracks, &selection).unwrap();
        }
        let ids = device_ids(dest.path());
        let legacy = create(dest.path(), Format::DeviceLibrary, "Gig", vec![ids[0], ids[1]]);
        let other = create(dest.path(), Format::OneLibrary, "Warmup", vec![ids[3]]);
        create(dest.path(), Format::OneLibrary, "Gig", vec![ids[2]]);
        assert_eq!(legacy, other);

        sync_to(dest.path(), &tracks, &selection).unwrap();
        let expected = vec![
            ("Gig".to_owned(), titles(&["Track 1", "Track 2"])),
            ("Gig".to_owned(), titles(&["Track 3"])),
            ("Set".to_owned(), titles(&["Track 1", "Track 2", "Track 3", "Track 4"])),
            ("Warmup".to_owned(), titles(&["Track 4"])),
        ];
        assert_eq!(playlists(dest.path(), Format::DeviceLibrary), expected, "record: {record}");
        assert_eq!(playlists(dest.path(), Format::OneLibrary), expected, "record: {record}");
    }
}

/// "Gig" in each library under its own id, export.pdb's (3) free in
/// exportLibrary.db, which holds its "Gig" under id 2: made under each
/// library in turn, after export.pdb used and freed id 2 for "Pad".
fn same_name_free_id(dest: &Path, legacy_tracks: Vec<u32>, one_tracks: Vec<u32>) {
    let pad = create(dest, Format::DeviceLibrary, "Pad", vec![]);
    let legacy = create(dest, Format::DeviceLibrary, "Gig", legacy_tracks);
    device_library::apply(dest, Format::DeviceLibrary, &Edit::Delete { id: pad }).unwrap();
    let one = create(dest, Format::OneLibrary, "Gig", one_tracks);
    assert_ne!(legacy, one);
    let library = device_library::read(dest, Format::OneLibrary).unwrap();
    assert!(library.nodes.iter().all(|n| n.id != legacy), "export.pdb's id is free in exportLibrary.db");
}

/// The same list made under each library, the two numbering it apart, is
/// kept twice, as before #353: export.pdb's keeps its own id, so the check
/// of each database by id before writing finds it (the round-2 review of
/// PR #369 found the sync refused here).
#[test]
fn same_named_playlists_with_the_same_tracks_under_free_ids_are_both_kept() {
    for record in [false, true] {
        for empty in [false, true] {
            let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
            let (tracks, selection) = stick(src.path(), dest.path());
            if record {
                sync_to(dest.path(), &tracks, &selection).unwrap();
            }
            let ids = device_ids(dest.path());
            let gig = if empty { vec![] } else { vec![ids[0], ids[1]] };
            same_name_free_id(dest.path(), gig.clone(), gig);

            let case = format!("record: {record}, empty: {empty}");
            sync_to(dest.path(), &tracks, &selection).unwrap_or_else(|e| panic!("{case}: {e}"));
            let gig = if empty { vec![] } else { titles(&["Track 1", "Track 2"]) };
            let expected = vec![
                ("Gig".to_owned(), gig.clone()),
                ("Gig".to_owned(), gig),
                ("Set".to_owned(), titles(&["Track 1", "Track 2", "Track 3", "Track 4"])),
            ];
            assert_eq!(playlists(dest.path(), Format::DeviceLibrary), expected, "{case}");
            assert_eq!(playlists(dest.path(), Format::OneLibrary), expected, "{case}");
            let first = Snapshot::read(dest.path()).unwrap();

            sync_to(dest.path(), &tracks, &selection).unwrap_or_else(|e| panic!("{case}, again: {e}"));
            let again = Snapshot::read(dest.path()).unwrap();
            assert_eq!((&again.legacy, &again.one), (&first.legacy, &first.one), "{case}: a repeat sync changes nothing");
        }
    }
}

/// How the two "Gig" lists' tracks relate.
#[derive(Debug, Clone, Copy)]
enum Contents {
    Equal,
    Empty,
    Different,
    Reordered,
}

/// `Gig` in each library, export.pdb's under an id exportLibrary.db gives
/// "Pad" (`collide`) or under one it leaves free; first synced from here
/// or not; then synced again. Each sync goes ahead (dev refuses only some
/// first syncs that collide, and silently drops export.pdb's "Gig" on a
/// later one), every list either library held is in both afterwards, the
/// two libraries agree, and a repeat sync changes nothing.
fn matrix(collide: bool) {
    for contents in [Contents::Equal, Contents::Empty, Contents::Different, Contents::Reordered] {
        for record in [false, true] {
            let case = format!("collide: {collide}, {contents:?}, {}", if record { "later sync" } else { "first sync" });
            let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
            let (tracks, selection) = stick(src.path(), dest.path());
            if record {
                sync_to(dest.path(), &tracks, &selection).unwrap();
            }
            let ids = device_ids(dest.path());
            let (legacy_gig, one_gig) = match contents {
                Contents::Equal => (vec![ids[0], ids[1]], vec![ids[0], ids[1]]),
                Contents::Empty => (vec![], vec![]),
                Contents::Different => (vec![ids[0], ids[1]], vec![ids[2]]),
                Contents::Reordered => (vec![ids[0], ids[1]], vec![ids[1], ids[0]]),
            };
            if collide {
                let legacy = create(dest.path(), Format::DeviceLibrary, "Gig", legacy_gig);
                let pad = create(dest.path(), Format::OneLibrary, "Pad", vec![]);
                create(dest.path(), Format::OneLibrary, "Gig", one_gig);
                assert_eq!(legacy, pad, "{case}");
            } else {
                same_name_free_id(dest.path(), legacy_gig, one_gig);
            }
            let before: Vec<_> = playlists(dest.path(), Format::DeviceLibrary)
                .into_iter()
                .chain(playlists(dest.path(), Format::OneLibrary))
                .collect();

            sync_to(dest.path(), &tracks, &selection).unwrap_or_else(|e| panic!("{case}: {e}"));
            let after = playlists(dest.path(), Format::DeviceLibrary);
            assert_eq!(playlists(dest.path(), Format::OneLibrary), after, "{case}: the two libraries agree");
            for list in &before {
                assert!(after.contains(list), "{case}: {list:?} is lost; after: {after:?}");
            }
            assert_eq!(device_ids(dest.path()).len(), tracks.len(), "{case}: no track is lost");
            let gigs = after.iter().filter(|(name, _)| name == "Gig").count();
            // Only the same list, under an id the other library gives to
            // "Pad", is one playlist; every other layout keeps both, as dev.
            let one_gig = collide && matches!(contents, Contents::Equal | Contents::Empty);
            assert_eq!(gigs, if one_gig { 1 } else { 2 }, "{case}: {after:?}");
            let first = Snapshot::read(dest.path()).unwrap();

            sync_to(dest.path(), &tracks, &selection).unwrap_or_else(|e| panic!("{case}, repeat sync: {e}"));
            let again = Snapshot::read(dest.path()).unwrap();
            assert_eq!((&again.legacy, &again.one), (&first.legacy, &first.one), "{case}: a repeat sync changes nothing");
        }
    }
}

#[test]
fn matrix_ids_that_collide() {
    matrix(true);
}

#[test]
fn matrix_ids_that_do_not_collide() {
    matrix(false);
}

/// A folder "F" holding "Gig" made under each library, export.pdb's folder
/// and list under ids exportLibrary.db leaves free: both folders are kept,
/// each with its list, on a first sync and a later one, as before #353.
#[test]
fn same_named_folders_under_free_ids_are_both_kept() {
    for record in [false, true] {
        let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
        let (tracks, selection) = stick(src.path(), dest.path());
        if record {
            sync_to(dest.path(), &tracks, &selection).unwrap();
        }
        let ids = device_ids(dest.path());
        let folder = |format: Format| {
            device_library::apply(dest.path(), format, &Edit::Create { parent: 0, name: "F".into(), folder: true }).unwrap().id
        };
        let child = |format: Format, parent: u32, tracks: Vec<u32>| {
            let id = device_library::apply(dest.path(), format, &Edit::Create { parent, name: "Gig".into(), folder: false }).unwrap().id;
            device_library::apply(dest.path(), format, &Edit::Add { playlist: id, tracks, duplicates: false }).unwrap();
        };
        let pad = create(dest.path(), Format::DeviceLibrary, "Pad", vec![]);
        let pad2 = create(dest.path(), Format::DeviceLibrary, "Pad2", vec![]);
        let legacy = folder(Format::DeviceLibrary);
        child(Format::DeviceLibrary, legacy, vec![ids[0]]);
        device_library::apply(dest.path(), Format::DeviceLibrary, &Edit::Delete { id: pad }).unwrap();
        device_library::apply(dest.path(), Format::DeviceLibrary, &Edit::Delete { id: pad2 }).unwrap();
        let one = folder(Format::OneLibrary);
        child(Format::OneLibrary, one, vec![ids[0]]);
        let one_ids: Vec<u32> = device_library::read(dest.path(), Format::OneLibrary).unwrap().nodes.iter().map(|n| n.id).collect();
        assert!(!one_ids.contains(&legacy) && !one_ids.contains(&(legacy + 1)), "record: {record}: {one_ids:?}");

        let case = format!("record: {record}");
        sync_to(dest.path(), &tracks, &selection).unwrap_or_else(|e| panic!("{case}: {e}"));
        let expected = vec![
            ("F".to_owned(), vec![]),
            ("F".to_owned(), vec![]),
            ("Gig".to_owned(), titles(&["Track 1"])),
            ("Gig".to_owned(), titles(&["Track 1"])),
            ("Set".to_owned(), titles(&["Track 1", "Track 2", "Track 3", "Track 4"])),
        ];
        assert_eq!(playlists(dest.path(), Format::DeviceLibrary), expected, "{case}");
        assert_eq!(playlists(dest.path(), Format::OneLibrary), expected, "{case}");
        let library = device_library::read(dest.path(), Format::DeviceLibrary).unwrap();
        for f in library.nodes.iter().filter(|n| n.folder) {
            assert_eq!(library.nodes.iter().filter(|n| n.parent == f.id).count(), 1, "{case}: each F keeps its Gig");
        }
    }
}
