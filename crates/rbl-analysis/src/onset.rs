//! Onset detection by spectral flux.
//!
//! Percussive onsets show up as a sudden rise in energy across many frequency
//! bins at once. Summing only the *increases* between consecutive spectra picks
//! those out while ignoring steady tones, which is what makes it work on dance
//! music where the kick dominates.

use realfft::RealFftPlanner;

/// Analysis frame size. At 44.1 kHz this is ~23 ms, short enough to place a
/// kick precisely and long enough for a usable spectrum.
pub const FRAME: usize = 1024;
/// The frame a low band is taken over: ~93 ms at 44.1 kHz.
///
/// Four times the usual, because a bin is only as narrow as the frame is long
/// and the whole of a kick lives in the first few. At 1024 samples everything
/// under 200 Hz is four and a half bins; at 4096 it is eighteen.
pub const LOW_FRAME: usize = 4096;
/// Frames advance by this many samples: ~5.8 ms at 44.1 kHz.
pub const HOP: usize = 256;

/// The onset strength signal, one value per hop.
#[derive(Debug, Clone)]
pub struct OnsetEnvelope {
    pub values: Vec<f32>,
    /// Envelope samples per second.
    pub rate: f64,
    /// Seconds into the file of envelope sample 0.
    ///
    /// A frame's flux is timestamped at the frame's centre, not its start:
    /// an onset raises the spectrum most as the window's peak passes over
    /// it, so the flux peaks when the centre reaches the onset. Timestamping
    /// at the start put every beat half a frame (12 ms) early, which is
    /// half the tolerance a grid has to match another one within.
    pub origin_secs: f64,
}

impl OnsetEnvelope {
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    /// Seconds into the file of envelope sample `x`, which may be
    /// fractional.
    pub fn time_of(&self, x: f64) -> f64 {
        if self.rate <= 0.0 { 0.0 } else { self.origin_secs + x / self.rate }
    }
    /// Seconds represented by `n` envelope samples.
    pub fn seconds(&self, n: usize) -> f64 {
        if self.rate <= 0.0 { 0.0 } else { n as f64 / self.rate }
    }
    /// The envelope between samples, by linear interpolation; zero outside.
    pub fn sample_at(&self, x: f64) -> f32 {
        if x < 0.0 {
            return 0.0;
        }
        let i = x.floor() as usize;
        let frac = (x - i as f64) as f32;
        let a = self.values.get(i).copied().unwrap_or(0.0);
        let b = self.values.get(i + 1).copied().unwrap_or(0.0);
        a + (b - a) * frac
    }
}

/// The frequencies an envelope is built from.
///
/// The full band is what tempo estimation has always used. A low band exists
/// because the kick drum defines the beat while the hi-hats between the beats
/// are what make a period of three halves of a beat score as well as the beat
/// itself — three tracks in the reference library come back at exactly two
/// thirds of rekordbox's tempo for that reason. Onsets taken from below a
/// couple of hundred hertz simply do not contain the hi-hats.
///
/// **Measured once, and the hypothesis lost on a frame size.** On 150 tracks
/// of the reference library, tempo from a 200 Hz band scored 79% against the
/// full band's 95%, fixing 5 tracks and breaking 30 — with a 1024-sample
/// frame, where a bin is 43 Hz wide and everything below 200 Hz is four and a
/// half bins. Spectral flux over four bins cannot place an onset.
///
/// So a band carries its own frame, and the longer one was tried: 4096
/// samples, eighteen bins under 200 Hz instead of four, the hop unchanged so
/// the envelope stays at the same rate.
///
/// **It made no difference, which is the useful part.** On the same 150
/// tracks the long-frame low band scored **75% test / 72% train, fixing 1 and
/// breaking 27** — against the short frame's 79% / 69%, fixing 5 and breaking
/// 30. Within the noise of each other, and both far below the full band's 95%.
/// Used only as a tie-breaker, where the two bands disagree by exactly the
/// 3:2 the whole idea was aimed at, it **fixed none of them and broke five**.
///
/// So the frame size was never the problem: the low band does not carry enough
/// to place a beat in this material, at any resolution. Whatever answers the
/// 3:2 error is not "look at the kick alone" — that hypothesis is finished,
/// and a sixth attempt should start somewhere else entirely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub low_hz: f32,
    /// `f32::INFINITY` for everything up to Nyquist.
    pub high_hz: f32,
    /// Samples in one analysis frame. A power of two, and at least `HOP`.
    pub frame: usize,
}

impl Band {
    /// Every frequency the signal carries.
    pub const FULL: Self = Self { low_hz: 0.0, high_hz: f32::INFINITY, frame: FRAME };
    /// Kick territory: low enough to exclude a hi-hat, wide enough to keep a
    /// kick's attack, which is not a pure tone — and over a frame long enough
    /// to resolve that far down.
    pub const LOW: Self = Self { low_hz: 0.0, high_hz: 200.0, frame: LOW_FRAME };
}

/// Computes the spectral-flux onset envelope over every frequency.
pub fn onset_envelope(samples: &[f32], sample_rate: u32) -> OnsetEnvelope {
    onset_envelope_band(samples, sample_rate, Band::FULL)
}

/// Computes the spectral-flux onset envelope over one band.
///
/// A band that ends far below Nyquist is taken from the audio low-passed
/// and decimated first, and costs a fraction of the full band: the kick
/// band's long frame over the whole signal was four times the price of the
/// full band, for bins a decimated signal resolves just as well.
pub fn onset_envelope_band(samples: &[f32], sample_rate: u32, band: Band) -> OnsetEnvelope {
    let rate = f64::from(sample_rate) / HOP as f64;
    let frame = band.frame.max(HOP);
    let origin_secs = if sample_rate == 0 { 0.0 } else { frame as f64 / 2.0 / f64::from(sample_rate) };
    if samples.len() < frame || sample_rate == 0 {
        return OnsetEnvelope { values: Vec::new(), rate, origin_secs };
    }
    let decimation = decimation_for(band, sample_rate, frame);
    let mut values = if decimation > 1 {
        let reduced = low_pass_and_decimate(samples, sample_rate, band.high_hz, decimation);
        spectral_flux(&reduced, sample_rate / decimation as u32, frame / decimation, HOP / decimation, band)
    } else {
        spectral_flux(samples, sample_rate, frame, HOP, band)
    };
    normalise(&mut values);
    if decimation > 1 {
        scale_to_strong_peaks(&mut values);
    }
    OnsetEnvelope { values, rate, origin_secs }
}

/// The share of an envelope's peaks that read at or above one after
/// `scale_to_strong_peaks`.
const STRONG_PEAKS: f64 = 0.02;

/// Rescales an envelope so that its strong peaks, not its single highest,
/// read as one: the `STRONG_PEAKS` share of its local maxima sit at one or
/// above, and the few above are left there, not clipped, so that every
/// ratio between two hits survives. Peak scaling leaves a track whose one
/// sub-bass boom is ten times any kick with every kick at a tenth, and a
/// level a later stage compares against a floor cannot be read off that.
/// The full band keeps peak scaling: the tempo stage's weights were tuned
/// on it.
fn scale_to_strong_peaks(values: &mut [f32]) {
    let mut peaks: Vec<f32> = values
        .windows(3)
        .filter_map(|w| (w[1] > 0.0 && w[1] > w[0] && w[1] >= w[2]).then_some(w[1]))
        .collect();
    if peaks.len() < 16 {
        return;
    }
    peaks.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = ((peaks.len() as f64 * (1.0 - STRONG_PEAKS)) as usize).min(peaks.len() - 1);
    let strong = peaks[at];
    if strong <= 0.0 {
        return;
    }
    for value in values.iter_mut() {
        *value /= strong;
    }
}

/// The power of two the audio may be decimated by for a band: the band's
/// top stays at most a quarter of the reduced rate, so the anti-alias
/// low-pass has two octaves to fall in, and the frame and hop stay whole
/// and the frame at least 64 samples. 32 for the kick band at 44.1 kHz,
/// 1 for any band that reaches Nyquist.
fn decimation_for(band: Band, sample_rate: u32, frame: usize) -> usize {
    if !band.high_hz.is_finite() || band.high_hz <= 0.0 {
        return 1;
    }
    let mut d = 1usize;
    while d < 64
        && f64::from(sample_rate) / (d * 2) as f64 >= 4.0 * f64::from(band.high_hz)
        && frame.is_multiple_of(d * 2)
        && HOP.is_multiple_of(d * 2)
        && frame / (d * 2) >= 64
    {
        d *= 2;
    }
    d
}

/// The audio low-passed at `high_hz` (two second-order sections, 24 dB per
/// octave) with every `decimation`th sample kept.
fn low_pass_and_decimate(samples: &[f32], sample_rate: u32, high_hz: f32, decimation: usize) -> Vec<f32> {
    let mut first = crate::attack::Biquad::low_pass(high_hz, sample_rate);
    let mut second = first;
    samples
        .iter()
        .map(|&x| second.run(first.run(x)))
        .enumerate()
        .filter_map(|(i, y)| (i % decimation == 0).then_some(y))
        .collect()
}

/// Spectral flux inside `band`: one value per `hop` samples of audio at
/// `sample_rate`, each from a frame of `frame` samples. Not normalised.
fn spectral_flux(samples: &[f32], sample_rate: u32, frame: usize, hop: usize, band: Band) -> Vec<f32> {
    if samples.len() < frame || sample_rate == 0 || hop == 0 {
        return Vec::new();
    }
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(frame);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();

    // Hann window: without it, frame edges produce spectral splatter that looks
    // like an onset on every frame.
    let window: Vec<f32> = (0..frame)
        .map(|i| {
            let x = std::f32::consts::PI * 2.0 * i as f32 / frame as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect();

    // The FFT bins the band covers. Bin `i` is centred at
    // `i * sample_rate / frame` hertz.
    let hz_per_bin = f64::from(sample_rate) / frame as f64;
    let first_bin = (f64::from(band.low_hz) / hz_per_bin).floor().max(0.0) as usize;
    let last_bin = if band.high_hz.is_finite() {
        (f64::from(band.high_hz) / hz_per_bin).ceil() as usize
    } else {
        usize::MAX
    };

    let frames = (samples.len().saturating_sub(frame)) / hop + 1;
    let mut values = Vec::with_capacity(frames);
    let mut previous = vec![0.0_f32; output.len()];

    for index in 0..frames {
        let start = index * hop;
        let Some(chunk) = samples.get(start..start + frame) else { break };
        for (i, slot) in input.iter_mut().enumerate() {
            *slot = chunk.get(i).copied().unwrap_or(0.0) * window.get(i).copied().unwrap_or(0.0);
        }
        if fft.process(&mut input, &mut output).is_err() {
            break;
        }

        let mut flux = 0.0_f32;
        for (i, bin) in output.iter().enumerate() {
            let magnitude = bin.norm();
            let rise = magnitude - previous.get(i).copied().unwrap_or(0.0);
            // Every bin's history is kept, in or out of band: a rise measured
            // against a spectrum that skipped frames would not be a rise.
            if let Some(slot) = previous.get_mut(i) {
                *slot = magnitude;
            }
            // Only increases count, and only inside the band: a decay is not
            // an onset, and neither is a hi-hat when the band excludes it.
            if rise > 0.0 && i >= first_bin && i <= last_bin {
                flux += rise;
            }
        }
        values.push(flux);
    }
    values
}

/// Subtracts a local mean and clips at zero, which removes slow loudness drift
/// so a quiet intro and a loud drop contribute equally to tempo estimation.
fn normalise(values: &mut [f32]) {
    const WINDOW: usize = 16;
    if values.len() <= WINDOW {
        return;
    }
    let original = values.to_vec();
    for (i, value) in values.iter_mut().enumerate() {
        let lo = i.saturating_sub(WINDOW);
        let hi = (i + WINDOW).min(original.len());
        let slice = original.get(lo..hi).unwrap_or(&[]);
        if slice.is_empty() {
            continue;
        }
        let mean: f32 = slice.iter().sum::<f32>() / slice.len() as f32;
        *value = (*value - mean).max(0.0);
    }
    let peak = values.iter().fold(0.0_f32, |a, &v| a.max(v));
    if peak > 0.0 {
        for value in values.iter_mut() {
            *value /= peak;
        }
    }
}
