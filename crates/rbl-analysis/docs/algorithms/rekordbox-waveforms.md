# rekordbox's blue, RGB and 3-band waveforms

[Analysis documentation](../README.md) · [Waveforms](waveform.md)

`src/wave_creator.rs` computes the sections rekordbox computes itself for its
RGB and 3Band waveforms — `PWVC`, `PWV6`, `PWV7`, `PWV4`, `PWV5` — and the
blue detail `PWV3`, from the same audio and with the same arithmetic.
`blue_overview.rs` computes `PWAV` and `PWV2` from the separate
`BeatAnalyzer::LoudViewMaker` pass; no saved waveform uses the legacy
[detail-column approximation](waveform.md).

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
`||L| − |R|| < 0.001` the louder of the two — the right on an exact tie —
otherwise their mean. Channels equal in level and opposite in sign keep one
rather than cancel; the `inverted` fixture shows which.

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

## `PWAV` and `PWV2` — the blue overviews

[OBS static] rekordbox **7.2.11.0342**, macOS arm64, `LoudViewMaker`:
`create`, `analyzePeak`, `PeakDetect`, `PeakDetect900`, `analyzeLoudColor`,
plus `Analyzer::setSample`, `peakss::BeatSensor::{create,put}`,
`CBeatAnalyzeLib::V_Devide`, `notifyDots` and `MstStoreLoudWave`.
This is a separate reference version from the RGB/3-band fixtures above.

The integer stereo mean feeds a double-precision filter cascade, with
single-precision output: a 150 Hz band-pass at Q=0.9 twice; alternating
283 Hz high-pass and 7900 Hz low-pass twice; and a 7900 Hz high-pass twice.
With BPM analysis enabled, `Analyzer::setSample` feeds the six-band
`BeatSensor` and passes its bands 2/3 to `LoudViewMaker::analyzePeak`.
The low band remains the overview generator's own 150 Hz filter. Its
all-null fallback uses 6000 Hz instead; matching that fallback alone does
not reproduce the usual BPM-enabled color codes.
The block size is `floor(rate/75)` frames. Peak tracking keeps a rising peak
for at least 320 ms, admits a new peak above 75% of it (66% for high), and
after 1.714 seconds of decay applies 0.9938303 every approximately 11.61 ms.
The dots pass considers maxima over four samples, with a 327.68 floor.

The 400-column overview combines block means of absolute raw amplitude
with the low band's peak density. A native threshold table quantizes height
to 2..25. Its six colors come from the three peak densities in dB, using
low/2, mid/4 and high. The 100-column dots preview uses the low peak density
and its own threshold table, with height 1..15 and no color bits.

Only peak events and block summaries are retained until EOF; decoded PCM
and filtered samples are discarded block by block. The final block is held
until EOF so flushing does not depend on the decoder's packet boundaries.

[OBS native routine] `tests/blue_overview.rs` compares both payloads exactly
against isolated native `BeatSensor` → `LoudViewMaker` output from
rekordbox **7.2.19.0342** on `chris-m2-mac` for the six generated signals,
a 200-second repetition of `kick-hats`, and a fractional final block. These `.blue-preview` files
are native-routine captures, **not complete files exported by rekordbox**.
See [fixture provenance](../../tests/rekordbox_waveform/README.md).
The tests also exercise arbitrary packet boundaries and empty/short silence.

[OBS read only] The extended `rekordbox_waveform_parity -- --library 8`
comparison covered eight existing library rows (five distinct audio files).
`PWV2` was exact throughout; `PWAV` height was 98.5–100% exact and
99.75–100% within one. Its color codes were only 64.75–83.5% exact.
[OBS controlled, 2026-10-16] On the same decoded real-song PCM, the
7.2.19 fallback reproduced 7.2.11 exactly but matched only 259/400 stored
color codes. Feeding the native BPM sensor's mid/high bands instead
matched **400/400 colors and 400/400 heights**. This isolates the color
discrepancy to the omitted 7900 Hz BPM-band input path, rather than a
change between those versions or the packed-byte writer.
[OBS copied-reference comparison] Running the corrected Rust parity example
on `chris-m2-mac` against copies of the five distinct files from that
comparison matched **2000/2000 PWAV color codes** and all PWV2 bytes.
PWAV height shares remained 98.5–100% exact / 99.75–100% within one.
[UNKNOWN] Other analyzer modes and versions are not established by this
experiment. RGB/3-band evidence above is separate.

## CDJ-3000 emulator check

[OBS emulator, 2026-10-16] AtEmu running CDJ-3000 firmware **3.20** loaded
analysis written by this branch's desktop app from a disposable 30-second
220 Hz tone/click fixture. The check invoked `analyse_track` on fixture track
10000, then loaded it through LINK and selected **BLUE**, **RGB**, and
**3BAND** from the player's SHORTCUT screen.

All three displayed nonempty, distinct overview and detailed waveforms.
For each mode, the player's network status reported PLAY and its on-screen
playhead advanced during a three-second observation. SHA-256 checks confirmed
that the DAT, EXT and 2EX files were unchanged after playback. Screenshots,
the exact script, file payloads, hashes, and `result.json` are retained in the
private companion repository under
`verification/pr489-waveforms/20261016/`.

The general private LINK suite was also attempted: source discovery passed,
but browsing assumed TRACK was the first category while this PR's base
presents ARTIST first. Its interaction client then attempted localhost,
although the database server bound to the selected LAN interface. That run
was **not a passing suite**. The focused check used the displayed TRACK row
and the real app's analysis/save path instead.

[UNKNOWN physical] This checks the stated rendering, load and transport flows
in emulated CDJ-3000 firmware. It does not establish measured audio output,
physical-player behavior, or other player models.

## Cost

`WaveCreator` is fed a packet at a time and keeps twelve 16-bit values a
millisecond: about 9 MB for a six-minute track and 170 MB for a two-hour
mix. Nothing is decided before the end, because the points divide a length
only known then. `WaveCreator::reserve` takes the container's frame count
first, so the values are not copied each time they outgrow their buffers,
which at the peak held twice as much.

[OBS before the blue overview extension] The app's analysis loop on an Apple M4, release build, the `Rbxport`
preset, peak memory footprint from `/usr/bin/time -l`:

| File | Without `WaveCreator` | With it |
| --- | --- | --- |
| 6-minute MP3 | 0.42 s, 145 MB | 1.1 s, 155 MB |
| 2-hour MP3 | 7.9 s, 718 MB | 21 s, 925 MB |

These measurements predate `blue_overview.rs`; its extra filters and block
summaries add cost. They are historical measurements, not timings of the
complete blue/RGB/3-band implementation.
