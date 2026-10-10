//! Exporting twice to the same stick.
//!
//! The first export writes everything. The second is a sync: it copies what
//! changed, leaves what did not, and takes off what is no longer selected —
//! while every track keeps the id a player already cached it under.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_export::{export, verify, Manifest, SourcePlaylist, SourceTrack};

/// Writes a dummy audio file and returns a track that points at it.
fn track(dir: &std::path::Path, id: u64, title: &str, artist: &str) -> SourceTrack {
    let path = dir.join(format!("source-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 2048]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: title.into(),
        artist: artist.into(),
        album: "Single".into(),
        genre: "House".into(),
        key: "Am".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        date_added: "2026-09-06".into(),
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

fn one_list(tracks: &[SourceTrack]) -> Vec<SourcePlaylist> {
    vec![SourcePlaylist { name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }]
}

fn ids(destination: &std::path::Path) -> Vec<(u64, u32)> {
    let mut out: Vec<(u64, u32)> = Manifest::load(destination)
        .expect("a manifest")
        .tracks
        .iter()
        .map(|t| (t.library_id, t.export_id))
        .collect();
    out.sort_unstable();
    out
}

#[test]
fn a_second_export_of_the_same_tracks_copies_nothing() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
    ];

    let first = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!(first.tracks, 2);
    assert_eq!(first.reused, 0);
    assert_eq!(first.bytes_copied, 4096);
    assert_eq!(first.analysis_files, 2);
    assert_eq!((first.playlists_added, first.playlists_removed), (1, 0));
    assert_eq!((first.tracks_added, first.tracks_updated), (2, 0));

    let second = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!(second.tracks, 2, "the stick still holds both");
    assert_eq!(second.reused, 2);
    assert_eq!(second.bytes_copied, 0, "nothing changed, so nothing was copied");
    assert_eq!(second.bytes_reused, 4096);
    assert_eq!(second.analysis_files, 0, "the analysis was already there");
    assert_eq!(second.removed, 0);
    assert_eq!((second.playlists_added, second.playlists_removed), (0, 0));
    assert_eq!((second.tracks_added, second.tracks_updated), (0, 0));

    // And the stick is still a stick.
    let check = verify(dest.path()).unwrap();
    assert!(check.is_ok());
    assert_eq!(check.tracks, 2);
    assert_eq!(check.playlist_entries, 2);
}

#[test]
fn a_changed_source_is_copied_again() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
    ];
    export(dest.path(), &tracks, &[]).unwrap();

    // A re-encode: different length.
    std::fs::write(&tracks[0].source_path, vec![9u8; 8192]).unwrap();

    let second = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!(second.reused, 1, "only the untouched track is left alone");
    assert_eq!(second.bytes_copied, 8192);

    let on_stick = dest.path().join("Contents/TRIODE/Single/source-1.mp3");
    assert_eq!(std::fs::metadata(&on_stick).unwrap().len(), 8192, "the stick has the new audio");
}

#[test]
fn a_rewrite_of_the_same_length_is_still_noticed() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();

    // Same byte count, different bytes — size alone cannot see this, which is
    // why the modification time is kept to nanoseconds.
    std::fs::write(&tracks[0].source_path, vec![7u8; 2048]).unwrap();

    let second = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!(second.reused, 0);
    assert_eq!(second.bytes_copied, 2048);
    let on_stick = dest.path().join("Contents/TRIODE/Single/source-1.mp3");
    assert_eq!(std::fs::read(&on_stick).unwrap()[0], 7, "the stick has the new bytes");
}

#[test]
fn a_track_dropped_from_the_selection_leaves_the_stick() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
    ];
    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    let gone = dest.path().join("Contents/ARTBAT/Single/source-2.mp3");
    assert!(gone.is_file());

    let kept = vec![tracks[0].clone()];
    let second = export(dest.path(), &kept, &one_list(&kept)).unwrap();
    assert_eq!(second.tracks, 1);
    assert_eq!(second.removed, 1);
    assert!(!gone.exists(), "its audio is off the stick");
    assert!(
        !dest.path().join("Contents/ARTBAT").exists(),
        "and the directory it emptied went with it"
    );
    assert!(dest.path().join("Contents/TRIODE/Single/source-1.mp3").is_file());

    let check = verify(dest.path()).unwrap();
    assert!(check.is_ok());
    assert_eq!(check.tracks, 1);
    assert_eq!(check.playlist_entries, 1);
}

#[test]
fn export_ids_survive_a_removal_so_a_deck_does_not_repoint() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 10, "One", "A"),
        track(src.path(), 20, "Two", "B"),
        track(src.path(), 30, "Three", "C"),
    ];
    export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!(ids(dest.path()), vec![(10, 1), (20, 2), (30, 3)]);

    // Drop the middle one. Reassigning by position would slide Three onto id 2
    // and every cached waveform on the deck would name the wrong track.
    let kept = vec![tracks[0].clone(), tracks[2].clone()];
    export(dest.path(), &kept, &[]).unwrap();
    assert_eq!(ids(dest.path()), vec![(10, 1), (30, 3)]);

    // A new track takes the freed id rather than growing the range forever.
    let mut grown = kept.clone();
    grown.insert(1, track(src.path(), 40, "Four", "D"));
    export(dest.path(), &grown, &[]).unwrap();
    assert_eq!(ids(dest.path()), vec![(10, 1), (30, 3), (40, 2)]);
}

#[test]
fn renaming_an_artist_moves_the_audio_and_leaves_no_copy_behind() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();
    assert!(dest.path().join("Contents/TRIODE/Single/source-1.mp3").is_file());

    tracks[0].artist = "Triode Live".into();
    let second = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!(second.reused, 0, "a different place on the stick means a copy");
    assert!(dest.path().join("Contents/Triode Live/Single/source-1.mp3").is_file());
    assert!(
        !dest.path().join("Contents/TRIODE").exists(),
        "the old path must not linger unreferenced"
    );

    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_rename_that_only_changes_case_keeps_the_track() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();

    // On a case-insensitive disk — which a Mac and a FAT32 stick both are —
    // the new path and the old path are the same file. Tidying away "the old
    // one" would delete the audio the databases point at.
    tracks[0].artist = "Triode".into();
    export(dest.path(), &tracks, &[]).unwrap();

    let check = verify(dest.path()).unwrap();
    assert!(check.is_ok(), "missing: {:?}", check.missing_audio);
    assert_eq!(check.audio_present, 1);
}

#[test]
fn a_skipped_track_does_not_shift_the_playlist_under_the_ones_after_it() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "Missing", "A"),
        track(src.path(), 2, "Present", "B"),
    ];
    // The first track's audio is gone by the time the export runs.
    std::fs::remove_file(&tracks[0].source_path).unwrap();

    let report = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!(report.tracks, 1);
    assert_eq!(report.skipped, vec!["Missing".to_owned()]);

    // The playlist named indices 0 and 1; only index 1 was written, and it must
    // be the track that survived, not the one that took its place in the list.
    let bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let entries = pdb
        .table(rbl_pdb::PageType::PlaylistEntries)
        .map(|t| pdb.playlist_entries(t))
        .unwrap_or_default();
    assert_eq!(entries.len(), 1);
    let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(entries[0].track_id, rows[0].id, "the entry names the track that was written");
    assert_eq!(rows[0].title, "Present");
}

#[test]
fn a_stick_with_no_manifest_of_ours_is_written_in_full() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();

    // As if rekordbox itself had written the stick: our record is not there.
    std::fs::remove_file(Manifest::path(dest.path())).unwrap();

    let second = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!(second.reused, 0, "with nothing to compare against, everything is written");
    assert_eq!(second.bytes_copied, 2048);
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_file_deleted_off_the_stick_is_put_back() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();

    let audio = dest.path().join("Contents/TRIODE/Single/source-1.mp3");
    std::fs::remove_file(&audio).unwrap();
    let saved = rbl_export::Manifest::load(dest.path()).unwrap();
    let anlz = dest.path().join(saved.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.DAT");
    std::fs::remove_file(&anlz).unwrap();

    // The manifest still says both are there. Trusting it would leave a stick
    // whose database names files that do not exist.
    let second = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!(second.reused, 0);
    assert_eq!(second.analysis_files, 1);
    assert!(audio.is_file());
    assert!(anlz.is_file());
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_sync_keeps_the_settings_the_stick_already_carries() {
    use rbl_onelibrary::settings::StickSettings;

    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();

    // What the device panel writes between two syncs.
    let db = dest.path().join("PIONEER/rekordbox/exportLibrary.db");
    let mut settings = StickSettings::read(&db).unwrap();
    settings.device_name = "FRIDAY".to_owned();
    settings.colors[2].name = "Peak time".to_owned();
    let genre = settings.categories.iter_mut().find(|s| s.menu_item == 1).unwrap();
    genre.visible = true;
    genre.seq = 11;
    settings.sub_column = Some(11);
    settings.write(&db).unwrap();

    // The database is rebuilt, and every one of those survives.
    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    let after = StickSettings::read(&db).unwrap();
    assert_eq!(after, settings);
}

#[test]
fn an_estimate_counts_what_a_sync_would_copy_keep_and_remove() {
    use rbl_export::estimate::estimate;
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
    ];

    // A stick with no export: everything is new.
    let cold = estimate(dest.path(), 0, &tracks, false, None);
    assert_eq!((cold.tracks_new, cold.copy_bytes, cold.tracks_kept), (2, 4096, 0));

    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    let db_id = Manifest::load(dest.path()).unwrap().db_id;

    // The same selection: nothing to copy, both tracks already there.
    let same = estimate(dest.path(), db_id, &tracks, false, None);
    assert_eq!((same.copy_bytes, same.tracks_kept, same.reuse_bytes), (0, 2, 4096));

    // One more track, and the second one dropped.
    let mut next = vec![tracks[0].clone(), track(src.path(), 3, "Nova", "KYAU")];
    let moved = estimate(dest.path(), db_id, &next, true, None);
    assert_eq!((moved.tracks_new, moved.copy_bytes), (1, 2048));
    assert_eq!((moved.tracks_removed, moved.free_bytes), (1, 2048));

    // A source that changed since it was written is copied again.
    std::fs::write(&next[0].source_path, vec![9u8; 3000]).unwrap();
    next.truncate(1);
    let changed = estimate(dest.path(), db_id, &next, true, None);
    assert_eq!((changed.tracks_changed, changed.copy_bytes), (1, 3000));

    // Another library's record is not this one's to trust.
    let other = estimate(dest.path(), db_id + 1, &tracks, false, None);
    assert_eq!((other.tracks_new, other.tracks_kept), (2, 0));
}

/// Moves a file's modification time `seconds` away from where it is, as a
/// write some time after the export would.
fn age(path: &std::path::Path, seconds: i64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    let now = file.metadata().unwrap().modified().unwrap();
    let moved = if seconds >= 0 {
        now + std::time::Duration::from_secs(seconds.unsigned_abs())
    } else {
        now - std::time::Duration::from_secs(seconds.unsigned_abs())
    };
    file.set_modified(moved).unwrap();
}

/// Every audio and analysis file under `root`, by path relative to it.
fn assets(root: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(path.strip_prefix(root).unwrap().to_string_lossy().into_owned(), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    for top in ["Contents", "PIONEER/USBANLZ"] {
        let dir = root.join(top);
        if dir.is_dir() {
            walk(root, &dir, &mut out);
        }
    }
    out
}

#[test]
fn the_manifest_records_the_copy_as_the_stick_holds_it() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();

    let saved = Manifest::load(dest.path()).unwrap();
    let meta = std::fs::metadata(dest.path().join("Contents/TRIODE/Single/source-1.mp3")).unwrap();
    let modified = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i64;
    assert_eq!(saved.tracks[0].copy_stamp, Some((meta.len(), modified)), "the published copy's size and time");
}

/// Issue #293: a sync of tracks that have not changed must not read the
/// stick's copies back. At USB 2.0 speeds that was every byte on the stick,
/// every sync.
#[cfg(unix)]
#[test]
fn a_sync_of_unchanged_tracks_does_not_read_the_copies_back() {
    use std::os::unix::fs::PermissionsExt;
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
    ];
    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();

    // Neither the copy nor the source can be opened; only their sizes and
    // times can be seen.
    let copy = dest.path().join("Contents/TRIODE/Single/source-1.mp3");
    for path in [&copy, &tracks[0].source_path] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    if std::fs::File::open(&copy).is_ok() {
        // Running as root: permissions do not stop a read, so there is
        // nothing to prove here.
        return;
    }
    let second = export(dest.path(), &tracks, &one_list(&tracks));
    for path in [&copy, &tracks[0].source_path] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let second = second.expect("a sync that has nothing to copy has nothing to read");
    assert_eq!((second.reused, second.bytes_copied), (2, 0));
    assert!(verify(dest.path()).unwrap().is_ok());
}

/// rekordbox exporting over a track rbx put on the stick rewrites the file,
/// and the write moves its time. The copy is then hashed, found different,
/// and replaced, and the stick ends up as a full export would leave it.
#[test]
fn a_copy_rewritten_on_the_stick_is_replaced_as_a_full_export_would() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
    ];
    export(dest.path(), &tracks, &one_list(&tracks)).unwrap();

    // Same length, other bytes, written a minute after the export.
    let copy = dest.path().join("Contents/TRIODE/Single/source-1.mp3");
    std::fs::write(&copy, vec![0xEE_u8; 2048]).unwrap();
    age(&copy, 60);

    let second = export(dest.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!((second.reused, second.bytes_copied), (1, 2048), "only the rewritten copy is written again");
    assert_eq!(std::fs::read(&copy).unwrap(), std::fs::read(&tracks[0].source_path).unwrap());
    assert!(verify(dest.path()).unwrap().is_ok());

    // The incremental sync leaves the same audio and analysis as exporting
    // the same tracks to an empty stick.
    let full = tempfile::tempdir().unwrap();
    export(full.path(), &tracks, &one_list(&tracks)).unwrap();
    assert_eq!(assets(dest.path()), assets(full.path()));
}

/// A copy whose time moved but whose bytes did not (rekordbox copying the
/// same file over it, a backup tool restoring it) is hashed once, kept, and
/// trusted again from its new time.
#[test]
fn a_copy_touched_but_unchanged_is_kept_and_its_new_time_recorded() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();
    let before = Manifest::load(dest.path()).unwrap().tracks[0].copy_stamp.unwrap();

    let copy = dest.path().join("Contents/TRIODE/Single/source-1.mp3");
    age(&copy, -3600);

    let second = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!((second.reused, second.bytes_copied), (1, 0));
    let after = Manifest::load(dest.path()).unwrap().tracks[0].copy_stamp.unwrap();
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1 - 3_600_000_000_000, "the copy's new time is what the next sync compares against");
}

/// A record written before the copy's stamp was kept has nothing to trust,
/// so the copy is hashed as before, and the stamp is recorded for next time.
#[test]
fn an_older_record_without_the_copy_stamp_is_checked_by_hash_once() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];
    export(dest.path(), &tracks, &[]).unwrap();

    let path = Manifest::path(dest.path());
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    json["tracks"][0].as_object_mut().unwrap().remove("copy_stamp");
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(Manifest::load(dest.path()).unwrap().tracks[0].copy_stamp, None);

    // Same length, other bytes, and the time left where it was: only the
    // hash can tell, and it still does.
    let copy = dest.path().join("Contents/TRIODE/Single/source-1.mp3");
    let time = std::fs::metadata(&copy).unwrap().modified().unwrap();
    std::fs::write(&copy, vec![0xEE_u8; 2048]).unwrap();
    std::fs::OpenOptions::new().write(true).open(&copy).unwrap().set_modified(time).unwrap();

    let second = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!((second.reused, second.bytes_copied), (0, 2048));
    assert_eq!(std::fs::read(&copy).unwrap()[0], 1, "the source's bytes are back");
    assert!(Manifest::load(dest.path()).unwrap().tracks[0].copy_stamp.is_some());
}
