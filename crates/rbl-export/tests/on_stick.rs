//! Exporting tracks whose files the library already keeps on the stick.
//!
//! rekordbox does not copy such a file: the databases name it where it is
//! (`DatabaseMediator::get_device_file_path_candidate`, 7.2.19). A DJ who
//! keeps music on the stick then gets only the databases and analysis
//! written, not a second copy of every track under `Contents/`.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use rbl_export::{export, verify, Manifest, SourcePlaylist, SourceTrack};

fn track(path: &Path, id: u64, title: &str, artist: &str, byte: u8) -> SourceTrack {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![byte; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path.to_owned(),
        title: title.into(),
        artist: artist.into(),
        album: "Single".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        date_added: "2026-10-08".into(),
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

fn one_list(tracks: &[SourceTrack]) -> Vec<SourcePlaylist> {
    vec![SourcePlaylist { name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }]
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() { out.extend(files_under(&path)); } else { out.push(path.to_string_lossy().into_owned()); }
        }
    }
    out
}

fn recorded(dest: &Path, library_id: u64) -> rbl_export::ManifestTrack {
    Manifest::load(dest).unwrap().tracks.into_iter().find(|t| t.library_id == library_id).unwrap()
}

#[test]
fn a_track_already_on_the_stick_is_pointed_at_not_copied() {
    let dest = tempfile::tempdir().unwrap();
    let file = dest.path().join("Music/TRIODE/All U Need.mp3");
    let tracks = vec![track(&file, 1, "All U Need", "TRIODE", 1)];

    let report = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!(report.tracks, 1);
    assert_eq!(report.in_place, 1);
    assert_eq!(report.reused, 1, "nothing was written for its audio");
    assert_eq!(report.bytes_copied, 0);
    assert_eq!(report.analysis_files, 1, "the analysis is still the export's to write");
    assert!(files_under(&dest.path().join("Contents")).is_empty(), "no second copy under Contents: {:?}", files_under(&dest.path().join("Contents")));
    assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048], "the file itself is untouched");

    let entry = recorded(dest.path(), 1);
    assert_eq!(entry.audio, "/Music/TRIODE/All U Need.mp3");
    assert!(entry.in_place);

    // Both databases name it there, its analysis sits where a CDJ derives it
    // from that path, and the analysis names the same path.
    let check = verify(dest.path()).unwrap();
    assert!(check.is_ok(), "{check:?}");
    assert_eq!(check.tracks, 1);
    assert_eq!(check.audio_present, 1);
    assert_eq!(check.playlist_entries, 1);
}

#[test]
fn only_the_tracks_not_on_the_stick_are_copied() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(&dest.path().join("Music/one.mp3"), 1, "One", "TRIODE", 1),
        track(&src.path().join("two.mp3"), 2, "Two", "ARTBAT", 2),
    ];

    let report = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!((report.tracks, report.in_place, report.bytes_copied), (2, 1, 2048));
    assert!(dest.path().join("Contents/ARTBAT/Single/two.mp3").is_file());
    assert!(!dest.path().join("Contents/TRIODE").exists());
    assert_eq!(recorded(dest.path(), 1).audio, "/Music/one.mp3");
    assert!(!recorded(dest.path(), 2).in_place);
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_second_sync_still_copies_nothing_and_leaves_the_file_alone() {
    let dest = tempfile::tempdir().unwrap();
    let file = dest.path().join("Music/one.mp3");
    let tracks = vec![track(&file, 1, "One", "TRIODE", 1)];
    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();

    let second = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!((second.in_place, second.bytes_copied, second.analysis_files), (1, 0, 0));
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), modified, "not rewritten");
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn dropping_a_track_never_deletes_the_librarys_own_file() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    // Where a copy of it would go, too: before, a sync "copied" it onto
    // itself, recorded it as its own, and deleted it once deselected.
    let file = dest.path().join("Contents/TRIODE/Single/one.mp3");
    let tracks = vec![
        track(&file, 1, "One", "TRIODE", 1),
        track(&src.path().join("two.mp3"), 2, "Two", "ARTBAT", 2),
    ];
    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    let analysis = dest.path().join(recorded(dest.path(), 1).anlz_dir.trim_start_matches('/'));
    assert!(analysis.is_dir());

    let kept = vec![tracks[1].clone()];
    let second = export(dest.path(), &kept, &one_list(&kept)).unwrap();
    assert_eq!(second.removed, 1);
    assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048], "the library's file stays on the stick");
    assert!(!analysis.exists(), "the analysis the export wrote for it goes");
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_copied_track_never_lands_on_a_file_the_library_keeps_there() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    // The library's file sits exactly where a copy of another track would go.
    let file = dest.path().join("Contents/TRIODE/Single/song.mp3");
    let mine = track(&file, 1, "Mine", "TRIODE", 1);
    let other = track(&src.path().join("song.mp3"), 2, "Other", "TRIODE", 2);

    let both = vec![other.clone(), mine.clone()];
    export(dest.path(), &both, &one_list(&both)).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048]);
    assert_ne!(recorded(dest.path(), 2).audio, "/Contents/TRIODE/Single/song.mp3");
    assert!(verify(dest.path()).unwrap().is_ok());

    // Still so once the library's track has left the selection.
    let alone = vec![other];
    export(dest.path(), &alone, &one_list(&alone)).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048]);
    assert_ne!(recorded(dest.path(), 2).audio, "/Contents/TRIODE/Single/song.mp3");
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_path_a_stick_cannot_name_is_copied_as_rekordbox_does() {
    let dest = tempfile::tempdir().unwrap();
    // A trailing dot is one of the things `hasSpecialCharInFilePath` rejects.
    let file = dest.path().join("Music/Vol. 2./one.mp3");
    let tracks = vec![track(&file, 1, "One", "TRIODE", 1)];

    let report = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!((report.in_place, report.bytes_copied), (0, 2048));
    assert!(dest.path().join("Contents/TRIODE/Single/one.mp3").is_file());
    assert!(file.is_file());
    assert!(verify(dest.path()).unwrap().is_ok());
}
