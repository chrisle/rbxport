//! Signed `OneLibrary` ordering is normalized only after preserving sibling order.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rbl_export::{
    device_library::{self, Format},
    export,
    snapshot::Snapshot,
    SourcePlaylist, SourceTrack,
};
use std::path::Path;

fn source_tracks(source: &Path) -> Vec<SourceTrack> {
    (1..=2)
        .map(|id| {
            let path = source.join(format!("{id}.mp3"));
            std::fs::write(&path, vec![u8::try_from(id).unwrap(); 128]).unwrap();
            SourceTrack {
                id,
                source_path: path,
                title: format!("Track {id}"),
                ..Default::default()
            }
        })
        .collect()
}

fn seed(root: &Path, source: &Path, lists: &[SourcePlaylist]) -> Vec<SourceTrack> {
    let tracks = source_tracks(source);
    export(root, &tracks, lists).unwrap();
    tracks
}
fn database(root: &Path) -> std::path::PathBuf {
    root.join("PIONEER/rekordbox/exportLibrary.db")
}

#[test]
fn signed_positions_keep_order_identity_parents_and_duplicate_membership_without_writes() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    seed(root.path(), source.path(), &[]);
    let conn = rbl_onelibrary::playlists::open_read_write(&database(root.path())).unwrap();
    let large = i64::from(u32::MAX) + 1;
    let rows = [
        (90, 0, -9, "Minus nine", 0),
        (10, 0, -1, "Minus one", 0),
        (80, 0, 0, "Zero", 0),
        (70, 0, 4, "Folder", 1),
        (5, 0, large, "Large", 0),
        (60, 70, -4, "Child negative", 0),
        (20, 70, -4, "Child tied", 0),
        (30, 70, 0, "Child zero", 0),
        (40, 70, large, "Child large", 0),
    ];
    for (id, parent, sequence, name, attribute) in rows {
        conn.execute("INSERT INTO playlist(playlist_id,playlist_id_parent,sequenceNo,name,attribute) VALUES(?1,?2,?3,?4,?5)",rusqlite::params![id,parent,sequence,name,attribute]).unwrap();
    }
    for id in [90, 20] {
        for (position, track) in [(-2, 2), (0, 1), (3, 2)] {
            conn.execute(
                "INSERT INTO playlist_content VALUES(?1,?2,?3)",
                rusqlite::params![id, track, position],
            )
            .unwrap();
        }
    }
    drop(conn);
    let before = std::fs::read(database(root.path())).unwrap();
    let snapshot = Snapshot::read(root.path()).unwrap();
    let one = snapshot.one.unwrap();
    assert_eq!(
        one.playlists
            .iter()
            .filter(|n| n.parent == 0)
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        [90, 10, 80, 70, 5]
    );
    assert_eq!(
        one.playlists
            .iter()
            .filter(|n| n.parent == 70)
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        [20, 60, 30, 40]
    );
    for (id, parent, _, name, folder) in rows {
        let node = one.playlists.iter().find(|n| n.id == id).unwrap();
        assert_eq!(
            (node.parent, node.name.as_str(), node.folder),
            (parent, name, folder == 1)
        );
        assert_eq!(
            node.tracks,
            if [90, 20].contains(&id) {
                vec![2, 1, 2]
            } else {
                vec![]
            }
        );
    }
    let browsed = device_library::read(root.path(), Format::OneLibrary).unwrap();
    assert_eq!(
        browsed
            .nodes
            .iter()
            .filter(|n| n.parent == 0)
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        [90, 10, 80, 70, 5]
    );
    assert_eq!(
        browsed
            .nodes
            .iter()
            .filter(|n| n.parent == 70)
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        [20, 60, 30, 40]
    );
    assert_eq!(browsed.node(90).unwrap().tracks, [2, 1, 2]);
    assert_eq!(std::fs::read(database(root.path())).unwrap(), before);
}

#[test]
fn invalid_playlist_identity_parent_and_membership_are_refused_without_coercion() {
    for sql in [
        "UPDATE playlist SET playlist_id=-1",
        "UPDATE playlist SET playlist_id=4294967296",
        "UPDATE playlist SET playlist_id_parent=-1",
        "UPDATE playlist SET playlist_id_parent=4294967296",
        "UPDATE playlist_content SET content_id=-1",
        "UPDATE playlist_content SET content_id=4294967296",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        seed(
            root.path(),
            source.path(),
            &[SourcePlaylist {
                id: 10,
                name: "Set".into(),
                track_indices: vec![0, 1],
                ..Default::default()
            }],
        );
        let conn = rbl_onelibrary::playlists::open_read_write(&database(root.path())).unwrap();
        conn.execute_batch(sql).unwrap();
        drop(conn);
        let before = std::fs::read(database(root.path())).unwrap();
        assert!(Snapshot::read(root.path()).is_err(), "{sql}");
        assert!(
            device_library::read(root.path(), Format::OneLibrary).is_err(),
            "{sql}"
        );
        assert_eq!(std::fs::read(database(root.path())).unwrap(), before);
    }
}

#[test]
fn first_and_repeated_sync_preserve_signed_order_without_copying_audio() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let mut lists = vec![
        SourcePlaylist {
            id: 10,
            name: "First".into(),
            track_indices: vec![0, 1, 0],
            ..Default::default()
        },
        SourcePlaylist {
            id: 20,
            name: "Folder".into(),
            folder: true,
            ..Default::default()
        },
        SourcePlaylist {
            id: 30,
            parent_id: 20,
            name: "Early".into(),
            track_indices: vec![1, 0],
            ..Default::default()
        },
        SourcePlaylist {
            id: 40,
            parent_id: 20,
            name: "Late".into(),
            track_indices: vec![0],
            ..Default::default()
        },
        SourcePlaylist {
            id: 50,
            name: "Last".into(),
            track_indices: vec![1],
            ..Default::default()
        },
    ];
    let tracks = source_tracks(source.path());
    let sync = rbl_export::SyncSource {
        db_id: 4242,
        ..Default::default()
    };
    rbl_export::export_full(
        root.path(),
        &tracks,
        &lists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    let baseline = Snapshot::read(root.path()).unwrap().one.unwrap();
    // Explicitly bind the selected source playlists to their existing device rows,
    // as a first sync does after reading the foreign device's playlists.
    for list in &mut lists {
        list.device_id = baseline
            .playlists
            .iter()
            .find(|node| node.name == list.name)
            .unwrap()
            .id;
    }
    let run = || {
        rbl_export::export_full(
            root.path(),
            &tracks,
            &lists,
            &[],
            None,
            Some(&sync),
            &mut |_| {},
        )
    };
    let conn = rbl_onelibrary::playlists::open_read_write(&database(root.path())).unwrap();
    for (name, position) in [
        ("First", -10),
        ("Folder", -1),
        ("Last", 0),
        ("Early", -5),
        ("Late", 0),
    ] {
        assert_eq!(
            conn.execute(
                "UPDATE playlist SET sequenceNo=?1 WHERE name=?2",
                rusqlite::params![position, name]
            )
            .unwrap(),
            1
        );
    }
    drop(conn);
    std::fs::remove_file(rbl_export::Manifest::path(root.path())).unwrap();
    let snapshot = Snapshot::read(root.path()).unwrap();
    assert_eq!(snapshot.one.as_ref().unwrap(), &baseline);
    snapshot.check_baseline(None, 0).unwrap();
    for _ in 0..2 {
        let report = run().unwrap();
        assert_eq!((report.bytes_copied, report.tracks_added), (0, 0));
        assert_eq!(Snapshot::read(root.path()).unwrap().one.unwrap(), baseline);
    }
}

fn position_fixture(root: &Path, source: &Path, positions: &[i64]) {
    seed(root, source, &[]);
    let conn = rbl_onelibrary::playlists::open_read_write(&database(root)).unwrap();
    for (index, sequence) in positions.iter().enumerate() {
        let id = i64::try_from(index).unwrap() + 1;
        conn.execute("INSERT INTO playlist(playlist_id,playlist_id_parent,sequenceNo,name,attribute) VALUES(?1,0,?2,?3,0)",rusqlite::params![id,sequence,format!("List {id}")]).unwrap();
    }
    for (position, track) in [(1, 2), (2, 1), (3, 2)] {
        conn.execute(
            "INSERT INTO playlist_content VALUES(1,?1,?2)",
            rusqlite::params![track, position],
        )
        .unwrap();
    }
}

fn raw_positions(root: &Path) -> Vec<(i64, i64)> {
    let db = rbl_onelibrary::ExportLibrary::open_read_only(&database(root)).unwrap();
    rbl_onelibrary::playlists::read(db.connection())
        .unwrap()
        .0
        .into_iter()
        .map(|row| (row.id, row.sequence))
        .collect()
}

fn assert_delete_order(positions: &[i64], removed: u32, expected: &[u32]) {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    position_fixture(root.path(), source.path(), positions);
    let pdb = root.path().join("PIONEER/rekordbox/export.pdb");
    let other = std::fs::read(&pdb).unwrap();
    device_library::apply(
        root.path(),
        Format::OneLibrary,
        &device_library::Edit::Delete { id: removed },
    )
    .unwrap();
    let library = device_library::read(root.path(), Format::OneLibrary).unwrap();
    assert_eq!(
        library.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(library.node(1).unwrap().tracks, [2, 1, 2]);
    assert_eq!(
        raw_positions(root.path()),
        expected
            .iter()
            .copied()
            .enumerate()
            .map(|(rank, id)| (i64::from(id), i64::try_from(rank).unwrap()))
            .collect::<Vec<_>>()
    );
    assert_eq!(std::fs::read(pdb).unwrap(), other);
}

#[test]
fn deleting_reifies_all_planned_siblings_after_signed_ordering() {
    assert_delete_order(&[-10, 100, 200, 300], 3, &[1, 2, 4]);
}

#[test]
fn deleting_reifies_all_planned_siblings_after_oversized_ordering() {
    let large = i64::from(u32::MAX) + 1;
    assert_delete_order(&[100, large, large + 1], 2, &[1, 3]);
}

#[test]
fn creating_reifies_signed_and_oversized_sibling_positions() {
    let large = i64::from(u32::MAX) + 1;
    for positions in [vec![-10, 100, 200], vec![100, large, large + 1]] {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        position_fixture(root.path(), source.path(), &positions);
        let made = device_library::apply(
            root.path(),
            Format::OneLibrary,
            &device_library::Edit::Create {
                parent: 0,
                name: "New".into(),
                folder: false,
            },
        )
        .unwrap();
        assert_eq!(made.id, 4);
        let library = device_library::read(root.path(), Format::OneLibrary).unwrap();
        assert_eq!(
            library.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
            [4, 1, 2, 3]
        );
        assert_eq!(library.node(1).unwrap().tracks, [2, 1, 2]);
        assert_eq!(raw_positions(root.path()), [(1, 1), (2, 2), (3, 3), (4, 0)]);
    }
}

#[test]
fn name_and_membership_edits_preserve_raw_signed_positions() {
    let large = i64::from(u32::MAX) + 1;
    for positions in [vec![-10, 100, 200], vec![100, large, large + 1]] {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        position_fixture(root.path(), source.path(), &positions);
        let before = raw_positions(root.path());
        for edit in [
            device_library::Edit::Rename {
                id: 1,
                name: "Renamed".into(),
            },
            device_library::Edit::Add {
                playlist: 1,
                tracks: vec![1],
                duplicates: true,
            },
            device_library::Edit::Remove {
                playlist: 1,
                tracks: vec![2],
            },
        ] {
            device_library::apply(root.path(), Format::OneLibrary, &edit).unwrap();
            assert_eq!(raw_positions(root.path()), before);
        }
        let library = device_library::read(root.path(), Format::OneLibrary).unwrap();
        assert_eq!(library.node(1).unwrap().name, "Renamed");
        assert_eq!(library.node(1).unwrap().tracks, [1, 1]);
    }
}
