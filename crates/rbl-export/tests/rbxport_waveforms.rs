//! Generated analysis fixtures verify cue-list placement and preview bounds.
//! These byte-level checks do not establish physical player compatibility.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

/// The six generated signals rekordbox's native `LoudViewMaker` routine was
/// run on, and its exact `PWAV`/`PWV2` for each (`*.blue-preview`), shared
/// with `rbl-analysis`'s own tests.
#[path = "../../rbl-analysis/tests/rekordbox_waveform/signals.rs"]
mod signals;

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

#[test]
fn generated_analysis_keeps_its_waveform_and_cue_list_order_on_export() {
    let src = tempfile::tempdir().unwrap();
    let dest = common::stick();
    let track = analysed_track(src.path());
    export(dest.path(), &[track], &[]).unwrap();

    let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
    let dir = dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/'));
    let dat = rbl_anlz::Anlz::read(&dir.join("ANLZ0000.DAT")).unwrap();
    let ext = rbl_anlz::Anlz::read(&dir.join("ANLZ0000.EXT")).unwrap();
    let two = rbl_anlz::Anlz::read(&dir.join("ANLZ0000.2EX")).unwrap();

    // The analysis author places cue lists between the EXT waveform groups.
    assert_eq!(tags(&dat), ["PPTH", "PVBR", "PQTZ", "PWAV", "PWV2", "PCOB", "PCOB"]);
    assert_eq!(tags(&ext), ["PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "PWV5", "PWV4"]);
    assert_eq!(tags(&two), ["PPTH", "PWV7", "PWV6", "PWVC"]);

    let pwav = dat.waveform(b"PWAV").unwrap().1;
    let pwv2 = dat.waveform(b"PWV2").unwrap().1;
    assert_eq!((pwav.len(), pwv2.len()), (400, 100));
    assert!(pwav.iter().all(|byte| byte & 0x1f > 0));
    assert!(pwv2.iter().all(|byte| (1..=15).contains(byte)));
    assert!(ext.waveform(b"PWV3").is_some() && ext.waveform(b"PWV5").is_some() && ext.waveform(b"PWV4").is_some());
}

#[test]
fn replacing_cues_retains_the_first_list_slot_and_other_sections() {
    let src = tempfile::tempdir().unwrap();
    let dest = common::stick();
    let mut track = analysed_track(src.path());
    let cues = vec![rbl_anlz::cues::ExportCue { kind: 5, time_ms: 1_234, comment: "Hot D".into(), ..Default::default() }];
    track.cues = Some(cues.clone());
    let (_, ext_bytes) = track.analysis.iter_mut().find(|(extension, _)| extension == "EXT").unwrap();
    let mut ext = rbl_anlz::parse(ext_bytes).unwrap();
    let marker = rbl_anlz::Section::new(b"ZZZZ", vec![1, 2, 3, 4], vec![9, 8, 7]);
    ext.sections.insert(4, marker.clone());
    *ext_bytes = ext.to_bytes();
    export(dest.path(), &[track], &[]).unwrap();
    let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
    let saved = dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.EXT");
    let exported = rbl_anlz::Anlz::read(&saved).unwrap();
    assert_eq!(tags(&exported), ["PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "ZZZZ", "PWV5", "PWV4"]);
    assert_eq!(exported.sections[2..6], rbl_anlz::cues::sections(&cues, true));
    assert_eq!(exported.sections[6], marker);
}

#[test]
fn a_revision_three_receipt_reprocesses_cue_order_without_recopying_audio() {
    let src = tempfile::tempdir().unwrap();
    let dest = common::stick();
    let mut track = analysed_track(src.path());
    for (extension, bytes) in &track.analysis {
        std::fs::write(src.path().join(format!("library.{extension}")), bytes).unwrap();
    }
    let original_ext = std::fs::read(src.path().join("library.EXT")).unwrap();
    track.analysis.clear();
    track.analysis_source = Some(src.path().join("library.DAT"));
    export(dest.path(), std::slice::from_ref(&track), &[]).unwrap();
    let mut older = rbl_export::Manifest::load(dest.path()).unwrap();
    let saved = dest.path().join(older.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.EXT");
    let mut old_ext = rbl_anlz::Anlz::read(&saved).unwrap();
    let old_cues: Vec<_> = old_ext.sections.iter().filter(|section| section.is_cue_list()).cloned().collect();
    old_ext.sections.retain(|section| !section.is_cue_list());
    old_ext.sections.extend(old_cues);
    std::fs::write(&saved, old_ext.to_bytes()).unwrap();
    older.generation.as_mut().unwrap().transform_version = 3;
    older.save(dest.path()).unwrap();
    let refreshed = export(dest.path(), std::slice::from_ref(&track), &[]).unwrap();
    assert_eq!(refreshed.bytes_copied, 0);
    assert!(refreshed.metrics.analysis_bytes_read > 0);
    assert_eq!(tags(&rbl_anlz::Anlz::read(&saved).unwrap()), ["PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "PWV5", "PWV4"]);
    assert_eq!(std::fs::read(src.path().join("library.EXT")).unwrap(), original_ext);
    let current = rbl_export::Manifest::load(dest.path()).unwrap();
    assert_eq!(current.generation.as_ref().unwrap().transform_version, rbl_export::incremental::TRANSFORM_VERSION);
    let again = export(dest.path(), &[track], &[]).unwrap();
    assert_eq!((again.bytes_copied, again.analysis_files), (0, 0));
}

#[test]
fn replacing_cues_keeps_the_existing_native_reference_order() {
    let src = tempfile::tempdir().unwrap();
    let dest = common::stick();
    let mut track = analysed_track(src.path());
    let reference = include_bytes!("../../rbl-analysis/tests/rekordbox_waveform/kick-hats.EXT");
    let expected = tags(&rbl_anlz::parse(reference).unwrap());
    assert_eq!(expected, ["PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "PQT2", "PWV5", "PWV4", "PSSI"]);
    track.analysis.iter_mut().find(|(extension, _)| extension == "EXT").unwrap().1 = reference.to_vec();
    track.cues = Some(vec![rbl_anlz::cues::ExportCue { kind: 5, time_ms: 1_234, ..Default::default() }]);
    export(dest.path(), &[track], &[]).unwrap();
    let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
    let saved = dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.EXT");
    assert_eq!(tags(&rbl_anlz::Anlz::read(&saved).unwrap()), expected);
}

/// The exported half of a local/exported analysis pair (#822): each reference
/// signal is analysed as the app analyses it, authored as the app saves it,
/// and exported through the share-tree load path (`analysis_source`). The
/// stick's `.DAT` must carry the blue overviews byte for byte as the native
/// routine wrote them, in the section framing of a rekordbox-written `.DAT`.
/// A stage-by-stage check: `rbl-analysis`'s `blue_overview` test holds the
/// local bytes alone, so a failure here with that test green is the export's.
/// This does not observe rekordbox's own USB view; no fixture captures it.
#[test]
fn exported_blue_overviews_match_the_native_reference_and_rekordboxs_framing() {
    let reference_dat = rbl_anlz::parse(include_bytes!("../../rbl-link/tests/fixtures/ANLZ0001.DAT")).unwrap();
    let framing = |tag: &[u8; 4]| reference_dat.section(tag).unwrap().header.clone();
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rbl-analysis/tests/rekordbox_waveform");
    for signal in &signals::SIGNALS {
        let reference = std::fs::read(fixtures.join(format!("{}.blue-preview", signal.name))).unwrap();
        let mut creator = rbl_analysis::wave_creator::WaveCreator::new(signal.rate);
        creator.push(&(signal.frames)(signal.rate));
        let w = creator.finish();
        assert_eq!(w.pwav, reference[..400], "{}: local PWAV", signal.name);
        assert_eq!(w.pwv2, reference[400..], "{}: local PWV2", signal.name);
        let waves = rbl_anlz::RekordboxWaves { pwav: w.pwav, pwv2: w.pwv2, pwvc: w.pwvc, pwv3: w.pwv3, pwv4: w.pwv4, pwv5: w.pwv5, pwv6: w.pwv6, pwv7: w.pwv7 };
        let beats: Vec<_> = (0..40).map(|i| rbl_anlz::Beat { beat_number: (i % 4 + 1) as u16, tempo_x100: 12_500, time_ms: 120 + i * 480 }).collect();
        let name = format!("{}.wav", signal.name);
        let files = rbl_anlz::author_with_rekordbox_waves(&rbl_anlz::share_tree_ppth(&name), &beats, &waves, rbl_anlz::Existing::default());

        let src = tempfile::tempdir().unwrap();
        let dest = common::stick();
        let audio = src.path().join(&name);
        std::fs::write(&audio, vec![7u8; 4096]).unwrap();
        let base = src.path().join("ANLZ0000.DAT");
        std::fs::write(&base, files.dat).unwrap();
        std::fs::write(base.with_extension("EXT"), files.ext).unwrap();
        std::fs::write(base.with_extension("2EX"), files.two_ex).unwrap();
        let track = SourceTrack {
            source_path: audio, title: signal.name.into(), artist: "RBX".into(), album: "Reference".into(),
            duration_sec: 20, sample_rate: signal.rate, bpm_x100: 12_500, cues: Some(Vec::new()),
            analysis_source: Some(base), ..SourceTrack::default()
        };
        let report = export(dest.path(), &[track], &[]).unwrap();
        assert!(report.verified && report.failed.is_empty() && report.skipped.is_empty(), "{}", signal.name);

        let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
        let dat = rbl_anlz::Anlz::read(&dest.path().join(manifest.tracks[0].anlz_dir.trim_start_matches('/')).join("ANLZ0000.DAT")).unwrap();
        let pwav = dat.section(b"PWAV").unwrap();
        let pwv2 = dat.section(b"PWV2").unwrap();
        assert_eq!(pwav.payload, reference[..400], "{}: exported PWAV", signal.name);
        assert_eq!(pwv2.payload, reference[400..], "{}: exported PWV2", signal.name);
        assert_eq!(pwav.header, framing(b"PWAV"), "{}: PWAV header as a rekordbox-written .DAT frames it", signal.name);
        assert_eq!(pwv2.header, framing(b"PWV2"), "{}: PWV2 header as a rekordbox-written .DAT frames it", signal.name);
    }
}
