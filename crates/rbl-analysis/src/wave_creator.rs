//! rekordbox's own colour and 3-band waveforms, computed the way its
//! `analyzer::WaveCreator` and `BeatAnalyzer` compute them [static reading of
//! rekordbox 7.2.18 and 7.2.19]: the 3-band overview (`PWV6`), the 3-band
//! detail (`PWV7`), their per-band gain (`PWVC`), the colour preview
//! (`PWV4`), the colour detail (`PWV5`) and the blue detail (`PWV3`).
//!
//! Fed 16-bit stereo frames on rekordbox's timeline, in order and in pieces,
//! so a long file is never held whole: what it keeps is a handful of 16-bit
//! values a millisecond, about 9 MB for a six-minute track. The formulas are
//! in `docs/algorithms/rekordbox-waveforms.md`; `tests/rekordbox_waveform.rs`
//! holds the result against rekordbox's own files for generated signals.

#![allow(
    clippy::manual_midpoint,
    reason = "rekordbox's own expressions, kept as written so the arithmetic matches it bit for bit: the integer mix truncates toward zero"
)]

use std::f64::consts::PI;

/// Columns in the overview (`PWV6`) and the colour preview (`PWV4`).
pub const OVERVIEW_COLUMNS: usize = 1200;
/// Points a second in the detail waveforms (`PWV7`, `PWV5`, `PWV3`).
pub const DETAIL_PER_SECOND: f64 = 150.0;
/// The per-band gain's floor, and its ceilings for low, mid and high.
const GAIN_FLOOR: f64 = 0.8;
const GAIN_CAP: [f64; 3] = [3.0, 3.0, 5.0];
/// How far back the `PWV7` envelope restarts, and how fast it falls.
const ENVELOPE_BACK_MS: [usize; 3] = [300, 200, 100];
const ENVELOPE_DECAY: [f32; 3] = [0.99, 0.98, 0.97];
/// The share of a block's peak below 150 Hz that makes it 0..6 parts white
/// in `PWV3`.
const WHITENESS: [f64; 7] = [0.875, 0.75, 0.625, 0.5, 0.375, 0.25, 0.125];

/// The section payloads, each as rekordbox stores it after its section header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waves {
    /// Low, mid and high gain as a percentage: the "wave normalize coefficient".
    pub pwvc: [u16; 3],
    /// 1,200 columns of low, mid and high.
    pub pwv6: Vec<u8>,
    /// 150 points a second of low, mid and high.
    pub pwv7: Vec<u8>,
    /// 1,200 columns of six bytes.
    pub pwv4: Vec<u8>,
    /// 150 points a second of one big-endian `u16`.
    pub pwv5: Vec<u8>,
    /// 150 points a second of one byte.
    pub pwv3: Vec<u8>,
}

/// The waveforms of audio that arrives in pieces.
pub struct WaveCreator {
    rate: f64,
    filters: Filters,
    per_ms: PerMs,
    /// Frames pushed so far, the millisecond being filled, and the frame that
    /// closes it. Millisecond `i` holds frames [ceil(i·rate/1000), ceil((i+1)·rate/1000)).
    frames: usize,
    at: usize,
    next: usize,
    colour_ms: ColourMs,
    blue: BlueBlocks,
}

impl WaveCreator {
    pub fn new(sample_rate: u32) -> Self {
        let rate = f64::from(sample_rate.max(1));
        Self {
            rate,
            filters: Filters::new(rate),
            per_ms: PerMs::default(),
            frames: 0,
            at: 0,
            next: (rate / 1000.0).ceil() as usize,
            colour_ms: ColourMs::default(),
            blue: BlueBlocks::new(rate),
        }
    }

    /// Folds in the next frames, in order.
    pub fn push(&mut self, frames: &[[i16; 2]]) {
        if self.per_ms.len() == 0 && !frames.is_empty() {
            self.per_ms.open();
        }
        for frame in frames {
            while self.frames >= self.next {
                self.colour_ms.close(&mut self.per_ms.colour, self.at);
                self.at += 1;
                self.next = (((self.at + 1) as f64) * self.rate / 1000.0).ceil() as usize;
                self.per_ms.open();
            }
            self.per_ms.take(self.at, to_mono(*frame), &mut self.filters, &mut self.colour_ms);
            self.blue.push(*frame);
            self.frames += 1;
        }
    }

    /// The sections for everything pushed.
    pub fn finish(mut self) -> Waves {
        self.colour_ms.close(&mut self.per_ms.colour, self.at);
        let ms = ((self.frames as f64) * 1000.0 / self.rate).ceil() as usize;
        self.per_ms.resize(ms);
        let columns = grid(ms, OVERVIEW_COLUMNS);
        let gains = Gains::of(&self.per_ms, &columns);
        let detail_points = ((self.frames as f64) * DETAIL_PER_SECOND / self.rate).ceil() as usize;
        let points = grid(ms, detail_points);
        let pwvc = gains.coefficients();
        Waves {
            pwvc,
            pwv6: overview(&gains),
            pwv7: detail(&self.per_ms, &points, pwvc),
            pwv4: colour_preview(&self.per_ms, &columns),
            pwv5: colour_detail(&self.per_ms, &points),
            pwv3: self.blue.finish(),
        }
    }
}

/// Channels that agree in level keep the louder one; otherwise their mean.
fn to_mono(frame: [i16; 2]) -> f64 {
    let left = f64::from(frame[0]) / 32767.0;
    let right = f64::from(frame[1]) / 32767.0;
    if (left.abs() - right.abs()).abs() >= 0.001 {
        (left + right) / 2.0
    } else if left.abs() >= right.abs() {
        left
    } else {
        right
    }
}

/// A sample as `WaveCreator` keeps it: ×32768, truncated, inside 16 bits.
fn scaled(x: f64) -> f64 {
    (x * 32768.0).trunc().clamp(-32768.0, 32767.0)
}

/// Every filter the per-millisecond values are read through.
struct Filters {
    /// The 3-band split: low ≤ 300 Hz, mid 250–1200 Hz, high 3–9 kHz.
    three: [Chain; 3],
    /// Below 400 Hz, for `PWV4`'s third byte.
    bass: Chain,
    /// The colour detail's bands: ≤ 100 Hz, 300–3000 Hz, ≥ 1500 Hz.
    detail: [Chain; 3],
    /// The colour preview's bands, each a pair of filters: ≤ 200 Hz,
    /// 200–2000 Hz, ≥ 2000 Hz.
    colour: [Chain; 3],
}

impl Filters {
    fn new(rate: f64) -> Self {
        let chain = |stages: &[(bool, f64)]| {
            Chain(
                stages
                    .iter()
                    .map(|&(low, hz)| if low { Biquad::low_pass(hz, rate) } else { Biquad::high_pass(hz, rate) })
                    .collect(),
            )
        };
        Self {
            three: [chain(&[(true, 300.0)]), chain(&[(false, 250.0), (true, 1200.0)]), chain(&[(false, 3000.0), (true, 9000.0)])],
            bass: chain(&[(true, 400.0)]),
            detail: [chain(&[(true, 100.0)]), chain(&[(false, 300.0), (true, 3000.0)]), chain(&[(false, 1500.0)])],
            colour: [
                chain(&[(true, 200.0), (true, 200.0)]),
                chain(&[(false, 200.0), (false, 200.0), (true, 2000.0), (true, 2000.0)]),
                chain(&[(false, 2000.0), (false, 2000.0)]),
            ],
        }
    }
}

/// A 2nd-order RBJ biquad at Q = 1/√2, the only filter `WaveCreator` uses.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn low_pass(hz: f64, rate: f64) -> Self {
        Self::new(hz, rate, |cos| [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0])
    }

    fn high_pass(hz: f64, rate: f64) -> Self {
        Self::new(hz, rate, |cos| [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0])
    }

    fn new(hz: f64, rate: f64, numerator: impl Fn(f64) -> [f64; 3]) -> Self {
        let w0 = 2.0 * PI * hz / rate;
        let alpha = w0.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self { b: numerator(w0.cos()).map(|b| b / a0), a: [-2.0 * w0.cos() / a0, (1.0 - alpha) / a0], x: [0.0; 2], y: [0.0; 2] }
    }

    /// `BeatAnalyzer`'s own low-pass for `PWV3`: a bilinear 2nd-order Butterworth.
    fn butterworth_low_pass(hz: f64, rate: f64) -> Self {
        let k = (PI * hz / rate).tan();
        let norm = 1.0 / (1.0 + std::f64::consts::SQRT_2 * k + k * k);
        Self {
            b: [k * k * norm, 2.0 * k * k * norm, k * k * norm],
            a: [2.0 * (k * k - 1.0) * norm, (1.0 - std::f64::consts::SQRT_2 * k + k * k) * norm],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// Biquads run one after another.
struct Chain(Vec<Biquad>);

impl Chain {
    fn run(&mut self, x: f64) -> f64 {
        self.0.iter_mut().fold(x, |v, stage| stage.run(v))
    }

    /// The filtered sample's magnitude as `WaveCreator` keeps it.
    fn peak(&mut self, x: f64) -> f64 {
        scaled(self.run(x).abs())
    }
}

/// Every per-millisecond value the sections are built from. Each is a whole
/// number inside 16 bits, so it is kept as one.
#[derive(Default)]
struct PerMs {
    /// The 3-band peaks.
    three: [Vec<i16>; 3],
    /// The highest and lowest signed sample, and the peak below 400 Hz.
    top: Vec<i16>,
    bottom: Vec<i16>,
    bass: Vec<i16>,
    /// The colour preview's band peaks, each weighted by the share of samples
    /// where that band led the other two.
    colour: [Vec<i16>; 3],
    /// The colour detail's band peaks.
    detail: [Vec<i16>; 3],
}

impl PerMs {
    fn len(&self) -> usize {
        self.top.len()
    }

    fn all(&mut self) -> [&mut Vec<i16>; 12] {
        let [t0, t1, t2] = &mut self.three;
        let [c0, c1, c2] = &mut self.colour;
        let [d0, d1, d2] = &mut self.detail;
        [t0, t1, t2, &mut self.top, &mut self.bottom, &mut self.bass, c0, c1, c2, d0, d1, d2]
    }

    /// A new millisecond, every value at rest.
    fn open(&mut self) {
        for values in self.all() {
            values.push(0);
        }
    }

    /// Exactly the track's milliseconds: the last frame can fall short of
    /// opening the last one.
    fn resize(&mut self, ms: usize) {
        for values in self.all() {
            values.resize(ms, 0);
        }
    }

    fn take(&mut self, at: usize, x: f64, filters: &mut Filters, colour_ms: &mut ColourMs) {
        for (values, filter) in self.three.iter_mut().zip(&mut filters.three) {
            raise(&mut values[at], filter.peak(x));
        }
        let sample = scaled(x);
        raise(&mut self.top[at], sample);
        if sample < f64::from(self.bottom[at]) {
            self.bottom[at] = sample as i16;
        }
        raise(&mut self.bass[at], filters.bass.peak(x));
        for (values, filter) in self.detail.iter_mut().zip(&mut filters.detail) {
            raise(&mut values[at], filter.peak(x));
        }
        colour_ms.add([filters.colour[0].run(x), filters.colour[1].run(x), filters.colour[2].run(x)]);
    }
}

fn raise(held: &mut i16, value: f64) {
    if value > f64::from(*held) {
        *held = value as i16;
    }
}

/// One millisecond of the colour bands, while it fills.
#[derive(Default)]
struct ColourMs {
    peak: [f64; 3],
    led: [usize; 3],
    samples: usize,
}

impl ColourMs {
    fn add(&mut self, bands: [f64; 3]) {
        let leader = (0..3).fold(0, |led, band| if bands[band] > bands[led] { band } else { led });
        self.led[leader] += 1;
        for (peak, value) in self.peak.iter_mut().zip(bands) {
            *peak = peak.max(scaled(value.abs()));
        }
        self.samples += 1;
    }

    fn close(&mut self, colour: &mut [Vec<i16>; 3], at: usize) {
        if self.samples > 0 {
            for (band, values) in colour.iter_mut().enumerate() {
                // A band at full scale stands whole, however briefly it led.
                values[at] = if self.peak[band] >= 32767.0 {
                    32767
                } else {
                    (self.peak[band] * self.led[band] as f64 / self.samples as f64).trunc() as i16
                };
            }
        }
        *self = Self::default();
    }
}

/// `PWV3`'s blocks of rate/150 frames on the integer mono mix: each block's
/// peak and its peak below 150 Hz.
struct BlueBlocks {
    per_block: usize,
    below_150: Biquad,
    open: (f64, f64),
    fill: usize,
    blocks: Vec<(f64, f64)>,
}

impl BlueBlocks {
    fn new(rate: f64) -> Self {
        Self {
            per_block: ((rate / DETAIL_PER_SECOND).floor() as usize).max(1),
            below_150: Biquad::butterworth_low_pass(150.0, rate),
            open: (0.0, 0.0),
            fill: 0,
            blocks: Vec::new(),
        }
    }

    fn push(&mut self, frame: [i16; 2]) {
        let x = f64::from((i32::from(frame[0]) + i32::from(frame[1])) / 2);
        let filtered = self.below_150.run(x).trunc().abs().min(32767.0);
        self.open = (self.open.0.max(x.abs().min(32767.0)), self.open.1.max(filtered));
        self.fill += 1;
        if self.fill == self.per_block {
            self.blocks.push(self.open);
            self.open = (0.0, 0.0);
            self.fill = 0;
        }
    }

    /// One byte a block: three whiteness bits over a 5-bit height against the
    /// loudest block.
    fn finish(mut self) -> Vec<u8> {
        if self.fill > 0 {
            self.blocks.push(self.open);
        }
        let loudest = self.blocks.iter().map(|block| block.0).fold(0.0, f64::max).max(1.0);
        self.blocks
            .iter()
            .map(|&(peak, low)| {
                let white =
                    if peak == 0.0 { 7 } else { WHITENESS.iter().position(|&share| low / peak >= share).unwrap_or(7) as u8 };
                white << 5 | height_of(peak, loudest).min(31)
            })
            .collect()
    }
}

/// Points over the millisecond grid: an accumulator gains `points` a
/// millisecond and closes a point each time it reaches `ms`; several closing
/// on one millisecond repeat its range. Each range is (first, last) ms.
fn grid(ms: usize, points: usize) -> Vec<(usize, usize)> {
    let (mut sum, mut first, mut out) = (0usize, 0usize, Vec::with_capacity(points));
    for at in 0..ms {
        sum += points;
        let closing = sum / ms.max(1);
        for _ in 0..closing {
            out.push((first, at));
        }
        sum -= closing * ms.max(1);
        if closing > 0 {
            first = at + 1;
        }
    }
    out.truncate(points);
    out
}

/// The per-band gain that equalises the bands' overview sums, scaled so the
/// loudest column fills the height, then held to what each band's peak and
/// [0.8, cap] allow.
struct Gains {
    means: Vec<[f64; 3]>,
    raw: [f64; 3],
    held: [f64; 3],
}

impl Gains {
    fn of(per_ms: &PerMs, columns: &[(usize, usize)]) -> Self {
        let ms = per_ms.len();
        let lead = [ms / 1000, (2 * (ms / 1000)) / 3, ms / 3000];
        let means: Vec<[f64; 3]> = columns
            .iter()
            .map(|&(first, last)| [0, 1, 2].map(|band| trailing_mean(&per_ms.three[band], first, last, lead[band])))
            .collect();
        let sums = [0, 1, 2].map(|band| means.iter().map(|column| column[band]).sum::<f64>());
        let largest = sums.iter().copied().fold(0.0, f64::max);
        let equal = [0, 1, 2].map(|band| if sums[band] == 0.0 { 1.0 } else { largest / sums[band] });
        let tallest = means
            .iter()
            .map(|column| (0..3).map(|band| equal[band] * column[band]).sum::<f64>())
            .fold(0.0, f64::max)
            .max(f64::MIN_POSITIVE);
        let raw = [0, 1, 2].map(|band| 32768.0 * equal[band] / tallest);
        let peak = [0, 1, 2].map(|band| f64::from(per_ms.three[band].iter().copied().max().unwrap_or(0)));
        let treble = |x: f64| 16384.0 * (1.0 - (PI * x / 32768.0).cos());
        let held = [
            raw[0].min(32768.0 / peak[0].max(1.0)),
            raw[1].min(32768.0 / peak[1].max(1.0)),
            raw[2].min(32768.0 / treble(peak[2]).max(1.0)),
        ];
        Self { means, raw, held }
    }

    /// `PWVC`: each band's held gain as a percentage, 80 below the floor.
    fn coefficients(&self) -> [u16; 3] {
        [0, 1, 2].map(|band| {
            if self.held[band] < GAIN_FLOOR {
                80
            } else {
                (100.0 * self.held[band].min(GAIN_CAP[band])).trunc() as u16
            }
        })
    }
}

/// The mean of `values` over [first − lead, last], what lies before the start
/// counting as silence.
fn trailing_mean(values: &[i16], first: usize, last: usize, lead: usize) -> f64 {
    let from = first as i64 - lead as i64;
    let sum: f64 = (from.max(0) as usize..=last).map(|at| f64::from(values[at])).sum();
    sum / (last as i64 - from + 1) as f64
}

/// `PWV6`: each column's band means at the unheld gain, clamped to [0.8, cap].
fn overview(gains: &Gains) -> Vec<u8> {
    let scale = [0, 1, 2].map(|band| gains.raw[band].clamp(GAIN_FLOOR, GAIN_CAP[band]) / 256.0);
    gains.means.iter().flat_map(|column| (0..3).map(move |band| (column[band] * scale[band]).trunc() as i64 as u8)).collect()
}

/// `PWV7`: per band, an envelope restarted a fixed span back at each point,
/// rising at once and falling by its decay, drawn at `PWVC`'s gain.
fn detail(per_ms: &PerMs, points: &[(usize, usize)], coefficients: [u16; 3]) -> Vec<u8> {
    let gain = coefficients.map(|c| f32::from(c) * 0.01);
    points
        .iter()
        .flat_map(|&(_, at)| {
            let level = [0, 1, 2].map(|band| envelope(&per_ms.three[band], at, band));
            [
                (level[0] * gain[0] / 256.0).trunc() as i64 as u8,
                (level[1] * gain[1] / 256.0).trunc() as i64 as u8,
                (gain[2] * 64.0 * (1.0 - (level[2] * std::f32::consts::PI / 32768.0).cos())).trunc() as i64 as u8,
            ]
        })
        .collect()
}

fn envelope(values: &[i16], at: usize, band: usize) -> f32 {
    let from = at.saturating_sub(ENVELOPE_BACK_MS[band]);
    (from + 1..=at).fold(f32::from(values[from]), |held, ms| {
        let value = f32::from(values[ms]);
        if value > held {
            value
        } else {
            value + (held - value) * ENVELOPE_DECAY[band]
        }
    })
}

/// `PWV4`: six signed bytes a column — the top and bottom of the waveform, the
/// bass peak, and the three colour bands over windows of ±12, ±1 and 0 ms.
fn colour_preview(per_ms: &PerMs, columns: &[(usize, usize)]) -> Vec<u8> {
    let byte = |value: i16| (f64::from(value) / 256.0).trunc() as i64 as i8 as u8;
    let most = |values: &[i16]| values.iter().copied().fold(0, i16::max);
    let least = |values: &[i16]| values.iter().copied().fold(0, i16::min);
    columns
        .iter()
        .flat_map(|&(first, last)| {
            [
                byte(most(window(&per_ms.top, first, last, 0))),
                byte(least(window(&per_ms.bottom, first, last, 0))),
                byte(most(window(&per_ms.bass, first, last, 0))),
                byte(most(window(&per_ms.colour[0], first, last, 12))),
                byte(most(window(&per_ms.colour[1], first, last, 1))),
                byte(most(window(&per_ms.colour[2], first, last, 0))),
            ]
        })
        .collect()
}

/// `PWV5`: a big-endian `u16` a point — three 3-bit colours and a 5-bit
/// height. The colours come from the bands' peaks over windows of ±12, ±1 and
/// 0 ms; the height from the point's peak against the whole track's.
fn colour_detail(per_ms: &PerMs, points: &[(usize, usize)]) -> Vec<u8> {
    let magnitude = |at: usize| f64::from(per_ms.top[at]).abs().max(f64::from(per_ms.bottom[at]).abs()).min(32767.0);
    let loudest = (0..per_ms.len()).map(magnitude).fold(0.0, f64::max);
    let most = |values: &[i16]| f32::from(values.iter().copied().fold(0, i16::max));
    points
        .iter()
        .flat_map(|&(first, last)| {
            let (red, green, blue) = detail_colour(
                most(window(&per_ms.detail[0], first, last, 12)),
                most(window(&per_ms.detail[1], first, last, 1)),
                most(window(&per_ms.detail[2], first, last, 0)),
            );
            let top = per_ms.top[first..=last].iter().copied().max().unwrap_or(0);
            let bottom = per_ms.bottom[first..=last].iter().copied().min().unwrap_or(0);
            let peak = f64::from(top).abs().max(f64::from(bottom).abs()).min(32767.0);
            let height = if loudest < 1.0 { 0 } else { u16::from(height_of(peak, loudest)) & 31 };
            (red << 13 | green << 10 | blue << 7 | height << 2).to_be_bytes()
        })
        .collect()
}

/// A detail point's three 3-bit colours from its bands' peaks: scaled to the
/// loudest, the mids pulled down where bass or treble is strong, blue raised a
/// third. Colours too dark to show light whichever band led.
fn detail_colour(low: f32, mid: f32, high: f32) -> (u16, u16, u16) {
    let loudest = low.max(mid).max(high);
    let (red, green, blue) = if loudest == 0.0 {
        (255.0_f32, 255.0_f32, 255.0_f32)
    } else {
        let blue = (high * 255.0 / loudest).clamp(0.0, 255.0);
        let (mut red, mut green) = (low * 255.0 / loudest, mid * 255.0 / loudest);
        if blue < 64.0 {
            let shared = green * red / 765.0 * (1.0 - blue / 64.0);
            red -= shared;
            green -= shared;
        }
        green += green * -0.002_343_75 * red.max(blue).min(128.0);
        (red, green, 1.3 * blue)
    };
    let [red, green, blue] = [red, green, blue].map(|c| c.clamp(0.0, 255.0).trunc() as u16);
    if red < 32 && green < 32 && blue < 32 {
        let led = red.max(green).max(blue);
        return (u16::from(red == led), u16::from(green == led), u16::from(blue == led));
    }
    (red >> 5, green >> 5, blue >> 5)
}

/// A peak's height in 0..31 against the loudest peak: rekordbox's square law.
fn height_of(peak: f64, loudest: f64) -> u8 {
    ((peak * 32767.0 / loudest).trunc().powi(2) * 2.932_745_123_302_746_6e-8).trunc() as u8
}

/// The milliseconds of a column widened by `pad` either side, inside the track.
fn window(values: &[i16], first: usize, last: usize, pad: usize) -> &[i16] {
    let end = (last + pad).min(values.len().saturating_sub(1));
    &values[first.saturating_sub(pad)..=end]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn waves_of(frames: &[[i16; 2]], rate: u32) -> Waves {
        let mut creator = WaveCreator::new(rate);
        creator.push(frames);
        creator.finish()
    }

    /// Pieces of any size give the bytes one push gives: nothing is decided
    /// until the end, when the track's length is known.
    #[test]
    fn audio_pushed_in_pieces_is_drawn_as_audio_pushed_whole() {
        let frames: Vec<[i16; 2]> = (0..44_100 * 3)
            .map(|i| {
                let t = f64::from(i) / 44_100.0;
                let x = ((2.0 * PI * 55.0 * t).sin() * (-30.0 * (t % 0.48)).exp() * 30_000.0) as i16;
                [x, x / 2]
            })
            .collect();
        let whole = waves_of(&frames, 44_100);
        let mut creator = WaveCreator::new(44_100);
        for piece in frames.chunks(1_152) {
            creator.push(piece);
        }
        assert_eq!(creator.finish(), whole);
    }

    /// Every section has the length rekordbox gives it: 1,200 columns for the
    /// previews, 150 points a second for the details.
    #[test]
    fn every_section_is_as_long_as_rekordbox_makes_it() {
        let frames = vec![[1000_i16, 1000]; 44_100 * 2 + 300];
        let waves = waves_of(&frames, 44_100);
        assert_eq!(waves.pwv6.len(), OVERVIEW_COLUMNS * 3);
        assert_eq!(waves.pwv4.len(), OVERVIEW_COLUMNS * 6);
        assert_eq!(waves.pwv7.len(), 302 * 3);
        assert_eq!(waves.pwv5.len(), 302 * 2);
        assert_eq!(waves.pwv3.len(), 302);
    }

    /// Silence draws nothing.
    #[test]
    fn silence_draws_nothing() {
        let waves = waves_of(&vec![[0_i16, 0]; 44_100], 44_100);
        assert!(waves.pwv6.iter().all(|&byte| byte == 0));
        assert!(waves.pwv7.iter().all(|&byte| byte == 0));
    }

    /// No audio at all is no sections rather than a panic.
    #[test]
    fn no_audio_is_no_sections() {
        let waves = WaveCreator::new(44_100).finish();
        assert!(waves.pwv6.is_empty() && waves.pwv3.is_empty());
    }

    /// Channels equal in level and opposite in sign keep the louder one,
    /// where their mean would be silence.
    #[test]
    fn channels_equal_in_level_keep_one_rather_than_cancel() {
        assert!((to_mono([16_000, -16_000]) - 16_000.0 / 32_767.0).abs() < 1e-12);
        assert!((to_mono([16_000, 0]) - 8_000.0 / 32_767.0).abs() < 1e-12);
    }
}
