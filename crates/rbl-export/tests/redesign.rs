//! End-to-end incremental planning and execution on disposable media.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use std::path::Path;
use rbl_export::{ExportOptions, Manifest, SourcePlaylist, SourceTrack, SyncSource};

fn track(root: &Path, id: u64) -> SourceTrack {
    let source = root.join(format!("track-{id}.mp3"));
    std::fs::write(&source, vec![id as u8; 8192]).unwrap();
    let base = root.join(format!("analysis-{id}/ANLZ0000.DAT"));
    std::fs::create_dir_all(base.parent().unwrap()).unwrap();
    let mut dat = rbl_anlz::AnlzBuilder::new();
    dat.path("/original.mp3").waveform_preview(b"PWAV", &[4, 8, 12]).beat_grid(&[
        rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 0 },
    ]);
    std::fs::write(&base, dat.finish()).unwrap();
    SourceTrack { id, source_path: source, title: format!("Track {id}"), artist: "Artist".into(),
        album: "Album".into(), duration_sec: 120, sample_rate: 44_100, analysis_source: Some(base), ..Default::default() }
}
fn playlists(indices: Vec<usize>) -> Vec<SourcePlaylist> {
    vec![SourcePlaylist { id: 10, name: "Playlist".into(), track_indices: indices, ..Default::default() }]
}
fn run(root: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist]) -> rbl_export::ExportReport {
    rbl_export::export_with_options(root, tracks, playlists, &[],
        &ExportOptions { sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() }, &mut |_| {}).unwrap()
}

#[test]
fn unchanged_and_playlist_only_syncs_do_not_read_any_media() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1), track(source.path(), 2)];
    run(usb.path(), &tracks, &playlists(vec![0, 1]));
    let before = std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let noop = run(usb.path(), &tracks, &playlists(vec![0, 1]));
    assert!(noop.up_to_date); assert_eq!(noop.metrics.database_builds, 0);
    assert_eq!((noop.metrics.audio_bytes_read, noop.metrics.analysis_bytes_read), (0, 0));
    assert_eq!(std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap(), before);
    let changed = run(usb.path(), &tracks, &playlists(vec![1, 0, 1]));
    assert!(!changed.up_to_date); assert_eq!(changed.metrics.database_builds, 1);
    assert_eq!((changed.bytes_copied, changed.analysis_files, changed.metrics.audio_bytes_read, changed.metrics.analysis_bytes_read), (0, 0, 0, 0));
    let db = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(db.legacy.as_ref().unwrap().playlists[0].tracks.len(), 3);
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
}

#[test]
fn changed_analysis_does_not_copy_audio_and_only_reads_its_source_bundle() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1), track(source.path(), 2)];
    run(usb.path(), &tracks, &playlists(vec![0, 1]));
    let base = tracks[0].analysis_source.as_ref().unwrap();
    let mut dat = rbl_anlz::AnlzBuilder::new(); dat.path("/original.mp3").waveform_preview(b"PWAV", &[20, 21]);
    std::fs::write(base, dat.finish()).unwrap();
    let changed = run(usb.path(), &tracks, &playlists(vec![0, 1]));
    assert_eq!(changed.bytes_copied, 0); assert_eq!(changed.metrics.audio_bytes_read, 0);
    assert_eq!(changed.analysis_files, 1);
    let manifest = Manifest::load(usb.path()).unwrap();
    let written = usb.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.DAT");
    assert_eq!(changed.metrics.analysis_bytes_read, std::fs::metadata(base).unwrap().len() + std::fs::metadata(written).unwrap().len());
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
}

#[test]
fn missing_source_keeps_the_old_device_track_and_new_playlist_membership() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let mut tracks = vec![track(source.path(), 1), track(source.path(), 2)];
    run(usb.path(), &tracks, &playlists(vec![0, 1]));
    let old = Manifest::load(usb.path()).unwrap();
    std::fs::remove_file(&tracks[0].source_path).unwrap();
    tracks[0].title = "Unavailable new metadata".into();
    let result = run(usb.path(), &tracks, &playlists(vec![1, 0, 0]));
    assert_eq!(result.kept_existing, 1); assert_eq!(result.bytes_copied, 0);
    assert_eq!(result.warnings[0].reason, "source-unavailable");
    let after = Manifest::load(usb.path()).unwrap();
    assert_eq!(after.tracks[0].export_id, old.tracks[0].export_id);
    let snapshot = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(snapshot.legacy.as_ref().unwrap().tracks[0].title, "Track 1");
    assert_eq!(snapshot.legacy.as_ref().unwrap().playlists[0].tracks.len(), 3);
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
}

#[test]
fn source_change_while_staging_preserves_the_entire_old_generation() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)];
    run(usb.path(), &tracks, &playlists(vec![0]));
    let before = std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let result = rbl_export::export_with_options(usb.path(), &tracks, &playlists(vec![0, 0]), &[],
        &ExportOptions { sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() }, &mut |p| {
            if p.stage == "database" { std::fs::write(&tracks[0].source_path, b"changed during staging").unwrap(); }
        });
    assert!(result.is_err());
    assert_eq!(std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap(), before);
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
}

#[test]
fn preview_deduplicates_shared_tracks_and_accounts_for_temporary_space() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)];
    let mut lists = playlists(vec![0, 0]);
    lists.push(SourcePlaylist { id: 11, name: "Other".into(), track_indices: vec![0], ..Default::default() });
    let options = ExportOptions { sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() };
    let plan = rbl_export::plan::preview(usb.path(), &tracks, &lists, &[], &options).unwrap();
    assert_eq!(plan.estimate.tracks_new, 1); assert_eq!(plan.estimate.copy_bytes, 8192);
    assert!(plan.peak_bytes > plan.estimate.copy_bytes);
    run(usb.path(), &tracks, &lists);
    let noop = rbl_export::plan::preview(usb.path(), &tracks, &lists, &[], &options).unwrap();
    assert_eq!((noop.net_additional_bytes, noop.peak_bytes), (0, 0));
    lists[0].name = "Renamed".into();
    let renamed = rbl_export::plan::preview(usb.path(), &tracks, &lists, &[], &options).unwrap();
    assert_eq!(renamed.estimate.copy_bytes, 0); assert!(renamed.peak_bytes > 0);
    assert!(renamed.peak_bytes as i64 > renamed.net_additional_bytes);
}

#[test]
fn three_destinations_share_one_conversion_and_warm_syncs_do_not_probe() {
    let source = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = rbl_export::cache::PreparationCache::new(cache_dir.path().to_owned());
    let mut audio = track(source.path(), 1);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rbl-audio/tests/fixtures/stereo-96k.flac");
    let flac = source.path().join("source.flac"); std::fs::copy(fixture, &flac).unwrap();
    audio.source_path = flac; audio.sample_rate = 96_000; audio.duration_sec = 2;
    let base = audio.analysis_source.as_ref().unwrap();
    let mut analysis = rbl_anlz::parse(&std::fs::read(base).unwrap()).unwrap();
    let mut seek = rbl_anlz::write::vbr_table_zero_section();
    seek.payload.fill(42);
    analysis.sections.push(seek);
    std::fs::write(base, analysis.to_bytes()).unwrap();
    let devices = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let reports = std::thread::scope(|scope| {
        let workers: Vec<_> = devices.iter().map(|device| {
            let audio = &audio; let cache = &cache;
            scope.spawn(move || rbl_export::export_with_options(device.path(), std::slice::from_ref(audio), &playlists(vec![0]), &[],
                &ExportOptions { compatibility: Some(rbl_export::CompatibilityFormat::Mp3), cache: Some(cache), ..Default::default() }, &mut |_| {}).unwrap())
        }).collect();
        workers.into_iter().map(|w| w.join().unwrap()).collect::<Vec<_>>()
    });
    assert!(reports.iter().all(|r| r.tracks == 1 && r.failed.is_empty() && r.skipped.is_empty()));
    assert_eq!(reports.iter().map(|r| r.metrics.conversions).sum::<u64>(), 1);
    for device in &devices {
        assert!(rbl_export::verify(device.path()).unwrap().is_ok());
        let manifest = Manifest::load(device.path()).unwrap();
        let dat = device.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.DAT");
        let analysis = rbl_anlz::parse(&std::fs::read(dat).unwrap()).unwrap();
        let seek = analysis.section(b"PVBR").unwrap();
        assert!(seek.header.iter().chain(&seek.payload).all(|b| *b == 0));
        let report = rbl_export::export_with_options(device.path(), std::slice::from_ref(&audio), &playlists(vec![0]), &[],
            &ExportOptions { compatibility: Some(rbl_export::CompatibilityFormat::Mp3), cache: Some(&cache), ..Default::default() }, &mut |_| {}).unwrap();
        assert!(report.up_to_date); assert_eq!((report.metrics.probes, report.metrics.conversions, report.bytes_copied), (0, 0, 0));
    }
}

#[cfg(unix)]
#[test]
fn same_size_source_rewrite_with_restored_mtime_is_detected_by_change_time() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)];
    run(usb.path(), &tracks, &playlists(vec![0]));
    let stamp = std::fs::metadata(&tracks[0].source_path).unwrap().modified().unwrap();
    std::fs::write(&tracks[0].source_path, vec![42; 8192]).unwrap();
    std::fs::OpenOptions::new().write(true).open(&tracks[0].source_path).unwrap().set_modified(stamp).unwrap();
    let result = run(usb.path(), &tracks, &playlists(vec![0]));
    assert_eq!(result.bytes_copied, 8192);
    let m = Manifest::load(usb.path()).unwrap();
    assert_eq!(std::fs::read(usb.path().join(m.tracks[0].audio.trim_start_matches('/'))).unwrap(), vec![42; 8192]);
}

#[test]
fn cancellation_during_copy_keeps_the_previous_generation() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)];
    run(usb.path(), &tracks, &playlists(vec![0]));
    let before = std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    std::fs::write(&tracks[0].source_path, vec![5; 4 * 1024 * 1024]).unwrap();
    let stop = std::cell::Cell::new(false);
    let result = rbl_export::export_cancellable(usb.path(), &tracks, &playlists(vec![0]), &[],
        &ExportOptions { sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() },
        &mut |p| { if p.stage == "copying" && p.bytes_done > 0 { stop.set(true); } }, &|| stop.get());
    assert!(matches!(result, Err(rbl_export::ExportError::Cancelled)));
    assert_eq!(std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap(), before);
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
    rbl_export::recover(usb.path()).unwrap(); rbl_export::recover(usb.path()).unwrap();
}

#[test]
fn a_reused_mount_path_is_rejected_before_any_export_writes() {
    let directory = tempfile::tempdir().unwrap();
    let usb = directory.path().join("usb"); std::fs::create_dir(&usb).unwrap();
    let identity = rbl_core::durable::VolumeIdentity::capture(&usb).unwrap();
    std::fs::rename(&usb, directory.path().join("original")).unwrap();
    std::fs::create_dir(&usb).unwrap();
    let source = tempfile::tempdir().unwrap(); let tracks = vec![track(source.path(), 1)];
    let result = rbl_export::export_with_options(&usb, &tracks, &playlists(vec![0]), &[],
        &ExportOptions { expected_volume: Some(&identity), ..Default::default() }, &mut |_| {});
    assert!(result.is_err()); assert_eq!(std::fs::read_dir(&usb).unwrap().count(), 0);
}

#[test]
fn full_integrity_check_detects_stamp_preserving_destination_edits() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)]; run(usb.path(), &tracks, &playlists(vec![0]));
    let m = Manifest::load(usb.path()).unwrap();
    let audio = usb.path().join(m.tracks[0].audio.trim_start_matches('/'));
    let modified = std::fs::metadata(&audio).unwrap().modified().unwrap();
    std::fs::write(&audio, vec![99; 8192]).unwrap();
    std::fs::OpenOptions::new().write(true).open(&audio).unwrap().set_modified(modified).unwrap();
    let before = std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let result = rbl_export::export_with_options(usb.path(), &tracks, &playlists(vec![0]), &[],
        &ExportOptions { verify_all: true, sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() }, &mut |_| {});
    assert!(result.unwrap_err().to_string().contains("Full integrity check failed"));
    assert_eq!(std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap(), before);
}

#[test]
fn source_hashing_checks_cancellation_between_chunks_and_keeps_old_databases() {
    let source = tempfile::tempdir().unwrap(); let cache_dir = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap(); let tracks = vec![track(source.path(), 1)];
    run(usb.path(), &tracks, &playlists(vec![0]));
    let before = std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    std::fs::write(&tracks[0].source_path, vec![42; 1024 * 1024]).unwrap();
    let cache = rbl_export::cache::PreparationCache::new(cache_dir.path().to_owned());
    let checks = std::cell::Cell::new(0);
    rbl_export::incremental::reset_metrics();
    let hash = cache.source_hash(&tracks[0].source_path, &|| { checks.set(checks.get() + 1); checks.get() >= 3 });
    assert!(hash.is_err());
    assert!(rbl_export::incremental::metrics().audio_bytes_read < 1024 * 1024);
    let stop = std::cell::Cell::new(false);
    let result = rbl_export::export_cancellable(usb.path(), &tracks, &playlists(vec![0]), &[],
        &ExportOptions { sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() },
        &mut |progress| { if progress.stage == "planning" && !progress.title.is_empty() { stop.set(true); } }, &|| stop.get());
    assert!(matches!(result, Err(rbl_export::ExportError::Cancelled)));
    assert_eq!(std::fs::read(usb.path().join("PIONEER/rekordbox/export.pdb")).unwrap(), before);
}

#[test]
fn an_unavailable_local_cache_does_not_block_export() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)];
    let unavailable = source.path().join("not-a-directory"); std::fs::write(&unavailable, b"occupied").unwrap();
    let cache = rbl_export::cache::PreparationCache::new(unavailable.join("cache"));
    let report = rbl_export::export_with_options(usb.path(), &tracks, &playlists(vec![0]), &[],
        &ExportOptions { cache: Some(&cache), ..Default::default() }, &mut |_| {}).unwrap();
    assert_eq!(report.tracks, 1); assert!(report.failed.is_empty()); assert_eq!(report.bytes_copied, 8192);
}

#[test]
fn changed_audio_does_not_keep_analysis_for_the_previous_contents() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)]; run(usb.path(), &tracks, &playlists(vec![0]));
    std::fs::write(&tracks[0].source_path, vec![50; 8192]).unwrap();
    let report = run(usb.path(), &tracks, &playlists(vec![0]));
    assert_eq!(report.bytes_copied, 8192); assert!(report.warnings.iter().any(|w| w.reason == "analysis-stale"));
    let manifest = Manifest::load(usb.path()).unwrap();
    assert!(manifest.tracks[0].analysis_extensions.is_empty()); assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
}

#[test]
fn compatibility_profiles_are_independent_and_corrupt_cache_outputs_are_reprepared() {
    let source = tempfile::tempdir().unwrap(); let cache = rbl_export::cache::PreparationCache::new(source.path().join("cache"));
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rbl-audio/tests/fixtures/stereo-96k.flac");
    for format in [rbl_audio::compatibility::Format::Wav, rbl_audio::compatibility::Format::Aiff, rbl_audio::compatibility::Format::Mp3] {
        let (path, size, reused) = cache.converted(&input, format, &mut |_, _| {}, &|| false).unwrap();
        assert!(!reused); assert!(size > 0); assert!(!rbl_audio::compatibility::needs_conversion(&path).unwrap());
        assert_eq!(cache.converted_size(&input, format), Some(size));
        std::fs::write(&path, b"corrupt").unwrap();
        let repaired = cache.converted(&input, format, &mut |_, _| {}, &|| false).unwrap();
        assert!(!repaired.2); assert!(!rbl_audio::compatibility::needs_conversion(&repaired.0).unwrap());
    }
}

#[test]
fn touching_a_source_without_changing_bytes_reuses_audio_and_analysis() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)]; run(usb.path(), &tracks, &playlists(vec![0]));
    let file = std::fs::OpenOptions::new().write(true).open(&tracks[0].source_path).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2)).unwrap();
    let report = run(usb.path(), &tracks, &playlists(vec![0]));
    assert_eq!((report.bytes_copied, report.analysis_files, report.metrics.analysis_bytes_read), (0, 0, 0));
    assert_eq!(report.reused, 1); assert!(report.warnings.is_empty());
    let next = run(usb.path(), &tracks, &playlists(vec![0, 0]));
    assert_eq!((next.metrics.audio_bytes_read, next.metrics.analysis_bytes_read), (0, 0));
}

#[test]
fn full_verification_compares_source_contents_as_well_as_usb_copies() {
    let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
    let tracks = vec![track(source.path(), 1)]; run(usb.path(), &tracks, &playlists(vec![0]));
    let modified = std::fs::metadata(&tracks[0].source_path).unwrap().modified().unwrap();
    std::fs::write(&tracks[0].source_path, vec![99; 8192]).unwrap();
    std::fs::OpenOptions::new().write(true).open(&tracks[0].source_path).unwrap().set_modified(modified).unwrap();
    // Simulate a filesystem whose file-information receipt cannot distinguish
    // this rewrite (e.g. a restored timestamp without a change-time field).
    let mut manifest = Manifest::load(usb.path()).unwrap();
    manifest.generation.as_mut().unwrap().inputs.insert(tracks[0].source_path.to_string_lossy().into_owned(), rbl_export::incremental::revision_fresh(&tracks[0].source_path).unwrap());
    manifest.save(usb.path()).unwrap();
    let result = rbl_export::export_with_options(usb.path(), &tracks, &playlists(vec![0]), &[],
        &ExportOptions { verify_all: true, sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() }, &mut |_| {}).unwrap();
    assert_eq!(result.bytes_copied, 8192);
    let manifest = Manifest::load(usb.path()).unwrap();
    assert_eq!(std::fs::read(usb.path().join(manifest.tracks[0].audio.trim_start_matches('/'))).unwrap(), vec![99; 8192]);
}
