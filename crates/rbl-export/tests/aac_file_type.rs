//! Standalone ADTS AAC, AAC-in-M4A and ALAC have distinct types. All media is synthetic.
//! The ADTS fixture is one second of generated 44.1 kHz mono tone, encoded by afconvert.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_export::{ExportOptions, Manifest, SourcePlaylist, SourceTrack, SyncSource};
use std::path::Path;

fn audio(root: &Path, name: &str) -> SourceTrack {
    let source = root.join(name);
    let bytes: &[u8] = if name.contains("alac") {
        include_bytes!("../../rbl-db/tests/fixtures/generated-alac.m4a")
    } else if name.ends_with("m4a") {
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
    for (name, expected) in [
        ("generated.AAC", 2),
        ("generated.m4a", 4),
        ("generated-alac.m4a", 6),
    ] {
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
    for (name, expected) in [
        ("generated.AAC", 2),
        ("generated.m4a", 4),
        ("generated-alac.m4a", 6),
    ] {
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

#[test]
fn alac_reload_refines_old_metadata_and_undo_preserves_the_previous_format() {
    let root = tempfile::tempdir().unwrap();
    let location = rbl_db::fixture::build(
        &root.path().join("collection"),
        rbl_db::fixture::Shape {
            tracks: 0,
            playlists: 0,
            history_sessions: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut writer = rbl_db::write::Writer::open(location, root.path().join("backups")).unwrap();
    let track = audio(root.path(), "generated-alac.m4a");
    let id = writer.import_file(&track.source_path).unwrap();
    let kind = |writer: &rbl_db::write::Writer| -> i64 {
        writer
            .library()
            .connection()
            .query_row("SELECT FileType FROM djmdContent WHERE ID=?1", [&id], |r| {
                r.get(0)
            })
            .unwrap()
    };
    let extras =
        rbl_db::export_info::track_extras(writer.library().connection(), std::slice::from_ref(&id))
            .unwrap();
    assert_eq!(extras[&id].metadata.audio_file_type, Some(6));
    writer
        .library()
        .connection()
        .execute("UPDATE djmdContent SET FileType=4 WHERE ID=?1", [&id])
        .unwrap();
    let (_, edits) = writer.reload_tags_with_undo(&id).unwrap();
    assert_eq!(kind(&writer), 6);
    assert_ne!(edits, []);
    writer.step_track_edits_at_once(&edits, true).unwrap();
    assert_eq!(kind(&writer), 4);
    writer.step_track_edits_at_once(&edits, false).unwrap();
    assert_eq!(kind(&writer), 6);
    // Same path, different actual codec: Reload Tag must not keep stale ALAC.
    std::fs::write(
        &track.source_path,
        include_bytes!("../../rbl-db/tests/fixtures/silent.m4a"),
    )
    .unwrap();
    let (_, edits) = writer.reload_tags_with_undo(&id).unwrap();
    assert_eq!(kind(&writer), 4);
    for edit in edits.iter().rev() {
        writer.undo_track_edit(edit).unwrap();
    }
    assert_eq!(kind(&writer), 6);
    for edit in &edits {
        writer.redo_track_edit(edit).unwrap();
    }
    assert_eq!(kind(&writer), 4);
    writer
        .library()
        .connection()
        .execute("UPDATE djmdContent SET FileType=2 WHERE ID=?1", [&id])
        .unwrap();
    assert!(writer.step_track_edits_at_once(&edits, true).is_err());
    assert_eq!(
        kind(&writer),
        2,
        "outside changes are not overwritten by Undo"
    );
}

#[test]
fn legacy_alac4_repairs_on_database_rebuild_without_audio_or_noop_probe_work() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = rbl_export::cache::PreparationCache::new(cache_dir.path().to_owned());
    let track = audio(source.path(), "generated-alac.m4a");
    let source_before = std::fs::read(&track.source_path).unwrap();
    let sync = SyncSource {
        db_id: 123,
        ..Default::default()
    };
    let options = ExportOptions {
        sync: Some(&sync),
        cache: Some(&cache),
        ..Default::default()
    };
    let run = |lists: &[SourcePlaylist]| {
        rbl_export::export_with_options(
            dest.path(),
            std::slice::from_ref(&track),
            lists,
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap()
    };
    let fresh = run(&[]);
    assert_eq!(fresh.metrics.probes, 1);
    assert_eq!(formats(dest.path()), (6, 6));
    for bad in [0, 1, 2, 3, 5, 11, 12] {
        set_formats(dest.path(), bad);
        assert!(
            !rbl_export::verify(dest.path()).unwrap().is_ok(),
            "unsupported M4A type {bad}"
        );
    }
    set_formats(dest.path(), 4);
    assert!(
        rbl_export::verify(dest.path()).unwrap().is_ok(),
        "legacy ALAC4 remains readable"
    );
    let mut manifest = Manifest::load(dest.path()).unwrap();
    let copied = dest
        .path()
        .join(manifest.tracks[0].audio.trim_start_matches('/'));
    let original = (
        std::fs::read(&copied).unwrap(),
        std::fs::metadata(&copied).unwrap().modified().unwrap(),
    );
    manifest.generation.as_mut().unwrap().databases =
        rbl_export::incremental::databases(dest.path()).unwrap();
    manifest.save(dest.path()).unwrap();
    let repeat = run(&[]);
    assert!(repeat.up_to_date);
    assert_eq!(
        (
            repeat.metrics.probes,
            repeat.metrics.audio_bytes_read,
            repeat.bytes_copied
        ),
        (0, 0, 0)
    );
    assert_eq!(formats(dest.path()), (4, 4));
    let lists = [SourcePlaylist {
        id: 10,
        name: "Metadata only".into(),
        track_indices: vec![0],
        ..Default::default()
    }];
    let repaired = run(&lists);
    assert_eq!(
        (
            repaired.metrics.database_builds,
            repaired.bytes_copied,
            repaired.analysis_files,
            repaired.metrics.probes
        ),
        (1, 0, 0, 0)
    );
    assert_eq!(formats(dest.path()), (6, 6));
    assert_eq!(
        (
            std::fs::read(&copied).unwrap(),
            std::fs::metadata(&copied).unwrap().modified().unwrap()
        ),
        original
    );
    assert_eq!(std::fs::read(&track.source_path).unwrap(), source_before);
    assert!(run(&lists).up_to_date);
}

#[test]
fn converted_alac_uses_the_actual_output_format_and_source_replacement_reclassifies() {
    for (compatibility, expected) in [
        (rbl_export::CompatibilityFormat::Wav, 11),
        (rbl_export::CompatibilityFormat::Aiff, 12),
        (rbl_export::CompatibilityFormat::Mp3, 1),
    ] {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let mut track = audio(source.path(), "generated-alac.m4a");
        track.metadata.audio_file_type = Some(6);
        let before = std::fs::read(&track.source_path).unwrap();
        let options = ExportOptions {
            compatibility: Some(compatibility),
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
        assert_eq!(std::fs::read(&track.source_path).unwrap(), before);
    }
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut track = audio(source.path(), "generated-alac.m4a");
    track.metadata.audio_file_type = Some(6);
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
    std::fs::write(
        &track.source_path,
        include_bytes!("../../rbl-db/tests/fixtures/silent.m4a"),
    )
    .unwrap();
    rbl_export::export_with_options(dest.path(), &[track], &[], &[], &options, &mut |_| {})
        .unwrap();
    assert_eq!(
        formats(dest.path()),
        (4, 4),
        "stale source type6 cannot override a changed AAC payload"
    );
}

#[test]
fn proven_content_replacement_clears_obsolete_alac_when_codec_is_unknown() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut track = audio(source.path(), "generated-alac.m4a");
    track.metadata.audio_file_type = Some(6);
    let sync = SyncSource {
        db_id: 123,
        ..Default::default()
    };
    let options = ExportOptions {
        sync: Some(&sync),
        ..Default::default()
    };
    let run = || {
        rbl_export::export_with_options(
            dest.path(),
            std::slice::from_ref(&track),
            &[],
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap()
    };
    run();
    assert_eq!(formats(dest.path()), (6, 6));
    // A changed revision alone is not evidence of different audio.
    let modified = std::fs::metadata(&track.source_path)
        .unwrap()
        .modified()
        .unwrap();
    std::fs::File::options()
        .write(true)
        .open(&track.source_path)
        .unwrap()
        .set_modified(modified + std::time::Duration::from_secs(2))
        .unwrap();
    run();
    assert_eq!(
        formats(dest.path()),
        (6, 6),
        "unchanged bytes retain known ALAC"
    );
    // An ordinary valid PCM WAV whose container was renamed. Export's copy
    // route admits it, but it is positively not an AAC/ALAC MP4 codec.
    let mut wav = b"RIFF".to_vec();
    wav.extend(88_236_u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(44_100_u32.to_le_bytes());
    wav.extend(88_200_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(88_200_u32.to_le_bytes());
    wav.resize(88_244, 0);
    std::fs::write(&track.source_path, &wav).unwrap();
    assert_eq!(
        rbl_audio::compatibility::inspect(&track.source_path)
            .unwrap()
            .mp4_file_type,
        Some(0)
    );
    let changed = run();
    assert!(changed.bytes_copied > 0);
    assert_eq!(
        formats(dest.path()),
        (4, 4),
        "proven new bytes cannot inherit old ALAC"
    );
    assert_eq!(std::fs::read(&track.source_path).unwrap(), wav);
    assert!(run().up_to_date);
}

#[test]
fn carried_alac_preserves_known_type_from_either_database_without_original_source() {
    for retained in [
        "both",
        "legacy",
        "one",
        "legacy6-one4",
        "legacy4-one6",
        "different-origin",
    ] {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = rbl_export::cache::PreparationCache::new(cache_dir.path().to_owned());
        let alac = audio(source.path(), "generated-alac.m4a");
        let retained_playlist = [SourcePlaylist {
            id: 10,
            name: "Held ALAC".into(),
            track_indices: vec![0],
            ..Default::default()
        }];
        let sync = SyncSource {
            db_id: 123,
            ..Default::default()
        };
        rbl_export::export_with_options(
            dest.path(),
            std::slice::from_ref(&alac),
            &retained_playlist,
            &[],
            &ExportOptions {
                sync: Some(&sync),
                ..Default::default()
            },
            &mut |_| {},
        )
        .unwrap();
        let manifest = Manifest::load(dest.path()).unwrap();
        let id = manifest.tracks[0].export_id;
        let copied = dest
            .path()
            .join(manifest.tracks[0].audio.trim_start_matches('/'));
        let before = (
            std::fs::read(&copied).unwrap(),
            std::fs::metadata(&copied).unwrap().modified().unwrap(),
        );
        std::fs::remove_file(&alac.source_path).unwrap();
        let base = rbl_export::export_root(dest.path());
        if retained == "legacy" {
            std::fs::remove_file(base.join("rekordbox/exportLibrary.db")).unwrap();
        }
        if retained == "one" {
            std::fs::remove_file(base.join("rekordbox/export.pdb")).unwrap();
        }
        if retained == "legacy6-one4" || retained == "different-origin" {
            let one = rbl_onelibrary::playlists::open_read_write(
                &base.join("rekordbox/exportLibrary.db"),
            )
            .unwrap();
            one.execute("UPDATE content SET fileType=4", []).unwrap();
            if retained == "different-origin" {
                one.execute("UPDATE content SET masterContentId=masterContentId+1", [])
                    .unwrap();
            }
        }
        if retained == "legacy4-one6" {
            set_formats(dest.path(), 4);
            let one = rbl_onelibrary::playlists::open_read_write(
                &base.join("rekordbox/exportLibrary.db"),
            )
            .unwrap();
            one.execute("UPDATE content SET fileType=6", []).unwrap();
        }
        let mut added = audio(source.path(), "generated.m4a");
        added.id = 2;
        let options = ExportOptions {
            keep_unlisted: true,
            sync: Some(&sync),
            cache: Some(&cache),
            ..Default::default()
        };
        let result =
            rbl_export::export_with_options(dest.path(), &[added], &[], &[], &options, &mut |_| {});
        if retained == "different-origin" {
            assert!(
                result.is_err(),
                "ambiguous existing master ownership must still refuse"
            );
            assert_eq!(
                (
                    std::fs::read(&copied).unwrap(),
                    std::fs::metadata(&copied).unwrap().modified().unwrap()
                ),
                before
            );
            continue;
        }
        let report = result.unwrap();
        assert_eq!(
            report.metrics.probes, 1,
            "only a proven paired origin can lend the ALAC classification: {retained}"
        );
        let bytes = std::fs::read(base.join("rekordbox/export.pdb")).unwrap();
        let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
        let row = pdb
            .rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap())
            .into_iter()
            .find(|row| pdb.u4_at(*row, 0x48) == id)
            .unwrap();
        assert_eq!(pdb.u2_at(row, 0x5a), 6, "{retained}");
        let one =
            rbl_onelibrary::ExportLibrary::open_read_only(&base.join("rekordbox/exportLibrary.db"))
                .unwrap();
        let kind: i64 = one
            .connection()
            .query_row(
                "SELECT fileType FROM content WHERE content_id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, 6, "{retained}");
        assert_eq!(
            (
                std::fs::read(&copied).unwrap(),
                std::fs::metadata(&copied).unwrap().modified().unwrap()
            ),
            before
        );
        assert!(rbl_export::verify(dest.path()).unwrap().is_ok());
    }
}

#[test]
fn old_probe_receipts_refresh_only_missing_mp4_codec_information() {
    let source = tempfile::tempdir().unwrap();
    let cache_root = tempfile::tempdir().unwrap();
    let track = audio(source.path(), "generated-alac.m4a");
    let cache = rbl_export::cache::PreparationCache::new(cache_root.path().to_owned());
    assert_eq!(
        cache.properties(&track.source_path).unwrap().mp4_file_type,
        Some(6)
    );
    drop(cache);
    let path = std::fs::read_dir(cache_root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "probe")
        })
        .unwrap();
    let mut old: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    old["properties"]
        .as_object_mut()
        .unwrap()
        .remove("mp4_file_type");
    let revision: rbl_export::incremental::FileRevision =
        serde_json::from_value(old["source"].clone()).unwrap();
    let properties: rbl_audio::compatibility::AudioProperties =
        serde_json::from_value(old["properties"].clone()).unwrap();
    old["checksum"] = rbl_export::incremental::digest(&(&revision, &properties))
        .unwrap()
        .into();
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let cache = rbl_export::cache::PreparationCache::new(cache_root.path().to_owned());
    rbl_export::incremental::reset_metrics();
    assert_eq!(
        cache.properties(&track.source_path).unwrap().mp4_file_type,
        Some(6)
    );
    assert_eq!(rbl_export::incremental::metrics().probes, 1);
    rbl_export::incremental::reset_metrics();
    assert_eq!(
        cache.properties(&track.source_path).unwrap().mp4_file_type,
        Some(6)
    );
    assert_eq!(rbl_export::incremental::metrics().probes, 0);
}
