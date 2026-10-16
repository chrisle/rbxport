//! Blue previews from `BeatAnalyzer`'s separate loud/dots pass [OBS static,
//! rekordbox 7.2.11.0342 arm64, `LoudViewMaker`]. Keep the peak events and 75 Hz
//! block summaries until the final duration establishes the overview grid.

use super::Biquad;

const HEIGHTS: [f32; 23] = [
    50790.4, 39321.6, 30801.92, 25886.72, 23265.28, 21299.2, 19398.4, 17367.04, 15892.48, 14909.44,
    13926.4, 12943.36, 11960.32, 10977.28, 9994.24, 9011.2, 8028.16, 7045.12, 6062.08, 5079.04,
    4096.0, 3112.96, 2195.2,
];
const DOT_HEIGHTS: [i64; 13] = [
    20675, 16422, 13045, 10362, 8230, 6538, 5193, 4125, 3276, 2602, 2067, 1642, 1304,
];

pub(crate) struct BlueOverview {
    rate: f64,
    per_block: usize,
    open: Vec<f32>,
    filters: [Vec<Biquad>; 3],
    peaks: [Peaks; 3],
    dots: [Peaks; 2],
    blocks: Vec<Block>,
    raw_sum: f32,
    raw_blocks: usize,
}

struct Block {
    frames: usize,
    mean: Option<f32>,
    counts: [usize; 3],
    dots: [usize; 2],
    low_started: bool,
}

impl BlueOverview {
    pub(crate) fn new(rate: u32) -> Self {
        let rate = f64::from(rate.max(1));
        let q = f64::from(std::f32::consts::FRAC_1_SQRT_2);
        Self {
            rate,
            per_block: ((rate / 75.0) as usize).max(1),
            open: Vec::new(),
            filters: [
                vec![
                    filter(150.0, rate, f64::from(0.9_f32), 0),
                    filter(150.0, rate, f64::from(0.9_f32), 0),
                ],
                vec![
                    filter(283.0, rate, q, 1),
                    filter(6000.0, rate, q, 2),
                    filter(283.0, rate, q, 1),
                    filter(6000.0, rate, q, 2),
                ],
                vec![filter(6000.0, rate, q, 1), filter(6000.0, rate, q, 1)],
            ],
            peaks: std::array::from_fn(|_| Peaks::new()),
            dots: std::array::from_fn(|_| Peaks::new()),
            blocks: Vec::new(),
            raw_sum: 0.0,
            raw_blocks: 0,
        }
    }

    pub(crate) fn push(&mut self, frame: [i16; 2]) {
        // Delay one block so EOF can flush the peak detectors at the actual
        // last sample, independent of the decoder's packet boundaries.
        if self.open.len() == self.per_block {
            self.close(false);
        }
        let mono = (i32::from(frame[0]) + i32::from(frame[1])) / 2;
        self.open.push(mono as f32 / 32768.0);
    }

    fn close(&mut self, last: bool) {
        let raw = std::mem::take(&mut self.open);
        let mut bands: [Vec<f32>; 3] = std::array::from_fn(|_| Vec::with_capacity(raw.len()));
        for &x in &raw {
            for (values, filters) in bands.iter_mut().zip(&mut self.filters) {
                let y = filters.iter_mut().fold(f64::from(x), |v, f| f.run(v));
                values.push((y as f32).abs() * 32768.0);
            }
        }
        let mut low_started = false;
        for (i, (peaks, values)) in self.peaks.iter_mut().zip(&bands).enumerate() {
            low_started |= peaks.block(
                values,
                self.rate,
                if i == 2 { 0.66 } else { 0.75 },
                false,
                last,
            ) && i == 0;
        }
        for (peaks, values) in self.dots.iter_mut().zip(&bands) {
            peaks.block(values, self.rate, 0.75, true, last);
        }
        let mut lanes = [0.0_f32; 16];
        lanes[0] = self.raw_sum;
        let n = raw.len() / 16 * 16;
        for (i, x) in raw[..n].iter().enumerate() {
            lanes[i % 16] += (x * 32768.0).abs();
        }
        self.raw_sum = lanes[12]
            + lanes[8]
            + lanes[4]
            + lanes[0]
            + lanes[13]
            + lanes[9]
            + lanes[5]
            + lanes[1]
            + lanes[14]
            + lanes[10]
            + lanes[6]
            + lanes[2]
            + lanes[15]
            + lanes[11]
            + lanes[7]
            + lanes[3];
        for x in &raw[n..] {
            self.raw_sum += (x * 32768.0).abs();
        }
        self.raw_blocks += 1;
        let mean = if self.raw_blocks >= 9 || low_started {
            let mean = self.raw_sum / (self.per_block * self.raw_blocks) as f32;
            self.raw_sum = 0.0;
            self.raw_blocks = 0;
            Some(mean)
        } else {
            None
        };
        self.blocks.push(Block {
            frames: raw.len(),
            mean,
            counts: self.peaks.each_ref().map(|p| p.count),
            dots: self.dots.each_ref().map(|p| p.count),
            low_started,
        });
    }

    pub(crate) fn finish(mut self) -> (Vec<u8>, Vec<u8>) {
        if !self.open.is_empty() {
            self.close(true);
        }
        let frames: usize = self.blocks.iter().map(|b| b.frames).sum();
        if frames == 0 {
            return (vec![0xa2; 400], vec![1; 100]);
        }
        let total = (frames as f32 / self.per_block as f32).ceil() as usize;
        let inv = 1.0 / total.max(1) as f32;
        let width = total as f32 * 0.005;
        let dot_width = (frames / self.per_block / 100).max(1);
        let mut band_columns: [Columns; 3] = std::array::from_fn(|_| Columns::new());
        let mut dots: [DotColumns; 2] = std::array::from_fn(|_| DotColumns::new());
        let mut raw_columns = [0.0_f32; 400];
        let (mut raw_at, mut previous, mut first, mut started_before) = (0, 0.0_f32, 0, false);
        for (at, block) in self.blocks.iter().enumerate() {
            let last = at + 1 == self.blocks.len();
            let tick = at + 1;
            let ready = block.mean.is_some();
            let mean = block.mean.unwrap_or(0.0);
            let next = if last {
                400
            } else {
                ((tick as f32 * 400.0 * inv) as usize).min(399)
            };
            raw_columns[raw_at.min(399)] = previous;
            if raw_at < next {
                raw_columns[raw_at] = previous * 15000.0 * inv;
                raw_columns[raw_at + 1..next].fill(0.0);
                raw_at = next;
            }
            if ready && raw_at < 400 {
                if started_before {
                    raw_columns[first] += if raw_at == first {
                        mean
                    } else {
                        mean * 15000.0 * inv
                    };
                } else {
                    raw_columns[raw_at] += mean;
                    first = raw_at;
                }
            }
            if raw_at < 400 {
                previous = raw_columns[raw_at];
            }
            if block.low_started {
                started_before = true;
            }
            if ready {
                started_before = block.low_started;
            }
            for (i, columns) in band_columns.iter_mut().enumerate() {
                columns.update(
                    tick,
                    &self.peaks[i].values,
                    block.counts[i],
                    last,
                    width,
                    inv,
                );
            }
            for (i, columns) in dots.iter_mut().enumerate() {
                columns.update(tick, &self.dots[i].values, block.dots[i], last, dot_width);
            }
        }
        let pwav = (0..400)
            .map(|i| {
                let low = band_columns[0].values[i];
                let mid = band_columns[1].values[i];
                let high = band_columns[2].values[i];
                let db = |v: f32| {
                    if v > 0.0 {
                        ((f64::from(v) / 32768.0).log10() * 20.0) as f32 + 30.5
                    } else {
                        0.0
                    }
                };
                let [l, m, h] =
                    [db(low * 0.5), db(mid * 0.25), db(high)].map(|v| v.clamp(0.0, 30.0) as i32);
                let colour = if l < 14 {
                    if m - h < 4 {
                        5
                    } else {
                        4
                    }
                } else if m < h {
                    if l <= h {
                        3
                    } else {
                        2
                    }
                } else {
                    u8::from(l <= m)
                };
                let level = ((1.0 / 32768.0) * low * 0.75 + 0.25).min(1.0) * raw_columns[i];
                let height = HEIGHTS
                    .iter()
                    .position(|&t| level >= t)
                    .map_or(2, |p| 25 - p as u8);
                colour << 5 | height
            })
            .collect();
        let pwv2 = dots[0]
            .values
            .iter()
            .zip(dots[1].values)
            .map(|(&low, mid)| {
                if low == 0 && mid == 0 {
                    1
                } else {
                    DOT_HEIGHTS
                        .iter()
                        .position(|&t| low >= t)
                        .map_or(2, |p| 15 - p as u8)
                }
            })
            .collect();
        (pwav, pwv2)
    }
}

fn filter(hz: f64, rate: f64, q: f64, kind: u8) -> Biquad {
    // Literal precision and single-precision Q match IirPassFilter.
    #[allow(clippy::approx_constant)]
    let angle = hz * 6.283_185_307_179_58 / rate;
    let (sin, cos) = angle.sin_cos();
    let alpha = sin / (q + q);
    let norm = 1.0 / (alpha + 1.0);
    let b = match kind {
        0 => [alpha * norm, 0.0, -alpha * norm],
        1 => [
            (cos * 0.5 + 0.5) * norm,
            -(cos + 1.0) * norm,
            (cos * 0.5 + 0.5) * norm,
        ],
        _ => [
            (0.5 - cos * 0.5) * norm,
            (1.0 - cos) * norm,
            (0.5 - cos * 0.5) * norm,
        ],
    };
    Biquad {
        b,
        a: [-2.0 * cos * norm, norm - norm * alpha],
        x: [0.0; 2],
        y: [0.0; 2],
    }
}

struct Peaks {
    values: Vec<f32>,
    count: usize,
    elapsed: usize,
}
impl Peaks {
    fn new() -> Self {
        Self {
            values: vec![0.01],
            count: 0,
            elapsed: 0,
        }
    }
    fn set(&mut self, at: usize, value: f32) {
        self.values.resize(self.values.len().max(at + 1), 0.0);
        self.values[at] = value;
    }
    #[allow(
        clippy::float_cmp,
        reason = "0.01 is the native peak detector sentinel, tested exactly"
    )]
    fn block(&mut self, samples: &[f32], rate: f64, ratio: f32, tiny: bool, last: bool) -> bool {
        let mut peak = self.values[self.count];
        let mut decay = 0;
        let mut four = 0.0_f32;
        let mut started = false;
        let floor = if tiny { 327.68 } else { 0.01 };
        for (i, &sample) in samples.iter().enumerate() {
            self.elapsed += 1;
            four = four.max(sample);
            let end = last && i + 1 == samples.len();
            if tiny && self.elapsed % 4 != 0 && !end {
                continue;
            }
            let input = if tiny { four } else { sample };
            if self.values[0] == 0.01 {
                if input > floor {
                    self.set(0, input);
                    self.count = 1;
                    self.elapsed = 0;
                    peak = input;
                    started = true;
                }
            } else if (peak > floor && input < peak * ratio) || (peak <= floor && input < floor) {
                if self.elapsed as f64 > rate * 1.714 {
                    decay += if tiny { 4 } else { 1 };
                    if decay >= (rate * 0.011_609_977_324_263_039) as usize {
                        peak = (peak * 0.993_830_3).max(floor);
                        decay = 0;
                    }
                } else {
                    decay = 0;
                }
            } else if (self.elapsed as f64) < rate * 0.32 {
                peak = peak.max(input);
            } else {
                self.set(self.count.saturating_sub(usize::from(!tiny)), peak);
                self.elapsed = 0;
                decay = 0;
                self.count += 1;
                peak = input;
                started = true;
            }
            if end {
                self.set(self.count, peak);
                self.count += 1;
            }
            four = 0.0;
        }
        self.set(self.count, peak);
        started
    }
}

struct Columns {
    values: [f32; 400],
    column: usize,
    consumed: usize,
    tick: usize,
    elapsed: f32,
    sum: f32,
}
impl Columns {
    fn new() -> Self {
        Self {
            values: [0.0; 400],
            column: 0,
            consumed: 0,
            tick: 0,
            elapsed: 0.0,
            sum: 0.0,
        }
    }
    fn update(
        &mut self,
        tick: usize,
        peaks: &[f32],
        count: usize,
        last: bool,
        width: f32,
        inv: f32,
    ) {
        if self.column >= 400 {
            return;
        }
        let stop = if last { count } else { count.saturating_sub(1) };
        if !last && self.consumed >= stop {
            return;
        }
        self.values[self.column] = self.sum;
        self.elapsed += (tick - self.tick) as f32;
        for (p, &peak) in peaks.iter().enumerate().take(stop).skip(self.consumed) {
            if self.elapsed >= width {
                self.values[self.column] = self.sum * 7500.0 * inv;
                if self.column + 1 < 400 {
                    self.values[self.column + 1] = self.values[self.column];
                }
                self.column += 2;
                self.elapsed -= width;
                while self.elapsed >= width && self.column < 400 {
                    self.column += 2;
                    self.elapsed -= width;
                }
                self.sum = 0.0;
            }
            if self.column >= 400 {
                break;
            }
            self.sum += if p == stop.saturating_sub(1) && last {
                0.0
            } else {
                peak as i32 as f32
            };
            self.values[self.column] = self.sum;
        }
        self.tick = tick;
        self.consumed = stop;
    }
}

struct DotColumns {
    values: [i64; 100],
    column: usize,
    consumed: usize,
    sum: i64,
}
impl DotColumns {
    fn new() -> Self {
        Self {
            values: [0; 100],
            column: 0,
            consumed: 0,
            sum: 0,
        }
    }
    fn update(&mut self, tick: usize, peaks: &[f32], count: usize, last: bool, width: usize) {
        if self.column >= 100 {
            return;
        }
        let next = if last { 100 } else { (tick / width).min(99) };
        self.values[self.column] = self.sum;
        if self.column < next {
            self.values[self.column] = self.sum * 75 / (width * 2) as i64;
            self.column = next;
            self.sum = 0;
        }
        if count > self.consumed && tick >= 51 && self.column < 100 {
            self.sum += peaks[self.consumed] as i64;
            self.values[self.column] = self.sum;
        }
        self.consumed = count;
    }
}
