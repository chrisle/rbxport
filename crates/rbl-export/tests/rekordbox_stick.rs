//! A stick as rekordbox leaves it, before its first sync from here (#284).
//!
//! Every stick here is written by the export pipeline into a temporary
//! directory and then given rekordbox's encodings; nothing touches a real
//! device.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

use rbl_export::device_library::{self, Format};
use rbl_export::snapshot::Snapshot;
use rbl_export::{export, SourcePlaylist, SourceTrack};

fn track(dir: &Path, id: u64, rating: u8) -> SourceTrack {
    let path = dir.join(format!("source-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: format!("Track {id}"),
        artist: "TRIODE".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        rating,
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

/// A stick holding tracks rated 0 to 5 in one playlist, with `OneLibrary`
/// ratings stored as the stars themselves, as `export.pdb` has them [OBS: a
/// rekordbox 7 stick, ratings 1 to 5 in `exportLibrary.db` equal to the same
/// tracks' `export.pdb` ratings]. The manifest is removed, so the stick is
/// one rbxport has never synced.
fn rekordbox_stick(src: &Path, dest: &Path) {
    let tracks: Vec<SourceTrack> = (0..=5).map(|stars| track(src, u64::from(stars) + 1, stars)).collect();
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    export(dest, &tracks, &playlists).unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest)).unwrap();
}

#[test]
fn rekordboxs_star_ratings_do_not_make_a_sticks_libraries_disagree() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    rekordbox_stick(src.path(), dest.path());
    let snapshot = Snapshot::read(dest.path()).unwrap();
    let stars = |library: &rbl_export::snapshot::Library| {
        let mut stars: Vec<(u32, u32)> = library.tracks.iter().map(|t| (t.id, t.rating)).collect();
        stars.sort_unstable();
        stars
    };
    let legacy = stars(snapshot.legacy.as_ref().unwrap());
    assert_eq!(legacy.iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(stars(snapshot.one.as_ref().unwrap()), legacy, "OneLibrary reads the same stars");
    snapshot.check_baseline(None, 0).unwrap();
}

#[test]
fn rbxports_own_ratings_still_read_as_stars() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let tracks: Vec<SourceTrack> = (0..=5).map(|stars| track(src.path(), u64::from(stars) + 1, stars)).collect();
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    export(dest.path(), &tracks, &playlists).unwrap();
    let snapshot = Snapshot::read(dest.path()).unwrap();
    let mut stars: Vec<(u32, u32)> = snapshot.one.as_ref().unwrap().tracks.iter().map(|t| (t.id, t.rating)).collect();
    stars.sort_unstable();
    assert_eq!(stars.iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn the_first_sync_of_a_rekordbox_stick_is_not_refused_as_a_conflict() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    rekordbox_stick(src.path(), dest.path());
    let tracks: Vec<SourceTrack> = (0..=5).map(|stars| track(src.path(), u64::from(stars) + 1, stars)).collect();
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    let report = export(dest.path(), &tracks, &playlists).unwrap();
    assert_eq!(report.tracks, tracks.len());
}

#[test]
fn the_sticks_onelibrary_browses_with_rekordboxs_stars() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    rekordbox_stick(src.path(), dest.path());
    let one = device_library::read(dest.path(), Format::OneLibrary).unwrap();
    let legacy = device_library::read(dest.path(), Format::DeviceLibrary).unwrap();
    let stars = |tracks: &[device_library::Track]| {
        let mut stars: Vec<(u32, u8)> = tracks.iter().map(|t| (t.id, t.rating)).collect();
        stars.sort_unstable();
        stars
    };
    assert_eq!(stars(&one.tracks), stars(&legacy.tracks));
    assert_eq!(stars(&one.tracks).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 5]);
}

// A stick another writer exported from this library, synced from here for
// the first time (#331). rekordbox's export finds a track the stick already
// holds by its master ids and updates its row without copying the audio
// again [OBS: rekordbox 7.2.19 DatabaseMediator::execExport calls
// rekordboxDBController::isCollectionSongForDuplicateSongId and then
// DatabaseMediator::overwrite_export, which copies no file, @0x1009b7598..
// 0x1009b7690].

const DB_ID: u64 = 4_242;

fn sync(db_id: u64) -> rbl_export::SyncSource {
    rbl_export::SyncSource { db_id, ..Default::default() }
}

fn sync_to(dest: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist], db_id: u64) -> rbl_export::Result<rbl_export::ExportReport> {
    rbl_export::export_full(dest, tracks, playlists, &[], None, Some(&sync(db_id)), &mut |_| {})
}

/// Tracks 1 to `count` in playlist 10, exported for library `db_id`, with
/// rbxport's own record of the export removed: the stick as rekordbox
/// leaves it, its rows naming the library's tracks by their master ids.
fn exported_elsewhere(src: &Path, dest: &Path, count: u64, db_id: u64) -> (Vec<SourceTrack>, Vec<SourcePlaylist>) {
    let tracks: Vec<SourceTrack> = (1..=count).map(|id| track(src, id, 0)).collect();
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    sync_to(dest, &tracks, &playlists, db_id).unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest)).unwrap();
    (tracks, playlists)
}

fn audio_files(dest: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().map(|e| e.unwrap()).filter(|e| !common::is_os_metadata(&e.file_name())) {
            let path = entry.path();
            if path.is_dir() { walk(&path, out) } else { out.push(path.to_string_lossy().into_owned()) }
        }
    }
    let mut out = Vec::new();
    walk(&dest.join("Contents"), &mut out);
    out.sort();
    out
}

#[test]
fn the_first_sync_keeps_the_copies_the_stick_already_holds() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, playlists) = exported_elsewhere(src.path(), dest.path(), 4, DB_ID);
    let files = audio_files(dest.path());

    let estimate = rbl_export::estimate::estimate(dest.path(), DB_ID, &tracks, false, None);
    assert_eq!((estimate.copy_bytes, estimate.tracks_new, estimate.tracks_kept), (0, 0, 4), "nothing to add: {estimate:?}");
    assert_eq!(estimate.reuse_bytes, 4 * 2048);

    let report = sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!((report.bytes_copied, report.tracks_added, report.reused), (0, 0, 4));
    assert_eq!(audio_files(dest.path()), files, "the stick's files stay where they were, and no second copy appears");

    // Recorded as this export's: the next sync trusts them without reading
    // the stick back, and still copies nothing.
    let again = sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!((again.bytes_copied, again.reused), (0, 4));
    assert!(rbl_export::verify_databases(dest.path()).unwrap().errors.is_empty());
}

#[test]
fn a_held_copy_of_another_size_is_copied_again() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (mut tracks, playlists) = exported_elsewhere(src.path(), dest.path(), 2, DB_ID);
    std::fs::write(&tracks[0].source_path, vec![9_u8; 4096]).unwrap();
    tracks[0].file_size = 4096;

    let estimate = rbl_export::estimate::estimate(dest.path(), DB_ID, &tracks, false, None);
    assert_eq!((estimate.copy_bytes, estimate.tracks_new, estimate.tracks_kept), (4096, 1, 1));
    let report = sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!(report.bytes_copied, 4096);
    assert!(audio_files(dest.path()).iter().any(|f| std::fs::read(f).unwrap() == vec![9_u8; 4096]));
}

#[test]
fn another_librarys_copies_are_not_taken_for_this_ones() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, _) = exported_elsewhere(src.path(), dest.path(), 2, DB_ID);
    let estimate = rbl_export::estimate::estimate(dest.path(), DB_ID + 1, &tracks, false, None);
    assert_eq!((estimate.tracks_new, estimate.tracks_kept), (2, 0));
}

/// Gives a track a different comment in `exportLibrary.db` than in
/// `export.pdb`.
fn disagree_on_comment(dest: &Path, device_id: u32) {
    let one = rbl_onelibrary::playlists::open_read_write(&dest.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(one.execute("UPDATE content SET djComment = 'edited' WHERE content_id = ?1", [device_id]).unwrap(), 1);
}

fn device_id(dest: &Path, library_id: u64) -> u32 {
    let snapshot = Snapshot::read(dest).unwrap();
    *snapshot.identity.iter().find(|(_, (_, content))| *content == library_id).unwrap().0
}

#[test]
fn a_disagreement_on_a_track_the_sync_rewrites_does_not_stop_it() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, playlists) = exported_elsewhere(src.path(), dest.path(), 3, DB_ID);
    disagree_on_comment(dest.path(), device_id(dest.path(), 2));
    let report = sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!(report.tracks, 3);
    let snapshot = Snapshot::read(dest.path()).unwrap();
    assert_eq!(snapshot.legacy, snapshot.one, "both databases carry the library's version again");
}

#[test]
fn a_disagreement_the_sync_would_carry_names_what_differs() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, _) = exported_elsewhere(src.path(), dest.path(), 3, DB_ID);
    disagree_on_comment(dest.path(), device_id(dest.path(), 3));
    // Track 3 is not selected: the stick keeps it as it has it.
    let selection = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }];
    let error = sync_to(dest.path(), &tracks[..2], &selection, DB_ID).unwrap_err().to_string();
    assert_eq!(
        error,
        "USB sync conflict: The USB's two libraries, export.pdb and exportLibrary.db, differ on the comment of track 'Track 3', \
         so this first sync stopped before changing anything. Include that track in this sync if it is in your library, \
         or delete it from the USB in rekordbox, then sync again."
    );
}

// A stick whose `exportLibrary.db` holds a negative `analysedBits` (#545,
// #583). The sync read the column as unsigned and stopped with
// "could not write exportLibrary.db: Integer -23 out of range at index 23"
// before changing anything. `export.pdb` keeps the same flags in the byte at
// row offset 0x56 [OBS: docs/reference/usb-export-db.md, a rekordbox 7
// stick], taken to be -23 as that byte, 0xE9, stored signed [ASSUME].

fn set_analysed_bits(dest: &Path, device_id: u32, bits: i64) {
    let one = rbl_onelibrary::playlists::open_read_write(&dest.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(one.execute("UPDATE content SET analysedBits = ?2 WHERE content_id = ?1", rusqlite::params![device_id, bits]).unwrap(), 1);
}

fn analysed_bits(dest: &Path, device_id: u32) -> i64 {
    let one = rbl_onelibrary::ExportLibrary::open_read_only(&dest.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    one.connection().query_row("SELECT analysedBits FROM content WHERE content_id = ?1", [device_id], |r| r.get(0)).unwrap()
}

fn set_release_year(dest: &Path, device_id: u32, year: i64) {
    let one = rbl_onelibrary::playlists::open_read_write(&dest.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(one.execute("UPDATE content SET releaseYear = ?2 WHERE content_id = ?1", rusqlite::params![device_id, year]).unwrap(), 1);
}

fn exported_years(dest: &Path, device_id: u32) -> (u16, i64) {
    let bytes = std::fs::read(dest.join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let tracks = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
    let legacy = tracks.iter().find(|track| track.id == device_id).unwrap().year;
    let one = rbl_onelibrary::ExportLibrary::open_read_only(&dest.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    let year = one.connection().query_row("SELECT releaseYear FROM content WHERE content_id = ?1", [device_id], |row| row.get(0)).unwrap();
    (legacy, year)
}

#[test]
fn a_negative_release_year_on_a_track_the_stick_keeps_does_not_stop_the_sync() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let mut tracks: Vec<SourceTrack> = (1..=3).map(|id| track(src.path(), id, 0)).collect();
    tracks[2].year = 2020;
    let initial = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1, 2], ..Default::default() }];
    sync_to(dest.path(), &tracks, &initial, DB_ID).unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
    let kept = device_id(dest.path(), 3);
    assert_eq!(exported_years(dest.path(), kept), (2020, 2020));
    set_release_year(dest.path(), kept, -9517);
    assert_eq!(exported_years(dest.path(), kept), (2020, -9517), "only OneLibrary has the malformed year");
    let selection = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }];
    let report = sync_to(dest.path(), &tracks[..2], &selection, DB_ID).unwrap();
    assert_eq!(report.tracks, 3);
    assert_eq!(exported_years(dest.path(), kept), (2020, 2020), "the valid DeviceSQL year survives in both databases");
    assert!(rbl_export::verify_databases(dest.path()).unwrap().errors.is_empty());
    sync_to(dest.path(), &tracks[..2], &selection, DB_ID).unwrap();
    assert_eq!(exported_years(dest.path(), kept), (2020, 2020));
}

#[test]
fn a_negative_release_year_on_a_track_the_sync_rewrites_uses_the_library_year() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let mut tracks: Vec<SourceTrack> = (1..=3).map(|id| track(src.path(), id, 0)).collect();
    tracks[1].year = 2019;
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1, 2], ..Default::default() }];
    sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
    let rewritten = device_id(dest.path(), 2);
    assert_eq!(exported_years(dest.path(), rewritten), (2019, 2019));
    set_release_year(dest.path(), rewritten, -9517);
    assert_eq!(exported_years(dest.path(), rewritten), (2019, -9517));
    tracks[1].year = 2020;
    let report = sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!(report.tracks, 3);
    assert_eq!(exported_years(dest.path(), rewritten), (2020, 2020));
    assert!(rbl_export::verify_databases(dest.path()).unwrap().errors.is_empty());
    sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!(exported_years(dest.path(), rewritten), (2020, 2020));
}

#[test]
fn a_negative_analysed_bits_on_a_track_the_stick_keeps_does_not_stop_the_sync() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, _) = exported_elsewhere(src.path(), dest.path(), 3, DB_ID);
    let kept = device_id(dest.path(), 3);
    set_analysed_bits(dest.path(), kept, -23);
    // Overwrite the playlist with two of the tracks: the stick keeps track 3
    // as it has it, so the sync reads its row back.
    let selection = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }];
    let report = sync_to(dest.path(), &tracks[..2], &selection, DB_ID).unwrap();
    assert_eq!(report.tracks, 3, "the stick still holds track 3");
    let kept = device_id(dest.path(), 3);
    // The flags keep their low byte, 0xE9, with the "analysing" bit 64
    // cleared as every exported row has it.
    assert_eq!(analysed_bits(dest.path(), kept), 0xE9 & !64);
    assert!(rbl_export::verify_databases(dest.path()).unwrap().errors.is_empty());
    sync_to(dest.path(), &tracks[..2], &selection, DB_ID).unwrap();
}

#[test]
fn a_negative_analysed_bits_on_a_track_the_sync_rewrites_does_not_stop_it() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, playlists) = exported_elsewhere(src.path(), dest.path(), 3, DB_ID);
    set_analysed_bits(dest.path(), device_id(dest.path(), 2), -23);
    let report = sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!(report.tracks, 3);
}

// A stick rekordbox exported from another library, holding a file at the
// path a selected track lays out to (#599). The sync renamed the stick's own
// track to make room and left its analysis where the old path hashes, so the
// staged export failed to verify with "Analysis path is not discoverable by
// the CDJ" and "Analysis audio path mismatch" for the same folder, and the
// stick could not be synced at all. rekordbox never replaces a file at a
// track's device path: `DatabaseMediator::calc_device_file_path` (7.2.19
// arm64 @0x1009b6e0c) numbers the new track's file instead [OBS static].

#[test]
fn a_stick_track_on_the_path_a_selected_track_takes_keeps_its_place() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    exported_elsewhere(src.path(), dest.path(), 2, DB_ID);
    let before = device_library::read(dest.path(), Format::DeviceLibrary).unwrap();
    let theirs = before.tracks.iter().find(|t| t.title == "Track 1").unwrap().clone();
    let their_audio = std::fs::read(dest.path().join(theirs.path.trim_start_matches('/'))).unwrap();
    let their_analysis = std::fs::read(dest.path().join(theirs.analysis_path.trim_start_matches('/'))).unwrap();

    // Another library's track with the same artist and file name, so it lays
    // out on the stick track's path, but other audio.
    let other = tempfile::tempdir().unwrap();
    let mine = track(other.path(), 1, 0);
    std::fs::write(&mine.source_path, vec![77_u8; 3000]).unwrap();
    let playlists = vec![SourcePlaylist { id: 20, name: "Mine".into(), track_indices: vec![0], ..Default::default() }];
    sync_to(dest.path(), &[mine], &playlists, DB_ID + 1).unwrap();

    let after = device_library::read(dest.path(), Format::DeviceLibrary).unwrap();
    assert_eq!(after.tracks.len(), 3, "the stick keeps both of its tracks and gains the selected one");
    let kept = after.tracks.iter().find(|t| t.title == "Track 1" && t.path == theirs.path).expect("the stick's track keeps its path");
    assert_eq!(kept.analysis_path, theirs.analysis_path, "and its analysis");
    assert_eq!(std::fs::read(dest.path().join(theirs.path.trim_start_matches('/'))).unwrap(), their_audio);
    assert_eq!(std::fs::read(dest.path().join(theirs.analysis_path.trim_start_matches('/'))).unwrap(), their_analysis);
    let added = after.tracks.iter().find(|t| t.title == "Track 1" && t.path != theirs.path).expect("the selected track is added beside it");
    assert_eq!(std::fs::read(dest.path().join(added.path.trim_start_matches('/'))).unwrap(), vec![77_u8; 3000]);
    assert_ne!(added.analysis_path, theirs.analysis_path);
    let report = rbl_export::verify(dest.path()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

// A stick where rekordbox put two tracks in one analysis directory, the
// second as `ANLZ0001` (#599, #616). The sync wrote the second track's
// analysis over the first's `ANLZ0000` and pointed the second at it, so the
// staged export failed to verify with "Analysis audio path mismatch" and the
// stick could not be synced. A player tells the two apart by `PPTH`
// [OBS static: XDJ-R1 native reader, alphatheta-docs
// `platform/rekordbox-xdj-r1-native-reader.md`, `ANLZ0000`..`ANLZ0010`].

fn named(dir: &Path, id: u64, name: &str) -> SourceTrack {
    SourceTrack { source_path: { let p = dir.join(name); std::fs::write(&p, vec![id as u8; 2048]).unwrap(); p }, ..track(dir, id, 0) }
}

fn with_companions(mut track: SourceTrack) -> SourceTrack {
    for extension in ["EXT", "2EX"] {
        track.analysis.push((extension.into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish()));
    }
    track
}

fn replace_bytes(bytes: &mut [u8], from: &[u8], to: &[u8]) -> usize {
    assert_eq!(from.len(), to.len());
    let mut count = 0;
    let mut at = 0;
    while at + from.len() <= bytes.len() {
        if &bytes[at..at + from.len()] == from {
            bytes[at..at + from.len()].copy_from_slice(to);
            count += 1;
            at += from.len();
        } else {
            at += 1;
        }
    }
    count
}

/// Tracks 1 and 2 exported for `DB_ID`, then made into the stick rekordbox
/// leaves when their paths collide: `c10013.mp3` and `c16700.mp3` under
/// `/Contents/TRIODE/UnknownAlbum/` both hash to `P009/0000DCB1`, and track
/// 2's analysis is `ANLZ0001` there, its `PPTH` naming its own path. Both
/// databases are edited in place, as rekordbox has them; the record is
/// removed.
fn collision_stick(src: &Path, dest: &Path) -> (Vec<SourceTrack>, Vec<SourcePlaylist>) {
    // Exported as c55555 (same length), so the export does not number it.
    let tracks = vec![with_companions(named(src, 1, "c10013.mp3")), with_companions(named(src, 2, "c55555.mp3"))];
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }];
    sync_to(dest, &tracks, &playlists, DB_ID).unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest)).unwrap();
    let library = device_library::read(dest, Format::DeviceLibrary).unwrap();
    let first = library.tracks.iter().find(|t| t.title == "Track 1").unwrap();
    let second = library.tracks.iter().find(|t| t.title == "Track 2").unwrap();
    assert_eq!(first.analysis_path, "/PIONEER/USBANLZ/P009/0000DCB1/ANLZ0000.DAT");
    let path = second.path.replace("c55555", "c16700");
    let analysis = first.analysis_path.replace("ANLZ0000", "ANLZ0001");
    std::fs::rename(dest.join(second.path.trim_start_matches('/')), dest.join(path.trim_start_matches('/'))).unwrap();
    let old = dest.join(second.analysis_path.trim_start_matches('/'));
    for extension in ["DAT", "EXT", "2EX"] {
        let source = old.with_extension(extension);
        let parsed = rbl_anlz::parse(&std::fs::read(&source).unwrap()).unwrap();
        std::fs::write(dest.join(analysis.trim_start_matches('/')).with_extension(extension), parsed.with_path(&path)).unwrap();
        std::fs::remove_file(source).unwrap();
    }
    std::fs::remove_dir(old.parent().unwrap()).unwrap();
    let pdb = dest.join("PIONEER/rekordbox/export.pdb");
    let mut bytes = std::fs::read(&pdb).unwrap();
    assert!(replace_bytes(&mut bytes, b"c55555", b"c16700") >= 1);
    assert_eq!(replace_bytes(&mut bytes, second.analysis_path.as_bytes(), analysis.as_bytes()), 1);
    std::fs::write(&pdb, bytes).unwrap();
    let one = rbl_onelibrary::playlists::open_read_write(&dest.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(one.execute("UPDATE content SET path = ?1, fileName = 'c16700.mp3', analysisDataFilePath = ?2 WHERE title = 'Track 2'", rusqlite::params![path, analysis]).unwrap(), 1);
    (tracks, playlists)
}

/// Every file under `dir`, relative to it, sorted.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { walk(root, &path, out); } else { out.push(path.strip_prefix(root).unwrap().to_string_lossy().into_owned()); }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn analysis_of(dest: &Path) -> Vec<(String, String, String, Vec<u8>)> {
    let mut out: Vec<_> = device_library::read(dest, Format::DeviceLibrary).unwrap().tracks.into_iter()
        .map(|t| { let bytes = std::fs::read(dest.join(t.analysis_path.trim_start_matches('/'))).unwrap(); (t.title, t.path, t.analysis_path, bytes) })
        .collect();
    out.sort();
    out
}

#[test]
fn a_stick_track_in_rekordboxs_second_analysis_slot_keeps_it() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    collision_stick(src.path(), dest.path());
    let before = analysis_of(dest.path());
    assert_eq!(before[1].2, "/PIONEER/USBANLZ/P009/0000DCB1/ANLZ0001.DAT");

    // Another library's track: both of the stick's are carried.
    let other = tempfile::tempdir().unwrap();
    let mine = vec![named(other.path(), 9, "mine.mp3")];
    let playlists = vec![SourcePlaylist { id: 20, name: "Mine".into(), track_indices: vec![0], ..Default::default() }];
    sync_to(dest.path(), &mine, &playlists, DB_ID + 1).unwrap();
    let after = analysis_of(dest.path());
    assert_eq!(after.len(), 3);
    assert_eq!(after[..2], before[..], "both keep their paths, slots and analysis bytes");
    assert!(rbl_export::verify(dest.path()).unwrap().is_ok());

    // And the next sync, from the record this one left, with Verify all
    // files, which checks every analysis file the record names.
    let source = sync(DB_ID + 1);
    rbl_export::export_with_options(dest.path(), &mine, &playlists, &[],
        &rbl_export::ExportOptions { verify_all: true, sync: Some(&source), ..Default::default() }, &mut |_| {}).unwrap();
    assert_eq!(analysis_of(dest.path())[..2], before[..]);
}

#[test]
fn a_selected_track_beside_one_in_rekordboxs_second_slot_syncs() {
    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    let (tracks, _) = collision_stick(src.path(), dest.path());
    let before = analysis_of(dest.path());
    let files_before = files_under(&dest.path().join("Contents"));
    let analysis_before = files_under(&dest.path().join("PIONEER/USBANLZ"));
    // Track 1, in `ANLZ0000`, is this library's and selected; track 2 is not.
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0], ..Default::default() }];
    sync_to(dest.path(), &tracks[..1], &playlists, DB_ID).unwrap();
    let after = analysis_of(dest.path());
    assert_eq!(after.len(), 2);
    assert_eq!(after[1], before[1], "the stick's track keeps its slot and analysis");
    // The carried track holds `ANLZ0001`, not the whole directory: track 1
    // keeps its path and `ANLZ0000`, and is not copied again beside it.
    assert_eq!(after[0], before[0], "the selected track keeps its path and ANLZ0000");
    assert_eq!(files_under(&dest.path().join("Contents")), files_before, "no second copy, nothing orphaned");
    assert_eq!(files_under(&dest.path().join("PIONEER/USBANLZ")), analysis_before);
    assert!(rbl_export::verify(dest.path()).unwrap().is_ok());
}

/// The exact PAWSA path reported in #616 shares an existing stick with a
/// rekordbox `ANLZ0001` collision. A new library selects a different audio
/// file that wants PAWSA's path. The carried PAWSA file must keep its name,
/// hashed analysis directory and PPTH through both syncs.
#[test]
fn a_pawsa_path_collision_keeps_every_carried_file_on_first_and_repeat_sync() {
    use std::collections::{BTreeMap, BTreeSet};

    type CarriedRow = (String, String, BTreeMap<String, Vec<u8>>);

    fn rows_and_bytes(dest: &Path) -> BTreeMap<String, CarriedRow> {
        device_library::read(dest, Format::DeviceLibrary).unwrap().tracks.into_iter().map(|row| {
            let mut files = BTreeMap::new();
            files.insert(row.path.clone(), std::fs::read(dest.join(row.path.trim_start_matches('/'))).unwrap());
            for extension in ["DAT", "EXT", "2EX"] {
                let relative = Path::new(&row.analysis_path).with_extension(extension).to_string_lossy().into_owned();
                let bytes = std::fs::read(dest.join(relative.trim_start_matches('/'))).unwrap();
                assert_eq!(rbl_anlz::parse(&bytes).unwrap().path().as_deref(), Some(row.path.as_str()), "{relative} PPTH");
                files.insert(relative, bytes);
            }
            (row.title, (row.path, row.analysis_path, files))
        }).collect()
    }

    fn assert_no_orphans(dest: &Path) {
        let rows = device_library::read(dest, Format::DeviceLibrary).unwrap().tracks;
        let audio: BTreeSet<_> = rows.iter().map(|row| row.path.trim_start_matches('/').trim_start_matches("Contents/").to_owned()).collect();
        let analysis: BTreeSet<_> = rows.iter().flat_map(|row| ["DAT", "EXT", "2EX"].map(move |extension| {
            Path::new(&row.analysis_path).with_extension(extension).to_string_lossy()
                .trim_start_matches('/').trim_start_matches("PIONEER/USBANLZ/").to_owned()
        })).collect();
        assert_eq!(audio.len(), rows.len(), "no duplicate audio paths");
        assert_eq!(analysis.len(), rows.len() * 3, "no duplicate analysis paths");
        assert_eq!(files_under(&dest.join("Contents")).into_iter().collect::<BTreeSet<_>>(), audio, "no orphan audio");
        assert_eq!(files_under(&dest.join("PIONEER/USBANLZ")).into_iter().collect::<BTreeSet<_>>(), analysis, "no orphan analysis");
    }

    let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
    collision_stick(src.path(), dest.path());
    let pawsa_name = "PAWSA - TOO COOL TO BE CARELESS.mp3";
    let mut pawsa = with_companions(named(src.path(), 3, pawsa_name));
    pawsa.artist = "PAWSA".into();
    pawsa.album = "TOO COOL TO BE CARELESS".into();
    let pawsa_playlist = [SourcePlaylist { id: 30, name: "PAWSA".into(), track_indices: vec![0], ..Default::default() }];
    sync_to(dest.path(), &[pawsa], &pawsa_playlist, DB_ID + 2).unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();

    let before = rows_and_bytes(dest.path());
    let pawsa_path = "/Contents/PAWSA/TOO COOL TO BE CARELESS/PAWSA - TOO COOL TO BE CARELESS.mp3";
    assert_eq!(before["Track 3"].0, pawsa_path);
    assert_eq!(before["Track 3"].1, "/PIONEER/USBANLZ/P059/0001CF89/ANLZ0000.DAT");
    assert_eq!(before["Track 2"].1, "/PIONEER/USBANLZ/P009/0000DCB1/ANLZ0001.DAT");
    assert_no_orphans(dest.path());

    let other = tempfile::tempdir().unwrap();
    let mut selected = with_companions(named(other.path(), 9, pawsa_name));
    selected.artist = "PAWSA".into();
    selected.album = "TOO COOL TO BE CARELESS".into();
    let playlists = [SourcePlaylist { id: 40, name: "New library".into(), track_indices: vec![0], ..Default::default() }];
    let source = sync(DB_ID + 3);
    for attempt in 0..2 {
        rbl_export::export_with_options(dest.path(), &[selected.clone()], &playlists, &[],
            &rbl_export::ExportOptions { verify_all: true, sync: Some(&source), ..Default::default() }, &mut |_| {}).unwrap();
        let after = rows_and_bytes(dest.path());
        assert_eq!(after.len(), 4);
        for (title, original) in &before {
            assert_eq!(&after[title], original, "carried {title} changed on attempt {attempt}");
        }
        assert_ne!(after["Track 9"].0, pawsa_path, "selected audio must take a suffix");
        assert_no_orphans(dest.path());
        assert!(rbl_export::verify(dest.path()).unwrap().is_ok());
    }
}

// Any other integer column of a stick's `exportLibrary.db` holding a value the
// unsigned type this crate writes cannot take (#787, the same "Integer N out of
// range at index N" as #545/#583) must not stop the sync either: the value
// reads as unknown (zero) and the rest of the stick syncs.
#[test]
fn an_out_of_range_integer_column_on_a_track_the_stick_keeps_does_not_stop_the_sync() {
    for (column, value) in [
        ("trackNo", -1i64), ("discNo", -1), ("bitDepth", -1), ("djPlayCount", -1),
        ("bitrate", -1), ("samplingRate", -1), ("length", -1), ("length", 70_000),
        ("color_id", -1),
    ] {
        let (src, dest) = (tempfile::tempdir().unwrap(), common::stick());
        let (tracks, _) = exported_elsewhere(src.path(), dest.path(), 3, DB_ID);
        let kept = device_id(dest.path(), 3);
        let one = rbl_onelibrary::playlists::open_read_write(&dest.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
        assert_eq!(one.execute(&format!("UPDATE content SET {column} = ?2 WHERE content_id = ?1"), rusqlite::params![kept, value]).unwrap(), 1);
        drop(one);
        let selection = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }];
        let report = sync_to(dest.path(), &tracks[..2], &selection, DB_ID).unwrap_or_else(|e| panic!("{column}={value}: {e}"));
        assert_eq!(report.tracks, 3, "{column}={value}: the stick still holds track 3");
        sync_to(dest.path(), &tracks[..2], &selection, DB_ID).unwrap_or_else(|e| panic!("{column}={value} second sync: {e}"));
    }
}
