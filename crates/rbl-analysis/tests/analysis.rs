//! Analysis correctness against synthesised signals with known answers.
//!
//! Real audio is judged by the golden rig (`examples/golden.rs`) against
//! rekordbox's stamps; these tests pin the mechanics — grid arithmetic,
//! segment handling, renumbering, degenerate input — on signals whose answer
//! is known by construction.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_analysis::onset::{onset_envelope, OnsetEnvelope};
use rbl_analysis::tempo::{beats_of, detect_tempo, Segment};
use rbl_analysis::{analyse, key::detect_key};

const SR: u32 = 44_100;

/// A click track: one short burst per beat at a known tempo, starting at
/// `first_secs`.
fn click_track(bpm: f64, secs: f64, first_secs: f64) -> Vec<f32> {
    let total = (secs * f64::from(SR)) as usize;
    let period = 60.0 / bpm * f64::from(SR);
    let mut out = vec![0.0_f32; total];
    let mut at = first_secs * f64::from(SR);
    while (at as usize) < total {
        let start = at as usize;
        // 5 ms decaying burst of noise-ish content.
        for i in 0..(SR as usize / 200) {
            if start + i >= total {
                break;
            }
            let decay = 1.0 - i as f32 / (SR as f32 / 200.0);
            let phase = i as f32 * 0.7;
            out[start + i] += phase.sin() * decay * 0.8;
        }
        at += period;
    }
    out
}

/// A sine at a given frequency.
fn tone(freq: f64, secs: f64) -> Vec<f32> {
    let total = (secs * f64::from(SR)) as usize;
    (0..total)
        .map(|i| (i as f64 * 2.0 * std::f64::consts::PI * freq / f64::from(SR)).sin() as f32 * 0.5)
        .collect()
}

/// A chord, for key detection.
fn chord(freqs: &[f64], secs: f64) -> Vec<f32> {
    let total = (secs * f64::from(SR)) as usize;
    (0..total)
        .map(|i| {
            let t = i as f64 / f64::from(SR);
            let sum: f64 = freqs.iter().map(|f| (t * 2.0 * std::f64::consts::PI * f).sin()).sum();
            (sum / freqs.len() as f64) as f32 * 0.5
        })
        .collect()
}

// ---------------------------------------------------------------- tempo

#[test]
fn finds_the_tempo_of_a_click_track() {
    for bpm in [120.0, 128.0, 140.0, 174.0] {
        let audio = click_track(bpm, 30.0, 0.5);
        let result = detect_tempo(&onset_envelope(&audio, SR));
        // Allow an octave error, then require the tempo itself to be close.
        let folded = rbl_analysis::tempo::nearest_octave(result.bpm, bpm);
        assert!((folded - bpm).abs() < 0.05, "expected ~{bpm}, got {} (folded {folded})", result.bpm);
    }
}

#[test]
fn the_beat_grid_lands_on_the_clicks_and_starts_at_the_file() {
    let bpm = 128.0;
    // The first click is a beat and a half in, so the grid must reach back
    // to the file's start: rekordbox starts a grid at the first grid
    // position after zero, not at the first onset.
    let first = 1.5 * 60.0 / bpm;
    let audio = click_track(bpm, 30.0, first);
    let result = detect_tempo(&onset_envelope(&audio, SR));
    assert!(!result.beats.is_empty(), "grid should not be empty");
    assert_eq!(result.segments.len(), 1, "one tempo, one segment");

    let period_ms = 60_000.0 / bpm;
    let start = f64::from(result.beats[0].time_ms);
    assert!(start < period_ms, "the first beat is inside the first period, got {start} ms");
    // Every click is on a beat, to within a few milliseconds.
    let mut at = first * 1000.0;
    while at < 29_000.0 {
        let nearest = result.beats.iter().map(|b| (f64::from(b.time_ms) - at).abs()).fold(f64::INFINITY, f64::min);
        assert!(nearest < 8.0, "click at {at:.0} ms is {nearest:.1} ms from the nearest beat");
        at += period_ms;
    }
    // Beat numbers cycle 1..4 from the first beat.
    assert_eq!(result.beats[0].beat_number, 1);
    assert_eq!(result.beats[4].beat_number, 1);
    assert_eq!(result.beats[5].beat_number, 2);
}

#[test]
fn silence_yields_no_tempo_rather_than_a_wrong_one() {
    let silence = vec![0.0_f32; SR as usize * 5];
    let result = detect_tempo(&onset_envelope(&silence, SR));
    assert!(result.confidence < 0.5, "silence should not look confident");
}

#[test]
fn very_short_audio_does_not_panic() {
    for len in [0_usize, 1, 100, 1023] {
        let audio = vec![0.1_f32; len];
        let result = detect_tempo(&onset_envelope(&audio, SR));
        assert_eq!(result.beats.len(), 0);
        let _ = analyse(&audio, SR);
    }
}

/// An onset envelope shaped like a real track's rather than a metronome's.
///
/// A bare impulse train is measured to a thousandth of a BPM by almost any
/// method, so it tests nothing. Real onsets are broad, sit on a noise floor,
/// and share the bar with off-beat percussion — and those are exactly the
/// conditions under which a coarse phase search picks the wrong period.
fn realistic_envelope(bpm: f64, seconds: f64) -> OnsetEnvelope {
    let rate = 44_100.0 / rbl_analysis::onset::HOP as f64;
    let count = (rate * seconds) as usize;
    let period = rate * 60.0 / bpm;
    let mut values = vec![0.0_f32; count];

    // A deterministic noise floor: a test that fails one run in ten is worse
    // than no test.
    let mut seed = 0x2545_F491_4F6C_DD1D_u64;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / 16_777_216.0
    };
    for value in &mut values {
        *value = noise() * 0.15;
    }

    // Onsets spread over a few samples, as a kick is: a triangular bump
    // centred on the true beat, which may fall between two samples.
    const SPREAD: f64 = 2.5;
    let add = |at: f64, gain: f32, values: &mut Vec<f32>| {
        let centre = at.round() as i64;
        for offset in -2_i64..=2 {
            let i = centre + offset;
            if i < 0 {
                continue;
            }
            let distance = (i as f64 - at).abs();
            if distance > SPREAD {
                continue;
            }
            let shape = (1.0 - distance / SPREAD) as f32;
            if let Some(slot) = values.get_mut(i as usize) {
                *slot += gain * shape;
            }
        }
    };

    // Beats are not all the same weight: a bar goes kick, hat, snare, hat, and
    // that unevenness is what makes the phase a candidate period is scored at
    // matter at all. A uniform impulse train is measured perfectly by anything.
    const BAR: [f32; 4] = [1.0, 0.55, 0.8, 0.5];
    let mut beat = 0_u32;
    let mut at = period;
    while at < count as f64 {
        add(at, BAR[(beat % 4) as usize], &mut values);
        // A hi-hat between the beats, which is what made an earlier half-time
        // correction fire on tracks that were already right.
        add(at + period / 2.0, 0.45, &mut values);
        beat += 1;
        at = period * f64::from(beat + 1);
    }
    OnsetEnvelope { values, rate, origin_secs: 0.0 }
}

#[test]
fn a_real_shaped_beat_is_measured_to_within_the_gate() {
    // 0.05 BPM is the gate the golden rig judges against. These tempos land
    // between whole envelope samples, which is the case the refinement's
    // phase search exists for. 174.3 used to come back as 116.20, two thirds
    // of it: the Fourier term is what settles that, since a signal periodic
    // at 174 has no component at 116.
    //
    // Nothing below 100 BPM is in the list on purpose. This envelope's
    // off-beat hats carry half the weight of its beats, and at 92.5 BPM the
    // estimator reads it as 185 with alternating accents — the same reading
    // that puts drum & bass at 174 rather than 87, which is what rekordbox
    // does and what the golden playlist demands. Whether a slow track with
    // hats that heavy is slow or fast is a convention, not a measurement.
    for bpm in [128.0, 150.25, 140.86, 174.3] {
        let envelope = realistic_envelope(bpm, 120.0);
        let result = detect_tempo(&envelope);
        let error = (result.bpm - bpm).abs();
        let table: Vec<String> = rbl_analysis::tempo::tempo_candidates(&envelope, rbl_analysis::tempo::TempoOptions::default())
            .iter().take(6).map(|c| format!("{:.2}: acf {:.3} fourier {:.3} prior {:.3} score {:.4}", c.bpm, c.acf, c.fourier, c.prior, c.score)).collect();
        assert!(error <= 0.05, "at {bpm} BPM we said {} (off by {error:.3})\n{}", result.bpm, table.join("\n"));
    }
}

#[test]
fn a_tempo_change_becomes_a_second_segment_with_the_count_running_on() {
    // 60 seconds at 128, then 60 at 140: a DJ edit.
    let mut audio = click_track(128.0, 60.0, 0.2);
    let tail = click_track(140.0, 60.0, 0.1);
    audio.extend_from_slice(&tail);
    let result = detect_tempo(&onset_envelope(&audio, SR));
    assert_eq!(result.segments.len(), 2, "segments: {:?}", result.segments);
    let (a, b) = (result.segments[0], result.segments[1]);
    assert!((a.bpm() - 128.0).abs() < 0.05, "first segment {}", a.bpm());
    assert!((b.bpm() - 140.0).abs() < 0.05, "second segment {}", b.bpm());
    assert!((b.from_secs - 60.1).abs() < 0.5, "the change is at 60.1 s, got {}", b.from_secs);
    assert!((a.to_secs - b.from_secs).abs() < 1e-9, "segments abut");
    // The library shows the tempo the track starts at.
    assert!((result.bpm - 128.0).abs() < 0.05);
    // The count runs on across the change rather than restarting.
    let first_of_b = a.beats();
    let expected = (first_of_b % 4 + 1) as u16;
    assert_eq!(result.beats[first_of_b].beat_number, expected);
    assert!((result.beats[first_of_b].tempo_x100 as f64 - 14_000.0).abs() < 5.0);
}

#[test]
fn a_rhythm_at_a_simple_ratio_is_not_a_tempo_change() {
    // A dotted-eighth delay for a stretch of the track: a real period at
    // four thirds of the beat, which must not split the grid.
    let bpm = 128.0;
    let mut audio = click_track(bpm, 90.0, 0.2);
    let dotted = click_track(bpm * 4.0 / 3.0, 30.0, 0.0);
    let at = 30 * SR as usize;
    for (i, v) in dotted.iter().enumerate() {
        if let Some(slot) = audio.get_mut(at + i) {
            *slot += v * 0.7;
        }
    }
    let result = detect_tempo(&onset_envelope(&audio, SR));
    assert_eq!(result.segments.len(), 1, "segments: {:?}", result.segments);
    assert!((result.bpm - bpm).abs() < 0.05, "tempo {}", result.bpm);
}

#[test]
fn the_attack_map_places_a_click_to_the_millisecond() {
    use rbl_analysis::attack::{AttackMap, AttackOptions};
    // Clicks at 120 BPM from 0.25 s: each a burst of 3 kHz.
    let mut audio = vec![0.0_f32; SR as usize * 6];
    let period = SR as usize / 2;
    let mut at = SR as usize / 4;
    while at + 400 < audio.len() {
        for i in 0..400 {
            let decay = (-(i as f32) / 80.0).exp();
            audio[at + i] += (i as f32 * 3000.0 * std::f32::consts::TAU / SR as f32).sin() * decay * 0.8;
        }
        at += period;
    }
    let map = AttackMap::new(&audio, SR, AttackOptions::default());
    // Asked near each click, the attack comes back within 2 ms of it.
    for k in 0..8 {
        let click = 0.25 + k as f64 * 0.5;
        let attack = map.attack_near(click + 0.012).expect("an attack");
        assert!((attack.secs - click).abs() < 0.002, "click at {click:.3}, attack at {:.4}", attack.secs);
        assert!(attack.height > 0.0);
    }
    // Between clicks there is nothing to find within a short reach.
    assert!(map.attack_within(0.5, 0.02).is_none());
}

#[test]
fn a_gradual_tempo_change_preserves_each_walked_beat() {
    // 32 bars at 128, then the period shrinks by 0.3 % per beat for 64
    // beats (to ~155), then 32 bars settled at that tempo.
    let mut clicks: Vec<f64> = Vec::new();
    let mut t = 0.2;
    let mut period = 60.0 / 128.0;
    for _ in 0..128 { clicks.push(t); t += period; }
    for _ in 0..64 { period *= 0.997; clicks.push(t); t += period; }
    let settled = period;
    for _ in 0..128 { clicks.push(t); t += settled; }
    let total = (t + 1.0) * f64::from(SR);
    let mut audio = vec![0.0_f32; total as usize];
    for &c in &clicks {
        let at = (c * f64::from(SR)) as usize;
        for i in 0..(SR as usize / 200) {
            if let Some(slot) = audio.get_mut(at + i) {
                let decay = 1.0 - i as f32 / (SR as f32 / 200.0);
                *slot += ((i as f32) * 0.7).sin() * decay * 0.8;
            }
        }
    }
    let analysis = analyse(&audio, SR);
    let segments = &analysis.tempo.segments;
    let onsets = onset_envelope(&audio, SR);
    let attacks = rbl_analysis::attack::AttackMap::new(&audio, SR, rbl_analysis::attack::AttackOptions::default());
    let (walk, why) = rbl_analysis::tempo::walk_report(&onsets, Some(&attacks), 128.0, 60.0 / settled, 55.0, 100.0);
    let walked: Vec<String> = walk.iter().map(|(t, b)| format!("{t:.2}:{b:.1}")).collect();
    assert!(
        segments.len() >= 3,
        "expected a first tempo, walked bars and a last tempo, got {} segments: {:?}\nwalk ({why}): {}",
        segments.len(),
        segments.iter().map(|s| format!("{:.1}-{:.1}s {:.2}", s.from_secs, s.to_secs, s.bpm())).collect::<Vec<_>>(),
        walked.join(" ")
    );
    let first = segments[0];
    let last = segments[segments.len() - 1];
    assert!((first.bpm() - 128.0).abs() < 0.05, "first {}", first.bpm());
    assert!((last.bpm() - 60.0 / settled).abs() < 0.5, "last {} vs {}", last.bpm(), 60.0 / settled);
    // Keep each measured beat rather than averaging four intervals into a bar.
    let walked = &segments[1..segments.len() - 1];
    assert!(walked.len() >= 8, "walked {} bars", walked.len());
    for pair in walked.windows(2) {
        assert!(pair[1].bpm() >= pair[0].bpm() - 1.0, "beats fall back beyond timing quantization: {} then {}", pair[0].bpm(), pair[1].bpm());
        assert_eq!(pair[0].beats(), 1);
    }
    // Every click is on a beat of the final grid.
    for &c in &clicks {
        let nearest = analysis.tempo.beats.iter().map(|b| (f64::from(b.time_ms) / 1000.0 - c).abs()).fold(f64::INFINITY, f64::min);
        assert!(nearest < 0.012, "click at {c:.3} is {:.1} ms from a beat", nearest * 1000.0);
    }
}

#[test]
fn segments_generate_beats_and_shift_by_half_a_beat() {
    let segment = Segment { from_secs: 0.0, to_secs: 10.0, period_secs: 0.5, phase_secs: 1.3 };
    // The grid is phase plus whole periods; the first at or after zero.
    assert!((segment.start_secs() - 0.3).abs() < 1e-9);
    assert_eq!(segment.beats(), 20);
    let beats = beats_of(&[segment], 0);
    assert_eq!(beats.len(), 20);
    assert_eq!(beats[0].time_ms, 300);
    assert_eq!(beats[1].time_ms, 800);
    assert_eq!(beats[0].beat_number, 1);
    assert_eq!(beats[3].beat_number, 4);
    assert_eq!(beats[4].beat_number, 1);
    assert_eq!(beats[0].tempo_x100, 12_000);
    // Numbered with a phase: two beats precede the first downbeat.
    let numbered = beats_of(&[segment], 2);
    assert_eq!(numbered[0].beat_number, 3);
    assert_eq!(numbered[2].beat_number, 1);
    // Half a beat later: the first beat wraps to before the old one.
    let shifted = segment.shifted_half_beat();
    assert!((shifted.start_secs() - 0.05).abs() < 1e-9);
    assert_eq!(shifted.beats(), 20);
    // An empty span has no beats and does not panic.
    let empty = Segment { from_secs: 5.0, to_secs: 5.0, period_secs: 0.5, phase_secs: 0.0 };
    assert_eq!(empty.beats(), 0);
    let degenerate = Segment { from_secs: 0.0, to_secs: 5.0, period_secs: 0.0, phase_secs: 0.0 };
    assert_eq!(degenerate.beats(), 0);
}

#[test]
fn a_change_on_a_beat_starts_with_it_and_drops_the_old_grid_s_same_hit() {
    // The old grid's beat falls 2 ms before the change, on the same hit as
    // the new grid's first beat: rekordbox's hand grids keep the new one.
    let a = Segment { from_secs: 0.0, to_secs: 10.002, period_secs: 0.5, phase_secs: 0.0 };
    let b = Segment { from_secs: 10.002, to_secs: 20.0, period_secs: 0.4, phase_secs: 10.002 };
    assert_eq!(a.beats(), 21, "the old grid alone still counts its beat at 10.000 s");
    assert!((b.start_secs() - 10.002).abs() < 1e-9, "a segment starting on its own beat starts with it");
    let beats = beats_of(&[a, b], 0);
    assert_eq!(beats[19].time_ms, 9_500);
    assert_eq!(beats[20].time_ms, 10_002, "the old grid's 10.000 s beat is dropped for the new grid's");
    assert_eq!(beats[20].tempo_x100, 15_000);
    // The count carries on across the join.
    assert_eq!(beats[20].beat_number, 1);
    // A change more than half a period after the old grid's last beat
    // keeps that beat.
    let c = Segment { from_secs: 0.0, to_secs: 10.3, period_secs: 0.5, phase_secs: 0.0 };
    let d = Segment { from_secs: 10.3, to_secs: 20.0, period_secs: 0.4, phase_secs: 10.3 };
    let kept = beats_of(&[c, d], 0);
    assert_eq!(kept[20].time_ms, 10_000);
    assert_eq!(kept[21].time_ms, 10_300);
}

/// A click track at `bpm` whose every click is moved by up to `jitter_secs`
/// either way, the same pseudo-random way every run: a sequenced tempo
/// played with a little looseness, which a fitted line lands near but not
/// exactly on.
fn jittered_click_track(bpm: f64, secs: f64, first_secs: f64, jitter_secs: f64) -> Vec<f32> {
    let total = (secs * f64::from(SR)) as usize;
    let period = 60.0 / bpm * f64::from(SR);
    let mut out = vec![0.0_f32; total];
    let mut seed: u32 = 0x2545_f491;
    let mut beat = first_secs * f64::from(SR);
    while (beat as usize) < total {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let unit = f64::from(seed >> 8) / f64::from(1_u32 << 24) * 2.0 - 1.0;
        let start = (beat + unit * jitter_secs * f64::from(SR)).max(0.0) as usize;
        for i in 0..(SR as usize / 200) {
            let Some(slot) = out.get_mut(start + i) else { break };
            let decay = 1.0 - i as f32 / (SR as f32 / 200.0);
            *slot += (i as f32 * 0.7).sin() * decay * 0.8;
        }
        beat += period;
    }
    out
}

#[test]
fn a_steady_tempo_near_a_whole_number_is_that_whole_number() {
    // Dance music is produced at whole tempos. Clicks at 138 played a few
    // milliseconds loose put the fitted line a little off 138; the line at
    // 138 sits on as many of them, so the tempo is 138.
    let audio = jittered_click_track(138.0, 120.0, 0.2, 0.004);
    let result = analyse(&audio, SR).tempo;
    assert!((result.bpm - 138.0).abs() < 1e-6, "expected exactly 138, got {}", result.bpm);
    assert!(result.beats.iter().all(|b| b.tempo_x100 == 13_800));
    // And one well away from a whole number is left where it was measured.
    let audio = click_track(127.6, 120.0, 0.2);
    let result = analyse(&audio, SR).tempo;
    assert!((result.bpm - 127.6).abs() < 0.05, "expected about 127.6, got {}", result.bpm);
}

#[test]
fn a_steady_tempo_a_few_hundredths_off_a_whole_number_keeps_its_measured_tempo() {
    // Rekordbox measures drum & bass tracks at 173.97 or 174.01 and tracks
    // from bands and turntables at 107.95, and keeps those tempos: it does
    // not round to a whole number. Rounding 173.97 to 174 would put the
    // grid 52 ms early by the end of five minutes, a sixth of a beat.
    for bpm in [173.97, 107.95] {
        let audio = click_track(bpm, 300.0, 0.2);
        let result = analyse(&audio, SR).tempo;
        assert!((result.bpm - bpm).abs() < 0.005, "expected {bpm}, got {}", result.bpm);
        assert_eq!(result.segments.len(), 1, "segments: {:?}", result.segments);
        let period = 60.0 / bpm;
        let last = result.beats.last().expect("a grid");
        let last_secs = f64::from(last.time_ms) / 1000.0;
        let click = 0.2 + ((last_secs - 0.2) / period).round() * period;
        assert!((last_secs - click).abs() < 0.005, "{bpm}: last beat {last_secs:.3}s, its click at {click:.3}s");
    }
}

#[test]
fn the_normal_preset_is_one_constant_grid_as_rekordbox_writes_it() {
    use rbl_analysis::{analyse_with, AnalysisPreset};
    // A DJ edit: 60 seconds at 128, then 60 at 140. RBXport follows the
    // change; rekordbox's Normal analysis is one tempo for the whole track.
    let mut audio = click_track(128.0, 60.0, 0.2);
    audio.extend_from_slice(&click_track(140.0, 60.0, 0.1));
    let followed = analyse_with(&audio, SR, AnalysisPreset::Rbxport.options()).tempo;
    assert_eq!(followed.segments.len(), 2, "segments: {:?}", followed.segments);
    let normal = analyse_with(&audio, SR, AnalysisPreset::Rekordbox.options()).tempo;
    assert_eq!(normal.segments.len(), 1, "segments: {:?}", normal.segments);
    let segment = normal.segments[0];
    assert_eq!(segment.from_secs, 0.0);
    assert!(segment.to_secs >= 119.9, "the grid runs to the end: {segment:?}");
    let first = normal.beats.first().map_or(0, |b| b.tempo_x100);
    assert!(normal.beats.iter().all(|b| b.tempo_x100 == first), "one tempo throughout");
}

#[test]
fn the_envelope_is_timestamped_at_the_frame_centre() {
    let audio = click_track(120.0, 5.0, 0.0);
    let envelope = onset_envelope(&audio, SR);
    let half_frame = rbl_analysis::onset::FRAME as f64 / 2.0 / f64::from(SR);
    assert!((envelope.origin_secs - half_frame).abs() < 1e-9);
    assert!((envelope.time_of(0.0) - half_frame).abs() < 1e-9);
    assert!((envelope.time_of(envelope.rate) - half_frame - 1.0).abs() < 1e-9, "one rate's worth of samples is one second");
}

// ---------------------------------------------------------------- downbeat

#[test]
fn the_downbeat_is_where_the_music_changes() {
    // Kicks on every beat for 64 bars, with a bass tone that changes pitch
    // every eight bars — from the third beat of the first bar on, so the
    // phrase boundaries land two beats after the grid's first beat.
    let bpm = 128.0;
    let period = 60.0 / bpm;
    let bars = 64;
    let secs = bars as f64 * 4.0 * period + 1.0;
    let mut audio = click_track(bpm, secs, 0.0);
    let total = audio.len();
    let phrase_secs = 8.0 * 4.0 * period;
    let offset = 2.0 * period;
    let notes = [55.0, 73.4, 65.4, 82.4];
    for (i, sample) in audio.iter_mut().enumerate().take(total) {
        let t = i as f64 / f64::from(SR);
        let phrase = ((t - offset) / phrase_secs).floor().max(0.0) as usize;
        let freq = notes[phrase % notes.len()];
        *sample += (t * 2.0 * std::f64::consts::PI * freq).sin() as f32 * 0.3;
    }
    let analysis = analyse(&audio, SR);
    let first_down = analysis.tempo.beats.iter().find(|b| b.beat_number == 1).unwrap();
    let expected_ms = offset * 1000.0;
    assert!(
        (f64::from(first_down.time_ms) - expected_ms).abs() < 30.0,
        "first downbeat at {} ms, expected ~{expected_ms:.0}",
        first_down.time_ms
    );
}

// ---------------------------------------------------------------- key

#[test]
fn detects_the_key_of_a_scale() {
    // A bare triad is genuinely ambiguous — A-C-E also sits inside F major —
    // so establish the key the way music does, with the full scale.
    // A natural minor: A B C D E F G A.
    let scale = [220.0, 246.94, 261.63, 293.66, 329.63, 349.23, 392.00, 440.0];
    let mut audio = Vec::new();
    for _ in 0..3 {
        for &f in &scale {
            audio.extend(chord(&[f, f * 2.0], 0.5));
        }
    }
    // Land on the tonic chord, as a phrase does.
    audio.extend(chord(&[220.0, 261.63, 329.63], 3.0));
    let key = detect_key(&audio, SR).expect("a key");
    assert_eq!(key.name, "Am", "got {}", key.name);
    assert_eq!(key.camelot(), "8A");
}

#[test]
fn a_major_scale_reads_as_major_despite_the_minor_bias() {
    // C major: C D E F G A B C, landing on the tonic chord.
    let scale = [261.63, 293.66, 329.63, 349.23, 392.00, 440.0, 493.88, 523.25];
    let mut audio = Vec::new();
    for _ in 0..3 {
        for &f in &scale {
            audio.extend(chord(&[f, f * 2.0], 0.5));
        }
    }
    audio.extend(chord(&[261.63, 329.63, 392.00], 3.0));
    let key = detect_key(&audio, SR).expect("a key");
    assert_eq!(key.name, "C", "got {}", key.name);
}

#[test]
fn camelot_codes_follow_the_wheel() {
    use rbl_analysis::key::MusicalKey;
    let cases = [("Abm", 8, true, "1A"), ("B", 11, false, "1B"), ("Am", 9, true, "8A"), ("C", 0, false, "8B"), ("Ebm", 3, true, "2A")];
    for (name, tonic, minor, code) in cases {
        let key = MusicalKey { name: name.into(), tonic, minor };
        assert_eq!(key.camelot(), code, "{name}");
    }
}

#[test]
fn a_pure_tone_has_no_meaningful_key_but_does_not_panic() {
    let audio = tone(440.0, 3.0);
    let _ = detect_key(&audio, SR);
    let silence = vec![0.0_f32; SR as usize * 3];
    assert!(detect_key(&silence, SR).is_none(), "silence has no key");
}

#[test]
fn the_key_rules_apply_in_order_and_report_what_they_changed() {
    use rbl_analysis::key::{judge, BassSource, KeyEvidence, KeyOptions, Rule, Verdict};
    // A chroma that reads as C major and C minor almost equally: the notes
    // of both (C D F G A B) with the third left out.
    let mut chroma = [0.0_f64; 12];
    for class in [0, 2, 5, 7, 9, 11] {
        chroma[class] = 1.0;
    }
    chroma[0] = 2.0;
    chroma[7] = 1.5;
    let mut bass = [[0.0_f64; 12]; 7];
    // The bass between beats says G.
    bass[4][7] = 1.0;
    let evidence = KeyEvidence { chroma, bass };
    let options = KeyOptions::default();

    // No rules: the profile match alone.
    let plain = judge(&evidence, options, &[]).unwrap();
    assert!(plain.applied.is_empty());
    assert_eq!(plain.matched.tonic, 0);

    // A toss-up goes to the minor, and the report says which rule did it.
    let minor = judge(&evidence, options, &[Rule::PreferMinor { bias: 0.3 }]).unwrap();
    assert_eq!(minor.key.name, "Cm", "got {}", minor.key.name);
    assert_eq!(minor.applied.len(), 1);
    assert_eq!(minor.applied[0].after, Verdict { tonic: 0, minor: true });

    // The bass rule fires only under its margin: a huge margin lets it
    // rename the tonic, a zero margin never fires.
    let fired = judge(&evidence, options, &[Rule::BassRoot { margin: 10.0, source: BassSource::SecondEighth }]).unwrap();
    assert_eq!(fired.key.tonic, 7, "got {}", fired.key.name);
    let quiet = judge(&evidence, options, &[Rule::BassRoot { margin: 0.0, source: BassSource::SecondEighth }]).unwrap();
    assert!(quiet.applied.is_empty());
    assert_eq!(quiet.key.tonic, 0);

    // A vote big enough moves the tonic too.
    let voted = judge(&evidence, options, &[Rule::BassVote { weight: 10.0, source: BassSource::SecondEighth }]).unwrap();
    assert_eq!(voted.key.tonic, 7);

    // Silence has no key, whatever the rules.
    let silent = KeyEvidence { chroma: [0.0; 12], bass };
    assert!(judge(&silent, options, &[Rule::PreferMinor { bias: 0.3 }]).is_none());
}

#[test]
fn the_tuning_offset_follows_a_detuned_track() {
    use rbl_analysis::key::{chroma_frames, tuning_offset, KeyOptions};
    let options = KeyOptions::default();
    let in_tune = chord(&[220.0, 261.63, 329.63], 4.0);
    assert_eq!(tuning_offset(&chroma_frames(&in_tune, SR, options).unwrap()), 0);
    // A third of a semitone sharp.
    let sharp: Vec<f64> = [220.0, 261.63, 329.63].iter().map(|f| f * 2.0_f64.powf(1.0 / 36.0)).collect();
    let detuned = chord(&sharp, 4.0);
    assert_eq!(tuning_offset(&chroma_frames(&detuned, SR, options).unwrap()), 1);
}

// ---------------------------------------------------------------- waveform and levels

#[test]
fn the_waveform_has_one_column_per_150th_of_a_second() {
    let audio = tone(440.0, 2.0);
    let waveform = rbl_analysis::waveform::compute(&audio, SR);
    assert_eq!(waveform.columns_per_sec, 150.0);
    assert!((waveform.columns.len() as f64 - 300.0).abs() <= 1.0);
}

#[test]
fn the_waveform_separates_bands() {
    let low = tone(60.0, 2.0);
    let high = tone(6000.0, 2.0);
    let lw = rbl_analysis::waveform::compute(&low, SR);
    let hw = rbl_analysis::waveform::compute(&high, SR);
    let mean = |cols: &[rbl_analysis::WaveformColumn], f: fn(&rbl_analysis::WaveformColumn) -> u8| {
        cols.iter().skip(30).map(|c| f64::from(f(c))).sum::<f64>() / cols.len().saturating_sub(30).max(1) as f64
    };
    assert!(mean(&lw.columns, |c| c.low) > mean(&lw.columns, |c| c.high) * 3.0, "a 60 Hz tone is low");
    assert!(mean(&hw.columns, |c| c.high) > mean(&hw.columns, |c| c.low) * 3.0, "a 6 kHz tone is high");
}

#[test]
fn peak_and_rms_are_measured() {
    let audio = tone(440.0, 1.0);
    let analysis = analyse(&audio, SR);
    assert!((analysis.peak - 0.5).abs() < 0.01, "peak {}", analysis.peak);
    // RMS of a sine is peak / sqrt(2).
    assert!((analysis.rms - 0.5 / 2.0_f32.sqrt()).abs() < 0.01, "rms {}", analysis.rms);
}

#[test]
fn phrase_and_vocal_detection_report_that_they_are_unimplemented() {
    use rbl_analysis::phrase::{PhraseAnalyzer, Unimplemented as Phrases};
    use rbl_analysis::vocal::{VocalDetector, Unimplemented as Vocals};
    let audio = tone(440.0, 1.0);
    // These must return None rather than invent structure that a DJ would see.
    assert!(Phrases.phrases(&audio, SR).is_none());
    assert!(Vocals.vocals(&audio, SR).is_none());
}

// ---------------------------------------------------------------- bands

/// Impulses at `bpm`, each a burst of a single frequency.
fn tone_clicks(bpm: f64, hz: f32, seconds: f64, sample_rate: u32) -> Vec<f32> {
    let count = (f64::from(sample_rate) * seconds) as usize;
    let period = f64::from(sample_rate) * 60.0 / bpm;
    let mut samples = vec![0.0_f32; count];
    let mut at = period;
    while at < count as f64 {
        let start = at as usize;
        // A short burst, windowed so it is an onset rather than a step.
        for i in 0..1024 {
            let Some(slot) = samples.get_mut(start + i) else { break };
            let t = i as f32 / sample_rate as f32;
            let decay = (-20.0_f32 * t).exp();
            *slot += (t * hz * std::f32::consts::TAU).sin() * decay;
        }
        at += period;
    }
    samples
}

/// Mean envelope value at the positions `period` samples apart from `offset`.
fn energy_at(envelope: &OnsetEnvelope, beat_secs: f64, offset: f64) -> f32 {
    let mut total = 0.0_f32;
    let mut count = 0_usize;
    let mut at = beat_secs + offset;
    while ((at - envelope.origin_secs) * envelope.rate) < envelope.values.len() as f64 {
        // The nearest envelope sample, plus its neighbours: an onset spans a
        // couple of hops and the grid does not land exactly on one.
        let centre = ((at - envelope.origin_secs) * envelope.rate).max(0.0) as usize;
        let mut peak = 0.0_f32;
        for i in centre.saturating_sub(2)..=(centre + 2) {
            peak = peak.max(envelope.values.get(i).copied().unwrap_or(0.0));
        }
        total += peak;
        count += 1;
        at += beat_secs;
    }
    if count == 0 { 0.0 } else { total / count as f32 }
}

#[test]
fn a_low_band_envelope_hears_the_kick_and_not_the_hi_hat() {
    use rbl_analysis::onset::{onset_envelope_band, Band};

    // A kick on the beat, a hi-hat exactly between the beats. A full-band
    // envelope sees an onset every half beat; below 200 Hz the hat is gone.
    let rate = 44_100;
    let beat_secs = 0.5; // 120 BPM
    let mut mixed = tone_clicks(120.0, 60.0, 10.0, rate);
    let hats = tone_clicks(120.0, 6_000.0, 10.0, rate);
    let half = (beat_secs / 2.0 * f64::from(rate)) as usize;
    for (i, value) in hats.iter().enumerate() {
        if let Some(slot) = mixed.get_mut(i + half) {
            *slot += *value;
        }
    }

    // The envelope is peak-normalised, so what matters is the ratio between
    // the on-beat and off-beat positions inside one envelope, never the total
    // of one envelope against another's.
    let full = onset_envelope_band(&mixed, rate, Band::FULL);
    let low = onset_envelope_band(&mixed, rate, Band::LOW);

    let full_ratio = energy_at(&full, beat_secs, beat_secs / 2.0) / energy_at(&full, beat_secs, 0.0);
    let low_ratio = energy_at(&low, beat_secs, beat_secs / 2.0) / energy_at(&low, beat_secs, 0.0);

    assert!(full_ratio > 0.4, "across the whole band the hi-hat should look much like the kick, got {full_ratio:.3}");
    assert!(
        low_ratio < full_ratio / 2.0,
        "below 200 Hz the hi-hat should mostly be gone: off-beat/on-beat {low_ratio:.3} against {full_ratio:.3}"
    );
}

#[test]
fn the_full_band_is_what_the_plain_call_still_does() {
    use rbl_analysis::onset::{onset_envelope_band, Band};

    let rate = 44_100;
    let mixed = tone_clicks(120.0, 60.0, 6.0, rate);
    let plain = onset_envelope(&mixed, rate);
    let explicit = onset_envelope_band(&mixed, rate, Band::FULL);
    assert_eq!(plain.values, explicit.values, "adding a band must not move the default");
}

/// The user's two-minute WAV, reproduced as PCM16 samples without an external fixture.
fn two_minute_ticks(first_sample: usize) -> Vec<f32> {
    let mut samples = vec![0.0; 48_000 * 120];
    for start in (first_sample..samples.len()).step_by(22_500) {
        for j in 0..240.min(samples.len() - start) {
            let value = 24_000.0 * (-(j as f64) / 45.0).exp()
                * (2.0 * std::f64::consts::PI * 4_000.0 * j as f64 / 48_000.0).sin();
            samples[start + j] = value.round() as i16 as f32 / 32_768.0;
        }
    }
    samples
}

#[test]
fn rbxport_two_minute_ticks_start_with_beat_one_at_exactly_zero() {
    let result = rbl_analysis::analyse_with(&two_minute_ticks(0), 48_000, rbl_analysis::AnalysisPreset::Rbxport.options());
    let grid = result.tempo;
    assert!((grid.bpm - 128.0).abs() < 0.01, "{grid:?}");
    assert_eq!(grid.first_beat_secs, 0.0);
    assert_eq!(grid.segments[0].start_secs(), 0.0);
    assert_eq!(grid.beats.len(), 256);
    for (i, beat) in grid.beats.iter().enumerate() {
        assert_eq!(beat.time_ms, (i as f64 * 468.75).round() as u32);
        assert_eq!(beat.beat_number, (i % 4 + 1) as u16);
    }
}

#[test]
fn file_start_gate_preserves_silence_and_later_attacks() {
    // An entire missing opening beat, a nearby later onset, and a grid
    // nowhere near zero must all keep their first beat after zero.
    for first_sample in [22_500, 480, 6_000] {
        let result = rbl_analysis::analyse_with(
            &two_minute_ticks(first_sample), 48_000, rbl_analysis::AnalysisPreset::Rbxport.options(),
        );
        assert!(result.tempo.first_beat_secs > 0.0, "offset {first_sample}");
        assert!(result.tempo.beats[0].time_ms > 0, "offset {first_sample}");
    }
}

#[test]
fn file_start_gate_handles_a_low_frequency_beat() {
    let mut samples = vec![0.0; 48_000 * 30];
    for start in (0..samples.len()).step_by(22_500) {
        for j in 0..4_800.min(samples.len() - start) {
            samples[start + j] = (0.8 * (-(j as f64) / 900.0).exp()
                * (2.0 * std::f64::consts::PI * 80.0 * j as f64 / 48_000.0).sin()) as f32;
        }
    }
    let result = rbl_analysis::analyse_with(&samples, 48_000, rbl_analysis::AnalysisPreset::Rbxport.options());
    assert!((result.tempo.bpm - 128.0).abs() < 0.01);
    assert_eq!(result.tempo.first_beat_secs, 0.0);
    assert_eq!(result.tempo.beats[0].time_ms, 0);
    assert_eq!(result.tempo.beats[0].beat_number, 1);
}

#[test]
fn a_trimmed_opening_beat_is_shorter_without_moving_later_beats() {
    // A longer percussion tail remains audible when the start is cut.
    // The waveform is not the original 5 ms test tick.
    let mut audio = vec![0.0_f32; 48_000 * 120];
    for start in (0..audio.len()).step_by(22_500) {
        for j in 0..4_800.min(audio.len() - start) {
            audio[start + j] = (0.8 * (-(j as f64) / 1_500.0).exp()
                * (2.0 * std::f64::consts::PI * 80.0 * j as f64 / 48_000.0).sin()) as f32;
        }
    }
    // Include a zero crossing and the last sample strictly below 20 ms.
    for trimmed_samples in [48, 240, 300, 480, 720, 912, 959, 960, 1008, 1440] {
        let trimmed_ms = trimmed_samples as f64 / 48.0;
        let samples = &audio[trimmed_samples..];
        let options = rbl_analysis::AnalysisPreset::Rbxport.options();
        let onsets = onset_envelope(samples, 48_000);
        let kicks = rbl_analysis::onset::onset_envelope_band(samples, 48_000, rbl_analysis::onset::Band::LOW);
        let attacks = rbl_analysis::attack::AttackMap::new(samples, 48_000, options.attacks);
        let fitted = rbl_analysis::tempo::detect_tempo_with(&onsets, Some(&kicks), Some(&attacks), options.tempo);
        let result = rbl_analysis::analyse_with(samples, 48_000, options).tempo;
        if trimmed_samples >= 960 {
            assert!(result.beats[0].time_ms > 0, "trim {trimmed_ms} ms must not qualify");
            assert_eq!(result.beats.len(), fitted.beats.len());
            for (after, before) in result.beats.iter().zip(&fitted.beats) {
                assert_eq!(after.time_ms, before.time_ms);
                assert_eq!(after.tempo_x100, before.tempo_x100);
            }
            continue;
        }
        assert_eq!(result.beats[0].time_ms, 0, "trim {trimmed_ms} ms");
        assert_eq!(result.beats[0].beat_number, 1);
        assert_eq!(result.first_beat_secs, 0.0);
        assert_eq!(result.beats.len(), fitted.beats.len() + 1);
        for (after, before) in result.beats[1..].iter().zip(&fitted.beats) {
            assert_eq!(after.time_ms, before.time_ms, "trim {trimmed_ms} ms moved a later beat");
            assert_eq!(after.tempo_x100, before.tempo_x100);
        }
        assert!(f64::from(result.beats[1].time_ms) < 60_000.0 / result.bpm);
        assert_eq!(beats_of(&result.segments, 0), result.beats);
    }
}
