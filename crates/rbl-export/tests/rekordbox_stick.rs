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

/// A stick holding tracks rated 0 to 5 in one playlist, whose `OneLibrary`
/// ratings are then rewritten the way rekordbox stores them: the stars
/// themselves, as `export.pdb` has them [OBS: a rekordbox 7 stick, ratings 1
/// to 5 in `exportLibrary.db` equal to the same tracks' `export.pdb`
/// ratings]. The manifest is removed, so the stick is one rbxport has never
/// synced.
fn rekordbox_stick(src: &Path, dest: &Path) {
    let tracks: Vec<SourceTrack> = (0..=5).map(|stars| track(src, u64::from(stars) + 1, stars)).collect();
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    export(dest, &tracks, &playlists).unwrap();
    let one = rbl_onelibrary::playlists::open_read_write(&dest.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    let rows = one.execute("UPDATE content SET rating = rating / 51", []).unwrap();
    assert_eq!(rows, tracks.len());
    drop(one);
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
