//! Editing a stick's playlists in place, one library at a time, as
//! rekordbox's Devices tree does.
//!
//! Every stick here is written by the export pipeline into a temporary
//! directory; nothing touches a real device.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use rbl_export::device_library::{self, Applied, Edit, Format};
use rbl_export::{export, verify, SourcePlaylist, SourceTrack};

fn track(dir: &Path, id: u64, title: &str) -> SourceTrack {
    let path = dir.join(format!("source-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: title.into(),
        artist: "TRIODE".into(),
        album: "Single".into(),
        genre: "House".into(),
        key: "Am".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        rating: 4,
        date_added: "2026-09-06".into(),
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

/// A stick with a folder holding one list, and a list at the top level.
fn stick(src: &Path, dest: &Path) {
    let tracks: Vec<SourceTrack> = (1..=4).map(|i| track(src, i, &format!("Track {i}"))).collect();
    let playlists = vec![
        SourcePlaylist { id: 10, name: "Sets".into(), folder: true, ..Default::default() },
        SourcePlaylist { id: 11, name: "Friday".into(), parent_id: 10, track_indices: vec![0, 1], ..Default::default() },
        SourcePlaylist { id: 12, name: "Warm Up".into(), track_indices: vec![2], ..Default::default() },
    ];
    export(dest, &tracks, &playlists).unwrap();
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

/// `(id, parent, name, folder, tracks)` of every node, in tree order.
fn shape(dest: &Path, format: Format) -> Vec<(u32, u32, String, bool, Vec<u32>)> {
    device_library::read(dest, format)
        .unwrap()
        .nodes
        .into_iter()
        .map(|n| (n.id, n.parent, n.name, n.folder, n.tracks))
        .collect()
}

fn id_of(dest: &Path, format: Format, name: &str) -> u32 {
    device_library::read(dest, format).unwrap().nodes.iter().find(|n| n.name == name).unwrap().id
}

#[test]
fn both_libraries_of_an_export_read_as_the_same_tree() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    stick(src.path(), dest.path());
    assert_eq!(device_library::formats(dest.path()), vec![Format::DeviceLibrary, Format::OneLibrary]);
    let legacy = device_library::read(dest.path(), Format::DeviceLibrary).unwrap();
    let one = device_library::read(dest.path(), Format::OneLibrary).unwrap();
    assert_eq!(shape(dest.path(), Format::DeviceLibrary), shape(dest.path(), Format::OneLibrary));
    let names: Vec<&str> = legacy.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Sets", "Friday", "Warm Up"]);
    assert_eq!(legacy.depth(legacy.nodes[1].id), 1);
    assert_eq!(legacy.tracks.len(), 4);
    let first = legacy.track(legacy.nodes[1].tracks[0]).unwrap();
    assert_eq!((first.title.as_str(), first.artist.as_str(), first.bpm_x100, first.rating), ("Track 1", "TRIODE", 12_800, 4));
    // Only OneLibrary names the desktop track each one was exported from.
    assert!(legacy.tracks.iter().all(|t| (t.master_db_id, t.master_content_id) == (0, 0)));
    let mut masters: Vec<u64> = one.tracks.iter().map(|t| t.master_content_id).collect();
    masters.sort_unstable();
    assert_eq!(masters, [1, 2, 3, 4], "masterContentId is the source track's id");
    assert_eq!(
        legacy.tracks,
        one.tracks.iter().map(|t| device_library::Track { master_db_id: 0, master_content_id: 0, ..t.clone() }).collect::<Vec<_>>()
    );
}

#[test]
fn an_edit_changes_its_own_library_and_leaves_the_other_byte_for_byte() {
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        stick(src.path(), dest.path());
        let untouched = db(dest.path(), other(format));
        let audio = std::fs::read(dest.path().join("Contents/TRIODE/Single/source-1.mp3")).unwrap();

        let sets = id_of(dest.path(), format, "Sets");
        let created = device_library::apply(dest.path(), format, &Edit::Create { parent: sets, name: "Saturday".into(), folder: false }).unwrap();
        assert_eq!(created.changed, 1);
        device_library::apply(dest.path(), format, &Edit::Add { playlist: created.id, tracks: vec![4, 3] }).unwrap();
        let warm = id_of(dest.path(), format, "Warm Up");
        device_library::apply(dest.path(), format, &Edit::Rename { id: warm, name: "Opening".into() }).unwrap();

        let tree = shape(dest.path(), format);
        let saturday = tree.iter().find(|n| n.2 == "Saturday").unwrap();
        assert_eq!((saturday.1, saturday.3, saturday.4.clone()), (sets, false, vec![4, 3]), "{format:?}");
        assert!(tree.iter().any(|n| n.2 == "Opening"));
        assert!(!tree.iter().any(|n| n.2 == "Warm Up"));
        // At the top of its folder, above the list that was already there,
        // as rekordbox puts a new one.
        let under: Vec<&str> = tree.iter().filter(|n| n.1 == sets).map(|n| n.2.as_str()).collect();
        assert_eq!(under, ["Saturday", "Friday"]);

        assert_eq!(db(dest.path(), other(format)), untouched, "{format:?} edit must not touch the other library");
        assert_eq!(std::fs::read(dest.path().join("Contents/TRIODE/Single/source-1.mp3")).unwrap(), audio);
        // The two now disagree, as rekordbox leaves them.
        assert_ne!(shape(dest.path(), format), shape(dest.path(), other(format)));
    }
}

#[test]
fn the_same_edits_made_in_both_libraries_leave_a_stick_that_verifies() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    stick(src.path(), dest.path());
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let folder = device_library::apply(dest.path(), format, &Edit::Create { parent: 0, name: "Gigs".into(), folder: true }).unwrap();
        let list = device_library::apply(dest.path(), format, &Edit::Create { parent: folder.id, name: "Club".into(), folder: false }).unwrap();
        device_library::apply(dest.path(), format, &Edit::Add { playlist: list.id, tracks: vec![1, 2, 3] }).unwrap();
        device_library::apply(dest.path(), format, &Edit::Remove { playlist: list.id, tracks: vec![2] }).unwrap();
        let friday = id_of(dest.path(), format, "Friday");
        device_library::apply(dest.path(), format, &Edit::Remove { playlist: friday, tracks: vec![1] }).unwrap();
        let sets = id_of(dest.path(), format, "Sets");
        let gone = device_library::apply(dest.path(), format, &Edit::Delete { id: sets }).unwrap();
        assert_eq!(gone, Applied { id: sets, changed: 2 });
    }
    assert_eq!(shape(dest.path(), Format::DeviceLibrary), shape(dest.path(), Format::OneLibrary));
    let names: Vec<String> = shape(dest.path(), Format::DeviceLibrary).into_iter().map(|n| n.2).collect();
    assert_eq!(names, ["Gigs", "Club", "Warm Up"]);
    let club = shape(dest.path(), Format::DeviceLibrary).into_iter().find(|n| n.2 == "Club").unwrap();
    assert_eq!(club.4, vec![1, 3]);
    let check = verify(dest.path()).unwrap();
    assert!(check.is_ok(), "{:?}", check.errors);
    assert_eq!(check.tracks, 4, "deleting a playlist leaves its tracks on the stick");
    assert_eq!(check.playlists, 3);
}

#[test]
fn a_large_playlist_grows_and_shrinks_across_pages() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let tracks: Vec<SourceTrack> = (1..=600).map(|i| track(src.path(), i, &format!("T{i}"))).collect();
    export(dest.path(), &tracks, &[SourcePlaylist { id: 1, name: "All".into(), track_indices: (0..600).collect(), ..Default::default() }]).unwrap();
    let all = id_of(dest.path(), Format::DeviceLibrary, "All");
    let copy = device_library::apply(dest.path(), Format::DeviceLibrary, &Edit::Create { parent: 0, name: "Copy".into(), folder: false }).unwrap();
    device_library::apply(dest.path(), Format::DeviceLibrary, &Edit::Add { playlist: copy.id, tracks: (1..=600).collect() }).unwrap();
    let read = device_library::read(dest.path(), Format::DeviceLibrary).unwrap();
    assert_eq!(read.node(copy.id).unwrap().tracks.len(), 600);
    assert_eq!(read.node(all).unwrap().tracks, (1..=600).collect::<Vec<u32>>());
    device_library::apply(dest.path(), Format::DeviceLibrary, &Edit::Remove { playlist: all, tracks: (1..=599).collect() }).unwrap();
    let read = device_library::read(dest.path(), Format::DeviceLibrary).unwrap();
    assert_eq!(read.node(all).unwrap().tracks, vec![600]);
    assert_eq!(read.node(copy.id).unwrap().tracks.len(), 600);
    let pdb = rbl_pdb::Pdb::parse(&db(dest.path(), Format::DeviceLibrary)).unwrap().census();
    assert_eq!(pdb["tracks"], 600);
}

#[test]
fn a_refused_edit_writes_nothing() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    stick(src.path(), dest.path());
    let before = (db(dest.path(), Format::DeviceLibrary), db(dest.path(), Format::OneLibrary));
    let friday = id_of(dest.path(), Format::DeviceLibrary, "Friday");
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        assert!(device_library::apply(dest.path(), format, &Edit::Add { playlist: friday, tracks: vec![99] }).is_err());
        assert!(device_library::apply(dest.path(), format, &Edit::Create { parent: friday, name: "x".into(), folder: false }).is_err());
        assert!(device_library::apply(dest.path(), format, &Edit::Rename { id: 404, name: "x".into() }).is_err());
        let unchanged = device_library::apply(dest.path(), format, &Edit::Rename { id: friday, name: "Friday".into() }).unwrap();
        assert_eq!(unchanged.changed, 0);
    }
    assert_eq!((db(dest.path(), Format::DeviceLibrary), db(dest.path(), Format::OneLibrary)), before);
}

#[test]
fn a_playlist_made_on_the_device_survives_the_next_sync() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let tracks: Vec<SourceTrack> = (1..=3).map(|i| track(src.path(), i, &format!("Track {i}"))).collect();
    let playlists = vec![SourcePlaylist { id: 1, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }];
    export(dest.path(), &tracks, &playlists).unwrap();
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let made = device_library::apply(dest.path(), format, &Edit::Create { parent: 0, name: "Made on the stick".into(), folder: false }).unwrap();
        device_library::apply(dest.path(), format, &Edit::Add { playlist: made.id, tracks: vec![2] }).unwrap();
    }
    // The same selection synced again keeps what the stick gained.
    export(dest.path(), &tracks, &playlists).unwrap();
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let tree = shape(dest.path(), format);
        let made = tree.iter().find(|n| n.2 == "Made on the stick").expect("kept");
        assert_eq!(made.4.len(), 1);
    }
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_hidden_root_stick_is_edited_where_it_is() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    rbl_export::export_with_options(
        dest.path(),
        &[track(src.path(), 1, "Hidden")],
        &[SourcePlaylist { id: 1, name: "Set".into(), track_indices: vec![0], ..Default::default() }],
        &[],
        &rbl_export::ExportOptions { root: Some(rbl_export::ExportRoot::Hidden), ..Default::default() },
        &mut |_| {},
    )
    .unwrap();
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        device_library::apply(dest.path(), format, &Edit::Create { parent: 0, name: "New".into(), folder: false }).unwrap();
        assert!(shape(dest.path(), format).iter().any(|n| n.2 == "New"));
    }
    assert!(!dest.path().join("PIONEER").exists(), "no second library root may appear");
}

/// The stick rekordbox was given on the rig: a folder holding a list, and a
/// list at the top level, written by this exporter.
fn rig_stick(src: &Path, dest: &Path) {
    let tracks: Vec<SourceTrack> = (1..=4).map(|i| track(src, i, &format!("Track {i}"))).collect();
    let playlists = vec![
        SourcePlaylist { id: 10, name: "Folder A".into(), folder: true, ..Default::default() },
        SourcePlaylist { id: 11, name: "Inside A".into(), parent_id: 10, track_indices: vec![0, 1], ..Default::default() },
        SourcePlaylist { id: 12, name: "Top List".into(), track_indices: vec![2, 3, 0], ..Default::default() },
    ];
    export(dest, &tracks, &playlists).unwrap();
}

/// `(id, parent, sequence, name, tracks)` of every node, by id.
fn rows(dest: &Path, format: Format) -> Vec<(u32, u32, u32, String, Vec<u32>)> {
    let mut out: Vec<_> = device_library::read(dest, format)
        .unwrap()
        .nodes
        .into_iter()
        .map(|n| (n.id, n.parent, n.sequence, n.name, n.tracks))
        .collect();
    out.sort();
    out
}

fn row(id: u32, parent: u32, sequence: u32, name: &str, tracks: &[u32]) -> (u32, u32, u32, String, Vec<u32>) {
    (id, parent, sequence, name.to_owned(), tracks.to_vec())
}

/// The edits made in rekordbox 7.2.14's Devices tree on the rig, made here on
/// the same stick, leave the rows rekordbox left [OBS Winrig 2026-10-08:
/// `parity/issue-186/rig-stick-0-connected` to `rig-stick-9`].
#[test]
fn the_rigs_rekordbox_edits_leave_the_same_rows_here() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    rig_stick(src.path(), dest.path());
    let d = dest.path();
    // What rekordbox read before any edit [OBS rig-stick-0-connected].
    assert_eq!(rows(d, Format::DeviceLibrary), vec![row(1, 0, 1, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 3, "Top List", &[3, 4, 1])]);
    assert_eq!(rows(d, Format::OneLibrary), vec![row(1, 0, 0, "Folder A", &[]), row(2, 1, 1, "Inside A", &[1, 2]), row(3, 0, 2, "Top List", &[3, 4, 1])]);

    let pdb = Format::DeviceLibrary;
    let made = device_library::apply(d, pdb, &Edit::Create { parent: 0, name: "Untitled Playlist".into(), folder: false }).unwrap();
    assert_eq!(made.id, 4);
    device_library::apply(d, pdb, &Edit::Rename { id: 4, name: "RB New".into() }).unwrap();
    assert_eq!(rows(d, pdb), vec![row(1, 0, 2, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 4, "Top List", &[3, 4, 1]), row(4, 0, 0, "RB New", &[])]);
    device_library::apply(d, pdb, &Edit::Create { parent: 0, name: "Untitled Folder".into(), folder: true }).unwrap();
    device_library::apply(d, pdb, &Edit::Add { playlist: 4, tracks: vec![3] }).unwrap();
    device_library::apply(d, pdb, &Edit::Remove { playlist: 3, tracks: vec![4] }).unwrap();
    device_library::apply(d, pdb, &Edit::Rename { id: 3, name: "Top Renamed".into() }).unwrap();
    assert_eq!(rows(d, pdb), vec![
        row(1, 0, 3, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 5, "Top Renamed", &[3, 1]),
        row(4, 0, 1, "RB New", &[3]), row(5, 0, 0, "Untitled Folder", &[]),
    ]);
    device_library::apply(d, pdb, &Edit::Delete { id: 4 }).unwrap();
    assert_eq!(rows(d, pdb), vec![row(1, 0, 1, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 2, "Top Renamed", &[3, 1]), row(5, 0, 0, "Untitled Folder", &[])]);
    // None of that touched OneLibrary, as none of it touched rekordbox's.
    assert_eq!(rows(d, Format::OneLibrary), vec![row(1, 0, 0, "Folder A", &[]), row(2, 1, 1, "Inside A", &[1, 2]), row(3, 0, 2, "Top List", &[3, 4, 1])]);

    let one = Format::OneLibrary;
    device_library::apply(d, one, &Edit::Create { parent: 0, name: "Untitled Playlist".into(), folder: false }).unwrap();
    assert_eq!(rows(d, one), vec![row(1, 0, 1, "Folder A", &[]), row(2, 1, 1, "Inside A", &[1, 2]), row(3, 0, 3, "Top List", &[3, 4, 1]), row(4, 0, 0, "Untitled Playlist", &[])]);
    device_library::apply(d, one, &Edit::Delete { id: 4 }).unwrap();
    assert_eq!(rows(d, one), vec![row(1, 0, 0, "Folder A", &[]), row(2, 1, 1, "Inside A", &[1, 2]), row(3, 0, 1, "Top List", &[3, 4, 1])]);
}

/// An edit that starts while another write holds the stick (an export, or
/// a second edit) waits for it, then plans against the file that write
/// left, not the one it would have read before.
#[test]
fn an_edit_waits_for_a_write_in_progress_and_plans_against_its_result() {
    for format in [Format::DeviceLibrary, Format::OneLibrary] {
        let src = tempfile::tempdir().unwrap();
        let (dest, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        stick(src.path(), dest.path());
        stick(src.path(), elsewhere.path());
        // What the other write is about to publish: the same stick with a
        // playlist more, made on a copy.
        let newer = device_library::apply(elsewhere.path(), format, &Edit::Create { parent: 0, name: "Newer".into(), folder: false }).unwrap();
        let newer_bytes = db(elsewhere.path(), format);
        let before = device_library::read(dest.path(), format).unwrap();
        assert!(before.nodes.iter().all(|n| n.name != "Newer"));

        let holding = rbl_core::durable::Publication::new(dest.path(), ".rbxport-publication").unwrap();
        let (done, finished) = std::sync::mpsc::channel();
        let root = dest.path().to_path_buf();
        let editing = std::thread::spawn(move || {
            let result = device_library::apply(&root, format, &Edit::Create { parent: 0, name: "Mine".into(), folder: false });
            done.send(result).unwrap();
        });
        assert!(
            finished.recv_timeout(std::time::Duration::from_millis(400)).is_err(),
            "{format:?}: the edit went ahead while another write held the stick"
        );
        let relative = Path::new("PIONEER/rekordbox");
        std::fs::create_dir_all(holding.stage().join(relative)).unwrap();
        std::fs::write(holding.stage().join(relative).join(format.file_name()), &newer_bytes).unwrap();
        let wal = |suffix: &str| relative.join(format!("{}{suffix}", format.file_name()));
        holding.commit(&[relative.join(format.file_name()), wal("-wal"), wal("-shm")]).unwrap();
        drop(holding);

        let applied = finished.recv_timeout(std::time::Duration::from_secs(30)).unwrap().unwrap();
        editing.join().unwrap();
        assert_eq!(applied, Applied { id: newer.id + 1, changed: 1 }, "{format:?}: planned against the newer file");
        let after = device_library::read(dest.path(), format).unwrap();
        // Both new playlists, the later one on top; the rest move down.
        let top: Vec<&str> = after.nodes.iter().filter(|n| n.parent == 0).map(|n| n.name.as_str()).collect();
        assert_eq!(top, ["Mine", "Newer", "Sets", "Warm Up"], "{format:?}");
        assert_eq!(after.nodes.iter().filter(|n| n.parent == 0).take(2).map(|n| n.sequence).collect::<Vec<_>>(), [0, 1], "{format:?}");
    }
}
