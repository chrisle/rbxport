//! A query ordinal identifies a column only within its owning read.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rbl_export::{export, snapshot::Snapshot, ExportError, SourcePlaylist, SourceTrack};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn failed_mytag_snapshot_names_its_query_and_ordinal() {
    refusal("myTag", "INSERT INTO myTag(myTag_id,sequenceNo,name,attribute,myTag_id_parent) VALUES(1,-1,'private-tag-name',0,0)");
}

#[test]
fn failed_playlist_snapshot_names_its_query_and_ordinal() {
    refusal(
        "playlists",
        "UPDATE playlist SET playlist_id_parent='private-invalid-value'",
    );
}

fn refusal(query: &str, change: &str) {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let audio = source.path().join("private-audio.mp3");
    std::fs::write(&audio, [3; 128]).unwrap();
    let tracks = vec![SourceTrack {
        id: 1,
        source_path: audio.clone(),
        title: "private-track-title".into(),
        ..Default::default()
    }];
    let lists = vec![SourcePlaylist {
        id: 1,
        name: "private-playlist-name".into(),
        track_indices: vec![0],
        ..Default::default()
    }];
    export(root.path(), &tracks, &lists).unwrap();
    let path = root.path().join("PIONEER/rekordbox/exportLibrary.db");
    let conn = rbl_onelibrary::playlists::open_read_write(&path).unwrap();
    conn.execute_batch(change).unwrap();
    let expected = if query == "myTag" {
        rusqlite::Error::IntegralValueOutOfRange(1, -1).to_string()
    } else {
        rbl_onelibrary::playlists::read(&conn)
            .err()
            .unwrap()
            .to_string()
    };
    drop(conn);
    let before = tree(root.path());
    let source_before = std::fs::read(&audio).unwrap();
    let log = tempfile::NamedTempFile::new().unwrap();
    let writer = log.reopen().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || writer.try_clone().unwrap())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let error = Snapshot::read(root.path()).err().unwrap();
        assert_eq!(
            error.to_string(),
            ExportError::OneLibrary(expected.clone()).to_string()
        );
    });
    let logged = std::fs::read_to_string(log.path()).unwrap();
    assert!(
        logged.contains(&format!("read_group=\"{query}\"")),
        "{logged}"
    );
    assert!(logged.contains("column_index=Some(1)"), "{logged}");
    let kind = if query == "myTag" {
        "integer-out-of-range"
    } else {
        "invalid-column-type"
    };
    assert!(
        logged.contains(&format!("error_kind=\"{kind}\"")),
        "{logged}"
    );
    assert_eq!(logged.lines().count(), 1);
    for private in [
        "private-",
        &*root.path().to_string_lossy(),
        &*source.path().to_string_lossy(),
    ] {
        assert!(
            !logged.contains(private),
            "private data entered a diagnostic"
        );
    }
    assert!(!logged.contains("-1"), "no rejected value is logged");
    assert!(
        !logged.contains(&expected),
        "no arbitrary error text is logged"
    );
    assert_eq!(tree(root.path()), before);
    assert_eq!(std::fs::read(&audio).unwrap(), source_before);
}

#[test]
fn supported_snapshot_values_and_signed_playlist_sync_do_not_log_a_failure() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let audio = source.path().join("private-audio.mp3");
    std::fs::write(&audio, [3; 128]).unwrap();
    let tracks = vec![SourceTrack {
        id: 1,
        source_path: audio.clone(),
        ..Default::default()
    }];
    let lists = vec![SourcePlaylist {
        id: 1,
        name: "Set".into(),
        track_indices: vec![0],
        ..Default::default()
    }];
    export(root.path(), &tracks, &lists).unwrap();
    let conn = rbl_onelibrary::playlists::open_read_write(
        &root.path().join("PIONEER/rekordbox/exportLibrary.db"),
    )
    .unwrap();
    conn.execute_batch("UPDATE playlist SET sequenceNo=-1; UPDATE content SET bpmx100=-1")
        .unwrap();
    drop(conn);
    let media = || {
        tree(root.path())
            .into_iter()
            .filter(|(p, _)| p.starts_with("Contents") || p.starts_with("PIONEER/USBANLZ"))
            .map(|(p, bytes)| {
                let modified = std::fs::metadata(root.path().join(&p))
                    .unwrap()
                    .modified()
                    .unwrap();
                (p, (bytes, modified))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let before = media();
    assert_ne!(before.len(), 0);
    let source_before = (
        std::fs::read(&audio).unwrap(),
        std::fs::metadata(&audio).unwrap().modified().unwrap(),
    );
    let log = tempfile::NamedTempFile::new().unwrap();
    let writer = log.reopen().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || writer.try_clone().unwrap())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let snapshot = Snapshot::read(root.path()).unwrap();
        assert_eq!(snapshot.one.unwrap().tracks[0].bpm, 0);
        // The later existing-track reconciliation reader is a separate query.
        // Its negative-BPM acceptance is not established by this snapshot test.
        let conn = rbl_onelibrary::playlists::open_read_write(
            &root.path().join("PIONEER/rekordbox/exportLibrary.db"),
        )
        .unwrap();
        conn.execute_batch("UPDATE content SET bpmx100=0").unwrap();
        drop(conn);
        for _ in 0..2 {
            assert_eq!(
                export(root.path(), &tracks, &lists).unwrap().bytes_copied,
                0
            );
        }
    });
    assert_eq!(std::fs::read_to_string(log.path()).unwrap(), "");
    assert_eq!(media(), before);
    assert_eq!(
        (
            std::fs::read(&audio).unwrap(),
            std::fs::metadata(&audio).unwrap().modified().unwrap()
        ),
        source_before
    );
}
