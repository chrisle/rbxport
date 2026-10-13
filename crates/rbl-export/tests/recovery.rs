//! Syncs onto a stick in states a USB export meets in use: an export that
//! stopped part way, on a stick something else then wrote to (#229), and a
//! stick whose files carry a clock ahead of this one. rekordbox rewrites
//! `export.pdb` and `exportLibrary.db` as soon as it sees a stick [OBS
//! 2026-10-09, rekordbox 7.2.14, Windows 11]; recovery refused such a stick
//! from then on, so it could not be synced again until it was reformatted.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;
use std::time::{Duration, SystemTime};

use rbl_export::{export_full, verify, Manifest, SourcePlaylist, SourceTrack, SyncNode, SyncSource};

fn tracks(src: &Path) -> Vec<SourceTrack> {
    (0..3_u64)
        .map(|i| {
            let path = src.join(format!("Kesä {i}.mp3"));
            std::fs::write(&path, vec![i as u8 + 1; 4096]).unwrap();
            SourceTrack {
                id: i + 1,
                source_path: path,
                title: format!("Track {i}"),
                artist: "Björk".into(),
                album: "Album".into(),
                analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
                ..Default::default()
            }
        })
        .collect()
}

fn sync(root: &Path, tracks: &[SourceTrack]) -> rbl_export::Result<rbl_export::ExportReport> {
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    let source = SyncSource { db_id: 123, tree: vec![SyncNode { id: 10, parent: 0, attribute: 0 }], automatic: false };
    export_full(root, tracks, &playlists, &[], None, Some(&source), &mut |_| {})
}

/// The file now carries the same bytes, written after the journal was: what
/// rekordbox leaves when it rewrites a database it sees.
fn rewrite_later(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    std::fs::write(path, bytes).unwrap();
    let later = std::fs::FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(30));
    std::fs::File::options().write(true).open(path).unwrap().set_times(later).unwrap();
}

/// A journal as a publication that stopped part way leaves it: `entries`
/// are (path, present, had_target), and `images` the new files it had not
/// published yet.
fn journal(root: &Path, entries: serde_json::Value, images: &[(&str, &[u8])], set_aside: &[(&str, &[u8])]) {
    let journal = root.join(".rbxport-publication");
    for (path, bytes) in images {
        let at = journal.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, bytes).unwrap();
    }
    for (path, bytes) in set_aside {
        let at = journal.join(".previous").join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, bytes).unwrap();
    }
    std::fs::write(journal.join("publication.json"), serde_json::to_vec(&entries).unwrap()).unwrap();
}

fn recovered(root: &Path) -> Vec<std::path::PathBuf> {
    let dir = root.join("PIONEER/rbxport");
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("recovered-"))
        .collect()
}

#[test]
fn a_stick_rekordbox_wrote_to_after_an_interrupted_sync_syncs_again() {
    let stick = common::stick();
    let root = stick.path();
    let src = tempfile::tempdir().unwrap();
    let tracks = tracks(src.path());
    sync(root, &tracks).unwrap();
    let read = |p: &str| std::fs::read(root.join(p)).unwrap();
    let manifest = "PIONEER/rbxport/manifest.json";
    let pdb = "PIONEER/rekordbox/export.pdb";
    let db = "PIONEER/rekordbox/exportLibrary.db";
    let (manifest_before, pdb_before) = (read(manifest), read(pdb));

    // The next sync stopped after replacing the manifest and export.pdb and
    // before exportLibrary.db: the device's own two are set aside in the
    // journal, the new ones in place, and the new database still an image.
    std::fs::write(root.join(manifest), b"{\"the\": \"new manifest\"}").unwrap();
    std::fs::write(root.join(pdb), b"the new export.pdb").unwrap();
    journal(
        root,
        serde_json::json!([
            {"path": manifest, "present": true, "had_target": true, "exact": true},
            {"path": pdb, "present": true, "had_target": true, "exact": true},
            {"path": db, "present": true, "had_target": true, "exact": true},
        ]),
        &[(db, b"the new database")],
        &[(manifest, &manifest_before), (pdb, &pdb_before)],
    );
    // Then rekordbox saw the stick and rewrote its database.
    rewrite_later(&root.join(db));
    let rewritten = read(db);

    rbl_export::recover(root).unwrap();

    assert!(!root.join(".rbxport-publication").exists(), "the journal is set aside");
    assert_eq!(read(db), rewritten, "rekordbox's database is left as it is");
    assert_eq!(read(manifest), manifest_before, "the manifest is as before that sync");
    assert_eq!(read(pdb), pdb_before, "so is export.pdb");
    let kept = recovered(root);
    assert_eq!(kept.len(), 1);
    assert!(kept[0].join("publication.json").is_file());

    // And the stick takes the next sync.
    let again = sync(root, &tracks).unwrap();
    assert_eq!(again.tracks, 3);
    let check = verify(root).unwrap();
    assert!(check.is_ok(), "{:?} {:?}", check.missing_audio, check.errors);
    assert!(Manifest::load(root).is_some());
}

/// The other writer saw the new generation: the sync had published its
/// databases before it stopped, as it does within a second on HFS+.
#[test]
fn a_stick_rewritten_after_an_interrupted_sync_published_its_databases_syncs_again() {
    let stick = common::stick();
    let root = stick.path();
    let src = tempfile::tempdir().unwrap();
    let tracks = tracks(src.path());
    sync(root, &tracks).unwrap();
    let files = ["PIONEER/rbxport/manifest.json", "PIONEER/rekordbox/export.pdb", "PIONEER/rekordbox/exportExt.pdb", "PIONEER/rekordbox/exportLibrary.db"];
    let record = "PIONEER/rekordbox/playlists3.sync";
    let read = |p: &str| std::fs::read(root.join(p)).unwrap();
    let before: Vec<Vec<u8>> = files.iter().map(|f| read(f)).collect();
    let record_before = read(record);
    // A second, real sync, then put the stick back to where it was when
    // that sync stopped: databases published, the sync record not.
    sync(root, &tracks[1..]).unwrap();
    let record_after = read(record);
    std::fs::write(root.join(record), &record_before).unwrap();
    let mut entries: Vec<serde_json::Value> =
        files.iter().map(|f| serde_json::json!({"path": f, "present": true, "had_target": true, "exact": true})).collect();
    entries.push(serde_json::json!({"path": record, "present": true, "had_target": true, "exact": true}));
    let previous: Vec<(&str, &[u8])> = files.iter().zip(&before).map(|(f, b)| (*f, b.as_slice())).collect();
    journal(root, serde_json::Value::Array(entries), &[(record, &record_after)], &previous);
    // Then rekordbox saw the stick and rewrote its database.
    rewrite_later(&root.join("PIONEER/rekordbox/exportLibrary.db"));

    rbl_export::recover(root).unwrap();

    assert!(!root.join(".rbxport-publication").exists());
    assert_eq!(read(record), record_after, "the rest of the sync is published");
    assert_eq!(recovered(root).len(), 1);
    let again = sync(root, &tracks[1..]).unwrap();
    assert_eq!((again.tracks, again.reused), (2, 2));
    let check = verify(root).unwrap();
    assert!(check.is_ok(), "{:?} {:?}", check.missing_audio, check.errors);
}

#[test]
fn a_first_sync_rekordbox_wrote_to_part_way_leaves_no_manifest_of_its_own() {
    let stick = common::stick();
    let root = stick.path();
    // A first sync stopped after placing its manifest; the stick had none.
    // rekordbox then wrote a database of its own where the sync had not yet
    // put one.
    let db = "PIONEER/rekordbox/exportLibrary.db";
    let manifest = "PIONEER/rbxport/manifest.json";
    std::fs::create_dir_all(root.join("PIONEER/rbxport")).unwrap();
    std::fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
    std::fs::write(root.join(manifest), b"{\"the\": \"new manifest\"}").unwrap();
    std::fs::write(root.join(db), b"rekordbox's own").unwrap();
    journal(
        root,
        serde_json::json!([
            {"path": manifest, "present": true, "had_target": false, "exact": true},
            {"path": db, "present": true, "had_target": false, "exact": true},
        ]),
        &[(db, b"the new database")],
        &[],
    );
    rewrite_later(&root.join(db));

    rbl_export::recover(root).unwrap();

    assert!(!root.join(".rbxport-publication").exists());
    assert!(!root.join(manifest).exists(), "the half-published manifest is gone");
    assert_eq!(std::fs::read(root.join(db)).unwrap(), b"rekordbox's own");
    assert_eq!(recovered(root).len(), 1);
}

#[test]
fn an_interrupted_sync_nothing_touched_since_is_still_finished() {
    let stick = common::stick();
    let root = stick.path();
    let src = tempfile::tempdir().unwrap();
    sync(root, &tracks(src.path())).unwrap();
    let manifest = "PIONEER/rbxport/manifest.json";
    let new_manifest = std::fs::read(root.join(manifest)).unwrap();
    let earlier = b"{\"the\": \"earlier manifest\"}".to_vec();
    std::fs::write(root.join(manifest), &earlier).unwrap();
    journal(
        root,
        serde_json::json!([{"path": manifest, "present": true, "had_target": true, "exact": true}]),
        &[(manifest, &new_manifest)],
        &[],
    );

    rbl_export::recover(root).unwrap();

    // Rolled forward as before, nothing set aside.
    assert_eq!(std::fs::read(root.join(manifest)).unwrap(), new_manifest);
    assert!(recovered(root).is_empty());
}

/// FAT keeps local time with no zone, so a stick last written somewhere
/// ahead of this machine's clock carries files that look newer than now.
/// A sync's own publication took that for another writer and failed at 99%
/// with "the device changed after sync failed".
#[test]
fn a_stick_whose_files_look_newer_than_this_clock_still_syncs() {
    let stick = common::stick();
    let root = stick.path();
    let src = tempfile::tempdir().unwrap();
    let tracks = tracks(src.path());
    sync(root, &tracks).unwrap();
    let ahead = std::fs::FileTimes::new().set_modified(SystemTime::now() + Duration::from_secs(3600));
    for file in ["PIONEER/rekordbox/exportLibrary.db", "PIONEER/rekordbox/export.pdb", "PIONEER/rbxport/manifest.json"] {
        std::fs::File::options().write(true).open(root.join(file)).unwrap().set_times(ahead).unwrap();
    }

    let again = sync(root, &tracks[1..]).unwrap();
    assert_eq!((again.tracks, again.removed), (2, 1));
    assert!(verify(root).unwrap().is_ok());
}
