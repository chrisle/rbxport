//! `wave_creator` against rekordbox's own files for generated signals.
//!
//! The fixtures beside this file are the `.EXT` and `.2EX` rekordbox 7.2.19
//! wrote analysing the signals in `signals.rs` as 16-bit WAVs; refresh them
//! with `--example rekordbox_waveform_fixtures`. What is left after the rule
//! is float rounding against rekordbox's own, so a section is held to the
//! share of exact values it reaches rather than to every byte.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "rekordbox_waveform/signals.rs"]
mod signals;

use std::path::PathBuf;

use rbl_analysis::wave_creator::{WaveCreator, Waves};
use rbl_anlz::Anlz;
use signals::{Signal, SIGNALS};

/// The 3Band waveform: the gain exactly, the overview and the detail as
/// rekordbox draws them.
#[test]
fn the_3band_waveform_is_rekordboxs_own() {
    for signal in &SIGNALS {
        let (ours, _, two_ex) = against_rekordbox(signal);
        let gain: Vec<u8> = ours.pwvc.iter().flat_map(|g| g.to_be_bytes()).collect();
        assert_eq!(gain, section(&two_ex, b"PWVC"), "{}: PWVC", signal.name);
        holds(signal, "PWV6", &bytes(&ours.pwv6), &bytes(&section(&two_ex, b"PWV6")), 0.9994, 0.9997);
        holds(signal, "PWV7", &bytes(&ours.pwv7), &bytes(&section(&two_ex, b"PWV7")), 0.9787, 0.9972);
    }
}

/// The RGB waveform: the colour preview and the colour detail's three
/// colours and height.
#[test]
fn the_rgb_waveform_is_rekordboxs_own() {
    for signal in &SIGNALS {
        let (ours, ext, _) = against_rekordbox(signal);
        holds(signal, "PWV4", &bytes(&ours.pwv4), &bytes(&section(&ext, b"PWV4")), 0.9995, 1.0);
        let (mine, theirs) = (words(&ours.pwv5), words(&section(&ext, b"PWV5")));
        for (field, (shift, mask)) in [("red", (13, 7)), ("green", (10, 7)), ("blue", (7, 7)), ("height", (2, 31))] {
            let pick = |w: &Vec<u16>| w.iter().map(|w| (w >> shift) & mask).collect::<Vec<_>>();
            holds(signal, &format!("PWV5 {field}"), &pick(&mine), &pick(&theirs), 1.0, 1.0);
        }
    }
}

/// The blue detail: whiteness and height.
#[test]
fn the_blue_detail_is_rekordboxs_own() {
    for signal in &SIGNALS {
        let (ours, ext, _) = against_rekordbox(signal);
        let theirs = section(&ext, b"PWV3");
        for (field, pick) in [("whiteness", (|b: &u8| u16::from(b >> 5)) as fn(&u8) -> u16), ("height", |b: &u8| u16::from(b & 31))] {
            let values = |bytes: &[u8]| bytes.iter().map(pick).collect::<Vec<_>>();
            holds(signal, &format!("PWV3 {field}"), &values(&ours.pwv3), &values(&theirs), 1.0, 1.0);
        }
    }
}

/// Ours for a signal, and rekordbox's `.EXT` and `.2EX` for it.
fn against_rekordbox(signal: &Signal) -> (Waves, Anlz, Anlz) {
    let mut creator = WaveCreator::new(signal.rate);
    creator.push(&(signal.frames)(signal.rate));
    let fixture = |ending: &str| {
        let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "rekordbox_waveform", &format!("{}.{ending}", signal.name)]
            .iter()
            .collect();
        Anlz::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    };
    (creator.finish(), fixture("EXT"), fixture("2EX"))
}

fn section(file: &Anlz, tag: &[u8; 4]) -> Vec<u8> {
    file.section(tag).unwrap_or_else(|| panic!("rekordbox wrote no {}", String::from_utf8_lossy(tag))).payload.clone()
}

fn bytes(payload: &[u8]) -> Vec<u16> {
    payload.iter().map(|&b| u16::from(b)).collect()
}

fn words(payload: &[u8]) -> Vec<u16> {
    payload.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect()
}

/// Ours is as long as rekordbox's, and at least `exact` of the values equal
/// it and `close` are within one. Each section's floors are the lowest shares
/// it reaches across the six signals, the same on macOS arm64 and Linux arm64
/// and x64, so one more value off on any signal fails.
fn holds(signal: &Signal, what: &str, ours: &[u16], theirs: &[u16], exact: f64, close: f64) {
    assert_eq!(ours.len(), theirs.len(), "{}: {what} length", signal.name);
    let share = |within: u16| {
        let matched = ours.iter().zip(theirs).filter(|(a, b)| a.abs_diff(**b) <= within).count();
        matched as f64 / theirs.len().max(1) as f64
    };
    assert!(share(0) >= exact, "{}: {what} {:.4} exact", signal.name, share(0));
    assert!(share(1) >= close, "{}: {what} {:.4} within one", signal.name, share(1));
}
