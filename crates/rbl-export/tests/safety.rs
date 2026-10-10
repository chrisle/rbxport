#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
use rbl_export::{export_full, verify, Manifest, SourcePlaylist, SourceTrack, SyncSource};
use std::path::Path;

#[test]
fn stopping_export_preserves_the_published_usb_library() {
    for stop_at in [3, 4] {
        let sources = tempfile::tempdir().unwrap();
        let usb = tempfile::tempdir().unwrap();
        let tracks = vec![track(sources.path(), 1), track(sources.path(), 2)];
        sync(usb.path(), &tracks[..1], &[playlist(10, &[0])]).unwrap();
        let db = rbl_export::export_root(usb.path()).join("rekordbox/export.pdb");
        let before = std::fs::read(&db).unwrap();
        let calls = std::cell::Cell::new(0);
        let result = rbl_export::export_cancellable(
            usb.path(), &tracks, &[playlist(10, &[0, 1])], &[],
            &rbl_export::ExportOptions { sync: Some(&SyncSource { db_id: 123, tree: vec![], automatic: false }), ..Default::default() }, &mut |_| {}, &|| {
                calls.set(calls.get() + 1);
                calls.get() >= stop_at
            },
        );
        assert!(matches!(result, Err(rbl_export::ExportError::Cancelled)), "{result:?}");
        assert_eq!(std::fs::read(&db).unwrap(), before);
        let check = verify(usb.path()).unwrap();
        assert!(check.is_ok());
        assert_eq!(check.tracks, 1);
    }
}
fn track(root: &Path, id: u64) -> SourceTrack {
    let path = root.join(format!("{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 128]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: format!("Track {id}"),
        artist: "Artist".into(),
        album: "Album".into(),
        analysis: vec![(
            "DAT".into(),
            rbl_anlz::AnlzBuilder::new().path("/original.mp3").finish(),
        )],
        ..Default::default()
    }
}
fn playlist(id: u64, indices: &[usize]) -> SourcePlaylist {
    SourcePlaylist {
        id,
        name: format!("Playlist {id}"),
        track_indices: indices.to_vec(),
        ..Default::default()
    }
}
fn sync(
    root: &Path,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
) -> rbl_export::Result<rbl_export::ExportReport> {
    export_full(
        root,
        tracks,
        playlists,
        &[],
        None,
        Some(&SyncSource {
            db_id: 123,
            tree: playlists
                .iter()
                .map(|p| rbl_export::SyncNode {
                    id: p.id,
                    parent: p.parent_id,
                    attribute: u8::from(p.folder),
                })
                .collect(),
            automatic: false,
        }),
        &mut |_| {},
    )
}
fn edit(root: &Path, sql: &str) {
    let conn = rusqlite::Connection::open(
        rbl_export::export_root(root).join("rekordbox/exportLibrary.db"),
    )
    .unwrap();
    conn.pragma_update(None, "cipher", "sqlcipher").unwrap();
    conn.pragma_update(None, "legacy", 4).unwrap();
    conn.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap())
        .unwrap();
    conn.execute_batch(sql).unwrap();
}
#[test]
fn missing_source_leaves_every_existing_database_and_audio_unchanged() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[playlist(10, &[0])]).unwrap();
    let pdb = std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    std::fs::remove_file(&t.source_path).unwrap();
    assert!(sync(usb.path(), &[t], &[playlist(10, &[0])]).is_err());
    assert_eq!(
        pdb,
        std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap()
    );
    assert!(verify(usb.path()).unwrap().is_ok());
}
#[test]
fn collisions_and_unicode_names_keep_distinct_audio() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let mut a = track(src.path(), 1);
    let mut b = track(src.path(), 2);
    std::fs::create_dir(src.path().join("second")).unwrap();
    let p = src.path().join("second/1.mp3");
    std::fs::rename(&b.source_path, &p).unwrap();
    b.source_path = p;
    a.artist = format!("{}é", "a".repeat(119));
    b.artist = a.artist.clone();
    sync(usb.path(), &[a.clone(), b.clone()], &[playlist(1, &[0, 1])]).unwrap();
    let m = Manifest::load(usb.path()).unwrap();
    assert_ne!(m.tracks[0].audio, m.tracks[1].audio);
    for (row, byte) in m.tracks.iter().zip([1, 2]) {
        assert_eq!(
            std::fs::read(usb.path().join(row.audio.trim_start_matches('/'))).unwrap(),
            vec![byte; 128]
        );
    }
    sync(usb.path(), &[b, a], &[playlist(1, &[0, 1])]).unwrap();
    assert!(verify(usb.path()).unwrap().is_ok());
}
#[test]
fn identity_is_written_and_another_master_cannot_reuse_the_manifest() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 42);
    sync(usb.path(), std::slice::from_ref(&t), &[]).unwrap();
    let snap = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(snap.identity.get(&1), Some(&(123, 42)));
    let result = export_full(
        usb.path(),
        &[t],
        &[],
        &[],
        None,
        Some(&SyncSource {
            db_id: 999,
            ..Default::default()
        }),
        &mut |_| {},
    );
    assert!(result.is_err());
    assert_eq!(
        rbl_export::snapshot::Snapshot::read(usb.path()).unwrap(),
        snap
    );
}
#[test]
fn device_metadata_conflict_does_not_replace_either_database() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[]).unwrap();
    edit(
        usb.path(),
        "UPDATE content SET title='Edited on device' WHERE content_id=1",
    );
    let before = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert!(sync(usb.path(), &[t], &[]).is_err());
    assert_eq!(
        before,
        rbl_export::snapshot::Snapshot::read(usb.path()).unwrap()
    );
}
#[test]
fn device_only_playlist_and_play_history_survive_deselection() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();
    edit(usb.path(),"INSERT INTO playlist VALUES(99,1,'On the deck',NULL,0,0); INSERT INTO playlist_content VALUES(99,1,1); INSERT INTO history VALUES(7,1,'Tonight',0,0); INSERT INTO history_content VALUES(7,1,1)");
    sync(usb.path(), &[], &[]).unwrap();
    let s = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert!(s
        .one
        .as_ref()
        .unwrap()
        .playlists
        .iter()
        .any(|p| p.name == "On the deck" && p.tracks == [1]));
    assert_eq!(s.history.len(), 1);
    assert_eq!(s.legacy_history.len(), 1);
    assert!(verify(usb.path()).unwrap().is_ok());
    sync(usb.path(), &[], &[]).unwrap();
    assert!(verify(usb.path()).unwrap().is_ok());
}
#[test]
fn last_selected_playlist_can_be_removed() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();
    sync(usb.path(), &[], &[]).unwrap();
    let check = verify(usb.path()).unwrap();
    assert!(check.is_ok());
    assert_eq!(check.tracks, 0);
    assert_eq!(check.playlists, 0);
}
#[test]
fn folders_and_playlist_ids_survive_reorder() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    let folder = SourcePlaylist {
        id: 90,
        name: "Sets".into(),
        folder: true,
        ..Default::default()
    };
    let mut a = playlist(10, &[0]);
    a.parent_id = 90;
    let b = playlist(20, &[0]);
    sync(
        usb.path(),
        std::slice::from_ref(&t),
        &[folder.clone(), a.clone(), b.clone()],
    )
    .unwrap();
    let before = Manifest::load(usb.path()).unwrap();
    sync(usb.path(), &[t], &[b, folder, a]).unwrap();
    let after = Manifest::load(usb.path()).unwrap();
    for p in before.playlists {
        assert_eq!(
            after
                .playlists
                .iter()
                .find(|n| n.library_id == p.library_id)
                .unwrap()
                .export_id,
            p.export_id
        );
    }
    assert!(verify(usb.path()).unwrap().is_ok());
    let record = rbl_export::sync_record::read(usb.path()).unwrap();
    assert_ne!(record.device_ids[&10], record.device_ids[&20]);
}
#[test]
fn verification_rejects_missing_audio_and_disagreement() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), &[t], &[]).unwrap();
    let m = Manifest::load(usb.path()).unwrap();
    std::fs::remove_file(usb.path().join(m.tracks[0].audio.trim_start_matches('/'))).unwrap();
    assert!(!verify(usb.path()).unwrap().is_ok());
    edit(usb.path(), "UPDATE content SET title='different'");
    assert!(!verify(usb.path()).unwrap().is_ok());
}
#[test]
fn one_library_only_initialization_preserves_tracks() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();
    std::fs::remove_file(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    rbl_export::create_library(
        usb.path(),
        None,
        &[],
        Some(&SyncSource {
            db_id: 123,
            ..Default::default()
        }),
    )
    .unwrap();
    let check = verify(usb.path()).unwrap();
    assert!(check.is_ok());
    assert_eq!(check.tracks, 1);
    assert_eq!(check.playlists, 1);
}
#[test]
fn hidden_library_stays_hidden_and_plus_only_record_is_read() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    std::fs::create_dir_all(usb.path().join(".PIONEER")).unwrap();
    std::fs::write(usb.path().join(".PIONEER/DEVSETTING.DAT"), b"existing").unwrap();
    sync(usb.path(), std::slice::from_ref(&t), &[playlist(10, &[0])]).unwrap();
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();
    assert!(!usb.path().join("PIONEER").exists());
    assert!(verify(usb.path()).unwrap().is_ok());
    std::fs::remove_file(usb.path().join(".PIONEER/rekordbox/playlists3.sync")).unwrap();
    assert_eq!(
        rbl_export::sync_record::read(usb.path()).unwrap().ticked,
        vec![10]
    );
}
#[test]
fn same_size_audio_corruption_is_repaired() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[]).unwrap();
    let m = Manifest::load(usb.path()).unwrap();
    let path = usb.path().join(m.tracks[0].audio.trim_start_matches('/'));
    std::fs::write(&path, vec![9; 128]).unwrap();
    let report = sync(usb.path(), &[t], &[]).unwrap();
    assert_eq!(report.reused, 0);
    assert_eq!(std::fs::read(path).unwrap(), vec![1; 128]);
}
#[test]
fn unsupported_schema_is_left_untouched() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[]).unwrap();
    edit(usb.path(), "UPDATE property SET dbVersion='9999'");
    let path = usb.path().join("PIONEER/rekordbox/exportLibrary.db");
    let bytes = std::fs::read(&path).unwrap();
    assert!(sync(usb.path(), &[t], &[]).is_err());
    assert_eq!(bytes, std::fs::read(path).unwrap());
}

#[test]
fn legacy_only_conversion_preserves_history_and_master_identity() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[playlist(10, &[0])]).unwrap();
    edit(
        usb.path(),
        "INSERT INTO history VALUES(7,1,'Tonight',0,0); INSERT INTO history_content VALUES(7,1,1)",
    );
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();
    std::fs::remove_file(usb.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    rbl_export::create_library(
        usb.path(),
        None,
        &[],
        Some(&SyncSource {
            db_id: 123,
            ..Default::default()
        }),
    )
    .unwrap();
    let s = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(s.identity.get(&1), Some(&(123, 1)));
    assert_eq!(s.history.len(), 1);
    assert_eq!(s.history[0].tracks, vec![1]);
    assert!(verify(usb.path()).unwrap().is_ok());
}
#[test]
fn same_size_artwork_change_and_removed_analysis_extension_are_published() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let mut t = track(src.path(), 1);
    let artwork = src.path().join("cover.jpg");
    std::fs::write(&artwork, b"first").unwrap();
    t.artwork = Some(artwork.clone());
    t.analysis.push((
        "EXT".into(),
        rbl_anlz::AnlzBuilder::new().path("/original.mp3").finish(),
    ));
    sync(usb.path(), std::slice::from_ref(&t), &[]).unwrap();
    let m = Manifest::load(usb.path()).unwrap();
    let image = usb.path().join(m.tracks[0].artwork.trim_start_matches('/'));
    let ext = usb
        .path()
        .join(m.tracks[0].anlz_dir.trim_start_matches('/'))
        .join("ANLZ0000.EXT");
    assert!(ext.exists());
    std::fs::write(artwork, b"other").unwrap();
    t.analysis.retain(|(e, _)| e != "EXT");
    sync(usb.path(), &[t], &[]).unwrap();
    assert_eq!(std::fs::read(image).unwrap(), b"other");
    assert!(!ext.exists());
}
#[test]
fn late_external_edit_aborts_publication() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[]).unwrap();
    let pdb = std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let result = export_full(
        usb.path(),
        &[t],
        &[],
        &[],
        None,
        Some(&SyncSource {
            db_id: 123,
            ..Default::default()
        }),
        &mut |_| edit(usb.path(), "UPDATE content SET title='Changed during copy'"),
    );
    assert!(result.is_err());
    assert_eq!(
        pdb,
        std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap()
    );
}
#[test]
fn device_only_tags_and_cue_rows_are_preserved() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let t = track(src.path(), 1);
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();
    edit(usb.path(),"INSERT INTO playlist VALUES(99,1,'On the deck',NULL,0,0); INSERT INTO playlist_content VALUES(99,1,1); INSERT INTO myTag VALUES(111,1,'Device tag',0,0); INSERT INTO myTag_content VALUES(111,1); INSERT INTO cue(cue_id,content_id,kind,inUsec,cueComment) VALUES(7,1,0,1000000,'device cue')");
    let before = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    sync(usb.path(), &[], &[]).unwrap();
    let after = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(after.cues, before.cues);
    assert_eq!(after.my_tags, before.my_tags);
    assert_eq!(after.tag_memberships, before.tag_memberships);
}
/// An export that adds to a stick (Export Playlist, Export Track) leaves
/// what earlier exports put there: playlists it does not name, even one the
/// library no longer has, and empty folders, each in its place and under
/// its device id, still recorded as the library's so a later SYNC can take
/// them off. Playlists made on a player stay too (#304).
#[test]
fn keep_unlisted_adds_to_the_stick_without_taking_anything_off() {
    let src = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let tracks = [track(src.path(), 1), track(src.path(), 2), track(src.path(), 3)];
    let folder = |id: u64, parent_id: u64| SourcePlaylist { id, name: format!("Folder {id}"), folder: true, parent_id, ..Default::default() };
    let mut a = playlist(10, &[0]);
    a.parent_id = 90;
    sync(usb.path(), &tracks[..2], &[folder(90, 0), folder(91, 90), a, playlist(20, &[1])]).unwrap();
    edit(usb.path(), "INSERT INTO playlist VALUES(99,1,'On the deck',NULL,0,0); INSERT INTO playlist_content VALUES(99,1,1)");
    let before = Manifest::load(usb.path()).unwrap();

    let add = SyncSource { db_id: 123, tree: vec![rbl_export::SyncNode { id: 30, parent: 0, attribute: 0 }], automatic: false };
    rbl_export::export_cancellable(
        usb.path(), &tracks[2..], &[playlist(30, &[0])], &[],
        &rbl_export::ExportOptions { sync: Some(&add), keep_unlisted: true, ..Default::default() },
        &mut |_| {}, &|| false,
    )
    .unwrap();

    let after = Manifest::load(usb.path()).unwrap();
    let listed: Vec<(u64, u32, bool)> = after.playlists.iter().filter(|p| !p.device_only).map(|p| (p.library_id, p.export_id, p.folder)).collect();
    let mut expected: Vec<(u64, u32, bool)> = before.playlists.iter().filter(|p| !p.device_only).map(|p| (p.library_id, p.export_id, p.folder)).collect();
    assert_eq!(listed[..expected.len()], expected[..], "what the stick held keeps its place, id and owner");
    expected.push((30, listed[expected.len()].1, false));
    assert_eq!(listed, expected, "the new playlist goes after them");
    let s = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    let names: Vec<&str> = s.one.as_ref().unwrap().playlists.iter().map(|p| p.name.as_str()).collect();
    for name in ["Folder 90", "Folder 91", "Playlist 10", "Playlist 20", "Playlist 30", "On the deck"] {
        assert!(names.contains(&name), "{name} is on the stick: {names:?}");
    }
    let one = s.one.as_ref().unwrap();
    let held = |name: &str| one.playlists.iter().find(|p| p.name == name).unwrap().tracks.len();
    assert_eq!((held("Playlist 10"), held("Playlist 20"), held("Playlist 30")), (1, 1, 1));
    assert!(verify(usb.path()).unwrap().is_ok());

    // Still the library's: a SYNC of only the new playlist takes them off,
    // and keeps the one made on the player.
    sync(usb.path(), &tracks[2..], &[playlist(30, &[0])]).unwrap();
    let s = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    let mut names: Vec<&str> = s.one.as_ref().unwrap().playlists.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["On the deck", "Playlist 30"]);
    assert!(verify(usb.path()).unwrap().is_ok());
}
