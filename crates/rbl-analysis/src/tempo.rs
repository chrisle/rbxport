//! Tempo and beat grid.
//!
//! Three stages, each reading the onset envelope rather than the audio:
//!
//! 1. **Candidates.** The autocorrelation of the envelope peaks at the beat
//!    period and at every multiple of it; the Fourier transform of the
//!    envelope peaks at the beat rate and every multiple of *that*. A wrong
//!    period at three halves of the beat correlates (it lands on kick, hat,
//!    kick, hat) but has no spectral line, so scoring a candidate by both
//!    leaves the octave as the only ambiguity, and a tempo prior settles it.
//! 2. **Fit.** The chosen period is refined to a fraction of an envelope
//!    sample by a comb, the phase is found the same way, and then every beat
//!    is snapped to the nearest onset peak and a line is fitted through the
//!    snapped beats. Six hundred beats average a five-millisecond hop down to
//!    a fraction of a millisecond, which is what a grid needs to stay on the
//!    beat for a whole track: 0.05 BPM of error is a beat and a half of drift
//!    by the end of a five-minute track.
//! 3. **Segments.** A tempo is measured in windows along the track, and a
//!    stretch that sustains a different one becomes its own segment with its
//!    own fit, the boundary placed at the beat where the onsets change sides.
//!    DJ edits jump tempo mid-track; a single line through such a track is
//!    wrong on both sides of the jump.
//!
//! The downbeat is not chosen here: every beat comes back numbered from the
//! first, and [`crate::downbeat`] renumbers the grid once it has looked at
//! the music.

use crate::attack::AttackMap;
use crate::onset::OnsetEnvelope;

/// Tempo search range. Dance music sits well inside this, and a wider range
/// mostly adds octave errors.
pub const MIN_BPM: f64 = 70.0;
pub const MAX_BPM: f64 = 180.0;

/// One beat of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// 1..=4, where 1 is the downbeat.
    pub beat_number: u16,
    /// BPM x100 at this beat, as ANLZ stores it.
    pub tempo_x100: u16,
    pub time_ms: u32,
}

/// A stretch of the track at one tempo: every `phase_secs + k × period_secs`
/// that falls in `from_secs..to_secs` is a beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Where the stretch begins, in seconds from the start of the file: 0
    /// for the first segment, the tempo change for the others.
    pub from_secs: f64,
    /// Where it ends: the next change, or the end of the track.
    pub to_secs: f64,
    /// Seconds between beats.
    pub period_secs: f64,
    /// Any beat's time; the grid is this plus whole periods either way.
    pub phase_secs: f64,
}

impl Segment {
    pub fn bpm(&self) -> f64 {
        if self.period_secs > 0.0 { 60.0 / self.period_secs } else { 0.0 }
    }
    /// The first beat, in seconds from the start of the file: the first
    /// grid point at or after `from_secs`. A segment that starts on one of
    /// its own beats — every tempo change does — starts with that beat,
    /// however the division rounds.
    pub fn start_secs(&self) -> f64 {
        if self.period_secs <= 0.0 {
            return self.from_secs;
        }
        let k = ((self.from_secs - self.phase_secs) / self.period_secs - 1e-6).ceil();
        self.phase_secs + k * self.period_secs
    }
    /// Beats in the segment.
    pub fn beats(&self) -> usize {
        if self.period_secs <= 0.0 || self.to_secs <= self.from_secs {
            return 0;
        }
        let first = self.start_secs();
        if first >= self.to_secs {
            return 0;
        }
        // A whole number of periods, give or take rounding, is that many
        // beats, not one more.
        ((self.to_secs - first) / self.period_secs - 1e-9).ceil() as usize
    }
    /// The same grid moved half a beat, which is where the beats are when
    /// the grid was built on the off-beat.
    #[must_use]
    pub fn shifted_half_beat(&self) -> Self {
        Self { phase_secs: self.phase_secs + self.period_secs / 2.0, ..*self }
    }
}

/// A segment's last beat is dropped when the next segment's first beat is
/// within this fraction of a period after it: the two grids have landed
/// on the same hit, which is the new tempo's. Rekordbox's hand grids keep
/// an old-tempo beat that sits more than three fifths of a period before
/// the change and drop one that sits a two-hundredth before it.
const SAME_HIT: f64 = 0.5;

/// Every beat of every segment, in order, numbered so that beat `i` is a
/// downbeat when `(i + phase) % 4 == 0`.
pub fn beats_of(segments: &[Segment], phase: usize) -> Vec<Beat> {
    let mut beats = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let tempo_x100 = u16::try_from((segment.bpm() * 100.0).round() as i64).unwrap_or(0);
        let start = segment.start_secs();
        let mut count = segment.beats().min(100_000);
        if let Some(next) = segments.get(index + 1) {
            let last = start + count.saturating_sub(1) as f64 * segment.period_secs;
            if count > 0 && next.start_secs() - last < SAME_HIT * segment.period_secs {
                count -= 1;
            }
        }
        for i in 0..count {
            let time = start + i as f64 * segment.period_secs;
            beats.push(Beat {
                beat_number: u16::try_from((beats.len() + phase) % 4 + 1).unwrap_or(1),
                tempo_x100,
                time_ms: u32::try_from((time * 1000.0).round() as i64).unwrap_or(0),
            });
        }
    }
    beats
}

#[derive(Debug, Clone)]
pub struct TempoResult {
    /// The tempo at the start of the track, which is what a library shows.
    pub bpm: f64,
    /// Confidence in 0..=1: how far the chosen candidate stands out from the
    /// next best that is not a multiple of it.
    pub confidence: f64,
    /// Seconds from the start to the first beat.
    pub first_beat_secs: f64,
    /// One entry per tempo, in order. A track at one tempo has one.
    pub segments: Vec<Segment>,
    /// Every beat, numbered 1..=4 cyclically from the first.
    pub beats: Vec<Beat>,
}

impl TempoResult {
    pub fn empty() -> Self {
        Self { bpm: 0.0, confidence: 0.0, first_beat_secs: 0.0, segments: Vec::new(), beats: Vec::new() }
    }
}

/// What the candidate stage is tuned by.
///
/// Held in a struct so a measurement rig can search them against real audio.
/// The defaults are what ships; anything else has to beat them on the golden
/// playlist before it becomes a default.
#[derive(Debug, Clone, Copy)]
pub struct TempoOptions {
    pub min_bpm: f64,
    pub max_bpm: f64,
    /// Where the tempo prior is centred, in BPM.
    pub prior_centre: f64,
    /// The prior's spread, in natural logs of tempo ratio.
    pub prior_width: f64,
    /// Phases tried when the comb refines a period.
    pub refine_phases: usize,
    /// Seconds per window when looking for a tempo change.
    pub segment_window_secs: f64,
    /// A window whose own tempo differs from the track's by more than this
    /// fraction, and which is followed by another that agrees with it, starts
    /// a new segment.
    pub segment_threshold: f64,
    /// Where each beat is placed when the line is fitted.
    pub placement: Placement,
    /// Whether the grid follows the music through tempo changes, ramps and
    /// re-phased returns. Without it the whole track is one constant grid,
    /// as rekordbox's Normal analysis writes it.
    pub follow_changes: bool,
}

/// What a beat is snapped to before the line is fitted through the beats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The nearest peak of the onset envelope, sharpened by a parabola:
    /// within a hop of the hit.
    Envelope,
    /// The start of the kick's click in the 900–9000 Hz band, to the
    /// millisecond ([`crate::attack`]). Falls back to the envelope where no
    /// attack map is given.
    Attack,
}

impl Default for TempoOptions {
    fn default() -> Self {
        Self {
            min_bpm: MIN_BPM,
            max_bpm: MAX_BPM,
            prior_centre: 132.0,
            prior_width: 0.5,
            refine_phases: 32,
            segment_window_secs: 16.0,
            segment_threshold: 0.02,
            placement: Placement::Attack,
            follow_changes: true,
        }
    }
}

/// One tempo the candidate stage considered, with everything it was scored
/// on. Exposed so a measurement rig can print the table for a track that
/// chose wrongly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub bpm: f64,
    /// Normalised autocorrelation at the beat period.
    pub acf: f64,
    /// Fourier magnitude at the beat rate, relative to the strongest in range.
    pub fourier: f64,
    /// The tempo prior at this BPM.
    pub prior: f64,
    /// What it was ranked by.
    pub score: f64,
}

/// Estimates tempo and builds the beat grid from the envelope alone.
pub fn detect_tempo(onsets: &OnsetEnvelope) -> TempoResult {
    detect_tempo_with(onsets, None, None, TempoOptions::default())
}

/// Where the fit reads its beats from: the envelope, the attack map when
/// there is one, and the kick band when there is one.
#[derive(Clone, Copy)]
struct Reader<'a> {
    values: &'a [f64],
    rate: f64,
    origin_secs: f64,
    attacks: Option<&'a AttackMap>,
    /// The onset envelope of the kick band alone (`Band::LOW`), on its own
    /// clock: its frame is longer, so its origin is later.
    kicks: Option<&'a OnsetEnvelope>,
}

/// One beat's evidence, from the three sources a beat can be heard in.
#[derive(Debug, Clone, Copy, Default)]
struct Evidence {
    /// The kick band's peak near the beat: the thump.
    kick: f64,
    /// The click band's spike at the beat: the kick's attack.
    attack: f64,
    /// The full band's peak near the beat: anything percussive.
    flux: f64,
}

impl Evidence {
    const SOURCES: usize = 3;
    fn source(self, i: usize) -> f64 {
        match i {
            0 => self.kick,
            1 => self.attack,
            _ => self.flux,
        }
    }
    fn sum(self) -> f64 {
        self.kick + self.attack + self.flux
    }
}

impl Reader<'_> {
    /// Everything heard at a beat predicted at envelope sample `x` of a
    /// grid with `period`: peaks within a tenth of a beat in the kick band
    /// and the full band, and the attack within the map's reach. A source
    /// the reader does not have reads as zero.
    fn evidence(&self, x: f64, period: f64) -> Evidence {
        let reach = (period * 0.1).max(1.0);
        let secs = self.origin_secs + x / self.rate;
        let kick = self.kicks.map_or(0.0, |kicks| kick_peak(kicks, (secs - kicks.origin_secs) * kicks.rate, reach));
        let attack = self.attacks.and_then(|a| a.attack_near(secs)).map_or(0.0, |a| a.height);
        let flux = local_peak(self.values, x, reach).map_or(0.0, |(_, h)| h);
        Evidence { kick, attack, flux }
    }

    /// The beat nearest `x` (an envelope sample), as an envelope sample and
    /// a weight, from the attack map if there is one and it finds a spike,
    /// else from the envelope.
    fn snap(&self, x: f64, reach: f64) -> Option<(f64, f64)> {
        if let Some(attacks) = self.attacks {
            let secs = self.origin_secs + x / self.rate;
            // The attack search is capped at the map's own reach: the
            // prediction is already within a few milliseconds of the hit,
            // and a wider window catches the clap after the kick, which
            // the line then chases pass after pass.
            let reach_secs = (reach / self.rate).min(attacks.options().reach_secs);
            if let Some(attack) = attacks.attack_within(secs, reach_secs) {
                return Some(((attack.secs - self.origin_secs) * self.rate, attack.height));
            }
            return None;
        }
        local_peak(self.values, x, reach)
    }
}

/// Estimates tempo with the candidate stage under the caller's control,
/// placing each beat on the kick's attack when an attack map is given, and,
/// when the kick band's envelope is given, placing a tempo change where it
/// says the new tempo's beat has arrived and judging which half of the
/// beat a stretch's kicks are on.
/// Transitions without usable kick or click timing can follow emphasised
/// full-band transients with a short release; settled fits use the originals.
#[allow(clippy::needless_pass_by_value, reason = "a Copy options struct")]
pub fn detect_tempo_with(
    onsets: &OnsetEnvelope,
    kicks: Option<&OnsetEnvelope>,
    attacks: Option<&AttackMap>,
    options: TempoOptions,
) -> TempoResult {
    detect_tempo_traced(onsets, kicks, attacks, options, None)
}

/// What the gap stage decided at one gap, for measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapDecision {
    /// The last beat with a hit before the gap, in seconds.
    pub last_supported_secs: f64,
    /// Where the old line has two bars of hits again, if it does.
    pub resumes_secs: Option<f64>,
    /// Whether the stretch after the gap sits on another phase, and by
    /// how much, in ms.
    pub moved: bool,
    pub shift_ms: f64,
    /// Beats the ramp walk placed, and how far its period got from the
    /// line's, as a fraction.
    pub walked: usize,
    pub departure: f64,
    /// Where the grid was cut, if it was.
    pub cut_secs: Option<f64>,
}

/// [`detect_tempo_with`], recording what the gap stage decided.
pub fn detect_tempo_traced(
    onsets: &OnsetEnvelope,
    kicks: Option<&OnsetEnvelope>,
    attacks: Option<&AttackMap>,
    options: TempoOptions,
    trace: Option<&mut Vec<GapDecision>>,
) -> TempoResult {
    if onsets.len() < 64 || onsets.rate <= 0.0 {
        return TempoResult::empty();
    }
    let values: Vec<f64> = onsets.values.iter().map(|&v| f64::from(v)).collect();
    let attacks = match options.placement {
        Placement::Attack => attacks,
        Placement::Envelope => None,
    };
    let kicks = kicks.filter(|k| k.rate > 0.0 && !k.is_empty());
    let reader = Reader { values: &values, rate: onsets.rate, origin_secs: onsets.origin_secs, attacks, kicks };

    let candidates = candidates(&values, onsets.rate, options);
    let Some(best) = candidates.first() else {
        return TempoResult::empty();
    };
    // Confidence: the winner against the best candidate that is not a
    // multiple of it, both of them prior-free.
    let rival = candidates
        .iter()
        .skip(1)
        .find(|c| !related(c.bpm, best.bpm))
        .map_or(0.0, |c| c.acf * c.fourier);
    let own = best.acf * best.fourier;
    let confidence = if own > 0.0 { ((own - rival) / own).clamp(0.0, 1.0) } else { 0.0 };

    // The whole track at the winning tempo, then split where it changes.
    let segments = segment(reader, best.bpm, options, trace);
    let beats = beats_of(&segments, 0);
    let (bpm, first_beat_secs) = segments.first().map_or((0.0, 0.0), |s| (s.bpm(), s.start_secs()));
    TempoResult { bpm, confidence, first_beat_secs, segments, beats }
}

/// The candidate stage alone, best first. For measurement.
pub fn tempo_candidates(onsets: &OnsetEnvelope, options: TempoOptions) -> Vec<Candidate> {
    if onsets.len() < 64 || onsets.rate <= 0.0 {
        return Vec::new();
    }
    let values: Vec<f64> = onsets.values.iter().map(|&v| f64::from(v)).collect();
    candidates(&values, onsets.rate, options)
}

/// Whether two tempos are a simple ratio of each other.
fn related(a: f64, b: f64) -> bool {
    if a <= 0.0 || b <= 0.0 {
        return false;
    }
    let ratio = if a > b { a / b } else { b / a };
    [1.0, 2.0, 3.0, 4.0, 1.5, 4.0 / 3.0]
        .iter()
        .any(|r| (ratio - r).abs() < 0.02 * r)
}

// ---------------------------------------------------------------- candidates

/// Every tempo worth considering, scored, best first.
fn candidates(values: &[f64], rate: f64, options: TempoOptions) -> Vec<Candidate> {
    let n = values.len();
    let mean = values.iter().sum::<f64>() / n as f64;
    let centred: Vec<f64> = values.iter().map(|v| v - mean).collect();
    let variance = centred.iter().map(|v| v * v).sum::<f64>() / n as f64;
    if variance <= 0.0 {
        return Vec::new();
    }

    let lag_of = |bpm: f64| rate * 60.0 / bpm;
    // Lags out to twice the slowest tempo, so a candidate's own double can be
    // read for whichever candidate asks.
    let min_lag = (lag_of(options.max_bpm * 2.0).floor() as usize).max(2);
    let max_lag = (lag_of(options.min_bpm / 2.0).ceil() as usize).min(n / 2);
    if max_lag <= min_lag + 2 {
        return Vec::new();
    }
    let acf = autocorrelation(&centred, variance, min_lag, max_lag);
    let acf_at = |lag: f64| interpolate(&acf, lag - min_lag as f64);

    // Peaks of the autocorrelation inside the range, with their multiples
    // and simple fractions, so that the octave a listener would choose is
    // in the running even when it is not the strongest correlation.
    let in_range = |bpm: f64| bpm >= options.min_bpm && bpm <= options.max_bpm;
    let mut bpms: Vec<f64> = Vec::new();
    let lo = lag_of(options.max_bpm).floor() as usize;
    let hi = lag_of(options.min_bpm).ceil() as usize;
    for lag in lo.max(min_lag + 1)..hi.min(max_lag - 1) {
        let here = acf_at(lag as f64);
        if here <= acf_at(lag as f64 - 1.0) || here < acf_at(lag as f64 + 1.0) || here <= 0.0 {
            continue;
        }
        // Parabolic interpolation of the peak.
        let a = acf_at(lag as f64 - 1.0);
        let c = acf_at(lag as f64 + 1.0);
        let denom = a - 2.0 * here + c;
        let offset = if denom.abs() > f64::EPSILON { 0.5 * (a - c) / denom } else { 0.0 };
        let bpm = rate * 60.0 / (lag as f64 + offset.clamp(-0.5, 0.5));
        for ratio in [1.0, 2.0, 0.5, 1.5, 2.0 / 3.0, 3.0, 1.0 / 3.0, 4.0 / 3.0, 0.75] {
            let relative = bpm * ratio;
            if in_range(relative) && !bpms.iter().any(|b| (b - relative).abs() < relative * 0.01) {
                bpms.push(relative);
            }
        }
    }
    if bpms.is_empty() {
        return Vec::new();
    }

    // The Fourier magnitude at every candidate rate, and the strongest in
    // range to normalise by.
    let fourier_raw: Vec<f64> = bpms.iter().map(|&bpm| fourier_magnitude(&centred, rate, bpm)).collect();
    let fourier_peak = fourier_raw.iter().fold(0.0_f64, |a, &b| a.max(b));
    let acf_peak = bpms.iter().map(|&bpm| acf_at(lag_of(bpm))).fold(0.0_f64, f64::max);

    let mut out: Vec<Candidate> = bpms
        .iter()
        .zip(&fourier_raw)
        .map(|(&bpm, &ft)| {
            let acf = (acf_at(lag_of(bpm)) / acf_peak.max(f64::EPSILON)).max(0.0);
            let fourier = if fourier_peak > 0.0 { ft / fourier_peak } else { 0.0 };
            let prior = tempo_prior(bpm, options);
            // The Fourier term enters as a square root: its job is to rule
            // out a period with no spectral line at all (a hundredth of the
            // strongest), not to prefer the hat rate over the beat, where
            // it is louder by half.
            Candidate { bpm, acf, fourier, prior, score: acf * fourier.sqrt() * prior }
        })
        .collect();
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Normalised autocorrelation for lags `min_lag..=max_lag`.
fn autocorrelation(centred: &[f64], variance: f64, min_lag: usize, max_lag: usize) -> Vec<f64> {
    let n = centred.len();
    (min_lag..=max_lag)
        .map(|lag| {
            let count = n.saturating_sub(lag);
            if count == 0 {
                return 0.0;
            }
            let sum: f64 = centred.iter().zip(centred.iter().skip(lag)).map(|(a, b)| a * b).sum();
            sum / (count as f64 * variance)
        })
        .collect()
}

/// Linear interpolation into a table, clamped at the ends.
fn interpolate(table: &[f64], x: f64) -> f64 {
    if table.is_empty() {
        return 0.0;
    }
    let x = x.clamp(0.0, (table.len() - 1) as f64);
    let i = x.floor() as usize;
    let frac = x - i as f64;
    let a = table.get(i).copied().unwrap_or(0.0);
    let b = table.get(i + 1).copied().unwrap_or(a);
    a + (b - a) * frac
}

/// Seconds per window of the Fourier magnitude.
///
/// Not the whole track: a sum over five minutes resolves 0.2 BPM, so a
/// candidate read off the autocorrelation a tenth of a BPM from the truth
/// drifts through most of a cycle and cancels itself. Twenty-second windows
/// resolve 3 BPM, which is coarse enough to be robust to that and fine
/// enough to keep a tempo apart from its three-halves relative.
const FOURIER_WINDOW_SECS: f64 = 20.0;

/// Magnitude of the envelope's Fourier component at one tempo, averaged
/// over Hann windows of `FOURIER_WINDOW_SECS`.
fn fourier_magnitude(centred: &[f64], rate: f64, bpm: f64) -> f64 {
    let omega = 2.0 * std::f64::consts::PI * bpm / 60.0 / rate;
    let window = ((FOURIER_WINDOW_SECS * rate) as usize).clamp(64, centred.len().max(64));
    let hop = window / 2;
    // A rotating phasor rather than a sin and cos per sample: the envelope
    // is fifty thousand samples long and there are dozens of candidates.
    let (step_re, step_im) = (omega.cos(), omega.sin());
    let mut total = 0.0;
    let mut windows = 0.0;
    let mut start = 0;
    while start + window <= centred.len() {
        let (mut re, mut im) = (1.0_f64, 0.0_f64);
        let (mut sum_re, mut sum_im) = (0.0_f64, 0.0_f64);
        for (i, &v) in centred[start..start + window].iter().enumerate() {
            let hann = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / window as f64).cos();
            let w = v * hann;
            sum_re += w * re;
            sum_im += w * im;
            let next_re = re * step_re - im * step_im;
            im = re * step_im + im * step_re;
            re = next_re;
        }
        total += (sum_re * sum_re + sum_im * sum_im).sqrt() / window as f64;
        windows += 1.0;
        start += hop;
        if start + window > centred.len() && windows == 0.0 {
            break;
        }
    }
    if windows > 0.0 { total / windows } else { 0.0 }
}

/// How readily a tempo is heard *as* the tempo: a log-normal centred where
/// dance music sits. It only breaks ties between octaves.
fn tempo_prior(bpm: f64, options: TempoOptions) -> f64 {
    if bpm <= 0.0 || options.prior_width <= 0.0 || options.prior_centre <= 0.0 {
        return 0.0;
    }
    let x = (bpm / options.prior_centre).ln() / options.prior_width;
    (-0.5 * x * x).exp()
}

// ---------------------------------------------------------------- fitting

/// A grid fitted to one stretch of the envelope: period and phase in
/// envelope samples.
#[derive(Debug, Clone, Copy)]
struct Fit {
    /// Envelope sample of the first beat, which may be negative: the grid is
    /// extended back to the start of the file.
    phase: f64,
    period: f64,
}

/// Fits a constant-tempo grid to `from..to` of the envelope near `bpm`.
fn fit(reader: Reader<'_>, bpm: f64, from: usize, to: usize, options: TempoOptions) -> Option<Fit> {
    let slice = reader.values.get(from..to)?;
    if slice.len() < 8 {
        return None;
    }
    let coarse = reader.rate * 60.0 / bpm;
    let period = refine_period(slice, coarse, options.refine_phases);
    let phase = best_phase(slice, period, 64);
    // The comb's phase may be on the off-beat: hats and an off-beat bass
    // carry as much flux as the kick. With the kick attacks to read, the
    // line is fitted from both halves of the beat and the one that collects
    // more kick is the grid. Measured on the golden playlist, the kick
    // detector tells the beat from the midpoint on 150 of 155 rekordbox
    // grids.
    let starts: &[f64] = if reader.attacks.is_some() { &[0.0, 0.5] } else { &[0.0] };
    let mut best: Option<(Fit, f64)> = None;
    for &half in starts {
        let mut fit = Fit { phase: phase + half * period + from as f64, period };
        let mut support = 0.0;
        // Snap and refit, three times: each pass moves the line a little
        // closer to the onsets and the next pass then snaps a few more.
        for _ in 0..3 {
            let Some((next, kept)) = snap_and_fit(reader, from, to, fit, None) else { break };
            fit = next;
            support = kept;
        }
        if best.is_none_or(|(_, s)| support > s) {
            best = Some((fit, support));
        }
    }
    best.map(|(fit, _)| whole_bpm(reader, from, to, fit))
}

/// A steady tempo measured within this of a whole number of BPM may be that
/// whole number. Dance music is produced at whole tempos: every one of the
/// 155 golden tracks is, and our fit lands within 0.05 of the whole number
/// on all of them.
const WHOLE_BPM_TOLERANCE: f64 = 0.1;

/// The share of the measured line's hits the whole-number line has to sit
/// on for the whole number to be taken.
///
/// A fit a few hundredths from a whole number is either a sequenced track
/// the fit got slightly wrong, where the whole number sits on as many hits
/// as the fit, or a track that really runs at 173.97 or 107.95 (a band, a
/// turntable, a remaster), where the whole number drifts off the hits a
/// little more every bar. Rekordbox measures those to the hundredth and
/// does not round them (its Normal analysis searches the tempo in 0.002 BPM
/// steps around its estimate, `BeatAnalyzer_1_0::UnitBeatAdjust::bpmAdjust`),
/// and 0.03 BPM is a sixth of a beat of drift over five minutes. Measured
/// on 145 hard dance and drum & bass and 168 band tracks against
/// rekordbox's own grids, and on the 155 golden tracks: at 0.85 the golden
/// grids hold (one is an edit whose half-level second kick pattern pulls
/// its first tempo's fit 0.04 BPM off, leaving the whole number on 88 % as
/// many hits), and thirteen drum & bass tracks get rekordbox's 173.97 to
/// 174.01 instead of 174.00.
const WHOLE_BPM_SUPPORT: f64 = 0.85;

/// The fit with its period snapped to the whole number of BPM it is
/// within `WHOLE_BPM_TOLERANCE` of, and its phase refitted through the
/// same hits at that period, so the line turns about the hits' centre
/// rather than its first beat. A fit further from a whole number is left
/// alone: a tempo like 127.7 is rare but real. So is one a few hundredths
/// from a whole number, where the whole number's line sits on clearly
/// fewer of the hits than the measured one (`WHOLE_BPM_SUPPORT`).
fn whole_bpm(reader: Reader<'_>, from: usize, to: usize, fit: Fit) -> Fit {
    if fit.period <= 0.0 {
        return fit;
    }
    let bpm = reader.rate * 60.0 / fit.period;
    let whole = bpm.round();
    if whole <= 0.0 || (bpm - whole).abs() > WHOLE_BPM_TOLERANCE {
        return fit;
    }
    let period = reader.rate * 60.0 / whole;
    let start = Fit { phase: fit.phase, period };
    let snapped = snap_and_fit(reader, from, to, start, Some(period)).map_or(start, |(f, _)| f);
    if on_line(reader, from, to, snapped) < on_line(reader, from, to, fit) * WHOLE_BPM_SUPPORT {
        return fit;
    }
    snapped
}

/// How far from a line a hit may be and still count towards it, in seconds.
const ON_LINE_SECS: f64 = 0.010;

/// How much of the music a line sits on: the hit nearest each of its beats
/// in `from..to`, by strength, counted in full on the line and less the
/// further it is from it, down to nothing at `ON_LINE_SECS`.
fn on_line(reader: Reader<'_>, from: usize, to: usize, f: Fit) -> f64 {
    if f.period < 2.0 {
        return 0.0;
    }
    let reach = (f.period * 0.2).max(1.0);
    let tolerance = ON_LINE_SECS * reader.rate;
    let first = ((from as f64 - f.phase) / f.period).ceil() as i64;
    let last = ((to as f64 - 1.0 - f.phase) / f.period).floor() as i64;
    (first..=last)
        .filter_map(|k| {
            let predicted = f.phase + k as f64 * f.period;
            let (at, height) = reader.snap(predicted, reach)?;
            Some(height * (1.0 - (at - predicted).abs() / tolerance).max(0.0))
        })
        .sum()
}

/// Comb-filter score of a grid: onset energy summed at every beat of the
/// period from `phase`, normalised per beat.
fn comb(values: &[f64], period: f64, phase: f64) -> f64 {
    if period < 2.0 || values.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut count = 0.0;
    let mut x = phase;
    while x < values.len() as f64 {
        sum += sample_at(values, x);
        count += 1.0;
        x += period;
    }
    if count > 0.0 { sum / count } else { 0.0 }
}

/// Linear interpolation between envelope samples.
fn sample_at(values: &[f64], x: f64) -> f64 {
    if x < 0.0 {
        return 0.0;
    }
    let i = x.floor() as usize;
    let frac = x - i as f64;
    let a = values.get(i).copied().unwrap_or(0.0);
    let b = values.get(i + 1).copied().unwrap_or(0.0);
    a + (b - a) * frac
}

/// Finds the fractional period near `coarse` that best explains the onsets.
///
/// Integer lags quantise the tempo badly: at 172 envelope samples per second
/// the lags either side of 128 BPM are 1.6 BPM apart. A fine search over
/// fractional periods, each scored at its best phase, removes that.
fn refine_period(values: &[f64], coarse: f64, phases: usize) -> f64 {
    let score = |period: f64| {
        let steps = phases.max(1);
        (0..steps)
            .map(|step| comb(values, period, period * step as f64 / steps as f64))
            .fold(0.0_f64, f64::max)
    };
    let mut best = (score(coarse), coarse);
    // Two passes: a coarse sweep of ±1 sample, then a fine one around the
    // winner. The sweep is what costs, so it is kept to a few hundred combs.
    for (span, step) in [(1.0, 0.02), (0.03, 0.002)] {
        let centre = best.1;
        let mut candidate = centre - span;
        while candidate <= centre + span {
            let s = score(candidate);
            if s > best.0 {
                best = (s, candidate);
            }
            candidate += step;
        }
    }
    best.1
}

/// The phase in `0..period` at which the comb collects the most energy.
fn best_phase(values: &[f64], period: f64, phases: usize) -> f64 {
    let steps = phases.max(1);
    let mut best = (f64::NEG_INFINITY, 0.0);
    for step in 0..steps {
        let phase = period * step as f64 / steps as f64;
        let s = comb(values, period, phase);
        if s > best.0 {
            best = (s, phase);
        }
    }
    // Sharpen with a parabola through the neighbours: the comb is smooth in
    // phase at the scale of one step.
    let step = period / steps as f64;
    let (a, b, c) = (
        comb(values, period, best.1 - step),
        best.0,
        comb(values, period, best.1 + step),
    );
    let denom = a - 2.0 * b + c;
    let offset = if denom.abs() > f64::EPSILON { 0.5 * (a - c) / denom } else { 0.0 };
    best.1 + offset.clamp(-1.0, 1.0) * step
}

/// Snaps every predicted beat in `from..to` to the nearest onset — the
/// kick's attack when there is an attack map, else the envelope's peak —
/// and fits a line through the snapped ones, weighted by the hit's
/// strength.
///
/// Also returns the total strength of the hits the line was fitted
/// through, so two candidate lines can be compared.
///
/// With `fixed_period`, only the phase is fitted: the line is kept at that
/// period and placed through the hits' weighted centre.
fn snap_and_fit(reader: Reader<'_>, from: usize, to: usize, current: Fit, fixed_period: Option<f64>) -> Option<(Fit, f64)> {
    let period = current.period;
    if period < 2.0 {
        return None;
    }
    // A beat may be snapped this far: less than half a beat, so that two
    // predictions cannot claim one onset, and less than a hat's distance.
    let reach = (period * 0.2).max(1.0);
    let first = ((from as f64 - current.phase) / period).ceil() as i64;
    let last = ((to as f64 - 1.0 - current.phase) / period).floor() as i64;
    if last < first + 4 {
        return None;
    }

    // Every predicted beat snapped to its nearest onset peak, with the
    // peak's height as its weight: a beat in a breakdown, with no onset to
    // snap to, should not pull the line.
    let mut snapped: Vec<(f64, f64, f64)> = Vec::new(); // (index, time, weight)
    for k in first..=last {
        let predicted = current.phase + k as f64 * period;
        if let Some((at, height)) = reader.snap(predicted, reach) {
            snapped.push((k as f64, at, height));
        }
    }
    if snapped.len() < 4 {
        return None;
    }
    // Beats with next to no onset are left out altogether rather than
    // merely down-weighted. An intro of pads has a noise-floor peak near
    // every predicted beat, and a minute of those, however light, tilts a
    // line that is then extrapolated back across the same minute.
    let mut heights: Vec<f64> = snapped.iter().map(|&(_, _, h)| h).collect();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let floor = heights.get(heights.len() / 2).copied().unwrap_or(0.0) * 0.2;
    let strong: Vec<(f64, f64, f64)> = snapped.iter().copied().filter(|&(_, _, h)| h >= floor).collect();
    let snapped = if strong.len() >= 4 { strong } else { snapped };

    // Weighted least squares of snapped time on beat index, twice: the
    // second pass leaves out the beats that sit furthest from the first
    // line. A section of swung or late-hitting percussion snaps its beats
    // consistently off the grid, and left in, it bends a whole track's
    // tempo by a hundredth of a BPM, which is a beat of drift by the end.
    let line = |points: &[(f64, f64, f64)]| -> Option<(f64, f64)> {
        let (mut sw, mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for &(x, y, w) in points {
            sw += w;
            sx += w * x;
            sy += w * y;
            sxx += w * x * x;
            sxy += w * x * y;
        }
        if sw <= 0.0 {
            return None;
        }
        if let Some(slope) = fixed_period {
            return Some((slope, (sy - slope * sx) / sw));
        }
        let denom = sw * sxx - sx * sx;
        if denom.abs() < f64::EPSILON {
            return None;
        }
        let slope = (sw * sxy - sx * sy) / denom;
        Some((slope, (sy - slope * sx) / sw))
    };
    let (slope, intercept) = line(&snapped)?;
    let mut residuals: Vec<f64> = snapped.iter().map(|&(x, y, _)| (y - intercept - slope * x).abs()).collect();
    residuals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Keep the closest four fifths, and never anything further than a
    // tenth of a beat from the line.
    let cutoff = residuals
        .get(residuals.len() * 4 / 5)
        .copied()
        .unwrap_or(f64::INFINITY)
        .min(period * 0.1);
    let kept: Vec<(f64, f64, f64)> = snapped
        .iter()
        .copied()
        .filter(|&(x, y, _)| (y - intercept - slope * x).abs() <= cutoff)
        .collect();
    let (slope, intercept) = if kept.len() >= 4 { line(&kept)? } else { (slope, intercept) };

    // A fit that walked away from the candidate period is a fit to the wrong
    // onsets; keep the old one.
    if (slope - period).abs() > period * 0.02 {
        return None;
    }
    let support = kept.iter().map(|&(_, _, w)| w).sum::<f64>();
    Some((Fit { phase: intercept, period: slope }, support))
}

/// The highest envelope sample within `reach` of `around`, with its
/// position sharpened by a parabola through its neighbours.
fn local_peak(values: &[f64], around: f64, reach: f64) -> Option<(f64, f64)> {
    let lo = (around - reach).floor().max(0.0) as usize;
    let hi = ((around + reach).ceil() as usize).min(values.len().saturating_sub(1));
    if lo >= hi {
        return None;
    }
    let mut best = (0.0_f64, lo);
    for i in lo..=hi {
        let v = values.get(i).copied().unwrap_or(0.0);
        if v > best.0 {
            best = (v, i);
        }
    }
    if best.0 <= 0.0 {
        return None;
    }
    let i = best.1;
    let a = if i > 0 { values.get(i - 1).copied().unwrap_or(0.0) } else { best.0 };
    let c = values.get(i + 1).copied().unwrap_or(0.0);
    let denom = a - 2.0 * best.0 + c;
    let offset = if denom.abs() > f64::EPSILON { 0.5 * (a - c) / denom } else { 0.0 };
    Some((i as f64 + offset.clamp(-0.5, 0.5), best.0))
}

/// The highest sample of the kick band within `reach` of `around`, both in
/// that envelope's own samples; zero outside it.
fn kick_peak(kicks: &OnsetEnvelope, around: f64, reach: f64) -> f64 {
    let lo = (around - reach).floor().max(0.0) as usize;
    let hi = ((around + reach).ceil().max(0.0) as usize).min(kicks.len().saturating_sub(1));
    if lo > hi {
        return 0.0;
    }
    kicks.values.get(lo..=hi).map_or(0.0, |s| s.iter().fold(0.0_f64, |m, &v| m.max(f64::from(v))))
}

// ---------------------------------------------------------------- segments

/// The tempo of each window along the track as a ratio to `bpm`, with the
/// window's start in envelope samples. For measurement.
pub fn local_tempos(onsets: &OnsetEnvelope, bpm: f64, options: TempoOptions) -> Vec<(f64, Option<f64>)> {
    let values: Vec<f64> = onsets.values.iter().map(|&v| f64::from(v)).collect();
    let window = ((options.segment_window_secs * onsets.rate) as usize).max(64);
    let hop = window / 2;
    let mut out = Vec::new();
    let mut start = 0;
    while start + window <= values.len() {
        out.push((onsets.time_of(start as f64), local_tempo(&values[start..start + window], onsets.rate, bpm, options)));
        start += hop;
    }
    out
}

/// How the fit stage arrives at a segment's tempo, for measurement: for
/// each starting half of the beat, the BPM after each snap-and-refit pass
/// and the support the final line collected.
pub fn fit_report(onsets: &OnsetEnvelope, attacks: Option<&AttackMap>, bpm: f64, options: TempoOptions) -> Vec<(Vec<f64>, f64)> {
    let values: Vec<f64> = onsets.values.iter().map(|&v| f64::from(v)).collect();
    let n = values.len();
    let reader = Reader { values: &values, rate: onsets.rate, origin_secs: onsets.origin_secs, attacks, kicks: None };
    let coarse = onsets.rate * 60.0 / bpm;
    let period = refine_period(&values, coarse, options.refine_phases);
    let phase = best_phase(&values, period, 64);
    [0.0, 0.5]
        .iter()
        .map(|&half| {
            let mut passes = vec![onsets.rate * 60.0 / period];
            let mut fit = Fit { phase: phase + half * period, period };
            let mut support = 0.0;
            for _ in 0..3 {
                let Some((next, kept)) = snap_and_fit(reader, 0, n, fit, None) else { break };
                fit = next;
                support = kept;
                passes.push(onsets.rate * 60.0 / fit.period);
            }
            (passes, support)
        })
        .collect()
}

/// The walk between two tempos, for measurement: every beat it placed
/// from `from_secs`, as `(seconds, bpm from the previous beat)`, and why it
/// stopped.
pub fn walk_report(
    onsets: &OnsetEnvelope,
    attacks: Option<&AttackMap>,
    from_bpm: f64,
    to_bpm: f64,
    from_secs: f64,
    to_secs: f64,
) -> (Vec<(f64, f64)>, &'static str) {
    let values: Vec<f64> = onsets.values.iter().map(|&v| f64::from(v)).collect();
    let reader = Reader { values: &values, rate: onsets.rate, origin_secs: onsets.origin_secs, attacks, kicks: None };
    let x_of = |secs: f64| (secs - onsets.origin_secs) * onsets.rate;
    let a_period = onsets.rate * 60.0 / from_bpm;
    let b_period = onsets.rate * 60.0 / to_bpm;
    let transients = TransitionTransients::new(reader, x_of(from_secs).max(0.0) as usize, x_of(to_secs).max(0.0) as usize);
    // Phase the old grid from the strongest hit near `from_secs`.
    let mut t = transients.snap(reader, x_of(from_secs), a_period, a_period * 0.5).map_or(x_of(from_secs), |(at, _)| at);
    let mut period = a_period;
    let mut out = Vec::new();
    let mut why = "reached the end";
    while out.len() < MAX_WALKED_BEATS && t < x_of(to_secs) {
        let predicted = t + period;
        let Some((next, _)) = transients.snap(reader, predicted, period, (RAMP_REACH * period).max(1.0)) else {
            why = "no hit within reach";
            break;
        };
        let next_period = next - t;
        if next_period <= 0.0 || (next_period - period).abs() > period * RAMP_STEP {
            why = "jump larger than the ramp step";
            out.push((onsets.time_of(next), onsets.rate * 60.0 / next_period));
            break;
        }
        period = next_period;
        t = next;
        out.push((onsets.time_of(t), onsets.rate * 60.0 / period));
        if (period - b_period).abs() <= b_period * SETTLED_TOLERANCE {
            why = "settled";
            break;
        }
    }
    (out, why)
}

/// Windows in a row that must agree on a new tempo before it is believed.
/// Three of them, hopping half a window, cover two windows' worth of
/// track: a shorter stretch is a fill, not a section.
const SEGMENT_MIN_WINDOWS: usize = 3;

/// Whether a ratio between two tempos is one a rhythm produces on its own.
///
/// A dotted-eighth delay puts a real period at four thirds of the beat; a
/// triplet feel at three halves. Windows that measure such a period have not
/// changed tempo, and a DJ edit that happens to jump by exactly that ratio
/// is rarer than the pattern. With interpolated local estimates, a 1%
/// neighbourhood preserves that guard without swallowing nearby real
/// changes such as 128 to 174 (about 2% away from four thirds).
fn rhythmic_ratio(ratio: f64) -> bool {
    [1.5, 2.0 / 3.0, 4.0 / 3.0, 0.75].iter().any(|r| (ratio - r).abs() < 0.01 * r)
}

/// Labels every window with a tempo and cuts the track into stretches to
/// fit: `(settled from, settled to, bpm, where the change after it may
/// begin)`, in order.
///
/// The tempos the track holds are the track's own plus any ratio that at
/// least `SEGMENT_MIN_WINDOWS` windows agree on (within the threshold),
/// that differs from the track's, and that is not a ratio a rhythm
/// produces. Every window is assigned to the nearest tempo or carries the
/// previous window's when it says nothing clearly; a lone window between
/// two of the other tempo is absorbed; leading windows that say nothing
/// take the first tempo heard. A stretch's settled part is its assigned
/// windows: the carried ones between two tempos are the change.
fn label_runs(
    local: &[Option<f64>],
    starts: &[usize],
    window: usize,
    n: usize,
    bpm: f64,
    options: TempoOptions,
) -> Vec<(usize, usize, f64, usize)> {
    let differs = |ratio: f64| (ratio - 1.0).abs() > options.segment_threshold && !rhythmic_ratio(ratio);
    let mut clusters: Vec<(f64, usize)> = Vec::new(); // (mean ratio, count)
    for ratio in local.iter().flatten().copied().filter(|&r| differs(r)) {
        // A new steady tempo needs repeatable measurements, not a broad
        // cluster of drifting rhythmic aliases. Use half the change
        // threshold relative to that candidate (1% by default).
        match clusters.iter_mut().find(|(centre, _)| (ratio - *centre).abs() < options.segment_threshold * *centre * 0.5) {
            Some((centre, count)) => {
                *centre = (*centre * *count as f64 + ratio) / (*count as f64 + 1.0);
                *count += 1;
            }
            None => clusters.push((ratio, 1)),
        }
    }
    clusters.retain(|&(_, count)| count >= SEGMENT_MIN_WINDOWS);

    let assigned: Vec<Option<f64>> = local
        .iter()
        .map(|ratio| {
            ratio.and_then(|r| {
                std::iter::once(1.0)
                    .chain(clusters.iter().map(|&(c, _)| c))
                    .filter(|c| (r - c).abs() < options.segment_threshold * 1.5)
                    .min_by(|a, b| (r - a).abs().partial_cmp(&(r - b).abs()).unwrap_or(std::cmp::Ordering::Equal))
            })
        })
        .collect();
    let mut previous = assigned.iter().flatten().next().copied().unwrap_or(1.0);
    let mut labels: Vec<f64> = vec![1.0; local.len()];
    for (i, value) in assigned.iter().enumerate() {
        previous = value.unwrap_or(previous);
        labels[i] = previous;
    }
    for i in 1..labels.len().saturating_sub(1) {
        if (labels[i - 1] - labels[i + 1]).abs() < 1e-9 && (labels[i] - labels[i - 1]).abs() > 1e-9 {
            labels[i] = labels[i - 1];
        }
    }
    // A run is a tempo only where at least `SEGMENT_MIN_WINDOWS` windows in
    // a row measured it. The cluster test above counts agreeing windows
    // anywhere in the track, so a lone window near a real second tempo's
    // ratio could otherwise start a stretch of its own, hundreds of beats
    // long, in the middle of the first tempo. Such a run takes the label
    // of the run before it (the one after, at the start of the track), and
    // the check repeats until every run is earned.
    loop {
        let runs = run_bounds(&labels);
        if runs.len() < 2 {
            break;
        }
        let settled_windows = |(start, end): (usize, usize)| {
            (start..end).filter(|&w| assigned.get(w).copied().flatten().is_some_and(|r| (r - labels[start]).abs() < 1e-9)).count()
        };
        let Some((index, &(start, end))) = runs.iter().enumerate().find(|(_, &r)| settled_windows(r) < SEGMENT_MIN_WINDOWS) else {
            break;
        };
        let label = if index == 0 { labels[end] } else { labels[start - 1] };
        for l in &mut labels[start..end] {
            *l = label;
        }
    }

    let mut runs: Vec<(usize, usize, f64, usize)> = Vec::new();
    let mut i = 0;
    while i < labels.len() {
        let mut end = i;
        while end < labels.len() && (labels[end] - labels[i]).abs() < 1e-9 {
            end += 1;
        }
        let is_settled = |w: usize| assigned.get(w).copied().flatten().is_some_and(|r| (r - labels[i]).abs() < 1e-9);
        let first_settled = (i..end).find(|&w| is_settled(w)).unwrap_or(i);
        let last_settled = (i..end).rev().find(|&w| is_settled(w)).unwrap_or(end - 1);
        let from = if i == 0 { 0 } else { starts.get(first_settled).copied().unwrap_or(0) };
        let to = if end >= labels.len() { n } else { (starts[last_settled] + window).min(n) };
        // A window still settled at this tempo may end past the change;
        // the change is looked for from that window's start.
        let change_from = starts.get(last_settled).copied().unwrap_or(from);
        runs.push((from, to, bpm * labels[i], change_from));
        i = end;
    }
    if runs.is_empty() {
        runs.push((0, n, bpm, n));
    }
    runs
}

/// The stretches of equal labels, as `(start, end)` window indices.
fn run_bounds(labels: &[f64]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < labels.len() {
        let mut end = i;
        while end < labels.len() && (labels[end] - labels[i]).abs() < 1e-9 {
            end += 1;
        }
        out.push((i, end));
        i = end;
    }
    out
}

/// Splits the track where the tempo changes and fits each stretch.
fn segment(reader: Reader<'_>, bpm: f64, options: TempoOptions, trace: Option<&mut Vec<GapDecision>>) -> Vec<Segment> {
    let (values, rate, origin_secs) = (reader.values, reader.rate, reader.origin_secs);
    let n = values.len();
    if !options.follow_changes {
        // One line through the whole track, from the start of the file to
        // its end, whatever the tempo does along the way.
        return fit(reader, bpm, 0, n, options)
            .map(|f| Segment { from_secs: 0.0, to_secs: to_secs_of(reader, n as f64), period_secs: f.period / rate, phase_secs: to_secs_of(reader, f.phase) })
            .into_iter()
            .collect();
    }
    let window = ((options.segment_window_secs * rate) as usize).max(64);
    // Local tempo per window, as a ratio to the track's, or None where the
    // window has too little to say.
    let hop = window / 2;
    let mut local: Vec<Option<f64>> = Vec::new();
    let mut starts: Vec<usize> = Vec::new();
    let mut start = 0;
    while start + window <= n {
        starts.push(start);
        local.push(local_tempo(&values[start..start + window], rate, bpm, options));
        start += hop;
    }

    let mut segments: Vec<Segment> = Vec::new();
    let runs = label_runs(&local, &starts, window, n, bpm, options);
    tracing::debug!(bpm, ?local, ?runs, "local tempo segments");

    // (change from, settled from, settled to, fit)
    let mut fits: Vec<(usize, usize, usize, Fit)> = Vec::new();
    for (from, to, run_bpm, change_from) in runs {
        let Some(f) = fit(reader, run_bpm, from, to, options) else { continue };
        fits.push((change_from, from, to, f));
    }
    if fits.is_empty() {
        return segments;
    }

    // Between each pair of fits: walk the change beat by beat from the old
    // grid until four beats agree with the new tempo and phase. Each
    // measured interval is its own segment; then use the settled fit.
    // Where neither kicks nor emphasised transients carry the walk, place
    // a cut using the support for the old and new grids.
    let to_secs = |x: f64| origin_secs + x / rate;
    let mut from_sample = -origin_secs * rate;
    let mut pending: Option<Fit> = None; // the fit whose segment is open
    // A fit over a stretch is refitted over the stretch its segment
    // actually covers, once that is known: the settled windows alone can
    // be a short stretch, and every hit up to the change sharpens the
    // line, with the snapping leaving out the hits that have drifted.
    let refit = |current: Fit, from: f64, to: f64| -> Fit {
        let bpm_here = rate * 60.0 / current.period;
        let (from_i, to_i) = (from.max(0.0) as usize, (to as usize).min(n));
        if to_i <= from_i {
            return current;
        }
        fit(reader, bpm_here, from_i, to_i, options).unwrap_or(current)
    };
    for (index, &(change_from, _, _, f)) in fits.iter().enumerate() {
        let current = pending.unwrap_or(f);
        let Some(&(_, from_next, to_next, next)) = fits.get(index + 1) else {
            // The last fit runs to the end of the track.
            if (n as f64) > from_sample {
                let current = if pending.is_some() { current } else { refit(current, from_sample, n as f64) };
                segments.push(Segment {
                    from_secs: to_secs(from_sample),
                    to_secs: to_secs(n as f64),
                    period_secs: current.period / rate,
                    phase_secs: to_secs(current.phase),
                });
            }
            break;
        };
        // The walk starts in the old tempo's last settled window and may
        // run on into the new tempo's settled stretch until it settles.
        let walked = walk_beats(reader, current, next, change_from, to_next);
        let (cut, next_fit) = match walked.first() {
            Some(&(first_beat_start, _)) => {
                let last_end = walked.last().map_or(first_beat_start, |&(_, end)| end);
                // The walk has reached the settled grid within 2 ms. Use the
                // fitted phase there, rather than carrying one quantized
                // attack's error through the whole final stretch.
                let phase = next.phase + ((last_end - next.phase) / next.period).round() * next.period;
                (first_beat_start, Fit { phase, period: next.period })
            }
            // No ramp to follow: the change is a cut, placed where the new
            // beat arrives reliably, searched over the gap and the new
            // tempo's settled stretch.
            None => (boundary(reader, change_from, to_next, from_next, to_next, current, next), next),
        };
        if cut > from_sample {
            // A grid re-phased at a walked beat keeps that phase; any
            // other is refitted over its whole stretch.
            let current = if pending.is_some() { current } else { refit(current, from_sample, cut) };
            segments.push(Segment {
                from_secs: to_secs(from_sample),
                to_secs: to_secs(cut),
                period_secs: current.period / rate,
                phase_secs: to_secs(current.phase),
            });
        }
        for &(start, end) in &walked {
            segments.push(Segment {
                from_secs: to_secs(start),
                to_secs: to_secs(end),
                period_secs: (end - start) / rate,
                phase_secs: to_secs(start),
            });
        }
        from_sample = if walked.is_empty() { cut } else { next_fit.phase };
        pending = Some(next_fit);
    }
    split_gaps(reader, &segments, options, trace)
}

/// Seconds into the file of envelope sample `x`.
fn to_secs_of(reader: Reader<'_>, x: f64) -> f64 {
    reader.origin_secs + x / reader.rate
}

/// The tempo of one window as a ratio to `bpm`, or None when the window has
/// no clear beat. Octaves of the track's tempo count as agreeing: a
/// breakdown that keeps only the hats is not a tempo change.
fn local_tempo(window: &[f64], rate: f64, bpm: f64, options: TempoOptions) -> Option<f64> {
    let n = window.len();
    let mean = window.iter().sum::<f64>() / n as f64;
    let centred: Vec<f64> = window.iter().map(|v| v - mean).collect();
    let variance = centred.iter().map(|v| v * v).sum::<f64>() / n as f64;
    if variance <= 0.0 {
        return None;
    }
    let lag_of = |b: f64| rate * 60.0 / b;
    let min_lag = (lag_of(options.max_bpm).floor() as usize).max(2);
    let max_lag = (lag_of(options.min_bpm).ceil() as usize).min(n / 3);
    if max_lag <= min_lag + 2 {
        return None;
    }
    let acf = autocorrelation(&centred, variance, min_lag, max_lag);
    let home = interpolate(&acf, lag_of(bpm) - min_lag as f64);
    // The strongest peak in the window.
    let mut best = (0.0_f64, 0usize);
    for i in 1..acf.len() - 1 {
        if acf[i] > acf[i - 1] && acf[i] >= acf[i + 1] && acf[i] > best.0 {
            best = (acf[i], i);
        }
    }
    if best.0 <= 0.0 {
        return None;
    }
    // Interpolate the peak before comparing rhythmic ratios. At the short
    // lags of fast tempos, integer bins can be more than a percent apart.
    let (left, middle, right) = (acf[best.1 - 1], acf[best.1], acf[best.1 + 1]);
    let curvature = left - 2.0 * middle + right;
    let offset = if curvature.abs() > f64::EPSILON { 0.5 * (left - right) / curvature } else { 0.0 };
    let peak_bpm = rate * 60.0 / ((best.1 + min_lag) as f64 + offset.clamp(-0.5, 0.5));
    // Fold onto the track's octave only while the result stays in the
    // requested BPM range. 128 relative to 174 must not become 256 BPM.
    let mut ratio = peak_bpm / bpm;
    while ratio > 1.5 && ratio * bpm / 2.0 >= options.min_bpm {
        ratio /= 2.0;
    }
    while ratio < 0.75 && ratio * bpm * 2.0 <= options.max_bpm {
        ratio *= 2.0;
    }
    // The track's own tempo still correlates well: no change. A different
    // period has to win outright, by a margin, to be believed.
    if home >= best.0 * 0.6 {
        return Some(1.0);
    }
    Some(ratio)
}

/// A walked bar counts as settled at the new tempo when its period is
/// within this fraction of the new fit's.
const SETTLED_TOLERANCE: f64 = 0.01;
/// From one beat to the next the period may move by this fraction and
/// still be a change of pace; a bigger jump is a cut, not a ramp.
const RAMP_STEP: f64 = 0.05;
/// How far either side of the predicted beat the next hit is looked for,
/// as a fraction of a beat.
const RAMP_REACH: f64 = 0.1;
/// Beats the walk gives up after, so a track whose tempo never settles
/// still ends.
const MAX_WALKED_BEATS: usize = 1024;

/// Emphasise quiet percussion only while resolving a tempo transition.
/// Positive flux rises get four times the gain and a 20 ms release; held
/// energy cannot keep the envelope open or manufacture another beat.
const TRANSIENT_GAIN: f64 = 4.0;
const TRANSIENT_RELEASE_SECS: f64 = 0.020;

struct TransitionTransients {
    values: Vec<f64>,
    start: usize,
}

impl TransitionTransients {
    fn new(reader: Reader<'_>, from: usize, to: usize) -> Self {
        // Include context for the first search and the release follower.
        let start = from.saturating_sub(reader.rate.ceil() as usize).min(reader.values.len());
        let end = to.saturating_add(reader.rate.ceil() as usize).min(reader.values.len()).max(start);
        let decay = (-1.0 / (reader.rate * TRANSIENT_RELEASE_SECS)).exp();
        let mut previous = start.checked_sub(1).map_or(0.0, |i| reader.values[i]);
        let mut released = 0.0_f64;
        let values = reader.values[start..end].iter().map(|&value| {
            let attack = (value - previous).max(0.0) * TRANSIENT_GAIN;
            previous = value;
            released = attack.max(released * decay);
            released
        }).collect();
        Self { values, start }
    }

    fn peak(&self, reader: Reader<'_>, x: f64, reach: f64) -> Option<(f64, f64)> {
        let local = x - self.start as f64;
        let (at, height) = peaks_in(&self.values, local - reach, local + reach, HIT_FLOOR)
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))?;
        // Use emphasis to select the hit, but keep the original envelope's
        // timestamp: the asymmetric release must not move the fitted phase.
        let (at, _) = local_peak(reader.values, at + self.start as f64, 1.0)?;
        ((at - x).abs() <= reach).then_some((at, height))
    }

    fn snap(&self, reader: Reader<'_>, x: f64, period: f64, reach: f64) -> Option<(f64, f64)> {
        let evidence = reader.evidence(x, period);
        if evidence.kick >= KICK_PRESENT {
            return reader.snap(x, reach);
        }
        // A clear click still gives finer timing than spectral flux.
        if reader.attacks.is_some() {
            if let Some(hit) = reader.snap(x, reach) {
                return Some(hit);
            }
        }
        self.peak(reader, x, reach)
    }

    fn emphasise(&self, reader: Reader<'_>, beats: &mut [(f64, Evidence)], period: f64) {
        for (at, evidence) in beats {
            if evidence.kick < KICK_PRESENT {
                evidence.flux = self.peak(reader, *at, (period * 0.1).max(1.0)).map_or(0.0, |(_, h)| h);
            }
        }
    }
}

/// Walks a tempo change beat by beat, from the last beat of `a` before
/// `from`, until four beats agree with `b`'s tempo and phase or the walk reaches `to`.
///
/// Each next beat is predicted from the period of the beat before it and
/// looked for within a tenth of a beat either side, through the reader
/// (the kick's attack when there is an attack map, emphasised transients
/// when neither kick nor click can place it). The period may drift
/// by up to `RAMP_STEP` per beat: that follows a rise or fall, gradual or
/// not, and stops at a jump, which is a cut. Returns the walked intervals as
/// `(start, end)` envelope samples — one beat each, from the first
/// walked beat — or nothing when the walk did not reach `b`'s tempo, when
/// the two tempos are too far apart to be a change of pace, or when no
/// beat could be placed.
fn walk_beats(reader: Reader<'_>, a: Fit, b: Fit, from: usize, to: usize) -> Vec<(f64, f64)> {
    if a.period < 2.0 || b.period < 2.0 {
        return Vec::new();
    }
    let ratio = b.period / a.period;
    if !(0.6..=1.67).contains(&ratio) {
        return Vec::new();
    }
    let transients = TransitionTransients::new(reader, from, to);
    // The first beat of `a` at or after `from`, moved onto the hit nearest
    // it: the walk measures every period from a hit to a hit.
    let k = ((from as f64 - a.phase) / a.period).ceil();
    let grid_point = a.phase + k * a.period;
    let Some((mut t, _)) = transients.snap(reader, grid_point, a.period, (RAMP_REACH * a.period).max(1.0)) else {
        return Vec::new();
    };
    let mut period = a.period;
    let mut beats = vec![t];
    let mut settled_run = 0usize;
    while beats.len() < MAX_WALKED_BEATS && t < to as f64 {
        let predicted = t + period;
        let Some((next, _)) = transients.snap(reader, predicted, period, (RAMP_REACH * period).max(1.0)) else { break };
        let next_period = next - t;
        if next_period <= 0.0 || (next_period - period).abs() > period * RAMP_STEP {
            break;
        }
        period = next_period;
        t = next;
        beats.push(t);
        // Settled once a whole bar has come out at the new tempo.
        let settled_beat = b.phase + ((t - b.phase) / b.period).round() * b.period;
        if (period - b.period).abs() <= b.period * SETTLED_TOLERANCE
            && (t - settled_beat).abs() <= reader.rate * 0.002
        {
            settled_run += 1;
            if settled_run >= 4 {
                break;
            }
        } else {
            settled_run = 0;
        }
    }
    if settled_run < 4 {
        return Vec::new();
    }
    // Keep the measured attack of every beat. Replacing four measured
    // intervals with their mean loses the curvature within a bar.
    beats.windows(2).map(|w| (w[0], w[1])).collect()
}

/// A bar of the new grid reads the kick when the kick band averages this
/// over its four beats (strong hits read near one). Below it the kick is
/// absent, and a stretch of such bars is a gap between two of the kick's
/// runs.
const KICK_PRESENT: f64 = 0.15;
/// A run of the kick is the beat only when its level is this share of the
/// strongest run's. Measured on the multi-tempo playlist: the incoming
/// track's kick pattern plays under the outgoing breakdown at 0.49 of its
/// eventual level for twenty seconds, stops for two bars, and drops, and
/// the hand grid switches at the drop; a track's kick returns after a
/// breakdown at 0.58 of the level it reaches a minute later, and the hand
/// grid switches at the return.
const RUN_STRENGTH: f64 = 0.55;
/// A bar whose next bar reads more than this many times as much kick is
/// a fill into it — a snare roll, a bar of pickups — and not the beat.
const FILL_RATIO: f64 = 2.0;
/// How much of the full-band flux, against its level over the new tempo's
/// settled stretch, the cut's bar must carry: a bar of kicks alone leads
/// into a drop, and the hand grid switches where the rest of the mix
/// arrives.
const BOUNDARY_STRENGTH: f64 = 0.7;
/// Bars a run of the kick must last to be the beat rather than a fill.
const BOUNDARY_BARS: usize = 2;
/// The beat the cut goes on, within the first reliable bar: the first
/// whose kick is at least this share of the bar's strongest. A kick roll
/// rises into a drop over the last few beats before it, and the drop is
/// the first beat that is a kick and not a roll.
const CUT_ON_A_KICK: f64 = 0.5;
/// A source whose settled level is below this carries nothing on this
/// stretch and does not vote: the kick band of a section whose kick has
/// no thump, the click band of one whose kick has no click. The envelopes
/// are scaled so their strong hits read near one; an attack's height is
/// the click band's RMS rise, a third to nine tenths on a real kick.
const LIVE_SOURCE: f64 = 0.1;

/// The envelope sample where the grid changes from `a` to `b`.
///
/// Not where `b` first appears: in a DJ edit the next track comes in under
/// the last one's breakdown — an impact on a downbeat, an arp in eighths,
/// claps on two and four, a snare roll into the drop, its own kick at half
/// level — bars before it drops, and the hand grids hold the old tempo
/// until the kick states the new one at full level. So the kick (the kick
/// band when `b`'s settled stretch, `settled_from..settled_to`, has one;
/// the click attack when it does not) is read on every beat of `b` from
/// `from` to the end of that stretch and cut into runs at its gaps; a run
/// weaker than `RUN_STRENGTH` of the strongest, or shorter than
/// `BOUNDARY_BARS` bars, is not the beat yet. The change is placed in the
/// first bar of the first run that is: the first bar, beat by beat over
/// the run's opening two bars, that is not a fill into the bar after it,
/// that starts on the beat, that carries the rest of the mix (the full
/// band's flux at `BOUNDARY_STRENGTH` of its settled level), and over
/// which `b`'s grid reads at least as well as `a`'s carried on — two grids
/// at nearby tempos drift through each other, and for a few beats every
/// cycle the new one lands on the old one's onsets. Within that bar the
/// cut goes on the first beat whose kick is `CUT_ON_A_KICK` of the bar's
/// strongest, with a click where the section's kicks have one.
///
/// Should no run qualify — the new tempo's stretch is a breakdown with no
/// kick of its own — the change goes where the onsets stop following `a`
/// and start following `b`, emphasising transient rises where the kick is
/// absent. The earliest supported beat wins when several tie: the
/// impact that ends a section belongs to the section after it.
fn boundary(reader: Reader<'_>, from: usize, to: usize, settled_from: usize, settled_to: usize, a: Fit, b: Fit) -> f64 {
    let n = reader.values.len();
    let lo = from.min(to);
    let hi = from.max(to).min(n);
    if hi <= lo || b.period < 2.0 || a.period < 2.0 {
        return lo as f64;
    }
    // Every beat of a grid over a stretch, with what is heard at it.
    let beats_of = |f: Fit, lo: usize, hi: usize| -> Vec<(f64, Evidence)> {
        let first = ((lo as f64 - f.phase) / f.period).ceil() as i64;
        let last = ((hi as f64 - f.phase) / f.period).floor() as i64;
        (first..=last)
            .map(|k| {
                let at = f.phase + k as f64 * f.period;
                (at, reader.evidence(at, f.period))
            })
            .collect()
    };
    let beats_b = beats_of(b, lo, hi.max(settled_to.min(n)));
    let beats_a = beats_of(a, lo, hi);
    let mean_sum = |beats: &[(f64, Evidence)], from: f64, to: f64| -> f64 {
        let run: Vec<f64> = beats.iter().filter(|(at, _)| *at >= from && *at < to).map(|(_, e)| e.sum()).collect();
        if run.is_empty() { 0.0 } else { run.iter().sum::<f64>() / run.len() as f64 }
    };

    let level = settled_levels(&beats_b, settled_from as f64, settled_to as f64);
    // The kick is source 0 when the band carries it, else the attack (1);
    // the flux (2) is asked whenever it carries anything.
    let kick_source = if level[0] >= LIVE_SOURCE { Some(0) } else if level[1] >= LIVE_SOURCE { Some(1) } else { None };
    tracing::debug!(
        from_secs = reader.origin_secs + lo as f64 / reader.rate,
        to_secs = reader.origin_secs + hi as f64 / reader.rate,
        a_bpm = reader.rate * 60.0 / a.period,
        b_bpm = reader.rate * 60.0 / b.period,
        kick = level[0],
        attack = level[1],
        flux = level[2],
        ?kick_source,
        "boundary: settled levels"
    );

    if let Some(ks) = kick_source {
        let mean_over = |i: usize, count: usize, s: usize| -> f64 {
            beats_b.get(i..i + count).map_or(0.0, |run| run.iter().map(|(_, e)| e.source(s)).sum::<f64>() / count as f64)
        };
        let bar = |i: usize, s: usize| mean_over(i, 4, s);
        let runs = kick_runs(&beats_b, ks);
        let strongest = runs.iter().map(|&(_, _, l)| l).fold(0.0_f64, f64::max);
        tracing::debug!(
            runs = runs.iter().map(|&(s, e, l)| format!("{:.1}s..{:.1}s {l:.2}", reader.origin_secs + beats_b[s].0 / reader.rate, reader.origin_secs + beats_b[e.min(beats_b.len() - 1)].0 / reader.rate)).collect::<Vec<_>>().join(", "),
            "boundary: the kick's runs on the new grid"
        );
        for &(start, end, run_level) in &runs {
            if run_level < RUN_STRENGTH * strongest || end < start + 4 * BOUNDARY_BARS || beats_b[start].0 >= hi as f64 {
                continue;
            }
            // The cut is in the run's first bar that is the beat proper:
            // not a fill into the bar after it, starting on the beat, with
            // the rest of the mix there, and read better by b's grid than
            // by a's. Looked for beat by beat over the run's first two
            // bars, because a bar that reads the kick may start a beat or
            // three before the kick does.
            for i in start..(start + 4 * BOUNDARY_BARS).min(end) {
                if i + 4 * BOUNDARY_BARS > beats_b.len() || beats_b[i].0 >= hi as f64 {
                    break;
                }
                let (this, next) = (bar(i, ks), bar(i + 4, ks));
                let opening = this.midpoint(next);
                let not_a_fill = next <= FILL_RATIO * this;
                let on_the_beat = mean_over(i, 2, ks) >= 0.5 * opening;
                let with_the_mix = level[2] < LIVE_SOURCE || bar(i, 2) >= BOUNDARY_STRENGTH * level[2];
                let cut = beats_b[i].0;
                let span = 4.0 * BOUNDARY_BARS as f64 * b.period;
                let better_than_a = mean_sum(&beats_b, cut, cut + span) >= mean_sum(&beats_a, cut, cut + span);
                if !(not_a_fill && on_the_beat && with_the_mix && better_than_a) {
                    continue;
                }
                // The cut goes on a kick: the first beat of the bar whose
                // kick reads at least `CUT_ON_A_KICK` of the bar's
                // strongest — and, where it can be had, one with the rest
                // of the mix on it and, where the section's kicks have a
                // click, with the click: the beat before a drop is a
                // pickup, a kick without the mix, and a kick roll into a
                // drop is thumps without clicks.
                let strongest_beat = (i..i + 4).map(|j| beats_b[j].1.source(ks)).fold(0.0_f64, f64::max);
                let is_kick = |j: usize| beats_b[j].1.source(ks) >= CUT_ON_A_KICK * strongest_beat;
                let has_mix = |j: usize| level[2] < LIVE_SOURCE || beats_b[j].1.flux >= BOUNDARY_STRENGTH * level[2];
                let has_click = |j: usize| level[1] < LIVE_SOURCE || beats_b[j].1.attack >= CUT_ON_A_KICK * level[1];
                let on_a_kick = (i..i + 4)
                    .find(|&j| is_kick(j) && has_mix(j) && has_click(j))
                    .or_else(|| (i..i + 4).find(|&j| is_kick(j) && has_mix(j)))
                    .or_else(|| (i..i + 4).find(|&j| is_kick(j)))
                    .unwrap_or(i);
                let cut = beats_b[on_a_kick].0;
                let heard: Vec<String> = (i..i + 8)
                    .map(|j| format!("{:.3}s k{:.2} a{:.2} f{:.2}", reader.origin_secs + beats_b[j].0 / reader.rate, beats_b[j].1.kick, beats_b[j].1.attack, beats_b[j].1.flux))
                    .collect();
                tracing::debug!(cut_secs = reader.origin_secs + cut / reader.rate, bar = heard.join(" | "), "boundary: the new beat is reliable from here");
                return cut;
            }
        }
    }

    // No reliable kick run: exaggerate the other transients where the kick
    // is absent so quiet percussion can say which grid owns the change.
    let transients = TransitionTransients::new(reader, lo, hi);
    let (mut beats_a, mut beats_b) = (beats_a, beats_b);
    transients.emphasise(reader, &mut beats_a, a.period);
    transients.emphasise(reader, &mut beats_b, b.period);

    // For every supported beat of b, the support for a before it plus the
    // support for b from it on; ties go to the earlier cut. An empty beat
    // before the first transient must not win that tie.
    let mut best = (f64::NEG_INFINITY, hi as f64);
    for &(cut, _) in beats_b.iter().filter(|(at, e)| *at < hi as f64 && e.sum() >= HIT_FLOOR) {
        let score: f64 = beats_a.iter().filter(|(t, _)| *t < cut).map(|(_, e)| e.sum()).sum::<f64>()
            + beats_b.iter().filter(|(t, _)| *t >= cut && *t < hi as f64).map(|(_, e)| e.sum()).sum::<f64>();
        if score > best.0 {
            best = (score, cut);
        }
    }
    tracing::debug!(cut_secs = reader.origin_secs + best.1 / reader.rate, "boundary: no reliable beat; cut where the onsets change sides");
    best.1
}

/// Each source's settled level: the median of its bar means over the
/// beats in `settled_from..settled_to` (envelope samples), so a pattern
/// that leaves beats empty (a kick on one and three) is measured by the
/// bar rather than the beat.
fn settled_levels(beats: &[(f64, Evidence)], settled_from: f64, settled_to: f64) -> [f64; Evidence::SOURCES] {
    let settled: Vec<Evidence> = beats.iter().filter(|(at, _)| *at >= settled_from && *at < settled_to).map(|(_, e)| *e).collect();
    let mut level = [0.0; Evidence::SOURCES];
    for (s, slot) in level.iter_mut().enumerate() {
        let mut means: Vec<f64> = if settled.len() >= 4 {
            settled.windows(4).map(|bar| bar.iter().map(|e| e.source(s)).sum::<f64>() / 4.0).collect()
        } else {
            settled.iter().map(|e| e.source(s)).collect()
        };
        means.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        *slot = means.get(means.len() / 2).copied().unwrap_or(0.0);
    }
    level
}

/// The kick's runs along a grid's beats: each stretch of beats whose bar
/// (the beat and the three after it) reads source `ks` at
/// `KICK_PRESENT` or more, as `(first beat, one past the last, level)`,
/// the level being the median of the run's bars.
fn kick_runs(beats: &[(f64, Evidence)], ks: usize) -> Vec<(usize, usize, f64)> {
    let bar = |i: usize| beats.get(i..i + 4).map_or(0.0, |bar| bar.iter().map(|(_, e)| e.source(ks)).sum::<f64>() / 4.0);
    let present: Vec<bool> = (0..beats.len()).map(|i| i + 4 <= beats.len() && bar(i) >= KICK_PRESENT).collect();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < present.len() {
        if !present[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < present.len() && present[i] {
            i += 1;
        }
        let mut bars: Vec<f64> = (start..i).map(bar).collect();
        bars.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        runs.push((start, i, bars.get(bars.len() / 2).copied().unwrap_or(0.0)));
    }
    runs
}

// ---------------------------------------------------------------- gaps

/// Beats in a row a line must go without a hit before the stretch is a
/// gap: two bars. A fill or a bar's drop-out is not one.
const GAP_BEATS: usize = 8;
/// Beats in a row with a hit before a gap that the walk into it may start
/// from: a bar.
const SUPPORTED_RUN: usize = 4;
/// The hits after a gap may sit this far from the old line, as a fraction
/// of a beat, and still be its grid; further, and the music has come back
/// on a new phase.
const PHASE_TOLERANCE: f64 = 0.1;
/// Below this share of the envelope's peak a hit is nothing, whatever the
/// beats around it.
const HIT_FLOOR: f64 = 0.02;
/// A line resumes on hits at least this share of its own floor: the bars
/// where a track comes back are quiet, a bass alone under a filter, but
/// they are not the noise between a slowing bass's notes.
const RESUME_FRACTION: f64 = 0.4;
/// A hit through a ramp must be at least this share of the mean of the
/// hits before it: the bass under a filter sweep fades, it does not stop.
const RAMP_FADE: f64 = 0.25;
/// Hits the ramp keeps in its running mean.
const RAMP_MEMORY: usize = 4;
/// From one beat to the next a ramp's period may change by this much
/// either way; a bigger jump is a cut, and the walk stops before it.
const RAMP_RATIO: (f64, f64) = (0.8, 1.3);
/// The next beat is looked for from three quarters of the last period to
/// five quarters of where the last two periods put it: the ramp may keep
/// steepening, or stop. The eighth note between two beats sits at half a
/// period and stays outside the window.
const RAMP_WINDOW: (f64, f64) = (0.75, 1.25);
/// A ramp is believed at this many beats, and when its period has moved
/// from the line's by at least `RAMP_DEPARTURE`.
const RAMP_MIN_BEATS: usize = 8;
const RAMP_DEPARTURE: f64 = 0.05;

/// The hit at every beat of `f` in `from..to`: the beat's envelope sample
/// and the highest peak within a tenth of a beat of it — a peak, not the
/// tail of one just outside the reach — with its height, zero where there
/// is none.
fn beat_hits(values: &[f64], f: Fit, from: usize, to: usize) -> Vec<(f64, f64, f64)> {
    if f.period < 2.0 || to <= from {
        return Vec::new();
    }
    let reach = (f.period * 0.1).max(1.0);
    let first = ((from as f64 - f.phase) / f.period).ceil() as i64;
    let last = ((to as f64 - 1.0 - f.phase) / f.period).floor() as i64;
    (first..=last)
        .map(|k| {
            let at = f.phase + k as f64 * f.period;
            let (hit, height) = peaks_in(values, at - reach, at + reach, HIT_FLOOR)
                .into_iter()
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or((at, 0.0));
            (at, hit, height)
        })
        .collect()
}

/// A line stands on the kicks of a stretch when it collects this many
/// times the kick of the same line moved half a beat.
const KICK_MARGIN: f64 = 1.5;

/// The kick the beats of `f` collect in `from..to`: the highest kick-band
/// peak within a tenth of a beat of each, summed. Zero without a kick
/// envelope.
fn kick_support(reader: Reader<'_>, f: Fit, from: f64, to: f64) -> f64 {
    let Some(kicks) = reader.kicks else { return 0.0 };
    if f.period < 2.0 || kicks.rate <= 0.0 || kicks.values.is_empty() {
        return 0.0;
    }
    let reach_secs = f.period * 0.1 / reader.rate;
    let first = ((from - f.phase) / f.period).ceil() as i64;
    let last = ((to - f.phase) / f.period).floor() as i64;
    (first..=last)
        .map(|k| {
            let secs = reader.origin_secs + (f.phase + k as f64 * f.period) / reader.rate;
            let x = (secs - kicks.origin_secs) * kicks.rate;
            let lo = ((x - reach_secs * kicks.rate).floor().max(0.0)) as usize;
            let hi = ((x + reach_secs * kicks.rate).ceil() as usize).min(kicks.values.len().saturating_sub(1));
            (lo..=hi).map(|i| f64::from(kicks.values[i])).fold(0.0, f64::max)
        })
        .sum()
}

/// `f` or `f` moved half a beat, whichever stands on the kicks of
/// `from..to` by `KICK_MARGIN`; `f` as it is without a kick envelope; and
/// nothing when neither wins — a quiet intro, a break — because a stretch
/// whose kicks cannot say which half of the beat they are on cannot say
/// that its grid differs from the rest of the track's.
fn on_the_kicks(reader: Reader<'_>, f: Fit, from: f64, to: f64) -> Option<Fit> {
    if reader.kicks.is_none() {
        return Some(f);
    }
    let moved = Fit { phase: f.phase + f.period / 2.0, ..f };
    let (here, there) = (kick_support(reader, f, from, to), kick_support(reader, moved, from, to));
    if there > here * KICK_MARGIN {
        Some(moved)
    } else if here > there * KICK_MARGIN {
        Some(f)
    } else {
        None
    }
}

/// A stretch this many beats long is fitted for its own period, and can
/// be believed to have a phase of its own. A shorter one takes its phase
/// from its own fit and its period from the line through the whole
/// segment, which more beats fixed better; and a shorter one after a gap
/// — an outro's last bars — holds the line rather than moving it.
const MIN_STRETCH_BEATS: usize = 64;

/// The two halves of a gap, each on its own kicks, and how far apart their
/// lines are modulo a beat. `None` when either half cannot say where its
/// kicks are, when the fit after the gap is at another tempo, or when
/// the stretch after the gap is too short to be believed.
fn judge_gap(reader: Reader<'_>, f: Fit, from: f64, after: f64, to: f64, probe: f64, options: TempoOptions) -> Option<(Fit, Fit, f64)> {
    let (rate, period) = (reader.rate, f.period);
    let same_tempo = move |b: &Fit| (b.period - period).abs() <= period * options.segment_threshold;
    let nearest = |g: Fit, x: f64| g.phase + ((x - g.phase) / g.period).round() * g.period;
    let own_period = |g: Fit, from: f64, to: f64| -> Fit {
        if (to - from) / g.period >= MIN_STRETCH_BEATS as f64 {
            g
        } else {
            Fit { period, phase: nearest(g, f64::midpoint(from, to)) }
        }
    };
    if (to - after) / period < MIN_STRETCH_BEATS as f64 {
        return None;
    }
    let bpm_here = rate * 60.0 / period;
    let before = fit(reader, bpm_here, from as usize, after as usize, options)
        .filter(same_tempo)
        .map_or(f, |g| own_period(g, from, after));
    let before = on_the_kicks(reader, before, from, after)?;
    let refit = fit(reader, bpm_here, after as usize, to as usize, options)
        .filter(same_tempo)
        .map(|g| own_period(g, after, to))
        .and_then(|b| on_the_kicks(reader, b, after, to))?;
    let d = nearest(refit, probe) - nearest(before, probe);
    Some((before, refit, d - (d / period).round() * period))
}

/// A stretch where a line has no hits: the index of the last beat of the
/// last supported run before it, and of the first beat of the first
/// supported run after it, when the line resumes.
#[derive(Debug, Clone, Copy)]
struct Gap {
    last_supported: usize,
    resumes: Option<usize>,
}

/// The first gap in `supported` at or after `from`.
fn first_gap(supported: &[bool], from: usize) -> Option<Gap> {
    let run_ends_at = |i: usize| (0..SUPPORTED_RUN).all(|d| i >= d && supported[i - d]);
    // A line resumes with as long a run of hits as the gap that lost it:
    // through a ramp the drifting hits land on any line a bar at a time.
    let run_starts_at = |i: usize| (0..GAP_BEATS).all(|d| supported.get(i + d).copied().unwrap_or(false));
    let mut i = from;
    while i < supported.len() {
        if supported[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < supported.len() && !supported[i] {
            i += 1;
        }
        if i - start < GAP_BEATS {
            continue;
        }
        let Some(last_supported) = (from..start).rev().find(|&j| run_ends_at(j)) else { continue };
        let resumes = (i..supported.len()).find(|&j| run_starts_at(j));
        return Some(Gap { last_supported, resumes });
    }
    None
}

/// Every local maximum of the envelope in `lo..=hi` at least `floor` high,
/// with its position sharpened by a parabola.
fn peaks_in(values: &[f64], lo: f64, hi: f64, floor: f64) -> Vec<(f64, f64)> {
    let lo = lo.floor().max(1.0) as usize;
    let hi = (hi.ceil() as usize).min(values.len().saturating_sub(2));
    let mut out = Vec::new();
    for i in lo..=hi {
        let (a, b, c) = (values[i - 1], values[i], values[i + 1]);
        if b < floor || b <= a || b < c {
            continue;
        }
        let denom = a - 2.0 * b + c;
        let offset = if denom.abs() > f64::EPSILON { 0.5 * (a - c) / denom } else { 0.0 };
        out.push((i as f64 + offset.clamp(-0.5, 0.5), b));
    }
    out
}

/// Follows the hits from `start` (a hit, in envelope samples) while their
/// spacing drifts — a slowdown or a speed-up the fitted line has lost —
/// until they stop, jump, or reach `until`, where the line resumes.
/// Returns the beats walked, `start` first, and the ratio the period was
/// changing by at the end.
fn walk_ramp(values: &[f64], start: f64, period: f64, until: f64) -> (Vec<f64>, f64) {
    let mut beats = vec![start];
    let mut heights: Vec<f64> = vec![local_peak(values, start, 1.0).map_or(HIT_FLOOR, |(_, h)| h)];
    let (mut t, mut p, mut g) = (start, period, 1.0_f64);
    while beats.len() < MAX_WALKED_BEATS {
        let lo = t + RAMP_WINDOW.0 * p * g.min(1.0);
        let hi = t + RAMP_WINDOW.1 * p * g.max(1.0);
        if lo >= until {
            break;
        }
        let predicted = t + p * g;
        let mean = heights.iter().rev().take(RAMP_MEMORY).sum::<f64>() / heights.len().min(RAMP_MEMORY) as f64;
        let floor = (mean * RAMP_FADE).max(HIT_FLOOR);
        // The strongest peak for its distance from the prediction: a bass
        // note a beat's width off beats a noise peak on the prediction.
        let reach = (hi - lo) / 2.0;
        let score = |&(x, h): &(f64, f64)| h / (1.0 + (x - predicted).abs() / reach);
        let Some(&(next, height)) = peaks_in(values, lo, hi, floor)
            .iter()
            .max_by(|a, b| score(a).partial_cmp(&score(b)).unwrap_or(std::cmp::Ordering::Equal))
        else {
            break;
        };
        let next_period = next - t;
        let ratio = next_period / p;
        if !(RAMP_RATIO.0..=RAMP_RATIO.1).contains(&ratio) {
            break;
        }
        // The resumed line's first beat is not a ramp beat.
        if (next - until).abs() <= next_period * 0.1 || next >= until {
            break;
        }
        beats.push(next);
        heights.push(height);
        t = next;
        p = next_period;
        g = ratio;
    }
    (beats, g)
}

/// Whether a walk's periods change in one direction, give or take a
/// wobble: a slowdown slows, a speed-up quickens, and a chain of
/// breakdown hits that speeds and slows by turns is not a ramp.
fn one_way(beats: &[f64]) -> bool {
    let periods: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    let Some((&first, &last)) = periods.first().zip(periods.last()) else { return false };
    let up = last > first;
    let against = periods
        .windows(2)
        .filter(|w| if up { w[1] < w[0] * (1.0 - RAMP_WOBBLE) } else { w[1] > w[0] * (1.0 + RAMP_WOBBLE) })
        .count();
    against * 4 < periods.len()
}

/// A ramp's period may step back against its direction by this fraction
/// and still be going one way.
const RAMP_WOBBLE: f64 = 0.01;

/// The ramp, for measurement: the beats walked from the hit nearest
/// `from_secs` at `bpm` until `until_secs`, as `(seconds, bpm from the
/// previous beat)`.
pub fn ramp_report(onsets: &OnsetEnvelope, bpm: f64, from_secs: f64, until_secs: f64) -> Vec<(f64, f64)> {
    let values: Vec<f64> = onsets.values.iter().map(|&v| f64::from(v)).collect();
    let x_of = |secs: f64| (secs - onsets.origin_secs) * onsets.rate;
    let period = onsets.rate * 60.0 / bpm;
    let start = local_peak(&values, x_of(from_secs), period * 0.1).map_or(x_of(from_secs), |(at, _)| at);
    let (beats, _) = walk_ramp(&values, start, period, x_of(until_secs));
    beats
        .windows(2)
        .map(|w| (onsets.time_of(w[1]), onsets.rate * 60.0 / (w[1] - w[0])))
        .collect()
}

/// The hit at the last beat of `f`'s last bar of hits at least `floor`
/// high before `until` (looking a little past it, since the line's beats
/// may still land on the first hits of a change), as an envelope sample.
fn last_supported_hit(values: &[f64], f: Fit, from: f64, until: f64, floor: f64) -> Option<f64> {
    let hits = beat_hits(values, f, from as usize, (until + f.period * 2.0) as usize);
    let supported: Vec<bool> = hits.iter().map(|&(at, _, h)| h >= floor && at < until).collect();
    (0..supported.len())
        .rev()
        .find(|&i| (0..SUPPORTED_RUN).all(|d| i >= d && supported[i - d]))
        .and_then(|i| hits.get(i))
        .map(|h| h.1)
}

/// Where `b` first has two bars of hits at least `floor` high in
/// `from..to`, as an envelope sample.
fn resume_of(values: &[f64], b: Fit, from: f64, to: f64, floor: f64) -> Option<f64> {
    let hits = beat_hits(values, b, from as usize, to as usize);
    let supported: Vec<bool> = hits.iter().map(|&(_, _, h)| h >= floor).collect();
    (0..supported.len())
        .find(|&i| (0..GAP_BEATS).all(|d| supported.get(i + d).copied().unwrap_or(false)))
        .and_then(|i| hits.get(i))
        .map(|h| h.0)
}

/// A line with no hits from its segment's start is a compromise that
/// landed on a later stretch's phase. The stretch up to where the line
/// first has two bars of hits is fitted on its own, and when that puts it
/// on its kicks somewhere else, that is the line from here.
fn refit_start(reader: Reader<'_>, f: Fit, from: f64, to: f64, hits: &[(f64, f64, f64)], supported: &[bool], options: TempoOptions) -> Option<Fit> {
    let leading = supported.iter().take_while(|&&s| !s).count();
    if leading < GAP_BEATS {
        return None;
    }
    let resumes = (0..supported.len()).find(|&i| (0..GAP_BEATS).all(|d| supported.get(i + d).copied().unwrap_or(false)));
    let until = resumes.and_then(|i| hits.get(i)).map_or(to, |h| h.0);
    let period = f.period;
    let g = fit(reader, reader.rate * 60.0 / period, from as usize, until as usize, options)
        .filter(|g| (g.period - period).abs() <= period * options.segment_threshold)
        .and_then(|g| on_the_kicks(reader, g, from, until))?;
    let d = g.phase - f.phase;
    ((d - (d / period).round() * period).abs() > PHASE_TOLERANCE * period).then_some(g)
}

/// The walked beats of a ramp as segments, one per beat, and from the last
/// walked beat to `cut` whole beats at the pace the ramp was going.
fn ramp_segments(walked: &[f64], growth: f64, cut: f64, rate: f64, origin_secs: f64) -> Vec<Segment> {
    let to_secs = |x: f64| origin_secs + x / rate;
    let beat = |start: f64, end: f64| Segment {
        from_secs: to_secs(start),
        to_secs: to_secs(end),
        period_secs: (end - start) / rate,
        phase_secs: to_secs(start),
    };
    let mut out: Vec<Segment> = walked.windows(2).map(|w| beat(w[0], w[1])).collect();
    let Some((&last, last_period)) = walked.last().zip(walked.windows(2).last().map(|w| w[1] - w[0])) else {
        return out;
    };
    let remaining = cut - last;
    if remaining > 0.0 {
        let count = ((remaining / (last_period * growth)).round() as usize).max(1);
        let step = remaining / count as f64;
        for i in 0..count {
            let start = last + i as f64 * step;
            out.push(beat(start, start + step));
        }
    }
    out
}

/// Splits every segment where its line loses its hits for two bars or
/// more and the music comes back somewhere else.
///
/// A line fitted through a whole stretch is right only where the hits
/// are on it. Where they stop — a breakdown, a slowdown — and come back on
/// the same grid, the line holds, as rekordbox holds it. Where they come
/// back at the same tempo on another phase, the stretch after the gap is
/// fitted on its own and the grid changes at the first bar of hits on the
/// new line, the old line dropping the beat within half a beat of the
/// cut. And where the hits through the gap drift — an eighth-note bass
/// under a tape-stop, slowing bar after bar with no kick to follow — they
/// are walked from the last supported beat and each becomes a beat of its
/// own length, up to the cut.
fn split_gaps(reader: Reader<'_>, segments: &[Segment], options: TempoOptions, mut trace: Option<&mut Vec<GapDecision>>) -> Vec<Segment> {
    let (values, rate, origin_secs) = (reader.values, reader.rate, reader.origin_secs);
    let n = values.len();
    let x_of = |secs: f64| (secs - origin_secs) * rate;
    let to_secs = |x: f64| origin_secs + x / rate;
    let mut out: Vec<Segment> = Vec::new();
    for segment in segments {
        let mut from = x_of(segment.from_secs).max(0.0);
        let to = x_of(segment.to_secs).min(n as f64);
        let mut f = Fit { phase: x_of(segment.phase_secs), period: segment.period_secs * rate };
        let line = |from: f64, to: f64, f: Fit| Segment {
            from_secs: to_secs(from),
            to_secs: to_secs(to),
            period_secs: f.period / rate,
            phase_secs: to_secs(f.phase),
        };
        let mut searched = 0usize;
        let mut start_refitted = false;
        loop {
            if f.period < 2.0 || to <= from {
                break;
            }
            let hits = beat_hits(values, f, from as usize, to as usize);
            let mut heights: Vec<f64> = hits.iter().map(|&(_, _, h)| h).filter(|&h| h >= HIT_FLOOR).collect();
            heights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let floor = heights.get(heights.len() / 2).map_or(HIT_FLOOR, |m| (m * 0.2).max(HIT_FLOOR));
            let supported: Vec<bool> = hits.iter().map(|&(_, _, h)| h >= floor).collect();
            // A line with no hits from the segment's start is a compromise
            // that landed on a later stretch's phase: the stretch up to
            // where it first has two bars of hits is fitted on its own,
            // and if that puts it on its kicks elsewhere, it is the line
            // from here.
            if !start_refitted && searched == 0 {
                start_refitted = true;
                if let Some(g) = refit_start(reader, f, from, to, &hits, &supported, options) {
                    f = g;
                    continue;
                }
            }
            let Some(gap) = first_gap(&supported, searched) else { break };

            // The stretches before and after the gap, each fitted on its
            // own. A line through both is a compromise: where the two
            // halves of a track sit on different phases it is on one of
            // them or on neither, and only the halves' own fits say which.
            let after = hits.get(gap.last_supported + 1).map_or(to, |&(at, _, _)| at);
            let period = f.period;
            let same_tempo = move |b: &Fit| (b.period - period).abs() <= period * options.segment_threshold;
            let bpm_here = rate * 60.0 / period;
            // Each half on its kicks: the attack judge inside `fit` is
            // wrong on one stretch in thirty, and two halves it puts on
            // different halves of the beat would read as a phase change.
            let probe = gap.resumes.map_or(f64::midpoint(after, to), |i| hits[i].0);
            let judged = judge_gap(reader, f, from, after, to, probe, options);
            let shift = judged.map_or(0.0, |(_, _, shift)| shift);
            // Moved: both halves say where their kicks are, the lines
            // differ, and the kicks after the gap are on the new line
            // rather than the old one carried on.
            let moved = judged.is_some_and(|(a, b, shift)| {
                shift.abs() > PHASE_TOLERANCE * period
                    && (reader.kicks.is_none() || kick_support(reader, b, after, to) > kick_support(reader, a, after, to) * KICK_MARGIN)
            });
            let (before, refit) = judged.map_or((f, None), |(a, b, _)| (a, Some(b)));
            let nearest = |g: Fit, x: f64| g.phase + ((x - g.phase) / g.period).round() * g.period;
            // The walk starts from the hit at the old line's own last
            // supported beat: read on `before`, not carried over from `f`,
            // which may sit half a beat from it, where the nearest beat is
            // a coin toss and the toss lands the walk on the off-beat.
            let last_hit = last_supported_hit(values, before, from, after, floor).unwrap_or_else(|| {
                let last_beat = nearest(before, hits[gap.last_supported].0);
                local_peak(values, last_beat, (before.period * 0.1).max(1.0)).map_or(last_beat, |(at, _)| at)
            });
            f = before;
            if !moved {
                // The line holds across the gap, whatever the break did:
                // the music came back on its own grid.
                let Some(resumes) = gap.resumes else { break };
                searched = resumes;
                continue;
            }
            // The cut: the first two bars of hits on the new line. Without
            // one the line holds to the end, as it is.
            let Some((resumed, cut)) = refit.and_then(|b| Some((b, resume_of(values, b, after, to, floor * RESUME_FRACTION)?))) else {
                break;
            };
            // The resumed line, fitted over the stretch it will cover.
            let resumed = fit(reader, bpm_here, cut as usize, to as usize, options)
                .filter(same_tempo)
                .and_then(|b| on_the_kicks(reader, b, cut, to))
                .unwrap_or(resumed);
            // The cut is the resumed line's own beat: a refit moves the line
            // by a fraction of a sample, and a beat a hair before the cut
            // would be the next segment's first beat lost.
            let cut = nearest(resumed, cut);

            // A ramp through the gap, if the hits drift.
            // A ramp through the gap, if the hits drift — and only where it
            // leads somewhere: a line that comes back on its own grid held
            // through whatever the breakdown did, as the hand grids do.
            let (walked, growth) = walk_ramp(values, last_hit, f.period, cut);
            let departure = walked.windows(2).map(|w| ((w[1] - w[0]) / f.period - 1.0).abs()).fold(0.0, f64::max);
            let ramp = walked.len() >= RAMP_MIN_BEATS && departure >= RAMP_DEPARTURE && one_way(&walked);
            if let Some(trace) = trace.as_deref_mut() {
                let resumes_secs = gap.resumes.and_then(|i| hits.get(i)).map(|h| to_secs(h.0));
                let (last_supported_secs, shift_ms, cut_secs) = (to_secs(last_hit), shift * 1000.0 / rate, Some(to_secs(cut)));
                trace.push(GapDecision { last_supported_secs, resumes_secs, moved, shift_ms, walked: walked.len(), departure, cut_secs });
            }
            if ramp {
                if walked[0] - f.period / 2.0 > from {
                    out.push(line(from, walked[0] - f.period / 2.0, f));
                }
                out.extend(ramp_segments(&walked, growth, cut, rate, origin_secs));
            } else if cut - f.period / 2.0 > from {
                out.push(line(from, cut - f.period / 2.0, f));
            }
            from = cut;
            f = resumed;
            searched = 0;
        }
        if to > from {
            out.push(line(from, to, f));
        }
    }
    out
}

/// Chooses the octave nearest a reference tempo.
///
/// Tempo estimation reliably finds *a* multiple of the beat; picking which one
/// a human would call the tempo needs a prior. When comparing against a known
/// value, this makes the comparison about accuracy rather than octave choice.
pub fn nearest_octave(bpm: f64, reference: f64) -> f64 {
    if bpm <= 0.0 || reference <= 0.0 {
        return bpm;
    }
    [0.25, 1.0 / 3.0, 0.5, 1.0, 2.0, 3.0, 4.0]
        .into_iter()
        .map(|m| bpm * m)
        .min_by(|a, b| {
            (a - reference)
                .abs()
                .partial_cmp(&(b - reference).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(bpm)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const RATE: f64 = 44_100.0 / 256.0;

    #[test]
    fn transition_transients_have_a_short_rate_independent_release() {
        for rate in [1000.0, 44_100.0 / 256.0, 48_000.0 / 256.0] {
            let mut values = vec![0.01; rate as usize];
            values[0] = 0.0;
            let reader = Reader { values: &values, rate, origin_secs: 0.0, attacks: None, kicks: None };
            let transients = TransitionTransients::new(reader, 0, values.len());
            assert!((transients.values[1] - 0.04).abs() < 1e-10);
            let after = (TRANSIENT_RELEASE_SECS * rate).round() as usize;
            let expected = 0.04 * (-(after as f64) / (rate * TRANSIENT_RELEASE_SECS)).exp();
            assert!((transients.values[1 + after] - expected).abs() < 1e-10);
            // A held sound and its release do not become later beats.
            assert!(transients.peak(reader, rate * 0.5, rate * 0.05).is_none());
        }
    }

    #[test]
    fn a_quiet_transition_without_kick_attacks_is_walked() {
        let rate = 1000.0;
        let mut hits = vec![1000_usize];
        let mut period = 500;
        for _ in 0..20 {
            hits.push(hits.last().unwrap() + period);
            period -= 5;
        }
        let settled = *hits.last().unwrap();
        for _ in 0..8 {
            hits.push(hits.last().unwrap() + period);
        }
        let end = hits.last().unwrap() + 100;
        let mut values = vec![0.0; end];
        for &hit in &hits {
            values[hit] = 0.01; // Below the ordinary hit floor.
        }
        let attacks = AttackMap::new(&[], 1000, crate::attack::AttackOptions::default());
        let kicks = OnsetEnvelope { values: vec![0.0; end], rate, origin_secs: 0.0 };
        let reader = Reader { values: &values, rate, origin_secs: 0.0, attacks: Some(&attacks), kicks: Some(&kicks) };
        assert!(reader.snap(1000.0, 50.0).is_none());
        let a = Fit { phase: 1000.0, period: 500.0 };
        let b = Fit { phase: settled as f64, period: 400.0 };
        let walked = walk_beats(reader, a, b, 1000, end);
        assert!(walked.len() >= 24, "{walked:?}");
        for ((start, finish), expected) in walked.iter().zip(hits.windows(2)) {
            assert!((start - expected[0] as f64).abs() < 1e-8);
            assert!((finish - expected[1] as f64).abs() < 1e-8);
        }
        // Silence or tiny background fluctuations cannot replace those hits.
        for level in [0.0, 0.001] {
            let quiet: Vec<f64> = values.iter().map(|v| if *v > 0.0 { level } else { 0.0 }).collect();
            assert_eq!(
                walk_beats(Reader { values: &quiet, ..reader }, a, b, 1000, end),
                [] as [(f64, f64); 0]
            );
        }
    }

    #[test]
    fn transition_emphasis_keeps_kick_timing_and_envelope_coordinates() {
        let mut values = vec![0.0; 5000];
        values[3100] = 0.1;
        values[3130] = 1.0;
        let mut kicks = OnsetEnvelope { values: vec![0.0; 5000], rate: 1000.0, origin_secs: 0.25 };
        kicks.values[3100] = 1.0;
        let reader = Reader { values: &values, rate: 1000.0, origin_secs: 0.25, attacks: None, kicks: Some(&kicks) };
        let transients = TransitionTransients::new(reader, 3000, 4000);
        assert!(transients.start > 0);
        assert_eq!(transients.snap(reader, 3100.0, 500.0, 50.0), reader.snap(3100.0, 50.0));
        let (at, height) = transients.peak(reader, 3100.0, 10.0).unwrap();
        assert!((at - 3100.0).abs() < 1e-8);
        assert!((height - 0.4).abs() < 1e-8);
    }

    #[test]
    fn a_kickless_cut_follows_quiet_transients() {
        let mut values = vec![0.0; 16_000];
        for at in (1100..8100).step_by(500).chain((8100..16_000).step_by(400)) {
            values[at] = 0.01;
        }
        let reader = Reader { values: &values, rate: 1000.0, origin_secs: 0.0, attacks: None, kicks: None };
        let cut = boundary(reader, 5000, 12_000, 10_000, 15_000,
            Fit { phase: 1100.0, period: 500.0 }, Fit { phase: 8100.0, period: 400.0 });
        assert!((cut - 8100.0).abs() < 1e-8, "cut at {cut}");
    }

    #[test]
    fn nearby_real_tempos_are_not_rhythmic_subdivisions() {
        assert!(rhythmic_ratio(4.0 / 3.0));
        assert!(rhythmic_ratio(2.0 / 3.0));
        assert!(!rhythmic_ratio(174.0 / 128.0));
        assert!(!rhythmic_ratio(128.0 / 174.0));
    }

    #[test]
    fn drifting_rhythmic_aliases_do_not_form_a_steady_tempo() {
        // Approximate two-thirds patterns must not merge into one broad
        // candidate and relabel earlier, exact two-thirds windows as a cut.
        let local = [0.6668, 0.6655, 0.6657, 0.6637, 1.0, 1.0, 1.0, 1.0,
            0.6774, 0.6776, 0.6956, 0.6884, 1.0, 1.0, 1.0, 1.0].map(Some);
        let starts: Vec<usize> = (0..local.len()).map(|i| i * 80).collect();
        let runs = label_runs(&local, &starts, 160, 1_360, 175.0, TempoOptions::default());
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert!((runs[0].2 - 175.0).abs() < f64::EPSILON);
    }

    /// An onset envelope with a peak at each of `hits`, in seconds.
    fn envelope(hits: &[f64], secs: f64) -> OnsetEnvelope {
        let mut values = vec![0.0_f32; (secs * RATE) as usize];
        for &t in hits {
            let i = (t * RATE).round() as usize;
            if let Some(v) = values.get_mut(i) {
                *v = 1.0;
            }
            if let Some(v) = values.get_mut(i + 1) {
                *v = 0.4;
            }
        }
        OnsetEnvelope { values, rate: RATE, origin_secs: 0.0 }
    }

    /// A track at 130 whose kick stops at 118 s, whose bass slows from an
    /// eighth of 231 ms to one of 1.4 s over the next 24 s, and which comes
    /// back at 130 at 147.03 s, `shift` seconds off the old line.
    fn slowdown(shift: f64) -> (OnsetEnvelope, OnsetEnvelope) {
        let period = 60.0 / 130.0;
        let first = 0.05;
        let mut kicks: Vec<f64> = (0..1000).map(|k| first + f64::from(k) * period).take_while(|&t| t < 118.0).collect();
        let mut all = kicks.clone();
        // Eighths that stretch by a fixed ratio per note.
        let (mut t, mut eighth) = (118.205, period / 2.0);
        while t < 142.5 {
            all.push(t);
            t += eighth;
            eighth *= 1.038;
        }
        // 130 again, on the old line moved by `shift`.
        let k = ((147.03 - first - shift) / period).ceil();
        let resume = first + shift + k * period;
        let back: Vec<f64> = (0..1000).map(|k| resume + f64::from(k) * period).take_while(|&t| t < 300.0).collect();
        kicks.extend(&back);
        all.extend(&back);
        (envelope(&all, 300.0), envelope(&kicks, 300.0))
    }

    fn options() -> TempoOptions {
        TempoOptions { placement: Placement::Envelope, ..TempoOptions::default() }
    }

    #[test]
    fn a_slowdown_that_comes_back_on_another_phase_is_walked_and_cut() {
        let (onsets, kicks) = slowdown(0.21);
        let result = detect_tempo_with(&onsets, Some(&kicks), None, options());
        let segments = &result.segments;
        let first = segments.first().expect("a first segment");
        let last = segments.last().expect("a last segment");
        assert!((first.bpm() - 130.0).abs() < 0.1, "first segment at {}", first.bpm());
        assert!((first.start_secs() - 0.05).abs() < 0.02, "first beat at {}", first.start_secs());
        assert!((last.bpm() - 130.0).abs() < 0.1, "last segment at {}", last.bpm());
        let expected_cut = 0.05 + 0.21 + ((147.03_f64 - 0.26) / (60.0 / 130.0)).ceil() * (60.0 / 130.0);
        assert!((last.from_secs - expected_cut).abs() < 0.03, "cut at {} not {expected_cut}", last.from_secs);
        // The ramp: one-beat segments, slowing.
        let ramp: Vec<&Segment> = segments.iter().filter(|s| s.beats() == 1).collect();
        assert!(ramp.len() >= 8, "{} ramp beats", ramp.len());
        assert!(ramp.first().is_some_and(|s| s.bpm() > 100.0) && ramp.last().is_some_and(|s| s.bpm() < 40.0));
        assert!(ramp.windows(2).filter(|w| w[1].bpm() > w[0].bpm() * 1.01).count() * 4 < ramp.len());
        // The old line stops before the ramp, and nothing overlaps.
        for w in segments.windows(2) {
            assert!(w[1].from_secs >= w[0].to_secs - 1e-9);
        }
    }

    #[test]
    fn a_slowdown_that_comes_back_on_its_own_grid_holds_the_line() {
        let (onsets, kicks) = slowdown(0.0);
        let result = detect_tempo_with(&onsets, Some(&kicks), None, options());
        assert_eq!(result.segments.len(), 1, "{:?}", result.segments);
        assert!((result.bpm - 130.0).abs() < 0.1);
    }
}
