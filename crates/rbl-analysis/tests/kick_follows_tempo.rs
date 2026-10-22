//! Tempo changes the kick does not follow, through the app's own pipeline
//! (`AnalysisPreset::Rbxport`: attack placement with an attack map and a
//! kick band). A pluck or arp in the attack map's 900-9000 Hz band holding
//! a period of its own is not a tempo change; a kick that moves is.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stdout, clippy::too_many_arguments)]

use rbl_analysis::{analyse_with, AnalysisPreset};

const RATE: u32 = 44_100;

/// Decaying sine hits at `bpm` from `first`, kept within `from..to`.
fn add_hits(audio: &mut [f32], bpm: f64, first: f64, from: f64, to: f64, freq: f64, amp: f32, decay_ms: f64) {
    let period = 60.0 / bpm;
    let mut t = first;
    while t < from {
        t += period;
    }
    let length = (decay_ms * 6.0 * f64::from(RATE) / 1000.0) as usize;
    while t < to {
        let start = (t * f64::from(RATE)) as usize;
        for j in 0..length {
            if start + j >= audio.len() {
                break;
            }
            let x = j as f64 / f64::from(RATE);
            audio[start + j] += amp * ((-x * 1000.0 / decay_ms).exp() * (2.0 * std::f64::consts::PI * freq * x).sin()) as f32;
        }
        t += period;
    }
}

/// A kick: a 55 Hz body and a short 2.5 kHz click.
fn add_kick(audio: &mut [f32], bpm: f64, first: f64, from: f64, to: f64, level: f32) {
    if level <= 0.0 {
        return;
    }
    add_hits(audio, bpm, first, from, to, 55.0, 0.6 * level, 60.0);
    add_hits(audio, bpm, first, from, to, 2500.0, 0.2 * level, 3.0);
}

/// `(bpm, segments as "from-to bpm")` of the app's analysis of `audio`.
fn analyse(audio: &[f32]) -> (f64, Vec<(f64, f64, f64)>) {
    let grid = analyse_with(audio, RATE, AnalysisPreset::Rbxport.options()).tempo;
    (grid.bpm, grid.segments.iter().map(|s| (s.from_secs, s.to_secs, s.bpm())).collect())
}

/// Five minutes of a kick at 150, with a pluck at 168.75 (nine eighths of
/// it) louder than the kick in the click band from 120 s to 180 s. Under
/// the pluck the kick plays at 150 at `kick_level`, or on the pluck's
/// tempo when `kick_follows`.
fn nine_eighths(kick_level: f32, kick_follows: bool) -> Vec<f32> {
    let secs = 300.0;
    let mut audio = vec![0.0_f32; (secs * f64::from(RATE)) as usize];
    add_kick(&mut audio, 150.0, 0.05, 0.0, 120.0, 1.0);
    add_kick(&mut audio, 150.0, 0.05, 180.0, secs, 1.0);
    if kick_follows {
        add_kick(&mut audio, 168.75, 120.0, 120.0, 180.0, 1.0);
    } else {
        add_kick(&mut audio, 150.0, 0.05, 120.0, 180.0, kick_level);
    }
    add_hits(&mut audio, 168.75, 120.0, 120.0, 180.0, 3000.0, 0.5, 20.0);
    audio
}

#[test]
fn a_pluck_the_kick_does_not_follow_is_not_a_tempo_change() {
    // The kick stays at 150 under the pluck, at full level, quietly, or
    // stops: the track has not changed tempo.
    let mut failures = Vec::new();
    for level in [1.0, 0.2, 0.0] {
        let (bpm, segments) = analyse(&nine_eighths(level, false));
        if segments.len() != 1 || (bpm - 150.0).abs() > 0.05 {
            failures.push(format!("kick at {level}: bpm {bpm:.2}, segments {segments:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_tempo_the_kick_follows_is_still_a_tempo_change() {
    // A DJ edit: the kick jumps to 168.75 with the pluck and back.
    let (_, segments) = analyse(&nine_eighths(0.0, true));
    assert!(segments.len() >= 3, "{segments:?}");
    assert!(segments.iter().any(|s| (s.2 - 168.75).abs() < 0.1), "{segments:?}");
    let (first, last) = (segments[0], segments[segments.len() - 1]);
    assert!((first.2 - 150.0).abs() < 0.05 && (last.2 - 150.0).abs() < 0.05, "{segments:?}");
}

#[test]
fn a_kickless_pluck_intro_at_another_period_takes_the_track_s_tempo() {
    // Forty seconds of a pluck at 96.75 (nine sixteenths of 172) with no
    // kick, then the kick at 172 to the end: one tempo, 172, from the
    // start of the file.
    let secs = 240.0;
    let mut audio = vec![0.0_f32; (secs * f64::from(RATE)) as usize];
    add_hits(&mut audio, 96.75, 0.1, 0.0, 40.0, 3000.0, 0.5, 20.0);
    add_kick(&mut audio, 172.0, 40.0, 40.0, secs, 1.0);
    let (bpm, segments) = analyse(&audio);
    assert_eq!(segments.len(), 1, "{segments:?}");
    assert!((bpm - 172.0).abs() < 0.05, "{bpm}");
    assert!(segments[0].0 < 60.0 / 172.0, "starts within the first beat: {segments:?}");
}
