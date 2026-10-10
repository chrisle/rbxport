//! Three-band waveform, matching what rekordbox draws.
//!
//! Each column holds low, mid and high magnitudes: low is the kick, mid the
//! body, high the hats. This is the same decomposition the ANLZ colour
//! waveforms encode, so the columns map onto `PWV4`/`PWV5` directly.

/// Columns per second, matching rekordbox's detail waveform resolution.
pub const COLUMNS_PER_SEC: f64 = 150.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WaveformColumn {
    pub low: u8,
    pub mid: u8,
    pub high: u8,
    /// Overall peak for the column, which drives the height drawn.
    pub peak: u8,
}

#[derive(Debug, Clone, Default)]
pub struct Waveform {
    pub columns: Vec<WaveformColumn>,
    pub columns_per_sec: f64,
    /// Independently measured PWV6 overview, already in its seven-bit range.
    pub overview: Vec<[u8; 3]>,
}

/// One-pole filters, enough to separate three bands cheaply and stably.
struct Bands {
    low_state: f32,
    high_state: f32,
    low_coeff: f32,
    high_coeff: f32,
}

impl Bands {
    fn new(sample_rate: f32) -> Self {
        // Crossovers at 200 Hz and 2 kHz, as the plan specifies.
        let coeff = |cutoff: f32| {
            let x = (-2.0 * std::f32::consts::PI * cutoff / sample_rate).exp();
            x.clamp(0.0, 0.9999)
        };
        Self { low_state: 0.0, high_state: 0.0, low_coeff: coeff(200.0), high_coeff: coeff(2000.0) }
    }

    /// Splits one sample into (low, mid, high).
    fn split(&mut self, sample: f32) -> (f32, f32, f32) {
        self.low_state = sample * (1.0 - self.low_coeff) + self.low_state * self.low_coeff;
        let low = self.low_state;
        self.high_state = sample * (1.0 - self.high_coeff) + self.high_state * self.high_coeff;
        let below_2k = self.high_state;
        let high = sample - below_2k;
        let mid = below_2k - low;
        (low, mid, high)
    }
}

/// Columns in a `PWAV` preview.
pub const PREVIEW_COLUMNS: usize = 400;
/// Columns in a `PWV2` preview.
pub const TINY_COLUMNS: usize = 100;
/// Columns in a `PWV4` colour preview.
pub const COLOUR_PREVIEW_COLUMNS: usize = 1200;

impl Waveform {
    /// The columns reduced to `count`, each taking the loudest of the
    /// columns it spans, so a transient survives the reduction the way it
    /// does on the CDJ's drawing.
    #[must_use]
    pub fn reduced(&self, count: usize) -> Vec<WaveformColumn> {
        if count == 0 || self.columns.is_empty() {
            return Vec::new();
        }
        (0..count)
            .map(|i| {
                let from = i * self.columns.len() / count;
                let to = ((i + 1) * self.columns.len() / count).max(from + 1).min(self.columns.len());
                self.columns[from..to].iter().fold(WaveformColumn::default(), |acc, c| WaveformColumn {
                    low: acc.low.max(c.low),
                    mid: acc.mid.max(c.mid),
                    high: acc.high.max(c.high),
                    peak: acc.peak.max(c.peak),
                })
            })
            .collect()
    }

    /// `PWAV`: 400 columns, height in the low five bits and whiteness — the
    /// high band — in the top three.
    #[must_use]
    pub fn pack_preview(&self) -> Vec<u8> {
        self.reduced(PREVIEW_COLUMNS).into_iter().map(blue_byte).collect()
    }

    /// `PWV2`: 100 columns of height alone, 0..=15 — no byte of any of 150
    /// reference files' `PWV2` exceeds 15 [OBS].
    #[must_use]
    pub fn pack_tiny(&self) -> Vec<u8> {
        self.reduced(TINY_COLUMNS).iter().map(|c| c.peak >> 4).collect()
    }

    /// `PWV3`: every column, packed as `PWAV` is.
    #[must_use]
    pub fn pack_detail(&self) -> Vec<u8> {
        self.columns.iter().copied().map(blue_byte).collect()
    }

    /// `PWV4`: 1,200 columns of six bytes — height 0..=255, two bytes whose
    /// meaning is [UNKNOWN] and are written zero, then the three colour
    /// channels 0..=127. The ranges are the reference files' [OBS: over 150
    /// `.EXT`s the first byte reaches 253, the last three 127].
    #[must_use]
    pub fn pack_colour_preview(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(COLOUR_PREVIEW_COLUMNS * 6);
        for c in self.reduced(COLOUR_PREVIEW_COLUMNS) {
            let (r, g, b) = rgb_of(c);
            out.extend_from_slice(&[c.peak, 0, 0, r >> 1, g >> 1, b >> 1]);
        }
        out
    }

    /// `PWV5`: every column as a big-endian word, `rrrgggbbbhhhhh00` — three
    /// bits each of red, green and blue, then five of height.
    #[must_use]
    pub fn pack_colour_detail(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.columns.len() * 2);
        for &c in &self.columns {
            let (r, g, b) = rgb_of(c);
            let word = (u16::from(r >> 5) << 13)
                | (u16::from(g >> 5) << 10)
                | (u16::from(b >> 5) << 7)
                | (u16::from(five_bits(c.peak)) << 2);
            out.extend_from_slice(&word.to_be_bytes());
        }
        out
    }
}

/// A magnitude 0..=255 as five bits.
fn five_bits(value: u8) -> u8 {
    value >> 3
}

/// The `PWAV`/`PWV3` byte: whiteness in the top three bits, height below.
fn blue_byte(c: WaveformColumn) -> u8 {
    ((c.high >> 5) << 5) | five_bits(c.peak)
}

/// A column's colour for the `pack_colour_*` diagnostics only: bass blue,
/// mids amber, highs white. This is not what rekordbox writes: its `PWV4`
/// and `PWV5` channels are low, mid and high as red, green and blue
/// [OBS, issues #158 and #288], and the files rbxport writes take their
/// colour from `rbl_anlz::encode` instead.
fn rgb_of(c: WaveformColumn) -> (u8, u8, u8) {
    let amber_green = u8::try_from(u16::from(c.mid) * 3 / 5).unwrap_or(u8::MAX);
    (c.mid.max(c.high), amber_green.max(c.high), c.low.max(c.high))
}

/// A second-order Butterworth section, used to keep bass out of the upper
/// overview bands. Unlike the detail peaks, overview bands measure energy.
struct OverviewFilter {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl OverviewFilter {
    fn new(rate: u32, cutoff: f64, highpass: bool) -> Self {
        let w = 2.0 * std::f64::consts::PI * cutoff.min(f64::from(rate) * 0.45) / f64::from(rate);
        let c = w.cos();
        let alpha = w.sin() / std::f64::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        let b = if highpass { [f64::midpoint(1.0, c), -(1.0 + c), f64::midpoint(1.0, c)] }
            else { [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0] };
        Self { b: b.map(|v| v / a0), a: [-2.0 * c / a0, (1.0 - alpha) / a0], z: [0.0; 2] }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// Independent 1,200-column, seven-bit PWV6 energy envelope. Measured against
/// nine rekordbox references: low-band RMS with gentle compression, rectified
/// mids/highs, and per-band track normalization. This is an approximation of
/// rekordbox's analysis, not a claim to reproduce its proprietary algorithm.
fn overview(samples: &[f32], rate: u32) -> Vec<[u8; 3]> {
    let mut raw = vec![[0.0_f64; 3]; COLOUR_PREVIEW_COLUMNS];
    let mut counts = vec![0_usize; COLOUR_PREVIEW_COLUMNS];
    let mut mid_filters = [OverviewFilter::new(rate, 200.0, true), OverviewFilter::new(rate, 2000.0, false)];
    let mut high_hp = OverviewFilter::new(rate, 2000.0, true);
    let low_coeff = (-2.0 * std::f64::consts::PI * 200.0 / f64::from(rate)).exp();
    let mut low = 0.0;
    let hop = (rate as usize / 150).max(1);
    let mut energy = 0.0;
    let mut level = 0.0;
    let mut frames = 0_usize;
    for (i, &sample) in samples.iter().enumerate() {
        let x = f64::from(sample);
        low = x * (1.0 - low_coeff) + low * low_coeff;
        let mid = mid_filters[0].process(x);
        let mid = mid_filters[1].process(mid);
        let high = high_hp.process(x);
        // Match floor-divided bucket boundaries, including very short audio.
        let bucket = (((i + 1) * COLOUR_PREVIEW_COLUMNS - 1) / samples.len()).min(COLOUR_PREVIEW_COLUMNS - 1);
        raw[bucket][0] += low * low;
        raw[bucket][1] += mid.abs();
        raw[bucket][2] += high.abs();
        counts[bucket] += 1;
        energy += x * x;
        if (i + 1) % hop == 0 || i + 1 == samples.len() {
            let count = i % hop + 1;
            level += (energy / count as f64).sqrt();
            frames += 1;
            energy = 0.0;
        }
    }
    finish_overview(raw, &counts, level, frames)
}

/// The overview's normalization, once the samples are summed into buckets:
/// shared by the in-memory pass and the [`Builder`].
fn finish_overview(mut raw: Vec<[f64; 3]>, counts: &[usize], level: f64, frames: usize) -> Vec<[u8; 3]> {
    let target = level / frames.max(1) as f64 * 92.0;
    let mut means = [0.0; 3];
    for (values, &count) in raw.iter_mut().zip(counts) {
        let n = count.max(1) as f64;
        values[0] = (values[0] / n).powf(0.45);
        values[1] /= n;
        values[2] /= n;
        for band in 0..3 { means[band] += values[band] / COLOUR_PREVIEW_COLUMNS as f64; }
    }
    // A two-bucket trailing average matches the reference envelope's release
    // and half-bucket alignment without blurring section boundaries widely.
    let mut previous = raw[0];
    for values in &mut raw {
        let current = *values;
        for band in 0..3 { values[band] = f64::midpoint(current[band], previous[band]); }
        previous = current;
    }
    raw.into_iter().map(|values| std::array::from_fn(|band| {
        if means[band] < 1e-6 { 0 } else {
            (values[band] * target / means[band]).round().clamp(0.0, 127.0) as u8
        }
    })).collect()
}

/// Computes the waveform.
pub fn compute(samples: &[f32], sample_rate: u32) -> Waveform {
    if samples.is_empty() || sample_rate == 0 {
        return Waveform { columns: Vec::new(), columns_per_sec: COLUMNS_PER_SEC, overview: vec![[0; 3]; COLOUR_PREVIEW_COLUMNS] };
    }
    let per_column = (f64::from(sample_rate) / COLUMNS_PER_SEC).max(1.0) as usize;
    let column_count = samples.len().div_ceil(per_column);
    let mut columns = Vec::with_capacity(column_count);
    let mut bands = Bands::new(sample_rate as f32);

    for chunk in samples.chunks(per_column) {
        let (mut low, mut mid, mut high, mut peak) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        for &sample in chunk {
            let (l, m, h) = bands.split(sample);
            low = low.max(l.abs());
            mid = mid.max(m.abs());
            high = high.max(h.abs());
            peak = peak.max(sample.abs());
        }
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        columns.push(WaveformColumn {
            low: to_u8(low),
            mid: to_u8(mid),
            high: to_u8(high),
            peak: to_u8(peak),
        });
    }

    Waveform { columns, columns_per_sec: COLUMNS_PER_SEC, overview: overview(samples, sample_rate) }
}

/// The waveform of audio that arrives in pieces, so a long file never has
/// to be held whole.
///
/// The detail columns are exactly what [`compute`] gives for the same
/// samples: the band filters run on across the pieces, and a column spans
/// them. The overview cannot be bucketed sample by sample, since its 1,200
/// buckets divide a length that is only known at the end; it is summed per
/// detail column instead and the columns bucketed at the end, which moves a
/// bucket's edge by under 1/150 s. Only used where that is nothing: past the
/// half hour [`compute`] is given, where a bucket is several seconds wide.
pub struct Builder {
    per_column: usize,
    bands: Bands,
    /// The column being filled: samples in it, and its four maxima.
    fill: usize,
    open: [f32; 4],
    columns: Vec<WaveformColumn>,
    // The overview's filters, as in `overview`, and its sums per column.
    mid_filters: [OverviewFilter; 2],
    high_hp: OverviewFilter,
    low_coeff: f64,
    low: f64,
    sums: [f64; 3],
    energy: f64,
    hops: Vec<[f64; 3]>,
    level: f64,
    samples: usize,
    peak: f32,
}

impl Builder {
    pub fn new(sample_rate: u32) -> Self {
        let rate = sample_rate.max(1);
        Self {
            per_column: (f64::from(rate) / COLUMNS_PER_SEC).max(1.0) as usize,
            bands: Bands::new(rate as f32),
            fill: 0,
            open: [0.0; 4],
            columns: Vec::new(),
            mid_filters: [OverviewFilter::new(rate, 200.0, true), OverviewFilter::new(rate, 2000.0, false)],
            high_hp: OverviewFilter::new(rate, 2000.0, true),
            low_coeff: (-2.0 * std::f64::consts::PI * 200.0 / f64::from(rate)).exp(),
            low: 0.0,
            sums: [0.0; 3],
            energy: 0.0,
            hops: Vec::new(),
            level: 0.0,
            samples: 0,
            peak: 0.0,
        }
    }

    /// Folds in the next samples, in order.
    pub fn push(&mut self, samples: &[f32]) {
        for &sample in samples {
            let (l, m, h) = self.bands.split(sample);
            self.open[0] = self.open[0].max(l.abs());
            self.open[1] = self.open[1].max(m.abs());
            self.open[2] = self.open[2].max(h.abs());
            self.open[3] = self.open[3].max(sample.abs());

            let x = f64::from(sample);
            self.low = x * (1.0 - self.low_coeff) + self.low * self.low_coeff;
            let mid = self.mid_filters[0].process(x);
            let mid = self.mid_filters[1].process(mid);
            let high = self.high_hp.process(x);
            self.sums[0] += self.low * self.low;
            self.sums[1] += mid.abs();
            self.sums[2] += high.abs();
            self.energy += x * x;

            self.fill += 1;
            if self.fill == self.per_column {
                self.close_column();
            }
        }
        self.samples += samples.len();
        self.peak = samples.iter().fold(self.peak, |p, s| p.max(s.abs()));
    }

    fn close_column(&mut self) {
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        let [low, mid, high, peak] = self.open;
        self.columns.push(WaveformColumn { low: to_u8(low), mid: to_u8(mid), high: to_u8(high), peak: to_u8(peak) });
        self.hops.push(self.sums);
        self.level += (self.energy / self.fill as f64).sqrt();
        self.open = [0.0; 4];
        self.sums = [0.0; 3];
        self.energy = 0.0;
        self.fill = 0;
    }

    /// Samples folded in so far.
    pub fn samples(&self) -> usize {
        self.samples
    }

    /// The loudest sample folded in so far.
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// The waveform of everything pushed.
    pub fn finish(mut self) -> Waveform {
        if self.samples == 0 {
            return compute(&[], 1);
        }
        // The last, short column, as `compute`'s last chunk is.
        let last = self.fill;
        if last > 0 {
            self.close_column();
        }
        let mut raw = vec![[0.0_f64; 3]; COLOUR_PREVIEW_COLUMNS];
        let mut counts = vec![0_usize; COLOUR_PREVIEW_COLUMNS];
        let total = self.samples;
        for (index, sums) in self.hops.iter().enumerate() {
            let first = index * self.per_column;
            let count = if index + 1 == self.hops.len() && last > 0 { last } else { self.per_column };
            // The bucket of the column's middle sample.
            let middle = first + count / 2;
            let bucket = (((middle + 1) * COLOUR_PREVIEW_COLUMNS - 1) / total).min(COLOUR_PREVIEW_COLUMNS - 1);
            for band in 0..3 { raw[bucket][band] += sums[band]; }
            counts[bucket] += count;
        }
        let frames = self.hops.len();
        let overview = finish_overview(raw, &counts, self.level, frames);
        Waveform { columns: self.columns, columns_per_sec: COLUMNS_PER_SEC, overview }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod pack_tests {
    use super::*;

    fn wave(columns: &[(u8, u8, u8, u8)]) -> Waveform {
        Waveform {
            columns: columns.iter().map(|&(low, mid, high, peak)| WaveformColumn { low, mid, high, peak }).collect(),
            columns_per_sec: COLUMNS_PER_SEC,
            overview: Vec::new(),
        }
    }

    #[test]
    fn overview_handles_silence_short_audio_and_impulses() {
        for samples in [vec![], vec![0.0; 2400], vec![1.0]] {
            let w = compute(&samples, 44100);
            assert_eq!(w.overview.len(), 1200);
            assert!(w.overview.iter().flatten().all(|&v| v <= 127));
            if samples.iter().all(|&v| v == 0.0) {
                assert!(w.overview.iter().flatten().all(|&v| v == 0));
            }
        }
        let rate = 12000;
        let steady: Vec<f32> = (0..rate * 4).map(|i| {
            let t = i as f32 / rate as f32;
            0.15 * (2.0 * std::f32::consts::PI * 80.0 * t).sin()
                + 0.1 * (2.0 * std::f32::consts::PI * 800.0 * t).sin()
                + 0.05 * (2.0 * std::f32::consts::PI * 4000.0 * t).sin()
        }).collect();
        let baseline = compute(&steady, rate);
        let mut transient = steady;
        transient[24000] = 1.0;
        let with_peak = compute(&transient, rate);
        let error: usize = baseline.overview.iter().flatten().zip(with_peak.overview.iter().flatten())
            .map(|(&a, &b)| usize::from(a.abs_diff(b))).sum();
        assert!(error < 360, "an isolated peak must not lift the whole overview: {error}");
        assert!(with_peak.columns.iter().any(|c| c.peak == 255), "detail keeps the peak");
    }

    #[test]
    fn the_builder_draws_what_compute_draws_however_the_audio_arrives() {
        let rate = 44_100;
        // Twenty seconds with a loud second half, so the overview has a shape.
        let samples: Vec<f32> = (0..rate * 20).map(|i| {
            let t = i as f32 / rate as f32;
            let gain = if t < 10.0 { 0.1 } else { 0.6 };
            gain * ((2.0 * std::f32::consts::PI * 90.0 * t).sin() + 0.3 * (2.0 * std::f32::consts::PI * 5000.0 * t).sin())
        }).collect();
        let whole = compute(&samples, rate as u32);
        let mut builder = Builder::new(rate as u32);
        // Packet-sized pieces that do not line up with the columns.
        for piece in samples.chunks(1_152) { builder.push(piece); }
        assert_eq!(builder.samples(), samples.len());
        assert!((builder.peak() - samples.iter().fold(0.0_f32, |p, s| p.max(s.abs()))).abs() < f32::EPSILON);
        let streamed = builder.finish();
        assert_eq!(streamed.columns, whole.columns, "detail columns are exact");
        assert_eq!(streamed.overview.len(), COLOUR_PREVIEW_COLUMNS);
        let off: usize = streamed.overview.iter().flatten().zip(whole.overview.iter().flatten())
            .map(|(&a, &b)| usize::from(a.abs_diff(b))).sum();
        assert!(off <= 3 * COLOUR_PREVIEW_COLUMNS / 50, "the overview stays within rounding of compute's: {off}");
        assert_eq!(Builder::new(rate as u32).finish().columns, []);
    }

    #[test]
    fn reduction_keeps_the_loudest_column_of_each_span() {
        let w = wave(&[(0, 0, 0, 10), (0, 0, 0, 200), (0, 0, 0, 30), (0, 0, 0, 40)]);
        let r = w.reduced(2);
        assert_eq!(r.iter().map(|c| c.peak).collect::<Vec<_>>(), vec![200, 40]);
        assert_eq!(w.reduced(0), [] as [WaveformColumn; 0]);
        assert_eq!(w.reduced(8).len(), 8, "more buckets than columns still yields every bucket");
    }

    #[test]
    fn previews_have_rekordbox_s_column_counts_and_bit_layouts() {
        let w = wave(&[(255, 0, 0, 255), (0, 255, 0, 128), (0, 0, 255, 8)]);
        assert_eq!(w.pack_preview().len(), PREVIEW_COLUMNS);
        assert_eq!(w.pack_tiny().len(), TINY_COLUMNS);
        assert_eq!(w.pack_colour_preview().len(), COLOUR_PREVIEW_COLUMNS * 6);
        assert_eq!(w.pack_detail().len(), 3);
        assert_eq!(w.pack_colour_detail().len(), 6);
        // Height fills five bits; whiteness three; nothing overflows.
        assert_eq!(w.pack_detail()[0], 0b000_11111);
        assert_eq!(w.pack_detail()[2], 0b111_00001);
        assert!(w.pack_tiny().iter().all(|&b| b < 16));
        // The bass column is blue, the mid amber, the high white.
        let words: Vec<u16> = w.pack_colour_detail().chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        let rgb = |v: u16| ((v >> 13) & 7, (v >> 10) & 7, (v >> 7) & 7);
        assert_eq!(rgb(words[0]), (0, 0, 7));
        assert_eq!(rgb(words[1]), (7, 4, 0));
        assert_eq!(rgb(words[2]), (7, 7, 7));
        assert!(words.iter().all(|v| v.trailing_zeros() >= 2), "the low two bits are unused");
        let colour = w.pack_colour_preview();
        assert!(colour.chunks_exact(6).all(|c| c[3] < 128 && c[4] < 128 && c[5] < 128), "colour channels stay within seven bits");
        assert_eq!(colour[0], 255, "height uses the whole byte");
    }
}
