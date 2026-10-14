//! The generated stereo signals the rekordbox waveform fixtures were analysed
//! from. Deterministic, so a test regenerates the exact samples rekordbox
//! read: `cargo run -p rbl-analysis --example rekordbox_waveform_fixtures`
//! writes them as WAV files for rekordbox to analyse again.

use std::f64::consts::PI;

/// How long every signal runs.
pub const SECONDS: f64 = 20.0;

/// One signal: its name, its sample rate and its stereo frames.
pub struct Signal {
    pub name: &'static str,
    pub rate: u32,
    pub frames: fn(u32) -> Vec<[i16; 2]>,
}

/// Each one leans on a different part of the waveform DSP.
pub const SIGNALS: [Signal; 6] = [
    // Low and high bands at once; both channels the same.
    Signal { name: "kick-hats", rate: 44_100, frames: kick_hats },
    // The same at another rate, which moves every millisecond boundary.
    Signal { name: "kick-hats-48k", rate: 48_000, frames: kick_hats },
    // A bass sweeping through every band's edge under a held chord.
    Signal { name: "bass-chord", rate: 44_100, frames: bass_chord },
    // Channels that disagree in level, mixed by their mean.
    Signal { name: "wide", rate: 44_100, frames: wide },
    // Channels equal in level and opposite in sign: the louder one is kept,
    // where their mean would be silence.
    Signal { name: "inverted", rate: 44_100, frames: inverted },
    // Near silence, then a clipped square at full scale.
    Signal { name: "quiet-then-loud", rate: 44_100, frames: quiet_then_loud },
];

fn kick_hats(rate: u32) -> Vec<[i16; 2]> {
    let mut noise = Noise::default();
    frames(rate, |t| {
        let x = kick(t) + hat(t, noise.next());
        [x, x]
    })
}

fn bass_chord(rate: u32) -> Vec<[i16; 2]> {
    let mut phase = 0.0;
    let step = 1.0 / f64::from(rate);
    frames(rate, |t| {
        // 40 Hz to 400 Hz over the signal, a decade.
        phase += 2.0 * PI * 40.0 * 10f64.powf(t / SECONDS) * step;
        let tremolo = 0.75 + 0.25 * (2.0 * PI * 0.5 * t).sin();
        let chord: f64 = [220.0, 261.63, 329.63].iter().map(|hz| (2.0 * PI * hz * t).sin()).sum();
        let x = 0.5 * phase.sin() + 0.15 * tremolo * chord;
        [x, x]
    })
}

fn wide(rate: u32) -> Vec<[i16; 2]> {
    frames(rate, |t| [kick(t), 0.25 * (2.0 * PI * 3000.0 * t).sin()])
}

fn inverted(rate: u32) -> Vec<[i16; 2]> {
    let mut noise = Noise::default();
    frames(rate, |t| {
        let x = kick(t) + hat(t, noise.next());
        [x, -x]
    })
}

fn quiet_then_loud(rate: u32) -> Vec<[i16; 2]> {
    let mut noise = Noise::default();
    frames(rate, |t| {
        let x = if t < SECONDS / 2.0 { 0.01 * noise.next() } else { (2.0 * PI * 100.0 * t).sin().signum() };
        [x, x]
    })
}

/// A 55 Hz kick on every beat at 125 BPM.
fn kick(t: f64) -> f64 {
    0.9 * (2.0 * PI * 55.0 * t).sin() * (-30.0 * (t % 0.48)).exp()
}

/// A burst of noise on every off-beat eighth.
fn hat(t: f64, noise: f64) -> f64 {
    let since = (t + 0.12) % 0.24;
    0.3 * noise * (-80.0 * since).exp()
}

/// `SECONDS` of frames, each channel quantised as a 16-bit file holds it.
fn frames(rate: u32, mut at: impl FnMut(f64) -> [f64; 2]) -> Vec<[i16; 2]> {
    let count = (SECONDS * f64::from(rate)) as usize;
    (0..count)
        .map(|i| at(i as f64 / f64::from(rate)).map(|x| (x * 32767.0).round().clamp(-32768.0, 32767.0) as i16))
        .collect()
}

/// xorshift32 in -1..1, the same sequence on every run.
struct Noise(u32);

impl Default for Noise {
    fn default() -> Self {
        Self(0x2545_f491)
    }
}

impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        f64::from(self.0) / f64::from(u32::MAX) * 2.0 - 1.0
    }
}
