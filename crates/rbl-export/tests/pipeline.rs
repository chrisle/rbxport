//! The whole chain on fixtures: analyse audio, author analysis, export, verify.
//!
//! No database, no webview, no rekordbox — this is what runs on any machine.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::Path;

const SR: u32 = 44_100;

/// Writes a 16-bit PCM WAV holding a click track at a known tempo.
fn write_click_wav(path: &Path, bpm: f64, secs: f64) {
    let total = (secs * f64::from(SR)) as usize;
    let period = (60.0 / bpm * f64::from(SR)) as usize;
    let mut samples = vec![0.0_f32; total];
    let mut at = 0;
    while at < total {
        for i in 0..(SR as usize / 200) {
            if at + i >= total {
                break;
            }
            let decay = 1.0 - i as f32 / (SR as f32 / 200.0);
            samples[at + i] += (i as f32 * 0.7).sin() * decay * 0.8;
        }
        at += period;
    }

    let data_len = u32::try_from(samples.len() * 2).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&SR.to_le_bytes());
    out.extend_from_slice(&(SR * 2).to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

#[test]
fn analyse_author_export_and_read_it_back() {
    let src = tempfile::tempdir().unwrap();
    let dest = common::stick();

    // 1. A track with a tempo we know.
    let audio_path = src.path().join("click-128.wav");
    write_click_wav(&audio_path, 128.0, 20.0);

    // 2. Analyse it.
    let audio = rbl_audio::decode_mono(&audio_path, None).unwrap();
    let analysis = rbl_analysis::analyse(&audio.samples, audio.sample_rate);
    let detected = rbl_analysis::tempo::nearest_octave(analysis.tempo.bpm, 128.0);
    assert!((detected - 128.0).abs() < 1.0, "detected {detected}");
    assert!(!analysis.tempo.beats.is_empty());
    assert!(!analysis.waveform.columns.is_empty());

    // 3. Author analysis files from what we found.
    let beats: Vec<rbl_anlz::Beat> = analysis
        .tempo
        .beats
        .iter()
        .map(|b| rbl_anlz::Beat {
            beat_number: b.beat_number,
            tempo_x100: b.tempo_x100,
            time_ms: b.time_ms,
        })
        .collect();
    let preview: Vec<u8> = analysis
        .waveform
        .columns
        .iter()
        .map(|c| {
            // PWAV packs height in the low five bits and whiteness in the top three.
            let height = u32::from(c.peak) * 31 / 255;
            let whiteness = u32::from(c.high) * 7 / 255;
            u8::try_from((whiteness << 5) | height).unwrap_or(0)
        })
        .collect();

    let mut builder = rbl_anlz::AnlzBuilder::new();
    builder
        .path(&audio_path.to_string_lossy())
        .beat_grid(&beats)
        .waveform_preview(b"PWAV", &preview)
        .empty_cue_list(false);
    let dat = builder.finish();

    // The authored file must be readable by our own parser.
    let parsed = rbl_anlz::parse(&dat).unwrap();
    assert_eq!(parsed.beat_grid().unwrap().len(), beats.len());
    assert_eq!(parsed.waveform(b"PWAV").unwrap().1.len(), preview.len());
    assert_eq!(parsed.to_bytes(), dat, "authored files must round-trip exactly");

    // 4. Export.
    let track = rbl_export::SourceTrack {
        source_path: audio_path,
        title: "Click 128".into(),
        artist: "Test".into(),
        album: "Fixtures".into(),
        key: analysis.key.as_ref().map(|k| k.name.clone()).unwrap_or_default(),
        bpm_x100: (detected * 100.0).round() as u32,
        duration_sec: audio.duration_secs() as u16,
        analysis: vec![("DAT".into(), dat)],
        ..rbl_export::SourceTrack::default()
    };
    let playlist = rbl_export::SourcePlaylist {
        name: "Fixture set".into(),
        track_indices: vec![0],
        ..Default::default()
    };
    let report = rbl_export::export(dest.path(), &[track], &[playlist]).unwrap();
    assert_eq!(report.tracks, 1);
    assert_eq!(report.analysis_files, 1);

    // 5. Read the export back the way a player would.
    let check = rbl_export::verify(dest.path()).unwrap();
    assert!(check.is_ok(), "{check:?}");
    assert_eq!(check.tracks, 1);
    assert_eq!(check.playlist_entries, 1);
    assert_eq!(check.analysis_present, 1);

    let bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
    assert_eq!(rows[0].title, "Click 128");
    let exported_bpm = f64::from(rows[0].tempo_x100) / 100.0;
    assert!((exported_bpm - 128.0).abs() < 1.0, "exported {exported_bpm}");

    // And the analysis the database points at must parse.
    let anlz_on_stick = dest.path().join(rows[0].analyze_path.trim_start_matches('/'));
    let reread = rbl_anlz::Anlz::read(&anlz_on_stick).unwrap();
    assert_eq!(reread.beat_grid().unwrap().len(), beats.len());
}

#[test]
fn a_track_that_cannot_be_decoded_does_not_break_the_pipeline() {
    let src = tempfile::tempdir().unwrap();
    let path = src.path().join("broken.wav");
    std::fs::write(&path, b"not audio").unwrap();
    assert!(rbl_audio::decode_mono(&path, None).is_err());

    // Exporting it still works: the file exists, so it is copied as-is.
    let dest = common::stick();
    let track = rbl_export::SourceTrack {
        source_path: path,
        title: "Broken".into(),
        ..rbl_export::SourceTrack::default()
    };
    let report = rbl_export::export(dest.path(), &[track], &[]).unwrap();
    assert_eq!(report.tracks, 1);
    assert!(rbl_export::verify(dest.path()).unwrap().is_ok());
}
