//! Standalone ADTS AAC differs from AAC carried in M4A. All media is synthetic.
//! The ADTS fixture is one second of generated 44.1 kHz mono tone, encoded by afconvert.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_export::{ExportOptions, Manifest, SourcePlaylist, SourceTrack, SyncSource};
use std::path::Path;

fn audio(root: &Path, name: &str) -> SourceTrack {
    let source = root.join(name);
    let bytes: &[u8] = if name.ends_with("m4a") {
        include_bytes!("../../rbl-db/tests/fixtures/silent.m4a")
    } else {
        include_bytes!("../../rbl-db/tests/fixtures/generated-tone.aac")
    };
    std::fs::write(&source, bytes).unwrap();
    SourceTrack {
        id: 1,
        source_path: source,
        title: "Generated AAC".into(),
        duration_sec: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}

fn formats(root: &Path) -> (u16, i64) {
    let base = rbl_export::export_root(root);
    let bytes = std::fs::read(base.join("rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let row = pdb.rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap())[0];
    let one =
        rbl_onelibrary::ExportLibrary::open_read_only(&base.join("rekordbox/exportLibrary.db"))
            .unwrap();
    (
        pdb.u2_at(row, 0x5a),
        one.connection()
            .query_row("SELECT fileType FROM content", [], |r| r.get(0))
            .unwrap(),
    )
}

fn set_formats(root: &Path, value: u16) {
    let base = rbl_export::export_root(root);
    let path = base.join("rekordbox/export.pdb");
    let mut bytes = std::fs::read(&path).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let offset = pdb.rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap())[0].offset;
    bytes[offset + 0x5a..offset + 0x5c].copy_from_slice(&value.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
    let db = rbl_onelibrary::playlists::open_read_write(&base.join("rekordbox/exportLibrary.db"))
        .unwrap();
    db.execute("UPDATE content SET fileType=?1", [value])
        .unwrap();
}

#[test]
fn imported_standalone_aac_has_its_own_type_and_preserves_m4a() {
    let temp = tempfile::tempdir().unwrap();
    let location = rbl_db::fixture::build(
        &temp.path().join("collection"),
        rbl_db::fixture::Shape {
            tracks: 0,
            playlists: 0,
            history_sessions: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut writer = rbl_db::write::Writer::open(location, temp.path().join("backups")).unwrap();
    for (name, expected) in [("generated.AAC", 2), ("generated.m4a", 4)] {
        let track = audio(temp.path(), name);
        let before = std::fs::read(&track.source_path).unwrap();
        let id = writer.import_file(&track.source_path).unwrap();
        let kind: Option<i64> = writer
            .library()
            .connection()
            .query_row("SELECT FileType FROM djmdContent WHERE ID=?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind, Some(expected), "{name}");
        assert_eq!(std::fs::read(&track.source_path).unwrap(), before);
    }
}

#[test]
fn fresh_aac_exports_both_types_and_repeats_without_recopy() {
    for (name, expected) in [("generated.AAC", 2), ("generated.m4a", 4)] {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let track = audio(source.path(), name);
        let before = std::fs::read(&track.source_path).unwrap();
        let sync = SyncSource {
            db_id: 123,
            ..Default::default()
        };
        let options = ExportOptions {
            sync: Some(&sync),
            ..Default::default()
        };
        rbl_export::export_with_options(
            dest.path(),
            std::slice::from_ref(&track),
            &[],
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(formats(dest.path()), (expected, i64::from(expected)));
        assert!(rbl_export::verify(dest.path()).unwrap().is_ok());
        let repeat = rbl_export::export_with_options(
            dest.path(),
            std::slice::from_ref(&track),
            &[],
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap();
        assert!(repeat.up_to_date);
        assert_eq!(repeat.bytes_copied, 0);
        assert_eq!(std::fs::read(&track.source_path).unwrap(), before);
    }
}

#[test]
fn legacy_aac4_is_readable_but_unrelated_formats_are_refused_and_db_edits_repair_it() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let track = audio(source.path(), "generated.aac");
    let source_before = std::fs::read(&track.source_path).unwrap();
    let sync = SyncSource {
        db_id: 123,
        ..Default::default()
    };
    let options = ExportOptions {
        sync: Some(&sync),
        ..Default::default()
    };
    rbl_export::export_with_options(
        dest.path(),
        std::slice::from_ref(&track),
        &[],
        &[],
        &options,
        &mut |_| {},
    )
    .unwrap();
    let manifest = Manifest::load(dest.path()).unwrap();
    let copied = dest
        .path()
        .join(manifest.tracks[0].audio.trim_start_matches('/'));
    let copied_before = (
        std::fs::read(&copied).unwrap(),
        std::fs::metadata(&copied).unwrap().modified().unwrap(),
    );
    for bad in [0, 1, 3, 5, 11, 12] {
        set_formats(dest.path(), bad);
        let report = rbl_export::verify(dest.path()).unwrap();
        assert!(
            report.errors.iter().any(|e| e.contains("audio format")),
            "{bad}: {:?}",
            report.errors
        );
    }
    set_formats(dest.path(), 4);
    assert!(rbl_export::verify(dest.path()).unwrap().is_ok());
    // Model an older successful writer's receipt, not an outside mutation.
    let mut older = Manifest::load(dest.path()).unwrap();
    older.generation.as_mut().unwrap().databases =
        rbl_export::incremental::databases(dest.path()).unwrap();
    older.save(dest.path()).unwrap();
    let repeat = rbl_export::export_with_options(
        dest.path(),
        std::slice::from_ref(&track),
        &[],
        &[],
        &options,
        &mut |_| {},
    )
    .unwrap();
    assert!(repeat.up_to_date);
    assert_eq!(formats(dest.path()), (4, 4));
    let lists = [SourcePlaylist {
        id: 10,
        name: "Metadata only".into(),
        track_indices: vec![0],
        ..Default::default()
    }];
    let changed = rbl_export::export_with_options(
        dest.path(),
        std::slice::from_ref(&track),
        &lists,
        &[],
        &options,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(changed.metrics.database_builds, 1);
    assert_eq!((changed.bytes_copied, changed.analysis_files), (0, 0));
    assert_eq!(formats(dest.path()), (2, 2));
    assert_eq!(
        (
            std::fs::read(&copied).unwrap(),
            std::fs::metadata(&copied).unwrap().modified().unwrap()
        ),
        copied_before
    );
    assert_eq!(std::fs::read(&track.source_path).unwrap(), source_before);
    let repeat =
        rbl_export::export_with_options(dest.path(), &[track], &lists, &[], &options, &mut |_| {})
            .unwrap();
    assert!(repeat.up_to_date);
    // The compatibility exception is only for standalone AAC, never M4A.
    let m4a_source = tempfile::tempdir().unwrap();
    let m4a_dest = tempfile::tempdir().unwrap();
    rbl_export::export(
        m4a_dest.path(),
        &[audio(m4a_source.path(), "generated.m4a")],
        &[],
    )
    .unwrap();
    set_formats(m4a_dest.path(), 2);
    assert!(!rbl_export::verify(m4a_dest.path()).unwrap().is_ok());
}
