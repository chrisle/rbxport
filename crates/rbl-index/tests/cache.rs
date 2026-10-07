//! The on-disk snapshot: it must reproduce the library exactly, refuse a stale
//! one, and survive a damaged file without panicking.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use rbl_index::cache::{decode, encode, Fingerprint};
use rbl_index::testing::{add_folder, add_history, add_playlist, library_from, TestTrack};
use rbl_index::{Cue, SortColumn, TrackSource, ViewSpec};

fn fingerprint() -> Fingerprint {
    Fingerprint {
        format: rbl_index::cache::FORMAT,
        db_len: 1_234,
        db_modified_ns: 99,
        wal_len: 7,
        wal_modified_ns: 5,
        db_version: 6000,
        content: 42,
        database: 7,
    }
}

fn sample() -> Vec<TestTrack> {
    vec![
        TestTrack {
            id: 1, title: "Zebra", artist: "ARTBAT", bpm_x100: 12800, rating: 4, comment: "hi",
            cues: vec![
                Cue { id: 12, position_ms: 165_046, out_ms: 0, kind: 2, colour: 21 },
                Cue { id: 11, position_ms: 46, out_ms: 0, kind: 1, colour: 46 },
                Cue { id: 10, position_ms: 46, out_ms: 8_046, kind: 0, colour: 0 },
            ],
            ..TestTrack::default()
        },
        TestTrack { id: 2, title: "apple", artist: "Meduza", bpm_x100: 13000, ..TestTrack::default() },
        TestTrack {
            id: 3, title: "Ébano", artist: "Tujamo", bpm_x100: 12400, rating: 2,
            cues: vec![Cue { id: 30, position_ms: 24, out_ms: 0, kind: 6, colour: 18 }],
            ..TestTrack::default()
        },
    ]
}

fn built() -> rbl_index::Library {
    let mut lib = library_from(&sample());
    add_playlist(&mut lib, "Set", &[2, 0]);
    add_folder(&mut lib, "Gigs");
    add_history(&mut lib, "HISTORY 2026-09-10", &[1, 2]);
    lib
}

#[test]
fn a_snapshot_reproduces_the_library_it_came_from() {
    let original = built();
    let restored = decode(&encode(&original, fingerprint()), fingerprint()).expect("decodes");

    assert_eq!(restored.len(), original.len());
    assert_eq!(restored.ids, original.ids);
    assert_eq!(restored.artist_ids, original.artist_ids);
    assert_eq!(restored.album_ids, original.album_ids);
    assert_eq!(restored.genre_ids, original.genre_ids);
    assert_eq!(restored.label_ids, original.label_ids);
    for row in 0..original.len() {
        assert_eq!(restored.title.get(row), original.title.get(row));
        assert_eq!(restored.comment.get(row), original.comment.get(row));
        assert_eq!(restored.artist_name(row as u32), original.artist_name(row as u32));
        assert_eq!(restored.rating[row], original.rating[row]);
        assert_eq!(restored.bpm_x100[row], original.bpm_x100[row]);
    }
    assert_eq!(restored.playlists().members, original.playlists().members);
    assert_eq!(restored.playlists().names.get(0), "Set");
    // An empty folder is still a folder on the way back.
    assert!(!restored.playlists().is_folder(0));
    assert!(restored.playlists().is_folder(1));
    // The histories come back too: formats before 4 dropped them, and every
    // cached start opened with no Histories section.
    assert_eq!(restored.histories().len(), 1);
    assert_eq!(restored.histories().names.get(0), "HISTORY 2026-09-10");
    assert_eq!(restored.histories().members, original.histories().members);
}

#[test]
fn a_snapshot_keeps_every_track_s_cues_with_their_colours() {
    // Format 1 dropped these, and a start that hit the snapshot drew a player
    // with no cues at all.
    let original = built();
    let restored = decode(&encode(&original, fingerprint()), fingerprint()).expect("decodes");
    for row in 0..original.len() as u32 {
        assert_eq!(restored.cues_of(row), original.cues_of(row), "row {row}");
    }
    assert_eq!(
        restored.cues_of(0),
        &[
            Cue { id: 10, position_ms: 46, out_ms: 8_046, kind: 0, colour: 0 },
            Cue { id: 11, position_ms: 46, out_ms: 0, kind: 1, colour: 46 },
            Cue { id: 12, position_ms: 165_046, out_ms: 0, kind: 2, colour: 21 },
        ]
    );
    assert!(restored.cues_of(1).is_empty());
    assert_eq!((restored.cues_of(2)[0].colour, restored.cues_of(2)[0].id), (18, 30));
    assert_eq!(restored.cues_of(0)[0].out_ms, 8_046, "a loop keeps its end");
}

#[test]
fn damage_at_the_end_of_the_snapshot_is_refused() {
    // The search arena and checksum now follow the cue index. Corruption at
    // the end must be rejected just as damage to the header is.
    let mut bytes = encode(&built(), fingerprint());
    let at = bytes.len() - 8;
    bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode(&bytes, fingerprint()).is_none());
}

#[test]
fn the_persisted_ranks_and_search_still_work() {
    // Persisted derived columns must preserve sort and search behavior.
    let restored = decode(&encode(&built(), fingerprint()), fingerprint()).expect("decodes");
    let spec = |sort, query: &str| ViewSpec {
        source: TrackSource::Collection,
        sort,
        descending: false,
        query: query.to_owned(),
        filter: Default::default(),
    };
    let by_title: Vec<&str> = restored
        .open_view(&spec(SortColumn::Title, ""))
        .rows
        .iter()
        .map(|&r| restored.title.get(r as usize))
        .collect();
    assert_eq!(by_title, vec!["apple", "Ébano", "Zebra"]);
    assert_eq!(restored.open_view(&spec(SortColumn::Title, "artbat")).rows.len(), 1);
}

#[test]
fn a_snapshot_keeps_the_detail_columns_the_browser_sorts_by() {
    let tracks = vec![
        TestTrack {
            id: 1, title: "One", track_number: 7, disc_no: 2, file_type: 11, bit_depth: 24,
            lyricist: "Words", date_created: "2024-05-01", publish: true, message: "hello",
            ..TestTrack::default()
        },
        TestTrack { id: 2, title: "Two", track_number: 3, file_type: 1, ..TestTrack::default() },
    ];
    let original = library_from(&tracks);
    let restored = decode(&encode(&original, fingerprint()), fingerprint()).expect("decodes");
    assert_eq!(restored.track_number, original.track_number);
    assert_eq!(restored.disc_no, original.disc_no);
    assert_eq!(restored.file_type, original.file_type);
    assert_eq!(restored.bit_depth, original.bit_depth);
    assert_eq!(restored.publish, original.publish);
    for row in 0..original.len() {
        assert_eq!(restored.lyricist.get(row), original.lyricist.get(row));
        assert_eq!(restored.date_created.get(row), original.date_created.get(row));
        assert_eq!(restored.message.get(row), original.message.get(row));
    }
    let spec = ViewSpec {
        source: TrackSource::Collection, sort: SortColumn::TrackNumber, descending: false,
        query: String::new(), filter: Default::default(),
    };
    assert_eq!(restored.open_view(&spec).rows, vec![1, 0]);
}

#[test]
fn prepared_snapshot_requires_live_validation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.snapshot");
    rbl_index::cache::save(&path, &built(), fingerprint()).unwrap();
    assert!(rbl_index::cache::prepare(&path).unwrap().validated(Fingerprint { content: 99, ..fingerprint() }).is_none());
    assert!(rbl_index::cache::prepare(&path).unwrap().validated(fingerprint()).is_some());
}

#[test]
fn corruption_anywhere_in_payload_is_rejected() {
    let original = encode(&built(), fingerprint());
    for at in (0..original.len()).step_by(13) {
        let mut damaged = original.clone();
        damaged[at] ^= 1;
        assert!(decode(&damaged, fingerprint()).is_none(), "byte {at}");
    }
}

#[test]
fn a_snapshot_of_a_different_database_is_refused() {
    let bytes = encode(&built(), fingerprint());
    // Every field on its own must be enough to reject it: this is the whole
    // defence against showing somebody a library that has moved on.
    for changed in [
        Fingerprint { content: 43, ..fingerprint() },
        Fingerprint { db_version: 6001, ..fingerprint() },
        Fingerprint { format: rbl_index::cache::FORMAT + 1, ..fingerprint() },
        Fingerprint { database: 8, ..fingerprint() },
    ] {
        assert!(decode(&bytes, changed).is_none(), "{changed:?} should have been refused");
    }
}

#[test]
fn a_rewritten_log_alone_does_not_throw_the_snapshot_away() {
    // rekordbox rewrites the write-ahead log constantly without changing a
    // row. Refusing the snapshot for that made it useless on every start
    // rekordbox happened to be running for, which is most of them.
    let bytes = encode(&built(), fingerprint());
    for same_content in [
        Fingerprint { wal_len: 8, ..fingerprint() },
        Fingerprint { wal_modified_ns: 999, ..fingerprint() },
        Fingerprint { db_len: 9999, ..fingerprint() },
        Fingerprint { db_modified_ns: 100, ..fingerprint() },
    ] {
        assert!(decode(&bytes, same_content).is_some(), "{same_content:?} should still match");
    }
}

#[test]
fn a_damaged_snapshot_is_refused_rather_than_trusted() {
    let bytes = encode(&built(), fingerprint());
    assert!(decode(&[], fingerprint()).is_none(), "empty");
    assert!(decode(b"not a snapshot at all", fingerprint()).is_none(), "wrong magic");
    // Truncated at every length, which is what a half-written file looks like.
    for cut in (1..bytes.len()).step_by(7) {
        assert!(decode(&bytes[..cut], fingerprint()).is_none(), "truncated to {cut}");
    }
}

#[test]
fn a_length_that_claims_more_than_the_file_holds_does_not_allocate_it() {
    // The failure this guards against is a corrupt count reserving gigabytes
    // before anything notices the file is far too small to hold them.
    let mut bytes = encode(&built(), fingerprint());
    let header = 4 + 4 + 8 + 8 + 8 + 8 + 4;
    // The row count is fine; the first vector's length is the one to poison.
    bytes[header + 8..header + 16].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(decode(&bytes, fingerprint()).is_none());
}
