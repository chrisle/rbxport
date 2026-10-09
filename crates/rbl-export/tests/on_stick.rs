//! Exporting tracks whose files the library already keeps on the stick.
//!
//! rekordbox does not copy such a file: the databases name it where it is
//! (`DatabaseMediator::get_device_file_path_candidate`, 7.2.19). A DJ who
//! keeps music on the stick then gets only the databases and analysis
//! written, not a second copy of every track under `Contents/`.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use rbl_export::{export, export_full, verify, Manifest, SourcePlaylist, SourceTrack, SyncNode, SyncSource};

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

/// A sync from a library: what the app does, with the stick's own history
/// and device playlists reconciled into the selection.
fn sync(root: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist]) -> rbl_export::Result<rbl_export::ExportReport> {
    let tree = playlists.iter().map(|p| SyncNode { id: p.id, parent: p.parent_id, attribute: u8::from(p.folder) }).collect();
    export_full(root, tracks, playlists, &[], None, Some(&SyncSource { db_id: 123, tree, automatic: false }), &mut |_| {})
}

fn listed(id: u64, tracks: &[SourceTrack]) -> Vec<SourcePlaylist> {
    vec![SourcePlaylist { id, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }]
}

/// Writes to the stick's exportLibrary.db as a player would.
fn on_the_player(root: &Path, sql: &str) {
    let conn = rusqlite::Connection::open(rbl_export::export_root(root).join("rekordbox/exportLibrary.db")).unwrap();
    conn.pragma_update(None, "cipher", "sqlcipher").unwrap();
    conn.pragma_update(None, "legacy", 4).unwrap();
    conn.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
    conn.execute_batch(sql).unwrap();
}

/// The audio paths export.pdb names.
fn pdb_paths(root: &Path) -> Vec<String> {
    let snapshot = rbl_export::snapshot::Snapshot::read(root).unwrap();
    snapshot.legacy.as_ref().unwrap().tracks.iter().map(|t| t.path.clone()).collect()
}

/// A manifest as dev and v1.2.0 wrote it: no `in_place` on any entry.
fn forget_in_place(root: &Path) {
    let path = Manifest::path(root);
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for entry in json["tracks"].as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("in_place");
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("in_place"));
}

/// Deselects the in-place track while something on the stick still lists it,
/// syncs, then (when given) drops that listing and syncs again.
fn deselect_while_the_stick_lists_it(lists_it: &str, drop_it: Option<&str>, older_manifest: bool) {
    let dest = tempfile::tempdir().unwrap();
    let file = dest.path().join("Music/one.mp3");
    let tracks = vec![track(&file, 1, "One", "TRIODE", 1)];
    sync(dest.path(), &tracks, &listed(10, &tracks)).unwrap();
    assert!(recorded(dest.path(), 1).in_place);
    if older_manifest { forget_in_place(dest.path()); }

    on_the_player(dest.path(), lists_it);
    let kept = sync(dest.path(), &[], &[]).expect("a sync that keeps the listed track succeeds");
    assert_eq!(kept.bytes_copied, 0, "nothing is copied over the library's file");
    assert_eq!(pdb_paths(dest.path()), ["/Music/one.mp3"], "the kept track keeps its path");
    let entry = Manifest::load(dest.path()).unwrap().tracks;
    assert_eq!(entry.len(), 1);
    assert_eq!(entry[0].audio, "/Music/one.mp3");
    assert!(entry[0].in_place, "still recorded as the library's file, not the export's");
    assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048]);
    assert!(verify(dest.path()).unwrap().is_ok());
    // And again: the next sync must not fail on it either.
    sync(dest.path(), &[], &[]).expect("every later sync succeeds too");

    if let Some(drop_it) = drop_it {
        on_the_player(dest.path(), drop_it);
        sync(dest.path(), &[], &[]).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048], "the library's file stays on the stick");
        assert!(verify(dest.path()).unwrap().is_ok());
    }
}

const IN_HISTORY: &str = "INSERT INTO history VALUES(7,1,'Tonight',0,0); INSERT INTO history_content VALUES(7,1,1);";
const OUT_OF_HISTORY: &str = "DELETE FROM history_content; DELETE FROM history;";
const IN_DEVICE_PLAYLIST: &str = "INSERT INTO playlist VALUES(99,1,'On the deck',NULL,0,0); INSERT INTO playlist_content VALUES(99,1,1);";

#[test]
fn a_deselected_in_place_track_the_history_lists_stays_where_it_is() {
    deselect_while_the_stick_lists_it(IN_HISTORY, Some(OUT_OF_HISTORY), false);
}

#[test]
fn a_deselected_in_place_track_a_device_playlist_lists_stays_where_it_is() {
    deselect_while_the_stick_lists_it(IN_DEVICE_PLAYLIST, None, false);
}

#[test]
fn a_track_an_older_manifest_names_in_place_survives_the_history_keeping_it() {
    deselect_while_the_stick_lists_it(IN_HISTORY, Some(OUT_OF_HISTORY), true);
}

#[test]
fn an_older_manifest_without_in_place_never_deletes_the_librarys_file() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    // Where dev and v1.2.0 copied it onto itself and recorded it as theirs.
    let file = dest.path().join("Contents/TRIODE/Single/one.mp3");
    let tracks = vec![
        track(&file, 1, "One", "TRIODE", 1),
        track(&src.path().join("two.mp3"), 2, "Two", "ARTBAT", 2),
    ];
    sync(dest.path(), &tracks, &listed(10, &tracks)).unwrap();
    forget_in_place(dest.path());
    assert!(!recorded(dest.path(), 1).in_place);

    // The first sync after upgrading also deselects it.
    let kept = vec![tracks[1].clone()];
    let second = sync(dest.path(), &kept, &listed(10, &kept)).unwrap();
    assert_eq!(second.removed, 1);
    assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048], "the library's file stays on the stick");
    assert!(dest.path().join("Contents/ARTBAT/Single/two.mp3").is_file());
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn an_older_manifest_never_deletes_the_librarys_file_when_its_path_changes() {
    let dest = tempfile::tempdir().unwrap();
    let file = dest.path().join("Contents/TRIODE/Single/one.mp3");
    let tracks = vec![track(&file, 1, "One", "TRIODE", 1)];
    sync(dest.path(), &tracks, &listed(10, &tracks)).unwrap();
    forget_in_place(dest.path());

    // The library moves its file elsewhere on the stick and keeps the old one
    // too: the entry's audio path changes, and the old file is still the
    // library's own, not a copy the export made.
    let moved = dest.path().join("Music/one.mp3");
    let tracks = vec![track(&moved, 1, "One", "TRIODE", 1)];
    assert_eq!(recorded(dest.path(), 1).source, file.to_string_lossy());
    sync(dest.path(), &tracks, &listed(10, &tracks)).unwrap();
    assert_eq!(recorded(dest.path(), 1).audio, "/Music/one.mp3");
    assert_eq!(std::fs::read(&file).unwrap(), vec![1; 2048], "the old library file is not removed as obsolete");
    assert!(verify(dest.path()).unwrap().is_ok());
}
