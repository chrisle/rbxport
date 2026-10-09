//! A stick as rekordbox leaves it, before its first sync from here (#284).
//!
//! Every stick here is written by the export pipeline into a temporary
//! directory and then given rekordbox's encodings; nothing touches a real
//! device.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    rekordbox_stick(src.path(), dest.path());
    let tracks: Vec<SourceTrack> = (0..=5).map(|stars| track(src.path(), u64::from(stars) + 1, stars)).collect();
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    let report = export(dest.path(), &tracks, &playlists).unwrap();
    assert_eq!(report.tracks, tracks.len());
}

#[test]
fn the_sticks_onelibrary_browses_with_rekordboxs_stars() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
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
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (tracks, playlists) = exported_elsewhere(src.path(), dest.path(), 3, DB_ID);
    disagree_on_comment(dest.path(), device_id(dest.path(), 2));
    let report = sync_to(dest.path(), &tracks, &playlists, DB_ID).unwrap();
    assert_eq!(report.tracks, 3);
    let snapshot = Snapshot::read(dest.path()).unwrap();
    assert_eq!(snapshot.legacy, snapshot.one, "both databases carry the library's version again");
}

#[test]
fn a_disagreement_the_sync_would_carry_names_what_differs() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
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
