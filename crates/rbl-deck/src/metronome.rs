//! The metronome: a click on every beat of a deck's grid while it plays.
//!
//! rekordbox's, from Preferences › Audio: three click sounds and three
//! volumes, switched on per deck from the grid-editing row. The grid is the
//! track's own — the beats the analysis wrote, handed over in output frames
//! — so the click lands where the grid says the beat is, and a wrong grid is
//! heard as a wrong grid, which is the point of a metronome in a grid editor.
//!
//! Every beat gets the same click. [OBS rekordbox 7.2.19 macOS arm64 static]
//! `rbxfrm::MetronomeUnit::processBlock` @0x1028f1e6c starts its one sound
//! source whenever the deck's beat position crosses a whole beat
//! (@0x1028f2248-0x1028f2270): it reads no bar position, and there is no
//! second sound for the downbeat (#344: an octave-up downbeat here left
//! beats 2 to 4 lost under the music).
//!
//! Added after the channel strip and before the master level: an EQ cut on
//! the deck must not muffle the click, and the master turns it down with
//! everything else. The click is synthesised, so nothing is read from disk
//! in the callback; see [`ClickSound`] for what each one is modelled on.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// Which click: rekordbox's Click Sound 01 to 03.
///
/// rekordbox plays `Contents/Resources/binary/Click Sound 01 Electronic.wav`,
/// `02 HiHat.wav` and `03 Cowbell.wav` (picked in
/// `DjEngineIF::setUsingMetronomeSound` @0x100916adc). Those recordings are
/// not ours to ship, so each is synthesised from what was measured of it
/// [OBS, 44.1 kHz files, measured 2026-10-12]:
///
/// - 01 Electronic: a 2.4 kHz tone held about 30 ms, gone by 42 ms.
/// - 02 hi-hat: noise centred near 7 kHz, down to 0.4 at 10 ms, 100 ms long.
/// - 03 Cowbell: 450 Hz with a partial near 2.35 kHz, a quick drop to a
///   third and a tail to about 95 ms.
///
/// The shapes below are fitted to those measurements by hand [ASSUME: close
/// in pitch, length and envelope; not sample-identical].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickSound {
    One = 1,
    Two = 2,
    Three = 3,
}

impl ClickSound {
    /// How long the click lasts, in seconds: the length of rekordbox's file.
    fn length(self) -> f32 {
        match self {
            ClickSound::One => 0.042,
            ClickSound::Two => 0.100,
            ClickSound::Three => 0.098,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => ClickSound::One,
            3 => ClickSound::Three,
            _ => ClickSound::Two,
        }
    }
}

/// Small, Middle, Large: the gain on the click.
///
/// [OBS rekordbox 7.2.19 static] `DjEngineIF::setMetronomeVelocity` @0x100916e84
/// indexes a table of three floats at 0x10379ca68 with the Preferences
/// volume (0, 1, 2 for Volume1 to Volume3, `BeatGridAdjustment`
/// @0x1010c0328) and hands the result to the metronome's gain:
/// 0.17741 (-15 dB), 0.26611 (-11.5 dB), 0.35481 (-9 dB). The click itself
/// peaks near full scale, as rekordbox's files do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickVolume {
    Small,
    Middle,
    Large,
}

impl ClickVolume {
    fn gain(self) -> f32 {
        match self {
            ClickVolume::Small => 0.177_406_7,
            ClickVolume::Middle => 0.266_110_06,
            ClickVolume::Large => 0.354_813_4,
        }
    }
}

/// The metronome as the interface sets it: shared by both decks, read by
/// the callback without a lock.
#[derive(Debug)]
pub struct MetronomeSettings {
    sound: AtomicU8,
    gain: AtomicU32,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        // The capture's defaults: Click Sound 02, Large [OBS].
        Self { sound: AtomicU8::new(2), gain: AtomicU32::new(ClickVolume::Large.gain().to_bits()) }
    }
}

impl MetronomeSettings {
    pub fn set_sound(&self, sound: ClickSound) {
        self.sound.store(sound as u8, Ordering::Relaxed);
    }

    pub fn sound(&self) -> ClickSound {
        ClickSound::from_u8(self.sound.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: ClickVolume) {
        self.gain.store(volume.gain().to_bits(), Ordering::Relaxed);
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }
}

/// One beat of a deck's grid, in output frames at 100 % tempo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridBeat {
    pub frame: u64,
    /// The first beat of its bar. Carried with the grid, but the click is
    /// the same on it as on any other beat, as rekordbox's is.
    pub downbeat: bool,
}

/// A click in progress.
#[derive(Debug, Clone, Copy)]
struct Click {
    sound: ClickSound,
    peak: f32,
    /// Frames rendered so far.
    at: u32,
    /// The hi-hat's noise: a xorshift state seeded the same for every click,
    /// so every beat sounds alike, as one sample played again does.
    noise: u32,
    /// The hi-hat's band-pass: coefficients for the device rate, and the
    /// filter's last two inputs and outputs.
    band: [f32; 4],
    history: [f32; 4],
}

/// The hi-hat's noise seed. Any fixed non-zero value; this one is the
/// golden ratio's bits.
const NOISE_SEED: u32 = 0x9E37_79B9;

impl Click {
    fn new(sound: ClickSound, peak: f32, rate: f32) -> Self {
        Self { sound, peak, at: 0, noise: NOISE_SEED, band: band_pass(6_000.0, 1.0, rate), history: [0.0; 4] }
    }

    /// The next sample, at full scale before the volume.
    fn next(&mut self, rate: f32) -> f32 {
        #[allow(clippy::cast_precision_loss, reason = "a click is a few thousand frames")]
        let t = self.at as f32 / rate;
        self.at = self.at.saturating_add(1);
        let tau = std::f32::consts::TAU;
        match self.sound {
            // A tone held near full scale, easing to 0.83 over the first
            // 15 ms; cut to 0.2 over the millisecond from 29 ms, and a 6 ms
            // tail after that.
            ClickSound::One => {
                let envelope = if t < 0.029 {
                    1.0 - 0.17 * (t / 0.015).min(1.0)
                } else if t < 0.030 {
                    0.83 - 0.63 * (t - 0.029) / 0.001
                } else {
                    0.2 * (-(t - 0.030) / 0.006).exp()
                };
                envelope * (tau * 2_400.0 * t).sin()
            }
            // Band-passed noise falling with an 11 ms time constant.
            ClickSound::Two => {
                self.noise ^= self.noise << 13;
                self.noise ^= self.noise >> 17;
                self.noise ^= self.noise << 5;
                #[allow(clippy::cast_precision_loss, reason = "noise needs no more than f32's precision")]
                let white = self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
                let [b0, b2, a1, a2] = self.band;
                let [x1, x2, y1, y2] = self.history;
                let y = b0 * white + b2 * x2 - a1 * y1 - a2 * y2;
                self.history = [white, x1, y, y1];
                (1.6 * y * (-t / 0.011).exp()).clamp(-1.0, 1.0)
            }
            // The bell's two tones: a strike held 6 ms and falling away
            // into a ringing tail, faded out over its last 15 ms.
            ClickSound::Three => {
                let strike = 0.7 * (-(t - 0.006).max(0.0) / 0.005).exp();
                let envelope = strike + 0.3 * (-t / 0.07).exp();
                let fade = ((self.sound.length() - t) / 0.015).clamp(0.0, 1.0);
                let tone = 0.88 * (tau * 450.0 * t).sin() + 0.12 * (tau * 2_350.0 * t).sin();
                envelope * fade * tone
            }
        }
    }

    fn done(&self, rate: f32) -> bool {
        #[allow(clippy::cast_precision_loss, reason = "a click is a few thousand frames")]
        let t = self.at as f32 / rate;
        t >= self.sound.length()
    }
}

/// An RBJ band-pass (0 dB peak) at `hz` with quality `q`, as
/// `[b0, b2, a1, a2]` normalised by a0 (b1 is zero). Kept under the
/// Nyquist frequency at low device rates.
fn band_pass(hz: f32, q: f32, rate: f32) -> [f32; 4] {
    let centre = hz.min(rate * 0.4);
    let w0 = std::f32::consts::TAU * centre / rate;
    let alpha = w0.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    [alpha / a0, -alpha / a0, -2.0 * w0.cos() / a0, (1.0 - alpha) / a0]
}

/// One deck's metronome.
#[derive(Debug)]
pub struct Metronome {
    on: AtomicBool,
    /// The grid, sorted by frame. Replaced whole on a load; the callback
    /// takes it with `try_lock` and skips a callback rather than wait.
    grid: Mutex<Arc<[GridBeat]>>,
    settings: Arc<MetronomeSettings>,
}

impl Metronome {
    pub fn new(settings: Arc<MetronomeSettings>) -> Self {
        Self { on: AtomicBool::new(false), grid: Mutex::new(Arc::from(Vec::new())), settings }
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// The deck's grid, in output frames, in any order.
    pub fn set_grid(&self, mut beats: Vec<GridBeat>) {
        beats.sort_by_key(|b| b.frame);
        if let Ok(mut grid) = self.grid.lock() {
            *grid = Arc::from(beats);
        }
    }

    /// The beats the playhead crossed moving from `from` to `to` frames of
    /// the track, as offsets into a buffer of `frames` output frames, with
    /// whether each is a downbeat. Nothing when the deck did not move, or
    /// the metronome is off.
    fn crossed(&self, from: u64, to: u64, frames: usize) -> Vec<(usize, bool)> {
        let mut out = Vec::new();
        if !self.is_on() || to <= from || frames == 0 {
            return out;
        }
        let Ok(grid) = self.grid.try_lock() else { return out };
        let first = grid.partition_point(|b| b.frame < from);
        // The first frame after a load is the downbeat, and a beat exactly
        // on `from` is counted here, not by the callback before this one.
        #[allow(clippy::cast_precision_loss, reason = "a buffer is at most a few thousand frames")]
        let scale = frames as f64 / (to - from) as f64;
        for beat in grid.iter().skip(first).take_while(|b| b.frame < to) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "under `frames` by construction")]
            let offset = (((beat.frame - from) as f64) * scale) as usize;
            out.push((offset.min(frames - 1), beat.downbeat));
        }
        out
    }
}

/// The clicks sounding on one deck, carried between callbacks.
#[derive(Debug, Default)]
pub struct MetronomeVoice {
    ringing: Vec<Click>,
}

impl MetronomeVoice {
    /// Adds the deck's clicks for this callback into `out` (interleaved
    /// stereo, `frames` frames), for a playhead that went from `from` to
    /// `to`. The deck's own position is what places a click, so a beat at
    /// half tempo still lands on its beat.
    pub fn render(
        &mut self,
        metronome: &Metronome,
        from: u64,
        to: u64,
        out: &mut [f32],
        rate: u32,
    ) {
        let frames = out.len() / 2;
        let settings = &metronome.settings;
        let sound = settings.sound();
        let peak = settings.gain();
        let starts = metronome.crossed(from, to, frames);
        if starts.is_empty() && self.ringing.is_empty() {
            return;
        }
        #[allow(clippy::cast_precision_loss, reason = "a sample rate is far below 2^24")]
        let rate = rate as f32;
        // Every ringing click continues; every new one starts at its offset,
        // the same click whichever beat of the bar it is.
        let mut pending = starts.into_iter().peekable();
        for (i, frame) in out.chunks_exact_mut(2).enumerate() {
            while pending.next_if(|(at, _)| *at <= i).is_some() {
                self.ringing.push(Click::new(sound, peak, rate));
            }
            let mut sum = 0.0_f32;
            for click in &mut self.ringing {
                if !click.done(rate) {
                    sum += click.peak * click.next(rate);
                }
            }
            frame[0] += sum;
            frame[1] += sum;
        }
        self.ringing.retain(|c| !c.done(rate));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;

    fn metronome() -> Metronome {
        let m = Metronome::new(Arc::new(MetronomeSettings::default()));
        m.set_grid(vec![
            GridBeat { frame: 1000, downbeat: true },
            GridBeat { frame: 500, downbeat: false },
            GridBeat { frame: 1500, downbeat: false },
        ]);
        m.set_on(true);
        m
    }

    #[test]
    fn the_beats_crossed_are_placed_where_the_playhead_passed_them() {
        let m = metronome();
        // From 400 to 1200 in 800 frames: beats at 500 and 1000, 100 and 600 in.
        assert_eq!(m.crossed(400, 1200, 800), vec![(100, false), (600, true)]);
        // Half speed: 400 to 800 over 800 frames, the beat at 500 lands 200 in.
        assert_eq!(m.crossed(400, 800, 800), vec![(200, false)]);
        // A beat on the boundary belongs to the callback that starts on it.
        assert_eq!(m.crossed(500, 1000, 500), vec![(0, false)]);
        assert_eq!(m.crossed(1000, 1500, 500), vec![(0, true)]);
    }

    #[test]
    fn off_or_still_is_silent() {
        let m = metronome();
        assert!(m.crossed(400, 400, 512).is_empty(), "no movement");
        m.set_on(false);
        assert!(m.crossed(0, 2000, 512).is_empty(), "off");
    }

    #[test]
    fn a_click_starts_at_its_beat_and_dies_away() {
        let m = metronome();
        let mut voice = MetronomeVoice::default();
        let mut out = vec![0.0_f32; 800 * 2];
        voice.render(&m, 400, 1200, &mut out, 44_100);
        // Nothing before the first beat, something at it, both channels alike.
        assert!(out[..200].iter().all(|s| *s == 0.0));
        let loud = out[200..400].iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(loud > 0.1, "the click is heard: {loud}");
        assert_eq!(out[202], out[203]);
        // The downbeat starts at 600 and rings past the end.
        assert!(!voice.ringing.is_empty());
        // And a later callback with no beats lets it end.
        let mut tail = vec![0.0_f32; 8192 * 2];
        voice.render(&m, 1200, 1300, &mut tail, 44_100);
        assert!(voice.ringing.is_empty(), "over within the callback");
    }

    #[test]
    fn the_volumes_and_sounds_are_the_capture_s_three_each() {
        let s = MetronomeSettings::default();
        assert_eq!(s.sound(), ClickSound::Two);
        assert_eq!(s.gain(), ClickVolume::Large.gain());
        s.set_sound(ClickSound::Three);
        s.set_volume(ClickVolume::Small);
        assert_eq!(s.sound(), ClickSound::Three);
        assert_eq!(s.gain(), ClickVolume::Small.gain());
    }

    /// One click of `sound` at full scale, at 44.1 kHz, the rate of
    /// rekordbox's files.
    fn one_click(sound: ClickSound) -> Vec<f32> {
        let rate = 44_100.0;
        let mut click = Click::new(sound, 1.0, rate);
        let mut out = Vec::new();
        while !click.done(rate) {
            out.push(click.next(rate));
        }
        out
    }

    /// The loudest frequency between 100 Hz and 16 kHz, and the spectral
    /// centroid, both in hertz, by a Hann-windowed DFT in 50 Hz steps: the
    /// measurement taken of rekordbox's files.
    fn spectrum(samples: &[f32]) -> (f32, f32) {
        let n = samples.len();
        let rate = 44_100.0_f32;
        let window: Vec<f32> = (0..n)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n - 1) as f32).cos())
            .zip(samples)
            .map(|(w, s)| w * s)
            .collect();
        let (mut best, mut best_hz, mut power, mut weighted) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        for hz in (100..16_000).step_by(50) {
            let hz = hz as f32;
            let (mut re, mut im) = (0.0_f32, 0.0_f32);
            for (i, x) in window.iter().enumerate() {
                let phase = std::f32::consts::TAU * hz * i as f32 / rate;
                re += x * phase.cos();
                im -= x * phase.sin();
            }
            let magnitude = re.hypot(im);
            if magnitude > best {
                best = magnitude;
                best_hz = hz;
            }
            power += magnitude * magnitude;
            weighted += magnitude * magnitude * hz;
        }
        (best_hz, weighted / power)
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s.abs()).fold(0.0, f32::max)
    }

    #[test]
    fn every_beat_of_the_bar_gets_the_same_click() {
        // #344: the downbeat clicked an octave up and the other three beats
        // were lost under the music. rekordbox plays one sound on every
        // beat (`MetronomeUnit::processBlock` @0x1028f1e6c).
        for sound in [ClickSound::One, ClickSound::Two, ClickSound::Three] {
            let settings = Arc::new(MetronomeSettings::default());
            settings.set_sound(sound);
            let m = Metronome::new(Arc::clone(&settings));
            let beat = 22_050_u64;
            m.set_grid((0..4).map(|i| GridBeat { frame: 1_000 + i * beat, downbeat: i == 0 }).collect());
            m.set_on(true);
            let mut voice = MetronomeVoice::default();
            let mut out = vec![0.0_f32; (beat as usize * 4 + 2_000) * 2];
            voice.render(&m, 0, beat * 4 + 2_000, &mut out, 44_100);
            let left: Vec<f32> = out.iter().step_by(2).copied().collect();
            let clicks: Vec<&[f32]> =
                (0..4).map(|i| &left[1_000 + i * beat as usize..][..4_000]).collect();
            for (n, click) in clicks.iter().enumerate().skip(1) {
                assert_eq!(*click, clicks[0], "{sound:?}: beat {} differs from the downbeat", n + 1);
            }
            assert!(peak(clicks[0]) > 0.3, "{sound:?} is heard: {}", peak(clicks[0]));
        }
    }

    #[test]
    fn the_volumes_are_rekordboxs_gain_table() {
        // [OBS static] the three floats at 0x10379ca68 that
        // `DjEngineIF::setMetronomeVelocity` @0x100916e84 hands the
        // metronome's gain for Volume1 to Volume3.
        assert!((ClickVolume::Small.gain() - 0.177_406_7).abs() < 1e-6);
        assert!((ClickVolume::Middle.gain() - 0.266_110_06).abs() < 1e-6);
        assert!((ClickVolume::Large.gain() - 0.354_813_4).abs() < 1e-6);
    }

    /// What was measured of rekordbox's three files (44.1 kHz, left channel,
    /// same method as `spectrum`): frames, peak, RMS, loudest frequency,
    /// centroid. `values-rekordbox-clicks.txt` in the #344 parity evidence.
    const REKORDBOX: [(ClickSound, usize, f32, f32, f32, f32); 3] = [
        (ClickSound::One, 1_856, 0.984, 0.462, 2_400.0, 2_396.0),
        (ClickSound::Two, 4_411, 1.000, 0.110, 5_850.0, 7_004.0),
        (ClickSound::Three, 4_337, 0.999, 0.182, 450.0, 511.0),
    ];

    #[test]
    fn each_click_is_modelled_on_rekordboxs_file() {
        for (sound, frames, rb_peak, rb_rms, rb_top, rb_centroid) in REKORDBOX {
            let click = one_click(sound);
            let (top, centroid) = spectrum(&click);
            assert!(click.len().abs_diff(frames) < 50, "{sound:?}: {} frames, rekordbox {frames}", click.len());
            assert!((peak(&click) - rb_peak).abs() < 0.05, "{sound:?}: peak {}", peak(&click));
            // Within 2 dB of the file's RMS.
            let db = 20.0 * (rms(&click) / rb_rms).log10();
            assert!(db.abs() < 2.0, "{sound:?}: RMS {} is {db:.1} dB from rekordbox's", rms(&click));
            // The loudest frequency within a tenth, the centroid within a fifth.
            assert!((top / rb_top - 1.0).abs() < 0.1, "{sound:?}: loudest {top} Hz, rekordbox {rb_top}");
            assert!((centroid / rb_centroid - 1.0).abs() < 0.2, "{sound:?}: centroid {centroid} Hz, rekordbox {rb_centroid}");
        }
    }
}
