//! SQLite's regenerable WAL index must not look like an external data edit.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

mod common;
use rbl_export::{ExportOptions, Manifest, SourcePlaylist, SourceTrack, SyncSource};
use std::path::Path;

fn lists(name: &str) -> Vec<SourcePlaylist> {
    vec![SourcePlaylist {
        id: 10,
        name: name.into(),
        track_indices: vec![0],
        ..Default::default()
    }]
}
fn sync(
    root: &Path,
    tracks: &[SourceTrack],
    name: &str,
    progress: &mut dyn FnMut(&rbl_export::ExportProgress),
) -> rbl_export::Result<rbl_export::ExportReport> {
    rbl_export::export_with_options(
        root,
        tracks,
        &lists(name),
        &[],
        &ExportOptions {
            sync: Some(&SyncSource {
                db_id: 123,
                ..Default::default()
            }),
            ..Default::default()
        },
        progress,
    )
}
fn tracks(source: &Path) -> Vec<SourceTrack> {
    let path = source.join("generated.mp3");
    std::fs::write(&path, vec![7; 128]).unwrap();
    vec![SourceTrack {
        id: 1,
        source_path: path,
        title: "Generated".into(),
        analysis: vec![(
            "DAT".into(),
            rbl_anlz::AnlzBuilder::new().path("/generated.mp3").finish(),
        )],
        ..Default::default()
    }]
}
fn db(root: &Path) -> std::path::PathBuf {
    root.join("PIONEER/rekordbox/exportLibrary.db")
}
fn sidecar(root: &Path, suffix: &str) -> std::path::PathBuf {
    root.join(format!("PIONEER/rekordbox/exportLibrary.db{suffix}"))
}
fn wal_snapshot(root: &Path, missing_shm: bool) {
    let c = rbl_onelibrary::playlists::open_read_write(&db(root)).unwrap();
    let mode: String = c
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    c.execute("UPDATE property SET deviceName='Generated WAL'", [])
        .unwrap();
    // Capture a committed generated WAL generation while its owner is open,
    // then restore that snapshot after close, as after an unclean shutdown.
    let images: Vec<_> = ["", "-wal", "-shm"]
        .map(|suffix| {
            let path = sidecar(root, suffix);
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .into();
    assert!(!images[1].1.is_empty());
    drop(c);
    for (path, bytes) in images {
        std::fs::write(path, bytes).unwrap();
    }
    if missing_shm {
        std::fs::remove_file(sidecar(root, "-shm")).unwrap();
    }
}
fn accepts_wal_reader(missing_shm: bool) {
    let source = tempfile::tempdir().unwrap();
    let usb = common::stick();
    let tracks = tracks(source.path());
    sync(usb.path(), &tracks, "Before", &mut |_| {}).unwrap();
    let old_receipt = Manifest::load(usb.path()).unwrap();
    assert!(old_receipt
        .generation
        .as_ref()
        .unwrap()
        .databases
        .contains_key("PIONEER/rekordbox/exportLibrary.db-shm"));
    wal_snapshot(usb.path(), missing_shm);
    let main_before = std::fs::read(db(usb.path())).unwrap();
    let wal_before = std::fs::read(sidecar(usb.path(), "-wal")).unwrap();
    let mut publishing = false;
    let report = sync(usb.path(), &tracks, "After", &mut |p| {
        if p.stage == "publishing" {
            publishing = true;
            assert_eq!(std::fs::read(db(usb.path())).unwrap(), main_before);
            assert_eq!(
                std::fs::read(sidecar(usb.path(), "-wal")).unwrap(),
                wal_before
            );
        }
    })
    .unwrap();
    assert!(publishing && report.verified);
    assert_eq!(report.bytes_copied, 0);
    for suffix in ["-wal", "-shm"] {
        assert!(!sidecar(usb.path(), suffix).exists());
    }
    let read = rbl_onelibrary::ExportLibrary::open_read_only(&db(usb.path())).unwrap();
    let name: String = read
        .connection()
        .query_row("SELECT deviceName FROM property", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "Generated WAL");
    drop(read);
    let snapshot = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(snapshot.one.as_ref().unwrap().playlists[0].name, "After");
    assert_eq!(snapshot.legacy.as_ref().unwrap().playlists[0].name, "After");
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
    let receipt = Manifest::load(usb.path()).unwrap();
    assert!(receipt
        .generation
        .as_ref()
        .unwrap()
        .databases
        .contains_key("PIONEER/rekordbox/exportLibrary.db-shm"));
    assert!(
        sync(usb.path(), &tracks, "After", &mut |_| {})
            .unwrap()
            .up_to_date
    );
}
#[test]
fn a_present_wal_index_can_be_refreshed_by_our_reader() {
    accepts_wal_reader(false);
}
#[test]
fn an_absent_wal_index_can_be_rebuilt_by_our_reader() {
    accepts_wal_reader(true);
}

#[test]
fn real_source_and_database_edits_still_refuse_before_publication() {
    for kind in ["source", "database", "wal", "analysis"] {
        let log = tempfile::NamedTempFile::new().unwrap();
        let writer_log = log.reopen().unwrap();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer_log.try_clone().unwrap())
            .finish();
        let _capture = tracing::subscriber::set_default(subscriber);
        let source = tempfile::tempdir().unwrap();
        let usb = common::stick();
        let tracks = tracks(source.path());
        sync(usb.path(), &tracks, "Before", &mut |_| {}).unwrap();
        if kind == "analysis" {
            // A supported older receipt lacks generation output stamps. With
            // current stamps the earlier exported-media guard owns this edit.
            let mut receipt = Manifest::load(usb.path()).unwrap();
            receipt.generation = None;
            receipt.save(usb.path()).unwrap();
        }
        let pdb = usb.path().join("PIONEER/rekordbox/export.pdb");
        let original = std::fs::read(&pdb).unwrap();
        // Keep a real WAL writer alive only in the WAL control. Its next
        // committed update changes WAL data while leaving the main DB alone.
        let writer = (kind == "wal").then(|| {
            let c = rbl_onelibrary::playlists::open_read_write(&db(usb.path())).unwrap();
            let _: String = c
                .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
                .unwrap();
            c.execute("UPDATE property SET deviceName='Before staging'", [])
                .unwrap();
            c
        });
        let analysis = rbl_export::snapshot::Snapshot::read(usb.path())
            .unwrap()
            .legacy
            .unwrap()
            .tracks[0]
            .analysis
            .clone();
        let mut changed = false;
        let mut published = false;
        let result = sync(usb.path(), &tracks, "After", &mut |p| {
            if p.stage == "publishing" {
                published = true;
            }
            if p.stage != "database" || changed {
                return;
            }
            changed = true;
            match kind {
                "analysis" => {
                    std::fs::File::open(usb.path().join(analysis.trim_start_matches('/')))
                        .unwrap()
                        .set_times(std::fs::FileTimes::new().set_modified(
                            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5),
                        ))
                        .unwrap()
                }
                "source" => std::fs::write(&tracks[0].source_path, b"new source bytes").unwrap(),
                "database" => {
                    let c = rbl_onelibrary::playlists::open_read_write(&db(usb.path())).unwrap();
                    c.execute(
                        "UPDATE property SET deviceName='Concurrent database edit'",
                        [],
                    )
                    .unwrap();
                }
                "wal" => {
                    writer
                        .as_ref()
                        .unwrap()
                        .execute("UPDATE property SET deviceName='Concurrent WAL edit'", [])
                        .unwrap();
                }
                _ => unreachable!(),
            }
        });
        assert!(changed && !published, "{kind}");
        assert_eq!(
            result.unwrap_err().to_string(),
            "USB sync conflict: The device changed during sync. Close other writers and retry.",
            "{kind}"
        );
        let logged = std::fs::read_to_string(log.path()).unwrap();
        let events: Vec<_> = logged
            .lines()
            .filter(|line| line.contains("USB sync input changed"))
            .collect();
        assert_eq!(events.len(), 1, "{kind}: {logged}");
        let role = if kind == "wal" || kind == "database" {
            "databases"
        } else {
            kind
        };
        assert!(
            events[0].ends_with(&format!("input_role=\"{role}\"")),
            "{logged}"
        );
        assert!(!events[0].contains(&usb.path().to_string_lossy().to_string()));
        assert!(!events[0].contains(&source.path().to_string_lossy().to_string()));
        assert_eq!(std::fs::read(pdb).unwrap(), original, "{kind}");
        if let Some(c) = writer {
            let name: String = c
                .query_row("SELECT deviceName FROM property", [], |r| r.get(0))
                .unwrap();
            assert_eq!(name, "Concurrent WAL edit");
        }
    }
}
