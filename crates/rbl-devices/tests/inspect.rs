//! Reading a real export back off a volume.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_export::{export, SourcePlaylist, SourceTrack};

fn track(dir: &std::path::Path, id: u64, title: &str) -> SourceTrack {
    let path = dir.join(format!("source-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 1024]).unwrap();
    SourceTrack {
        id,
        source_path: path,
        title: title.into(),
        artist: "TRIODE".into(),
        album: "Single".into(),
        duration_sec: 300,
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

#[test]
fn a_stick_we_wrote_reports_what_is_on_it_and_that_it_can_be_synced() {
    let src = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "One"), track(src.path(), 2, "Two")];
    let playlists = vec![SourcePlaylist { name: "Set".into(), track_indices: vec![0, 1], ..Default::default() }];
    export(stick.path(), &tracks, &playlists).unwrap();

    let found = rbl_devices::inspect(stick.path()).expect("an export");
    assert_eq!(found.tracks, 2);
    assert_eq!(found.playlists, 1);
    assert!(found.ours, "we wrote it, so the next export can be a sync");
    assert!(!found.written.is_empty(), "and it says when");
}

#[test]
fn a_stick_someone_else_wrote_still_reports_its_contents() {
    let src = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "One")];
    let playlists = vec![SourcePlaylist { name: "Set".into(), track_indices: vec![0], ..Default::default() }];
    export(stick.path(), &tracks, &playlists).unwrap();

    // As rekordbox would have left it: the databases, but no record of ours.
    std::fs::remove_file(rbl_export::Manifest::path(stick.path())).unwrap();

    let found = rbl_devices::inspect(stick.path()).expect("an export");
    assert_eq!(found.tracks, 1, "read out of export.pdb instead");
    assert_eq!(found.playlists, 1);
    assert!(!found.ours, "which means the next export writes it in full");
    assert!(found.written.is_empty());
}

#[test]
fn a_stick_with_only_a_manifest_is_not_an_export() {
    let stick = tempfile::tempdir().unwrap();
    let manifest = rbl_export::Manifest { generation: None, db_id: 0, baseline: None,
        version: rbl_export::manifest::MANIFEST_VERSION,
        written: "2026-09-08 00:00:00.000 +00:00".to_owned(),
        tracks: Vec::new(),
        playlists: Vec::new(),
        loose: Vec::new(),
    };
    manifest.save(stick.path()).unwrap();
    // Without export.pdb no player can read it, so there is nothing to report.
    assert_eq!(rbl_devices::inspect(stick.path()), None);
}

/// Renaming a colour on the device panel renames it for the player too.
#[test]
fn a_renamed_colour_reaches_the_pdb_as_well_as_the_library() {
    let source = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1, "One")];
    let playlists = vec![SourcePlaylist { name: "Set".into(), track_indices: vec![0], ..Default::default() }];
    rbl_export::export(stick.path(), &tracks, &playlists).unwrap();

    let mut settings = rbl_devices::settings::read(stick.path());
    let library = settings.library.as_mut().expect("exportLibrary.db");
    library.colors[0].name = "Vocal".to_owned();
    rbl_devices::settings::write(stick.path(), &settings).unwrap();

    let bytes = std::fs::read(stick.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let colours = pdb.named_rows(pdb.table(rbl_pdb::PageType::Colors).unwrap());
    assert_eq!(colours[0].name, "Vocal");
    assert_eq!(colours.len(), 8);
    // The rest of the file is untouched: the tracks still read.
    assert_eq!(pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap()).len(), 1);
    assert_eq!(rbl_devices::settings::read(stick.path()).library.unwrap().colors[0].name, "Vocal");
}


fn tree_bytes(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                out.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
    }
    out
}

/// A settings write rewrites `exportLibrary.db` in place, so a schema that is
/// not rbxport's own (`10000` readable-only, `99999` unknown) is refused with
/// every file on the stick unchanged (#791). [OBS: fixtures only; rbxport-built
/// sticks with `dbVersion` edited.]
#[test]
fn a_settings_write_is_refused_on_unwritable_schemas() {
    for version in ["10000", "99999"] {
        let source = tempfile::tempdir().unwrap();
        let stick = tempfile::tempdir().unwrap();
        let tracks = vec![track(source.path(), 1, "One")];
        let playlists = vec![SourcePlaylist { name: "Set".into(), track_indices: vec![0], ..Default::default() }];
        rbl_export::export(stick.path(), &tracks, &playlists).unwrap();

        let mut settings = rbl_devices::settings::read(stick.path());
        settings.library.as_mut().expect("exportLibrary.db").colors[0].name = "Vocal".to_owned();

        let conn = rusqlite::Connection::open(stick.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
        conn.pragma_update(None, "cipher", "sqlcipher").unwrap();
        conn.pragma_update(None, "legacy", 4).unwrap();
        conn.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
        conn.execute_batch(&format!("UPDATE property SET dbVersion='{version}'")).unwrap();
        drop(conn);
        let before = tree_bytes(stick.path());

        let err = rbl_devices::settings::write(stick.path(), &settings).unwrap_err().to_string();
        assert!(err.contains(&format!("schema {version}")) && err.contains("read but not write"), "{version}: {err}");
        assert_eq!(before, tree_bytes(stick.path()), "{version}: a refused write must not change the device");
    }
}
