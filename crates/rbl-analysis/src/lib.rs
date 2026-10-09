//! Track analysis: tempo, beat grid, key and waveforms.
//!
//! Everything here is offline and deterministic — the same audio always yields
//! the same result — so a analysis can be compared against rekordbox's own
//! stamps as a regression test.
//!
//! Placeholders: phrase detection (`PSSI`) and vocal detection (`PVDI`) are
//! declared as traits with unimplemented stubs. They are deliberately not
//! guessed; see [`phrase`] and [`vocal`].

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "DSP converts freely between sample counts and float time; every such cast is bounded by the buffer length"
)]

pub mod attack;
pub mod downbeat;
pub mod key;
pub mod onset;
pub mod phrase;
pub mod tempo;
pub mod vocal;
pub mod waveform;

pub use key::{detect_key, MusicalKey};
pub use tempo::{detect_tempo, Beat, Segment, TempoResult};
pub use waveform::{Waveform, WaveformColumn};

/// Everything one pass over a track produces.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub tempo: TempoResult,
    pub key: Option<MusicalKey>,
    pub waveform: Waveform,
    /// Peak sample magnitude, for the gain rekordbox stores in `djmdMixerParam`.
    pub peak: f32,
    /// RMS over the whole track.
    pub rms: f32,
}

/// Beats before the first downbeat, in `0..4`, for a grid whose downbeat is
/// nearest `downbeat_secs`.
fn phase_for(segments: &[tempo::Segment], downbeat_secs: f64) -> usize {
    let unnumbered = tempo::beats_of(segments, 0);
    let index = unnumbered
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let da = (f64::from(a.1.time_ms) / 1000.0 - downbeat_secs).abs();
            let db = (f64::from(b.1.time_ms) / 1000.0 - downbeat_secs).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(0, |(i, _)| i);
    (4 - index % 4) % 4
}

/// Everything the stages are tuned by.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnalysisOptions {
    pub tempo: tempo::TempoOptions,
    pub attacks: attack::AttackOptions,
    pub key: key::KeyOptions,
}

/// Named application presets. Both use the requested 70–180 BPM range.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnalysisPreset {
    /// Rekordbox's Normal analysis: one constant tempo for the whole track.
    /// Rekordbox builds it with its constant-tempo analyser
    /// (`BeatAnalyzer_1_0::BA_CreateConst44k`) and its Dynamic mode with
    /// another (`BA_CreateVaried44k`). Of 35,262 grids in one rekordbox
    /// library, 32 have a tempo change, and 30 of those are hand-gridded DJ
    /// edits or grids rbxport's own test rig wrote.
    Rekordbox,
    /// Follows tempo changes, ramps and re-phased returns, for DJ edits
    /// and electronic music.
    #[default]
    Rbxport,
}

impl AnalysisPreset {
    pub fn options(self) -> AnalysisOptions {
        let mut options = AnalysisOptions::default();
        options.tempo.min_bpm = 70.0;
        options.tempo.max_bpm = 180.0;
        options.tempo.follow_changes = self == Self::Rbxport;
        options
    }
}

/// Runs the full analysis over mono audio.
pub fn analyse(samples: &[f32], sample_rate: u32) -> Analysis {
    analyse_with(samples, sample_rate, AnalysisOptions::default())
}

/// Runs the full analysis with every stage under the caller's control.
#[allow(clippy::needless_pass_by_value, reason = "a Copy options struct")]
pub fn analyse_with(samples: &[f32], sample_rate: u32, options: AnalysisOptions) -> Analysis {
    // Everything that reads the audio alone runs at once, on its own
    // thread: the onset envelope, the kick band's envelope, the attack map,
    // the band frames the downbeat stage reads, the waveform, the level,
    // and the key when no rule of its reads the grid. Each is one pass over
    // the samples, none waits for another, and a track on one core took
    // twice as long as its longest stage.
    let front = std::thread::scope(|scope| {
        let onsets = scope.spawn(|| onset::onset_envelope(samples, sample_rate));
        // The kick band's own envelope: what judges which half of the beat
        // a stretch's kicks are on, where a track's halves have to be
        // judged apart.
        let kicks = scope.spawn(|| onset::onset_envelope_band(samples, sample_rate, onset::Band::LOW));
        // The kick attacks, when the fit is to place beats on them.
        let attacks = scope.spawn(|| match options.tempo.placement {
            tempo::Placement::Attack => Some(attack::AttackMap::new(samples, sample_rate, options.attacks)),
            tempo::Placement::Envelope => None,
        });
        let frames = scope.spawn(|| downbeat::band_frames(samples, sample_rate));
        let waveform = scope.spawn(|| waveform::compute(samples, sample_rate));
        let level = scope.spawn(|| level(samples));
        let key = scope.spawn(|| {
            if key::wants_grid(key::DEFAULT_RULES) {
                return KeyFront::NeedsGrid;
            }
            let no_grid = key::KeyGrid { beats: Vec::new(), phrase_starts: Vec::new() };
            KeyFront::Found(key::detect_key_with(samples, sample_rate, options.key, key::DEFAULT_RULES, &no_grid))
        });
        Front {
            onsets: onsets.join().unwrap_or_else(|_| onset::OnsetEnvelope { values: Vec::new(), rate: 0.0, origin_secs: 0.0 }),
            kicks: kicks.join().ok(),
            attacks: attacks.join().ok().flatten(),
            frames: frames.join().unwrap_or_default(),
            waveform: waveform.join().unwrap_or_else(|_| waveform::compute(&[], sample_rate)),
            level: level.join().unwrap_or((0.0, 0.0)),
            key: key.join().unwrap_or(KeyFront::NeedsGrid),
        }
    });
    let Front { onsets, kicks, attacks, frames, waveform, level: (peak, rms), key } = front;
    let mut tempo = tempo::detect_tempo_with(&onsets, kicks.as_ref(), attacks.as_ref(), options.tempo);
    // The grid comes back numbered from its first beat, and possibly on the
    // off-beat. The downbeat stage looks at the music's structure and says
    // both; the grid is moved if it has to be and renumbered so that 1 is
    // the downbeat.
    let beat_secs: Vec<f64> = tempo.beats.iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
    // The downbeat stage decides both which half of the beat the kicks
    // are on and which beat is 1. The fit on the kick's attacks has a view
    // on the first question too, but the phrase structure is the better
    // judge: on the golden playlist it tells the beat from the midpoint on
    // 153 of 155 rekordbox grids, the kick detector on 150 — the misses
    // being off-beat claps with a sharper transient than the kick.
    let grid = downbeat::grid_phase_from(&frames, sample_rate, &beat_secs);
    if grid.half_beat_off {
        for segment in &mut tempo.segments {
            *segment = segment.shifted_half_beat();
        }
        tempo.first_beat_secs = tempo.segments.first().map_or(0.0, tempo::Segment::start_secs);
    }
    // The beat nearest the chosen downbeat is beat 1, and the count runs
    // on from there through every segment, across every tempo change, as
    // rekordbox numbers a hand grid: on all ten changes in the multi-tempo
    // playlist the new tempo's first beat carries the number after the old
    // tempo's last, whatever the music does there. So beat 1 is decided on
    // the first tempo's own beats when it has phrases of its own: the
    // later tempos' downbeats are wherever the carried count puts them,
    // and a track-wide vote they outnumber would move the first tempo's
    // beat 1 onto theirs.
    let downbeat_secs = match tempo.segments.first() {
        Some(first) if tempo.segments.len() > 1 && first.beats() >= downbeat::MIN_BEATS_FOR_OWN_PHASE => {
            let own: Vec<f64> = tempo::beats_of(&tempo.segments[..1], 0).iter().map(|b| f64::from(b.time_ms) / 1000.0).collect();
            downbeat::grid_phase_from(&frames, sample_rate, &own).downbeat_secs
        }
        _ => grid.downbeat_secs,
    };
    tempo.beats = tempo::beats_of(&tempo.segments, phase_for(&tempo.segments, downbeat_secs));
    anchor_file_start(&mut tempo, samples, sample_rate, downbeat_secs);
    // The key rules may read the bass on or between beats and after phrase
    // starts; when one does, the key waits for the grid and is found here.
    let key = match key {
        KeyFront::Found(report) => report,
        KeyFront::NeedsGrid => {
            let key_grid = key::KeyGrid {
                beats: tempo.beats.iter().map(|b| (f64::from(b.time_ms) / 1000.0, b.beat_number)).collect(),
                phrase_starts: grid.phrase_starts.clone(),
            };
            key::detect_key_with(samples, sample_rate, options.key, key::DEFAULT_RULES, &key_grid)
        }
    };
    let key = key.map(|report| report.key);

    Analysis { tempo, key, waveform, peak, rms }
}

/// What the front stages produce from the audio alone.
struct Front {
    onsets: onset::OnsetEnvelope,
    kicks: Option<onset::OnsetEnvelope>,
    attacks: Option<attack::AttackMap>,
    frames: Vec<[f64; downbeat::BANDS]>,
    waveform: waveform::Waveform,
    level: (f32, f32),
    key: KeyFront,
}

/// The key stage's part of the front: found from the audio alone, or
/// waiting for the grid because a rule reads the bass on it.
enum KeyFront {
    /// `None` is a track too short to have a key.
    Found(Option<key::KeyReport>),
    NeedsGrid,
}

/// Peak sample magnitude and RMS over the whole track.
fn level(samples: &[f32]) -> (f32, f32) {
    let mut peak = 0.0_f32;
    let mut sum_squares = 0.0_f64;
    for &s in samples {
        peak = peak.max(s.abs());
        sum_squares += f64::from(s) * f64::from(s);
    }
    let rms = if samples.is_empty() { 0.0 } else { (sum_squares / samples.len() as f64).sqrt() as f32 };
    (peak, rms)
}

/// Restore a boundary beat lost when the fit falls just before zero. Only
/// correct a grid near zero (including an opening cut shorter than 20 ms), with
/// audio starting in the first millisecond. A later onset or silent intro
/// must not become beat 1.1 at zero. This uses full-band audio, not a
/// particular instrument or the kick detector's frequency band.
fn anchor_file_start(tempo: &mut TempoResult, samples: &[f32], sample_rate: u32, downbeat_secs: f64) {
    let Some(first) = tempo.segments.first() else { return };
    let reach = attack::AttackOptions::default().reach_secs;
    if sample_rate == 0 || first.from_secs > reach || first.period_secs <= 0.0 || tempo.beats.is_empty() {
        return;
    }
    // Preserve a distinct later musical downbeat. The fallback downbeat
    // at the first emitted beat may itself be the boundary omission.
    let cut = opening_cut_secs(first, samples, sample_rate);
    let bar = 4.0 * first.period_secs;
    let downbeat_phase = (downbeat_secs + cut.max(0.0)).rem_euclid(bar);
    if downbeat_secs > first.start_secs() + reach && downbeat_phase.min(bar - downbeat_phase) > reach {
        return;
    }
    let phase = first.phase_secs.rem_euclid(first.period_secs);
    let clipped = cut > 0.5 / f64::from(sample_rate);
    if (clipped && cut >= 0.020 - 0.5 / f64::from(sample_rate))
        || (!clipped && phase.min(first.period_secs - phase) > reach)
    {
        return;
    }
    // A sample at exactly zero may be a waveform's zero crossing. Compare
    // the first millisecond with the local peak, allowing that crossing but
    // rejecting a later attack and negligible leading noise.
    let opening = (sample_rate as usize / 1000).max(1);
    let window = (f64::from(sample_rate) * reach).ceil() as usize;
    let peak = |n| samples.iter().take(n).fold(0.0_f32, |p, s| p.max(s.abs()));
    let local_peak = peak(window);
    if local_peak < 1e-5 || peak(opening) < local_peak * 0.1 {
        return;
    }
    if clipped {
        // Keep the fitted segment and every later beat in place. A one-beat
        // opening segment represents the shortened interval without changing
        // the track's BPM; serialization reads the explicit beat timestamps.
        let next = first.start_secs();
        let opening = tempo::Segment { from_secs: 0.0, to_secs: next, phase_secs: 0.0, ..*first };
        tempo.segments[0].from_secs = next;
        tempo.segments.insert(0, opening);
        tempo.first_beat_secs = 0.0;
        tempo.beats.insert(0, Beat { beat_number: 1, tempo_x100: tempo.beats[0].tempo_x100, time_ms: 0 });
        for (i, beat) in tempo.beats.iter_mut().enumerate() {
            beat.beat_number = (i % 4 + 1) as u16;
        }
        return;
    }
    // The envelope starts at its first frame centre, a few milliseconds
    // into the file. Include the recovered boundary beat in the segment.
    tempo.segments[0].from_secs = 0.0;
    tempo.segments[0].phase_secs = 0.0;
    tempo.first_beat_secs = 0.0;
    tempo.beats = tempo::beats_of(&tempo.segments, 0);
}

/// Estimate how far the opening beat precedes the file. An isolated next
/// attack can refine the estimate to a sample: the RMS attack fit is about
/// a millisecond early, which must not turn a 19 ms trim into a 20 ms one.
/// With continuous audio around that beat, keep the fitted estimate.
fn opening_cut_secs(first: &tempo::Segment, samples: &[f32], sample_rate: u32) -> f64 {
    let next = first.start_secs();
    let inferred = first.period_secs - next;
    if inferred <= 0.0 || inferred > 0.035 {
        return 0.0;
    }
    let rate = f64::from(sample_rate);
    let lo = ((next - 0.015).max(0.0) * rate) as usize;
    let hi = (((next + 0.015) * rate) as usize).min(samples.len());
    let Some(window) = samples.get(lo..hi) else { return inferred };
    let peak = window.iter().fold(0.0_f32, |p, s| p.max(s.abs()));
    let threshold = peak * 0.01;
    let Some(index) = window.iter().position(|s| s.abs() > threshold) else { return inferred };
    // Require quiet before this attack. Otherwise this could be a waveform
    // crossing in sustained audio rather than the start of the next beat.
    if index < (sample_rate as usize / 1000).max(1) || peak < 1e-5 {
        return inferred;
    }
    let now = f64::from(window[index].abs());
    let previous = f64::from(window[index - 1].abs());
    let onset_sample = (lo as f64 + index as f64 - now / (now - previous)).round();
    first.period_secs - onset_sample / rate
}
