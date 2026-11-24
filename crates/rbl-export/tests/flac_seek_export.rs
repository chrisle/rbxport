//! Disposable export regression for contributor PR #828.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use rbl_export::{export, Manifest, SourceTrack};
fn crc(bytes: &[u8], width: u32, poly: u32) -> u32 {
    let mut c = 0;
    for byte in bytes {
        c ^= u32::from(*byte) << (width - 8);
        for _ in 0..8 {
            c = ((c << 1) ^ if c & (1 << (width - 1)) != 0 { poly } else { 0 })
                & ((1 << width) - 1);
        }
    }
    c
}
fn flac() -> Vec<u8> {
    let mut b = b"fLaC".to_vec();
    b.extend_from_slice(&[0x80, 0, 0, 34]);
    let mut info = vec![0x10, 0, 0x10, 0, 0, 0, 0, 0, 0, 0];
    info.extend_from_slice(&((44100u64 << 44) | (15u64 << 36) | 4096).to_be_bytes());
    info.extend_from_slice(&[0; 16]);
    b.extend(info);
    let mut frame = vec![0xff, 0xf8, 0xc9, 0x08, 0];
    frame.push(crc(&frame, 8, 7) as u8);
    frame.extend_from_slice(&[0, 0, 0]);
    frame.extend_from_slice(&(crc(&frame, 16, 0x8005) as u16).to_be_bytes());
    b.extend(frame);
    b
}
#[test]
fn export_appends_flac_index_and_sync_preserves_it() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let path = source.path().join("tone.flac");
    std::fs::write(&path, flac()).unwrap();
    let track = SourceTrack {
        id: 1,
        source_path: path,
        title: "Tone".into(),
        duration_sec: 1,
        analysis: vec![(
            "EXT".into(),
            rbl_anlz::AnlzBuilder::new().path("/tone.flac").finish(),
        )],
        ..Default::default()
    };
    export(dest.path(), std::slice::from_ref(&track), &[]).unwrap();
    let m = Manifest::load(dest.path()).unwrap();
    let ext = dest
        .path()
        .join(m.tracks[0].anlz_dir.trim_start_matches('/'))
        .join("ANLZ0000.EXT");
    let bytes = std::fs::read(&ext).unwrap();
    let anlz = rbl_anlz::parse(&bytes).unwrap();
    assert_eq!(
        anlz.sections.last().unwrap().tag,
        rbl_core::FourCc::new(b"PVB2")
    );
    assert_eq!(anlz.section(b"PVB2").unwrap().payload.len(), 8000);
    export(dest.path(), &[track], &[]).unwrap();
    assert_eq!(bytes, std::fs::read(ext).unwrap());
}

#[test]
fn non_flac_export_removes_stale_index() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let path = source.path().join("tone.mp3");
    std::fs::write(&path, [0; 64]).unwrap();
    let mut anlz =
        rbl_anlz::parse(&rbl_anlz::AnlzBuilder::new().path("/tone.mp3").finish()).unwrap();
    anlz.sections
        .push(rbl_anlz::Section::new(b"PVB2", vec![0; 20], vec![0; 8000]));
    let track = SourceTrack {
        id: 1,
        source_path: path,
        title: "Tone".into(),
        duration_sec: 1,
        analysis: vec![("EXT".into(), anlz.to_bytes())],
        ..Default::default()
    };
    export(dest.path(), &[track], &[]).unwrap();
    let m = Manifest::load(dest.path()).unwrap();
    let ext = dest
        .path()
        .join(m.tracks[0].anlz_dir.trim_start_matches('/'))
        .join("ANLZ0000.EXT");
    assert!(rbl_anlz::parse(&std::fs::read(ext).unwrap())
        .unwrap()
        .section(b"PVB2")
        .is_none());
}

#[test]
fn a_revision_four_receipt_adds_the_missing_index_without_recopying_audio() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let audio = source.path().join("tone.flac");
    std::fs::write(&audio, flac()).unwrap();
    let original = rbl_anlz::AnlzBuilder::new().path("?/tone.flac").finish();
    let library = source.path().join("library.EXT");
    std::fs::write(&library, &original).unwrap();
    std::fs::write(source.path().join("library.DAT"), &original).unwrap();
    let track = SourceTrack { id: 1, source_path: audio, title: "Tone".into(),
        analysis_source: Some(source.path().join("library.DAT")), ..Default::default() };
    export(dest.path(), std::slice::from_ref(&track), &[]).unwrap();
    let mut manifest = Manifest::load(dest.path()).unwrap();
    let saved = dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.EXT");
    let mut previous = rbl_anlz::Anlz::read(&saved).unwrap();
    previous.sections.retain(|section| section.tag != rbl_core::FourCc::new(b"PVB2"));
    std::fs::write(&saved, previous.to_bytes()).unwrap();
    manifest.generation.as_mut().unwrap().transform_version = 4;
    manifest.save(dest.path()).unwrap();
    let refreshed = export(dest.path(), std::slice::from_ref(&track), &[]).unwrap();
    assert_eq!(refreshed.bytes_copied, 0);
    assert!(refreshed.metrics.analysis_bytes_read > 0);
    let parsed = rbl_anlz::Anlz::read(&saved).unwrap();
    assert_eq!(parsed.sections.last().unwrap().tag, rbl_core::FourCc::new(b"PVB2"));
    assert_eq!(parsed.section(b"PVB2").unwrap().payload.len(), 8_000);
    assert_eq!(std::fs::read(&library).unwrap(), original);
    assert_eq!(Manifest::load(dest.path()).unwrap().generation.unwrap().transform_version,
        rbl_export::incremental::TRANSFORM_VERSION);
    let again = export(dest.path(), &[track], &[]).unwrap();
    assert_eq!((again.bytes_copied, again.analysis_files), (0, 0));
}

#[test]
fn converting_flac_to_wav_removes_the_source_seek_offsets() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let audio = source.path().join("tone.flac");
    let original = include_bytes!("../../rbl-audio/tests/fixtures/stereo-96k.flac");
    std::fs::write(&audio, original).unwrap();
    let mut ext = rbl_anlz::parse(&rbl_anlz::AnlzBuilder::new().path("?/tone.flac").finish()).unwrap();
    ext.sections.push(rbl_export::flac_seek::section(&audio).unwrap().unwrap());
    let track = SourceTrack { id: 1, source_path: audio.clone(), title: "Tone".into(), sample_rate: 96_000,
        analysis: vec![("EXT".into(), ext.to_bytes())], ..Default::default() };
    let options = rbl_export::ExportOptions { compatibility: Some(rbl_export::CompatibilityFormat::Wav), ..Default::default() };
    rbl_export::export_with_options(dest.path(), &[track], &[], &[], &options, &mut |_| {}).unwrap();
    let manifest = Manifest::load(dest.path()).unwrap();
    assert!(manifest.tracks[0].audio.ends_with(".wav"));
    let saved = dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.EXT");
    assert!(rbl_anlz::Anlz::read(&saved).unwrap().section(b"PVB2").is_none());
    assert_eq!(std::fs::read(audio).unwrap(), original);
}

#[test]
fn an_existing_index_is_preserved_byte_for_byte() {
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let audio = source.path().join("tone.flac");
    std::fs::write(&audio, flac()).unwrap();
    let index = rbl_export::flac_seek::section(&audio).unwrap().unwrap();
    let mut ext = rbl_anlz::parse(&rbl_anlz::AnlzBuilder::new().path("?/tone.flac").finish()).unwrap();
    ext.sections.push(index.clone());
    let track = SourceTrack { id: 1, source_path: audio, title: "Tone".into(),
        analysis: vec![("EXT".into(), ext.to_bytes())], ..Default::default() };
    export(dest.path(), &[track], &[]).unwrap();
    let manifest = Manifest::load(dest.path()).unwrap();
    let saved = dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.EXT");
    assert_eq!(rbl_anlz::Anlz::read(&saved).unwrap().section(b"PVB2"), Some(&index));
}
