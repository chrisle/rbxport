//! Import Playlist from a device's own library, against a fixture with the
//! real schema.
//!
//! The values pinned here are rekordbox's, read from rekordbox 7.2.11 for
//! macOS (static, with symbols): where the new playlist goes, the name it
//! takes when the top level already has one by that name, and the order and
//! numbering of its tracks. See `Writer::import_device_playlist`.
//!
//! Every test builds its own encrypted library in a tempdir; nothing here
//! can reach an installed library.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_db::fixture::{self, track_id, Shape};
use rbl_db::write::{device_import_name, DeviceEntry, Writer, ATTRIBUTE_FOLDER, ATTRIBUTE_PLAYLIST, ROOT};
use rbl_db::DbError;
use rusqlite::params;

struct Fixture {
    _dir: tempfile::TempDir,
    writer: Writer,
}

fn fixture_with(shape: Shape) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), shape).expect("build the fixture");
    let writer = Writer::open(location, dir.path().join("backups")).expect("open for writing");
    Fixture { _dir: dir, writer }
}

impl Fixture {
    fn conn(&self) -> &rusqlite::Connection {
        self.writer.library().connection()
    }

    /// `(Name, Seq, Attribute, ParentID)` of one playlist row.
    fn row(&self, id: &str) -> (String, i64, i64, String) {
        self.conn()
            .query_row(
                "SELECT Name, Seq, Attribute, ParentID FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
    }

    /// `(TrackNo, ContentID)` of a playlist's entries, in order.
    fn entries(&self, playlist: &str) -> Vec<(i64, String)> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT TrackNo, ContentID FROM djmdSongPlaylist
                 WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo",
            )
            .unwrap();
        stmt.query_map(params![playlist], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn a_name_nobody_has_is_kept() {
    assert_eq!(device_import_name("TAG LIST", &names(&["Playlist 0", "Warmup"])), "TAG LIST");
    assert_eq!(device_import_name("TAG LIST", &[]), "TAG LIST");
}

#[test]
fn a_taken_name_counts_up_from_one_in_parentheses() {
    // L" (%d)" from 1 [static, `importDevicePlaylist` @0x10166939c, format @0x102e2f2f4].
    assert_eq!(device_import_name("Set", &names(&["Set"])), "Set (1)");
    assert_eq!(device_import_name("Set", &names(&["Set", "Set (1)"])), "Set (2)");
    // Order does not matter: each try is checked against every sibling.
    assert_eq!(device_import_name("Set", &names(&["Set (2)", "Set (1)", "Set"])), "Set (3)");
    // A free number below a taken one is used.
    assert_eq!(device_import_name("Set", &names(&["Set", "Set (2)"])), "Set (1)");
}

#[test]
fn the_comparison_is_case_and_space_sensitive() {
    // `juce::String::compare` is an exact comparison.
    assert_eq!(device_import_name("set", &names(&["Set"])), "set");
    assert_eq!(device_import_name("Set ", &names(&["Set"])), "Set ");
}

#[test]
fn the_playlist_goes_at_the_end_of_the_top_level_with_the_devices_order() {
    let mut f = fixture_with(Shape::default());
    // The fixture's top level is Playlist 0..2 at Seq 0..2.
    let tracks = vec![track_id(7), track_id(3), track_id(12)];
    let made = f.writer.import_device_playlist("TAG LIST 001", &tracks).unwrap();
    assert_eq!(made.name, "TAG LIST 001");
    assert_eq!(made.tracks, 3);
    assert_eq!(f.row(&made.id), ("TAG LIST 001".to_owned(), 3, ATTRIBUTE_PLAYLIST, ROOT.to_owned()));
    assert_eq!(
        f.entries(&made.id),
        vec![(1, track_id(7)), (2, track_id(3)), (3, track_id(12))],
        "TrackNo from 1, in the device's order"
    );
}

#[test]
fn a_track_the_device_lists_twice_is_added_twice() {
    // `addMultiTracksToPlaylist` @0x1008df574 inserts every id it is given.
    let mut f = fixture_with(Shape::default());
    let tracks = vec![track_id(1), track_id(2), track_id(1)];
    let made = f.writer.import_device_playlist("Twice", &tracks).unwrap();
    assert_eq!(made.tracks, 3);
    assert_eq!(f.entries(&made.id), vec![(1, track_id(1)), (2, track_id(2)), (3, track_id(1))]);
}

#[test]
fn a_name_the_top_level_already_has_is_numbered_against_playlists_and_folders() {
    let mut f = fixture_with(Shape::default());
    let folder = f.writer.create_folder("Playlist 1 (1)", ROOT).unwrap();
    let made = f.writer.import_device_playlist("Playlist 1", &[track_id(0)]).unwrap();
    assert_eq!(made.name, "Playlist 1 (2)");
    assert_eq!(f.row(&made.id).0, "Playlist 1 (2)");
    assert_eq!(f.row(&folder).2, ATTRIBUTE_FOLDER);
    // Importing the same playlist again numbers it on.
    let again = f.writer.import_device_playlist("Playlist 1", &[track_id(0)]).unwrap();
    assert_eq!(again.name, "Playlist 1 (3)");
}

#[test]
fn a_name_only_inside_a_folder_does_not_clash() {
    let mut f = fixture_with(Shape::default());
    let folder = f.writer.create_folder("Gigs", ROOT).unwrap();
    f.writer.create_playlist("Friday", &folder).unwrap();
    let made = f.writer.import_device_playlist("Friday", &[]).unwrap();
    assert_eq!(made.name, "Friday");
}

#[test]
fn a_deleted_playlists_name_is_free() {
    let mut f = fixture_with(Shape::default());
    let old = f.writer.create_playlist("Gone", ROOT).unwrap();
    f.writer.delete_playlist(&old).unwrap();
    let made = f.writer.import_device_playlist("Gone", &[]).unwrap();
    assert_eq!(made.name, "Gone");
}

#[test]
fn the_first_playlist_of_an_empty_top_level_has_seq_one() {
    // `insertPlaylist` @0x10090041c writes 1 where `createNewList` passes 0.
    let mut f = fixture_with(Shape { playlists: 0, ..Shape::default() });
    let made = f.writer.import_device_playlist("First", &[track_id(0)]).unwrap();
    assert_eq!(f.row(&made.id).1, 1);
    let second = f.writer.import_device_playlist("Second", &[]).unwrap();
    assert_eq!(f.row(&second.id).1, 2);
}

#[test]
fn an_empty_device_playlist_makes_an_empty_playlist() {
    let mut f = fixture_with(Shape::default());
    let made = f.writer.import_device_playlist("Empty", &[]).unwrap();
    assert_eq!(made.tracks, 0);
    assert!(f.entries(&made.id).is_empty());
}

#[test]
fn a_track_that_is_not_in_the_collection_refuses_the_whole_import() {
    let mut f = fixture_with(Shape::default());
    let before = (
        f.count("SELECT COUNT(*) FROM djmdPlaylist"),
        f.count("SELECT COUNT(*) FROM djmdSongPlaylist"),
    );
    let deleted = track_id(5);
    f.conn().execute("UPDATE djmdContent SET rb_local_deleted = 1 WHERE ID = ?1", params![deleted]).unwrap();
    let refused = f.writer.import_device_playlist("Half", &[track_id(1), deleted]).unwrap_err();
    assert!(matches!(refused, DbError::WriteRefused(_)), "{refused:?}");
    let refused = f.writer.import_device_playlist("Half", &[track_id(1), "999999999".to_owned()]).unwrap_err();
    assert!(matches!(refused, DbError::WriteRefused(_)), "{refused:?}");
    assert_eq!(
        (f.count("SELECT COUNT(*) FROM djmdPlaylist"), f.count("SELECT COUNT(*) FROM djmdSongPlaylist")),
        before,
        "nothing is written"
    );
}

#[test]
fn every_new_row_gets_a_rising_usn_and_the_counter_follows() {
    let mut f = fixture_with(Shape::default());
    let made = f.writer.import_device_playlist("Counted", &[track_id(1), track_id(2)]).unwrap();
    let playlist_usn: i64 = f
        .conn()
        .query_row("SELECT rb_local_usn FROM djmdPlaylist WHERE ID = ?1", params![made.id], |r| r.get(0))
        .unwrap();
    let mut stmt = f
        .conn()
        .prepare("SELECT rb_local_usn FROM djmdSongPlaylist WHERE PlaylistID = ?1 ORDER BY TrackNo")
        .unwrap();
    let entry_usns: Vec<i64> = stmt.query_map(params![made.id], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(entry_usns.len(), 2);
    assert!(playlist_usn < entry_usns[0] && entry_usns[0] < entry_usns[1]);
    let counter: i64 = f
        .conn()
        .query_row("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(counter, entry_usns[1]);
}

/// A minimal but genuine WAV, so the tag reader has something real to open.
fn write_wav(path: &std::path::Path, seconds: u32) {
    let rate = 44_100_u32;
    let data_len = rate * seconds * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
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
fn copied_files_become_tracks_and_take_their_place_in_the_devices_order() {
    // rekordbox copies the tracks the collection lacks and imports the whole
    // playlist (`import_from_d_file_copy`, 7.2.19 @0x1001daa68).
    let mut f = fixture_with(Shape::default());
    let audio = tempfile::tempdir().unwrap();
    let (a, b) = (audio.path().join("New A.wav"), audio.path().join("New B.wav"));
    write_wav(&a, 1);
    write_wav(&b, 1);
    let tracks_before = f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0");
    let entries = vec![
        DeviceEntry::Track(track_id(4)),
        DeviceEntry::File(a.clone()),
        DeviceEntry::Track(track_id(2)),
        DeviceEntry::File(b.clone()),
        // The device lists the new track A again: one new row, two entries.
        DeviceEntry::File(a.clone()),
    ];
    let made = f.writer.import_device_playlist_entries("TAG LIST", &entries).unwrap();
    assert_eq!(made.tracks, 5);
    assert_eq!(made.added.len(), 2);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0"), tracks_before + 2);
    let (new_a, new_b) = (made.added[0].0.clone(), made.added[1].0.clone());
    assert_eq!((made.added[0].1.as_path(), made.added[1].1.as_path()), (a.as_path(), b.as_path()));
    let folder: String = f
        .conn()
        .query_row("SELECT FolderPath FROM djmdContent WHERE ID = ?1", params![new_a], |r| r.get(0))
        .unwrap();
    assert_eq!(folder, a.to_string_lossy());
    assert_eq!(
        f.entries(&made.id),
        vec![(1, track_id(4)), (2, new_a.clone()), (3, track_id(2)), (4, new_b), (5, new_a)]
    );
    // The new rows are counted before the playlist's, and the counter follows.
    let counter: i64 = f
        .conn()
        .query_row("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", [], |r| r.get(0))
        .unwrap();
    let highest: i64 = f.count("SELECT MAX(rb_local_usn) FROM djmdSongPlaylist");
    assert_eq!(counter, highest);
}

#[test]
fn a_file_that_cannot_be_imported_writes_nothing_at_all() {
    let mut f = fixture_with(Shape::default());
    let audio = tempfile::tempdir().unwrap();
    let good = audio.path().join("Good.wav");
    write_wav(&good, 1);
    let bad = audio.path().join("Broken.wav");
    std::fs::write(&bad, b"not audio").unwrap();
    let before = (
        f.count("SELECT COUNT(*) FROM djmdContent"),
        f.count("SELECT COUNT(*) FROM djmdPlaylist"),
        f.count("SELECT COUNT(*) FROM djmdSongPlaylist"),
    );
    let refused = f
        .writer
        .import_device_playlist_entries(
            "Broken",
            &[DeviceEntry::File(good), DeviceEntry::Track(track_id(1)), DeviceEntry::File(bad)],
        )
        .unwrap_err();
    assert!(matches!(refused, DbError::WriteRefused(_)), "{refused:?}");
    assert_eq!(
        (
            f.count("SELECT COUNT(*) FROM djmdContent"),
            f.count("SELECT COUNT(*) FROM djmdPlaylist"),
            f.count("SELECT COUNT(*) FROM djmdSongPlaylist"),
        ),
        before,
        "neither the good file's track nor the playlist is kept"
    );
}

#[test]
fn a_file_the_collection_already_holds_is_used_as_it_is() {
    let mut f = fixture_with(Shape::default());
    let audio = tempfile::tempdir().unwrap();
    let held = audio.path().join("Held.wav");
    write_wav(&held, 1);
    let id = f.writer.import_file(&held).unwrap();
    let made = f.writer.import_device_playlist_entries("Held", &[DeviceEntry::File(held)]).unwrap();
    assert!(made.added.is_empty());
    assert_eq!(f.entries(&made.id), vec![(1, id)]);
}

fn cue(hot_cue: u32, kind: u8, time_ms: u32, loop_time_ms: u32) -> rbl_anlz::CueEntry {
    rbl_anlz::CueEntry { hot_cue, kind, time_ms, loop_time_ms, color_id: 0, comment: None, color_code: Some(if hot_cue == 0 { 0 } else { 1 }), rgb: None }
}

#[test]
fn a_copied_track_takes_the_devices_bpm_key_analysis_and_cues() {
    // rekordbox gives each track Import Playlist copies the device's row
    // (BPM, key), its analysis files and its cues [static, rekordbox 7.2.19
    // importFromDeviceThread::run @0x1001d9618]. Without that the copy came
    // in as a bare file: no BPM, waveform, cues or key (#568).
    let mut f = fixture_with(Shape::default());
    f.conn()
        .execute("INSERT INTO djmdKey (ID, ScaleName, Seq, created_at, updated_at) VALUES ('12', 'Fm', 7, '2026-01-01', '2026-01-01')", [])
        .unwrap();
    let audio = tempfile::tempdir().unwrap();
    let copy = audio.path().join("Copied.wav");
    write_wav(&copy, 1);
    let made = f.writer.import_device_playlist_entries("From USB", &[DeviceEntry::File(copy)]).unwrap();
    let (content, _) = made.added[0].clone();
    let path = f.writer.analysis_data_path_for(&content).unwrap();
    let cues = [cue(1, 1, 1000, 0), cue(0, 2, 2000, 4000), cue(17, 1, 5, 0), cue(0, 2, 9000, 8000)];
    f.writer
        .import_device_analysis(
            &content,
            &rbl_db::write::DeviceAnalysis { bpm_x100: 12_800, key: Some("Fm"), analysis_data_path: Some(&path), cues: &cues },
        )
        .unwrap();
    let row: (i64, String, String, i64, i64) = f
        .conn()
        .query_row(
            "SELECT BPM, KeyID, AnalysisDataPath, Analysed, CAST(COALESCE(AnalysisUpdated,'0') AS INTEGER) FROM djmdContent WHERE ID = ?1",
            params![content],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(row, (12_800, "12".to_owned(), path, rbl_db::write::ANALYSED_FULL, 1));
    let mut stmt = f
        .conn()
        .prepare("SELECT InMsec, OutMsec, Kind FROM djmdCue WHERE ContentID = ?1 AND rb_local_deleted = 0 ORDER BY InMsec")
        .unwrap();
    let stored: Vec<(i64, Option<i64>, i64)> =
        stmt.query_map(params![content], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(stored, vec![(1000, None, 1), (2000, Some(4000), 0)], "the hot cue past H and the backwards loop are left out");
}

#[test]
fn a_copied_track_without_analysis_files_keeps_its_own_path_and_flag() {
    let mut f = fixture_with(Shape::default());
    let audio = tempfile::tempdir().unwrap();
    let copy = audio.path().join("Bare.wav");
    write_wav(&copy, 1);
    let made = f.writer.import_device_playlist_entries("Bare", &[DeviceEntry::File(copy)]).unwrap();
    let (content, _) = made.added[0].clone();
    let before: (Option<String>, Option<i64>) = f
        .conn()
        .query_row("SELECT AnalysisDataPath, Analysed FROM djmdContent WHERE ID = ?1", params![content], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    f.writer
        .import_device_analysis(
            &content,
            &rbl_db::write::DeviceAnalysis { bpm_x100: 9_950, key: Some("not a key"), analysis_data_path: None, cues: &[] },
        )
        .unwrap();
    let after: (i64, Option<String>, Option<i64>, Option<String>) = f
        .conn()
        .query_row("SELECT BPM, AnalysisDataPath, Analysed, KeyID FROM djmdContent WHERE ID = ?1", params![content], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert_eq!(after.0, 9_950);
    assert_eq!((after.1, after.2), before, "no analysis path, no analysed flag");
    assert!(after.3.is_none_or(|k| k.is_empty() || k == "0"), "an unknown key name invents nothing");
}
