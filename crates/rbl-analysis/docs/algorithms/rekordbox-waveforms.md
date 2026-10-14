# rekordbox's colour and 3-band waveforms

[Analysis documentation](../README.md) · [Waveforms](waveform.md)

`src/wave_creator.rs` computes the sections rekordbox computes itself for its
RGB and 3Band waveforms — `PWVC`, `PWV6`, `PWV7`, `PWV4`, `PWV5` — and the
blue detail `PWV3`, from the same audio and with the same arithmetic.
`PWAV` and `PWV2` still come from [the detail columns](waveform.md).

## Evidence

- [static] rekordbox 7.2.19 and 7.2.18, macOS arm64: `analyzer::WaveCreator`
  (`createWholeWaveData`, `create3BandsWholeWaveData`, `createColorWaveData`,
  `calcZoomWaveColour`, `analyzer::BandPassFilter`) and, for `PWV3`,
  `BeatAnalyzer_2_0::ZoomViewMaker`.
- [OBS] `tests/rekordbox_waveform.rs` holds every section against the files
  rekordbox 7.2.19 wrote for generated signals
  (`tests/rekordbox_waveform/signals.rs`).
- [OBS] `examples/rekordbox_waveform_parity.rs` against tracks of a
  7,800-track library rekordbox analysed: `PWVC` exact on every track; every
  other field 98.9–100 % exact and 99.6–100 % within one. What is left is
  float rounding, not a different rule.

## Input

16-bit stereo frames on rekordbox's timeline: the MP3 lead
`rbl_core::mpeg::rekordbox_lead_frames` measures, and a damaged packet as
silence for as long as it lasts (`rbl_audio::MonoStream::next_frames`). A
frame off that timeline by one MP3 frame takes the 150-a-second sections
from nearly all exact to about one value in ten.

Each frame becomes one mono value: `L/32767` and `R/32767`; where
`||L| − |R|| < 0.001` the louder of the two, otherwise their mean. Channels
equal in level and opposite in sign keep one rather than cancel.

A filtered or raw sample is kept as `trunc(x · 32768)` clamped into 16 bits.

## Filters

RBJ biquads at Q = 1/√2, run in series where a band has several:

| Band | Filters | Feeds |
| --- | --- | --- |
| 3-band low | LP 300 Hz | `PWVC`, `PWV6`, `PWV7` |
| 3-band mid | HP 250 Hz, LP 1200 Hz | `PWVC`, `PWV6`, `PWV7` |
| 3-band high | HP 3 kHz, LP 9 kHz | `PWVC`, `PWV6`, `PWV7` |
| bass | LP 400 Hz | `PWV4` byte 2 |
| colour low | LP 200 Hz ×2 | `PWV4` byte 3 |
| colour mid | HP 200 Hz ×2, LP 2 kHz ×2 | `PWV4` byte 4 |
| colour high | HP 2 kHz ×2 | `PWV4` byte 5 |
| detail low | LP 100 Hz | `PWV5` red |
| detail mid | HP 300 Hz, LP 3 kHz | `PWV5` green |
| detail high | HP 1.5 kHz | `PWV5` blue |

## Milliseconds and points

Millisecond `i` holds frames `[ceil(i·rate/1000), ceil((i+1)·rate/1000))`;
a track has `ceil(frames·1000/rate)` of them. Each keeps the peak of every
band, the highest and lowest raw sample, and for the colour bands the peak
weighted by the share of the millisecond's samples where that band led the
other two — or 32767 where the band reached full scale, however briefly it
led.

Columns and points are laid over the milliseconds by an accumulator: it gains
`n` each millisecond and closes a point each time it passes the track's
millisecond count, so a point covers `(first, last)` milliseconds and several
closing on one millisecond repeat it. The previews have `n = 1200`; the
details `n = ceil(frames·150/rate)`.

## `PWVC` — the per-band gain

Over the 1,200 columns, each band's mean takes in the column and a lead
before it (`ms/1000`, `2·(ms/1000)/3` and `ms/3000` milliseconds for low, mid
and high), what lies before the start counting as silence. The band sums are
equalised to the largest, and scaled so the tallest column of equalised sums
fills 32768: `raw = 32768 · equal / tallest`. Each gain is then held to what
its band's peak allows — `32768/peak` for low and mid, and for high
`32768 / (16384 · (1 − cos(π · peak/32768)))`.

`PWVC` stores `trunc(100 · min(held, cap))` with caps 3, 3 and 5, or 80
where `held < 0.8`; a big-endian `u16` each behind a two-byte header of
zeros, after `PWV6`.

## `PWV6` — the 3-band overview

Each column's three band means times `clamp(raw, 0.8, cap) / 256`,
truncated: the unheld gain, which is why a quiet band stands taller here
than in `PWV7`.

## `PWV7` — the 3-band detail

At each point's last millisecond, per band, an envelope run from 300, 200 or
100 ms back: it rises to a higher value at once, and otherwise moves toward
the millisecond's value keeping 0.99, 0.98 or 0.97 of the gap. Low and mid are
`trunc(level · gain/256)`, high `trunc(gain · 64 · (1 − cos(π · level/32768)))`,
each `gain` being `PWVC`'s value / 100.

## `PWV4` — the colour preview

Six bytes a column, each a value / 256 truncated to a signed byte:

0. the highest raw sample;
1. the lowest raw sample;
2. the bass peak;
3. to 5. the colour bands' peaks over the column widened by 12, 1 and 0
   milliseconds either side.

## `PWV5` — the colour detail

A big-endian `u16` a point, `rrrgggbbbhhhhh00`.

The colours come from the detail bands' peaks over the point widened by 12,
1 and 0 ms. Each is scaled so the loudest of the three is 255. Where blue is
under 64, red and green both lose `green · red / 765 · (1 − blue/64)`. Green
is multiplied by `1 − 0.00234375 · min(max(red, blue), 128)` and blue by 1.3.
Each is clamped to 0..255 and shifted right five bits — except that where all
three are under 32, the one that led is 1 and the others 0. Silence is white.

The height is `trunc(trunc(peak · 32767/loudest)² · 2.9327451233027466e-8)`,
where `peak` is the point's largest raw sample magnitude and `loudest` the
whole track's.

## `PWV3` — the blue detail

`BeatAnalyzer`'s own pass, on the integer mix `(L + R) / 2` (truncating),
in blocks of `floor(rate/150)` frames. Each block keeps its peak and its peak
below 150 Hz, through a bilinear 2nd-order Butterworth low-pass. A byte a
block: whiteness in the top three bits — the first `k` with
`low/peak ≥ 0.875 − k/8`, 7 where none is or the block is silent — over the
height, on the same square law as `PWV5`'s against the loudest block.

## Memory

`WaveCreator` is fed a packet at a time and keeps twelve 16-bit values a
millisecond: about 9 MB for a six-minute track and 170 MB for a two-hour
mix. Nothing is decided before the end, because the points divide a length
only known then.
