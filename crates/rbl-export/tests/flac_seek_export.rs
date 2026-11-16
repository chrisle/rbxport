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
