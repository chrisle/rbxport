//! #822: a track RBXport analysed must reach the stick with the waveform tags
//! and shape rekordbox writes for it, and with a preview a Nexus-family player
//! (XDJ-RR 1.08 included) accepts as complete.
//!
//! [OBS, static] XDJ-RR 1.08 `TotalWaveWacher_CheckWaveDataComplete`
//! (`0x000cf6a8`, alphatheta-docs `devices/xdj-rr/.../chunk_000c.c`) reads the
//! 400-column preview as height/whiteness pairs and the 100-column tiny
//! preview: a height of 0 is "incomplete", a height above 0x1f, a whiteness
//! above 7 or a tiny byte above 0xf makes it reject the whole preview.
//! [UNKNOWN] No XDJ-RR was available; nothing here is a physical check.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use rbl_export::{export, SourceTrack};

/// Two stereo bursts of noise-free tone either side of digital silence and a
/// near-silent tail: the shapes that give a preview its zero columns.
fn signal(rate: u32, seconds: u32) -> Vec<[i16; 2]> {
    (0..rate * seconds)
        .map(|n| {
            let t = f64::from(n) / f64::from(rate);
            let level = match (t / f64::from(seconds) * 10.0) as u32 { 0..=2 => 0.8, 3..=5 => 0.0, 6..=7 => 0.02, _ => 0.4 };
            let v = ((t * 2.0 * std::f64::consts::PI * 110.0).sin() + 0.5 * (t * 2.0 * std::f64::consts::PI * 3000.0).sin()) * level * 12_000.0;
            [v as i16, v as i16]
        })
        .collect()
}

fn analysed_track(dir: &std::path::Path) -> SourceTrack {
    let rate = 44_100;
    let mut creator = rbl_analysis::wave_creator::WaveCreator::new(rate);
    creator.push(&signal(rate, 30));
    let w = creator.finish();
    let waves = rbl_anlz::RekordboxWaves { pwav: w.pwav, pwv2: w.pwv2, pwvc: w.pwvc, pwv3: w.pwv3, pwv4: w.pwv4, pwv5: w.pwv5, pwv6: w.pwv6, pwv7: w.pwv7 };
    let beats: Vec<_> = (0..64).map(|i| rbl_anlz::Beat { beat_number: (i % 4 + 1) as u16, tempo_x100: 12_800, time_ms: 100 + i * 469 }).collect();
    let files = rbl_anlz::author_with_rekordbox_waves("?/click.mp3", &beats, &waves, rbl_anlz::Existing::default());
    let audio = dir.join("click.mp3");
    std::fs::write(&audio, vec![7u8; 4096]).unwrap();
    SourceTrack {
        source_path: audio, title: "Click".into(), artist: "RBX".into(), album: "Single".into(), duration_sec: 30,
        sample_rate: 44_100, bpm_x100: 12_800, cues: Some(Vec::new()),
        analysis: vec![("DAT".into(), files.dat), ("EXT".into(), files.ext), ("2EX".into(), files.two_ex)],
        ..SourceTrack::default()
    }
}

fn tags(file: &rbl_anlz::Anlz) -> Vec<String> { file.sections.iter().map(|s| s.tag.to_string()).collect() }

/// XDJ-RR's `CheckWaveDataComplete`: 2 means complete.
fn firmware_verdict(pwav: &[u8], pwv2: &[u8]) -> u8 {
    let (mut bad, mut zero) = (false, false);
    for byte in pwav {
        let (height, whiteness) = (byte & 0x1f, byte >> 5);
        if height == 0 { zero = true; }
        if whiteness > 7 { bad = true; }
    }
    for byte in pwv2 {
        if *byte == 0 { zero = true } else if *byte > 0xf { bad = true }
    }
    if bad { 0 } else if zero { 1 } else { 2 }
}

#[test]
fn an_rbxport_analysed_track_exports_the_waveform_set_rekordbox_writes() {
    let src = tempfile::tempdir().unwrap();
    let dest = common::stick();
    let track = analysed_track(src.path());
    export(dest.path(), &[track], &[]).unwrap();

    let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
    let dir = dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/'));
    let dat = rbl_anlz::Anlz::read(&dir.join("ANLZ0000.DAT")).unwrap();
    let ext = rbl_anlz::Anlz::read(&dir.join("ANLZ0000.EXT")).unwrap();
    let two = rbl_anlz::Anlz::read(&dir.join("ANLZ0000.2EX")).unwrap();

    // A rekordbox 7 export's tag set and order, per file.
    assert_eq!(tags(&dat), ["PPTH", "PVBR", "PQTZ", "PWAV", "PWV2", "PCOB", "PCOB"]);
    assert_eq!(tags(&ext), ["PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "PWV5", "PWV4"]);
    assert_eq!(tags(&two), ["PPTH", "PWV7", "PWV6", "PWVC"]);

    let pwav = dat.waveform(b"PWAV").unwrap().1;
    let pwv2 = dat.waveform(b"PWV2").unwrap().1;
    assert_eq!((pwav.len(), pwv2.len()), (400, 100));
    assert_eq!(firmware_verdict(pwav, pwv2), 2, "the player would treat the preview as incomplete or invalid");
    assert!(ext.waveform(b"PWV3").is_some() && ext.waveform(b"PWV5").is_some() && ext.waveform(b"PWV4").is_some());
}
