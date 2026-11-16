//! OneLibrary's My Tag `sequenceNo` is zero-based and consecutive in each
//! sibling group, as the Device Library writer and rekordbox have it, and a
//! sync tolerates the one-based/zero-based renumbering without a false
//! "changed on the USB" conflict (#893). Synthetic library and temporary
//! stick only; rekordbox on a physical USB was not run [UNKNOWN].
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;

use std::path::Path;

use rbl_export::{Manifest, SourceMyTag, SourcePlaylist, SourceTrack};

const DB_ID: u64 = 893;
const P: u64 = 8930;

fn track(dir: &Path, id: u64, title: &str) -> SourceTrack {
    let path = dir.join(format!("source-893-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: title.into(),
        artist: "TRIODE".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        my_tags: vec![11],
        analysis: vec![(
            "DAT".into(),
            rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish(),
        )],
        ..SourceTrack::default()
    }
}

fn tag(id: u64, seq: u32, name: &str, attribute: u8, parent: u64) -> SourceMyTag {
    SourceMyTag {
        id,
        seq,
        name: name.into(),
        attribute,
        parent,
    }
}

/// Two categories, one with two tags, one with one; the library numbers
/// from 1.
fn library_tags() -> Vec<SourceMyTag> {
    vec![
        tag(1, 1, "Genre", 1, 0),
        tag(2, 2, "Mood", 1, 0),
        tag(11, 1, "Peak", 0, 1),
        tag(12, 2, "Warm-up", 0, 1),
        tag(21, 1, "Dark", 0, 2),
    ]
}

fn sync(
    dest: &Path,
    tracks: &[SourceTrack],
    tags: &[SourceMyTag],
) -> rbl_export::Result<rbl_export::ExportReport> {
    let playlists = vec![SourcePlaylist {
        id: P,
        name: "P".into(),
        track_indices: vec![0],
        ..Default::default()
    }];
    let tree = vec![rbl_export::sync_record::SyncNode {
        id: P,
        parent: 0,
        attribute: 0,
    }];
    let sync = rbl_export::SyncSource {
        db_id: DB_ID,
        tree,
        ..Default::default()
    };
    rbl_export::export_full(
        dest,
        tracks,
        &playlists,
        tags,
        None,
        Some(&sync),
        &mut |_| {},
    )
}

type Edit = fn(&Path);

fn db(dest: &Path) -> std::path::PathBuf {
    dest.join("PIONEER/rekordbox/exportLibrary.db")
}

fn positions(dest: &Path) -> Vec<(i64, i64)> {
    let lib = rbl_onelibrary::ExportLibrary::open_read_only(&db(dest)).unwrap();
    let mut q = lib
        .connection()
        .prepare("SELECT myTag_id, sequenceNo FROM myTag ORDER BY myTag_id")
        .unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// What rekordbox does on open, or an older rbxport left: every position
/// moved by `delta` in `exportLibrary.db`.
fn shift_database(dest: &Path, delta: i64) {
    let conn = rbl_onelibrary::playlists::open_read_write(&db(dest)).unwrap();
    conn.execute("UPDATE myTag SET sequenceNo = sequenceNo + ?1", [delta])
        .unwrap();
}

/// Records the baseline as an older rbxport left it, one-based, with the
/// database to match.
fn make_one_based(dest: &Path) {
    shift_database(dest, 1);
    let mut manifest = Manifest::load(dest).unwrap();
    for t in &mut manifest.baseline.as_mut().unwrap().my_tags {
        t.seq += 1;
    }
    manifest.save(dest).unwrap();
}

#[test]
fn a_fresh_export_writes_consecutive_zero_based_positions_per_sibling_group() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    sync(
        dest.path(),
        &[track(src.path(), 1, "House 1")],
        &library_tags(),
    )
    .unwrap();
    assert_eq!(
        positions(dest.path()),
        vec![(1, 0), (2, 1), (11, 0), (12, 1), (21, 0)]
    );
}

#[test]
fn device_only_tags_keep_their_place() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = [track(src.path(), 1, "House 1")];
    // The stick holds a tag the library does not, last in "Genre".
    sync(dest.path(), &lib, &library_tags()).unwrap();
    {
        let conn = rbl_onelibrary::playlists::open_read_write(&db(dest.path())).unwrap();
        conn.execute("INSERT INTO myTag (myTag_id, sequenceNo, name, attribute, myTag_id_parent) VALUES (13, 2, 'Device only', 0, 1)", []).unwrap();
    }
    let mut manifest = Manifest::load(dest.path()).unwrap();
    manifest
        .baseline
        .as_mut()
        .unwrap()
        .my_tags
        .push(tag(13, 2, "Device only", 0, 1));
    manifest.save(dest.path()).unwrap();
    // A library change makes the sync rewrite the tags.
    let mut tags = library_tags();
    tags[0].name = "Genres".into();
    sync(dest.path(), &lib, &tags).unwrap();
    assert_eq!(
        positions(dest.path()),
        vec![(1, 0), (2, 1), (11, 0), (12, 1), (13, 2), (21, 0)]
    );
}

#[test]
fn gaps_in_the_librarys_numbers_become_consecutive_in_the_same_order() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let mut tags = library_tags();
    tags[2].seq = 9;
    tags[3].seq = 4;
    tags[4].seq = 0;
    sync(dest.path(), &[track(src.path(), 1, "House 1")], &tags).unwrap();
    assert_eq!(
        positions(dest.path()),
        vec![(1, 0), (2, 1), (11, 1), (12, 0), (21, 0)]
    );
}

#[test]
fn an_existing_one_based_export_migrates_without_recopying_audio_or_a_conflict() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = [track(src.path(), 1, "House 1")];
    sync(dest.path(), &lib, &library_tags()).unwrap();
    make_one_based(dest.path());
    assert_eq!(positions(dest.path())[0], (1, 1));
    let report = sync(dest.path(), &lib, &library_tags()).unwrap();
    assert_eq!(report.bytes_copied, 0, "audio is not copied again");
    assert_eq!(report.reused, 1);
    assert_eq!(
        positions(dest.path()),
        vec![(1, 0), (2, 1), (11, 0), (12, 1), (21, 0)]
    );
}

#[test]
fn rekordbox_renumbering_a_one_based_export_does_not_conflict() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let lib = [track(src.path(), 1, "House 1")];
    sync(dest.path(), &lib, &library_tags()).unwrap();
    make_one_based(dest.path());
    // rekordbox opens the stick and rewrites the positions from 0.
    shift_database(dest.path(), -1);
    // The library changed too, so the sync rewrites the tags rather than
    // finding itself up to date.
    let mut tags = library_tags();
    tags[0].name = "Genres".into();
    sync(dest.path(), &lib, &tags).unwrap();
    assert_eq!(positions(dest.path())[0], (1, 0));
}

#[test]
fn genuine_reorders_renames_and_assignment_changes_still_conflict() {
    let reorders: [(&str, Edit); 3] = [
        ("reorder", |d| {
            let conn = rbl_onelibrary::playlists::open_read_write(&db(d)).unwrap();
            conn.execute(
                "UPDATE myTag SET sequenceNo = 1 - sequenceNo WHERE myTag_id IN (11, 12)",
                [],
            )
            .unwrap();
        }),
        ("rename", |d| {
            let conn = rbl_onelibrary::playlists::open_read_write(&db(d)).unwrap();
            conn.execute("UPDATE myTag SET name = 'Renamed' WHERE myTag_id = 11", [])
                .unwrap();
        }),
        ("assignment", |d| {
            let conn = rbl_onelibrary::playlists::open_read_write(&db(d)).unwrap();
            conn.execute("INSERT INTO myTag_content (myTag_id, content_id) SELECT 12, content_id FROM myTag_content", []).unwrap();
        }),
    ];
    for (what, edit) in reorders {
        let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
        let lib = [track(src.path(), 1, "House 1")];
        sync(dest.path(), &lib, &library_tags()).unwrap();
        make_one_based(dest.path());
        shift_database(dest.path(), -1);
        edit(dest.path());
        let mut tags = library_tags();
        tags[0].name = "Genres".into();
        let error = sync(dest.path(), &lib, &tags)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_else(|| panic!("{what}: no conflict"));
        assert!(
            error.contains("changed on the USB") || error.contains("My Tags changed"),
            "{what}: {error}"
        );
    }
}
