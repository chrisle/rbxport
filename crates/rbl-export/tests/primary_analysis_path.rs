//! Source analysis must be read and receipted under its stored DAT pathname.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod common;
use std::path::Path;
use rbl_export::{ExportOptions, Manifest, SourcePlaylist, SourceTrack};

fn write_dat(base: &Path, value: u8, beats: usize) {
    let grid: Vec<_> = (0..beats).map(|index| rbl_anlz::Beat {
        beat_number: (index % 4 + 1) as u16, tempo_x100: 12000, time_ms: index as u32 * 500,
    }).collect();
    let mut file = rbl_anlz::AnlzBuilder::new();
    file.path("/generated.wav").beat_grid(&grid)
        .waveform_preview(b"PWAV", &[value; 400]).waveform_preview(b"PWV2", &[value; 100]);
    std::fs::write(base, file.finish()).unwrap();
}

fn check_export(root: &Path, expected: u8) -> Manifest {
    let manifest = Manifest::load(root).unwrap();
    let track = &manifest.tracks[0];
    assert_eq!(track.analysis_extensions, ["DAT", "EXT", "2EX"]);
    let relative = format!("{}/ANLZ0000.DAT", track.anlz_dir);
    let path = root.join(relative.trim_start_matches('/'));
    let dat = rbl_anlz::Anlz::read(&path).unwrap();
    assert_eq!(dat.waveform(b"PWAV").unwrap().1, [expected; 400]);
    assert_eq!(dat.waveform(b"PWV2").unwrap().1, [expected; 100]);
    let ext = rbl_anlz::Anlz::read(&path.with_extension("EXT")).unwrap();
    assert_eq!(ext.waveform(b"PWV3").unwrap().1, [0x29; 300]);
    for extension in ["DAT", "EXT", "2EX"] {
        assert_eq!(rbl_anlz::Anlz::read(&path.with_extension(extension)).unwrap().path().as_deref(), Some(track.audio.as_str()));
    }
    let bytes = std::fs::read(root.join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].file_path, track.audio);
    assert_eq!(rows[0].analyze_path, relative);
    let one = rbl_onelibrary::ExportLibrary::open_read_only(&root.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    let paths: (String, String) = one.connection().query_row("SELECT path, analysisDataFilePath FROM content", [],
        |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(paths, (track.audio.clone(), relative));
    assert!(rbl_export::verify(root).unwrap().is_ok());
    manifest
}

fn export_case(extension: &str, require_distinct_case: bool) -> bool {
    let source = tempfile::tempdir().unwrap();
    let base = source.path().join(format!("ANLZ0000.{extension}"));
    write_dat(&base, 9, 2);
    if require_distinct_case && extension != "DAT" && base.with_extension("DAT").exists() {
        eprintln!("case-sensitive source test not exercised: host aliases the primary spelling");
        return false;
    }
    let audio = source.path().join("generated.wav");
    let rate = 44100_u32;
    let count = rate * 2;
    let mut wav = Vec::new();
    wav.extend(b"RIFF"); wav.extend((36 + count * 2).to_le_bytes()); wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes()); wav.extend(1_u16.to_le_bytes()); wav.extend(1_u16.to_le_bytes());
    wav.extend(rate.to_le_bytes()); wav.extend((rate * 2).to_le_bytes());
    wav.extend(2_u16.to_le_bytes()); wav.extend(16_u16.to_le_bytes()); wav.extend(b"data"); wav.extend((count * 2).to_le_bytes());
    for n in 0..count { wav.extend(((n % 1000) as i16 * 16 - 8000).to_le_bytes()); }
    std::fs::write(&audio, &wav).unwrap();
    let mut ext = rbl_anlz::AnlzBuilder::new();
    ext.path("/generated.wav").waveform_scroll(b"PWV3", 1, &[0x29; 300]);
    std::fs::write(base.with_extension("EXT"), ext.finish()).unwrap();
    let mut two_ex = rbl_anlz::AnlzBuilder::new();
    two_ex.path("/generated.wav").waveform_scroll(b"PWV6", 3, &[9; 3600]);
    std::fs::write(base.with_extension("2EX"), two_ex.finish()).unwrap();
    let original: Vec<_> = [base.clone(), base.with_extension("EXT"), base.with_extension("2EX")]
        .iter().map(|path| std::fs::read(path).unwrap()).collect();
    let track = SourceTrack { id: 1, source_path: audio.clone(), analysis_source: Some(base.clone()),
        title: "Stored primary".into(), artist: "Fixture".into(), duration_sec: 2, sample_rate: rate,
        metadata: rbl_core::ExportMetadata { analysed: rbl_db::write::ANALYSED_FULL as u32, ..Default::default() },
        ..Default::default() };
    let playlists = [SourcePlaylist { id: 1, name: "Generated".into(), track_indices: vec![0], ..Default::default() }];
    let destination = common::stick();
    let run = || rbl_export::export_with_options(destination.path(), std::slice::from_ref(&track), &playlists,
        &[], &ExportOptions::default(), &mut |_| {}).unwrap();
    let fresh = run();
    assert!(fresh.verified && fresh.failed.is_empty() && fresh.skipped.is_empty());
    let mut manifest = check_export(destination.path(), 9);
    let copied_audio = destination.path().join(manifest.tracks[0].audio.trim_start_matches('/'));
    assert_eq!(std::fs::read(&copied_audio).unwrap(), wav);
    let audio_stamp = rbl_export::incremental::revision_fresh(&copied_audio).unwrap();
    assert!(run().up_to_date);
    if extension != "DAT" {
        // Simulate an old wrong-primary receipt key. Its changed name must
        // force a normal refresh, without an unrelated transform bump.
        let generation = manifest.generation.as_mut().unwrap();
        assert!(generation.inputs.remove(&base.to_string_lossy().into_owned()).unwrap().is_some());
        generation.inputs.insert(base.with_extension("DAT").to_string_lossy().into_owned(), None);
        manifest.save(destination.path()).unwrap();
        let repaired_receipt = run();
        assert!(!repaired_receipt.up_to_date);
        assert_eq!(repaired_receipt.bytes_copied, 0);
        check_export(destination.path(), 9);
        assert!(run().up_to_date);
    }
    for (path, image) in [base.clone(), base.with_extension("EXT"), base.with_extension("2EX")].iter().zip(&original) {
        assert_eq!(&std::fs::read(path).unwrap(), image, "export does not alter source analysis");
    }
    write_dat(&base, 13, 3); // Different size makes the source revision deterministic.
    let expected_dat = std::fs::read(&base).unwrap();
    let changed = run();
    assert!(!changed.up_to_date);
    assert_eq!(changed.bytes_copied, 0);
    assert!(changed.analysis_files > 0);
    check_export(destination.path(), 13);
    assert_eq!(rbl_export::incremental::revision_fresh(&copied_audio).unwrap(), audio_stamp);
    assert!(run().up_to_date);
    assert_eq!(std::fs::read(audio).unwrap(), wav);
    assert_eq!(std::fs::read(base.with_extension("EXT")).unwrap(), original[1]);
    assert_eq!(std::fs::read(base.with_extension("2EX")).unwrap(), original[2]);
    run();
    assert_eq!(std::fs::read(base).unwrap(), expected_dat);
    true
}

#[test]
fn uppercase_primary_exports_and_reuses_analysis_normally() { assert!(export_case("DAT", false)); }

#[test]
fn stored_primary_case_exports_and_resyncs_on_case_sensitive_filesystems() {
    for extension in ["dat", "dAt"] { export_case(extension, true); }
}

#[test]
fn stored_primary_receipt_keys_refresh_on_any_filesystem() {
    // Exercises the mixed-spelling receipt and source-change sequence even
    // when the host aliases filenames; it is not a case-sensitive IO claim.
    assert!(export_case("dAt", false));
}
