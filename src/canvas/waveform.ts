/**
 * Waveform drawing.
 *
 * rekordbox's `PWAV` preview packs each column into one byte: the low five bits
 * are the height and the top three a "whiteness" that shades the column. Drawing
 * that faithfully is what makes a preview recognisable rather than a grey blob.
 */

/** A rendered preview, cached as a bitmap so scrolling is a `drawImage`. */
import theme from "@/styles/theme";

export interface RenderedWaveform {
  bitmap: ImageBitmap | HTMLCanvasElement;
  width: number;
  height: number;
}

const HEIGHT_MASK = 0x1f;
const WHITENESS_SHIFT = 5;

/**
 * Which waveform is being drawn.
 *
 * Only the middle band differs: the overview strip uses a brighter amber than
 * the detail does. Both are in the tokens, measured off the capture.
 */
export type WaveBand = "overview" | "detail";

/** How a waveform is drawn: centred, or a half from the bottom — see `drawBands`. */
export type HalfWaveform = boolean | "overlaid";

/**
 * Which of rekordbox's palettes: View › Color › Waveform color. Each reads
 * its own tags, so the bytes a palette is handed are in that palette's
 * layout — see `strideOf`.
 */
export type WavePalette = "blue" | "rgb" | "3band";

/** Bytes per column of the tag a palette reads, at either resolution. */
export function strideOf(palette: WavePalette, detail: boolean): number {
  switch (palette) {
    case "3band": return 3;
    case "blue": return 1;
    case "rgb": return detail ? 2 : 6;
  }
}

/** The tag to ask the backend for, per palette and resolution. */
export function waveformKindOf(palette: WavePalette, detail: boolean):
  "bands" | "bandsDetail" | "mono" | "monoDetail" | "colour" | "colourDetail" {
  switch (palette) {
    case "3band": return detail ? "bandsDetail" : "bands";
    case "blue": return detail ? "monoDetail" : "mono";
    case "rgb": return detail ? "colourDetail" : "colour";
  }
}

/**
 * rekordbox's 3Band colours: not one per band but one per *combination* of
 * bands. Where only the low band reaches, the waveform is blue; where the
 * mid overlaps it, brown; where all three overlap, the cream core; the mid
 * alone is amber and the high alone white. Seven colours, sampled from
 * rekordbox itself (mixxxdj/mixxx#12326), of which a real track shows five
 * or so — a high band that outreaches the mid is rare. The `--c-wave-*`
 * tokens carry the same values, and the test guards against drift.
 */
function rgbTuple(hex: string): readonly [number, number, number] {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

const LOW = rgbTuple(theme.color.waveLow.value);
const MID = rgbTuple(theme.color.waveMid.value);
const HIGH = rgbTuple(theme.color.waveHigh.value);
const LOW_MID = rgbTuple(theme.color.waveLowMid.value);
const LOW_HIGH = rgbTuple(theme.color.waveLowHigh.value);
const MID_HIGH = rgbTuple(theme.color.waveMidHigh.value);
const ALL = rgbTuple(theme.color.waveAll.value);

/** The bit each band sets in a combination. */
export const BAND_LOW = 1;
export const BAND_MID = 2;
export const BAND_HIGH = 4;

/** The colour rekordbox paints where this combination of bands reaches. */
export function bandColour(combination: number): string {
  const c = [
    LOW, LOW, MID, LOW_MID, HIGH, LOW_HIGH, MID_HIGH, ALL,
  ][combination & 7] ?? LOW;
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

/**
 * The stops of a ramp through the three bands, low to cream, for a drawing
 * that has one value a column rather than three: the row preview's stacked
 * slabs, and any caller that wants "how bright is this column" as a colour.
 * The same palette for the overview and the detail.
 */
export function bandStops(_band: WaveBand = "overview"): readonly (readonly number[])[] {
  return [LOW, MID, ALL];
}

/** Colour at `t` (0..1) along a ramp through every stop in turn. */
export function ramp(stops: readonly (readonly number[])[], t: number): string {
  const last = stops.length - 1;
  if (last < 1) {
    const only = stops[0] ?? LOW;
    return `rgb(${only[0] ?? 0},${only[1] ?? 0},${only[2] ?? 0})`;
  }
  const clamped = Math.min(Math.max(Number.isFinite(t) ? t : 0, 0), 1);
  const scaled = clamped * last;
  const i = Math.min(Math.floor(scaled), last - 1);
  const a = stops[i] ?? LOW;
  const b = stops[i + 1] ?? ALL;
  const f = scaled - i;
  const c = (n: number) => Math.round((a[n] ?? 0) + ((b[n] ?? 0) - (a[n] ?? 0)) * f);
  return `rgb(${c(0)},${c(1)},${c(2)})`;
}

/**
 * How tall one band's value can be.
 *
 * Seven bits. Measured across 80 tracks of the reference library: `PWV7`
 * reaches the full 127 and `PWV6` reaches 98, so both are on the same scale.
 * An earlier reading of 63 came from a single track and drew every overview at
 * double height.
 */
export const BAND_FULL_SCALE = 127;

/**
 * What each band contributes to a stacked half waveform, as its full scale.
 *
 * The three are not weighted alike. Measured against a 2x capture of rekordbox
 * drawing "Take Me Home (ft. Bonn)" — 1,200 painted columns matched column for
 * column against that track's own `PWV6` — the blue slab rises 0.542 px per
 * unit over a 60 px band, the amber 0.270 and the near-white 0.567 (r = 0.99,
 * 0.97, 0.96). That is the low and high bands over 128 and the mid over 256,
 * and `(2·low + mid + 2·high) / 256` predicts the painted height to within
 * 3.4 % of the band — closer than an unconstrained least-squares fit, and far
 * closer than the flat `/150` this used to divide by, which drew the amber
 * thin, capped every loud passage in white and flattened the whole waveform.
 */
const STACK_SCALE = [128, 256, 128] as const;

/**
 * Draws a three-band waveform: `PWV6` or `PWV7`, three bytes a column.
 *
 * This is what rekordbox 7 actually shows, and what a CDJ-3000 shows. The
 * bytes are the energy in the low, mid and high thirds of the spectrum, and
 * each is drawn from the centre line in its own colour: blue underneath, amber
 * over it, near-white on top. Highs carry the least energy — a mean of 3
 * against 30 for the other two on a real track — so drawing them last is what
 * puts the bright core in the middle rather than burying it.
 */
export function drawBands(
  ctx: CanvasRenderingContext2D,
  data: Uint8Array,
  width: number,
  height: number,
  band: WaveBand = "overview",
  /**
   * Half height, growing from the bottom.
   *
   * `true` stacks the bands: blue, then amber on it, then near-white. This is
   * the row preview in the track list, where stacking is what gives the blue
   * its flat top with the amber riding above it — overlaying from a baseline
   * would hide the amber entirely whenever the lows are louder, which is most
   * of the time.
   *
   * `"overlaid"` draws each band from the baseline over the last, blue first
   * as the outer envelope and near-white last as the core — the centred
   * waveform's own layering, single-sided. This is the 2 PLAYER detail, where
   * the two decks' halves meet at the line between them [OBS].
   */
  half: HalfWaveform = false,
  /**
   * Rows left clear at the top and bottom, in device pixels.
   *
   * rekordbox's waveform does not reach the edges of its band: the strip above
   * it carries the bar count and the heads of the cue markers. Measured at 8pt
   * above and 2pt below.
   */
  inset: { top: number; bottom: number } = { top: 0, bottom: 0 },
): void {
  ctx.clearRect(0, 0, width, height);
  const columns = Math.floor(data.length / 3);
  if (columns === 0 || width <= 0 || height <= 0) return;

  const stops = bandStops(band);
  // The band the waveform actually draws into. Clamped so a large inset on a
  // short strip leaves something rather than inverting it.
  const top = Math.max(0, Math.min(inset.top, height / 2 - 1));
  const bottom = Math.max(0, Math.min(inset.bottom, height / 2 - 1));
  const usable = Math.max(1, height - top - bottom);
  const floor = height - bottom;
  const centre = top + usable / 2;
  const step = columns / width;

  for (let x = 0; x < width; x++) {
    // The loudest column in this pixel's span, per band, so a transient is not
    // swallowed when many columns share a pixel.
    let low = 0;
    let mid = 0;
    let high = 0;
    const first = Math.floor(x * step);
    if (step < 1 && band === "detail") {
      // Interpolate small variations in either direction, keeping every stored
      // peak. Only large attacks stay vertical at the next sample: ramping
      // those early turns a kick into a diamond. This is our rendering heuristic,
      // not a reconstruction of rekordbox's audio-derived zoom waveform.
      const next = Math.min(first + 1, columns - 1);
      const fraction = x * step - first;
      const interpolate = (channel: number) => {
        const a = data[first * 3 + channel] ?? 0;
        const b = data[next * 3 + channel] ?? a;
        // Even a quiet onset must not grow out of a silent bin early.
        const sharpAttack = (a === 0 && b > 0) || (b - a >= 16 && b >= a * 2);
        return sharpAttack ? a : a + (b - a) * fraction;
      };
      low = interpolate(0);
      mid = interpolate(1);
      high = interpolate(2);
    } else {
      const last = Math.max(first + 1, Math.floor((x + 1) * step));
      for (let i = first; i < last && i < columns; i++) {
        const at = i * 3;
        low = Math.max(low, data[at] ?? 0);
        mid = Math.max(mid, data[at + 1] ?? 0);
        high = Math.max(high, data[at + 2] ?? 0);
      }
    }
    const bands = [
      [low, stops[0], STACK_SCALE[0]],
      [mid, stops[1], STACK_SCALE[1]],
      [high, stops[2], STACK_SCALE[2]],
    ] as const;

    // A silent part of the actual file is still a meaningful zero-amplitude
    // signal. Draw its reference line here, inside the data span only; the
    // caller has already clipped that span at the file's start and end.
    if (low === 0 && mid === 0 && high === 0) {
      ctx.fillStyle = bandColour(BAND_LOW | BAND_HIGH);
      ctx.fillRect(x, half ? floor : centre, 1, 1);
      continue;
    }

    if (half === "overlaid") {
      // From the baseline, reaching the whole band at full scale as the
      // centred waveform reaches half of it each way; each stretch of the
      // column in the colour of the bands that reach it.
      for (const [reach, combination] of segments(low, mid, high, usable)) {
        ctx.fillStyle = bandColour(combination);
        ctx.fillRect(x, floor - reach, 1, reach);
      }
      continue;
    }

    if (half) {
      // Stacked from the bottom: blue, then amber on it, then near-white.
      let base = floor;
      for (const [value, colour, scale] of bands) {
        if (value === 0) continue;
        const tall = Math.min(value / scale, 1) * usable;
        ctx.fillStyle = `rgb(${colour?.[0] ?? 0},${colour?.[1] ?? 0},${colour?.[2] ?? 0})`;
        ctx.fillRect(x, Math.max(top, base - tall), 1, Math.min(tall, base - top));
        base -= tall;
        if (base <= top) break;
      }
      continue;
    }

    // Centred: the furthest-reaching band's stretch first, as the outer
    // envelope, then each nearer stretch over it in the colour of every band
    // that reaches that far, the core last.
    for (const [reach, combination] of segments(low, mid, high, usable / 2)) {
      ctx.fillStyle = bandColour(combination);
      ctx.fillRect(x, centre - reach, 1, reach * 2);
    }
  }
}

/**
 * A column as stretches from the outside in: each band's reach, furthest
 * first, paired with the bands that reach at least that far. Drawn in this
 * order each nearer stretch covers the last, so a pixel ends up in the
 * colour of exactly the bands that reach it. Bands with the same reach
 * share a stretch; a silent band has none.
 */
export function segments(low: number, mid: number, high: number, full: number): [number, number][] {
  const reachOf = (value: number) =>
    value === 0 ? 0 : Math.max(0.5, (Math.min(value, BAND_FULL_SCALE) / BAND_FULL_SCALE) * full);
  const reaches: [number, number][] = [
    [reachOf(low), BAND_LOW],
    [reachOf(mid), BAND_MID],
    [reachOf(high), BAND_HIGH],
  ];
  reaches.sort((a, b) => b[0] - a[0]);
  const out: [number, number][] = [];
  let combination = 0;
  for (const [reach, band] of reaches) {
    if (reach === 0) break;
    combination |= band;
    const last = out[out.length - 1];
    if (last && last[0] === reach) last[1] = combination;
    else out.push([reach, combination]);
  }
  return out;
}

/**
 * One column of a waveform, whatever tag it came from: how tall, and what
 * colour. `height` is 0 to 1 of full scale.
 */
interface Column {
  height: number;
  colour: string;
}

/**
 * Reads a `PWAV` / `PWV3` column: five bits of height, three of whiteness,
 * drawn as blue shading to near-white — the BLUE palette [DOC].
 */
function monoColumn(data: Uint8Array, at: number): Column {
  const byte = data[at] ?? 0;
  return {
    height: (byte & HEIGHT_MASK) / HEIGHT_MASK,
    colour: ramp([LOW, ALL], (byte >> WHITENESS_SHIFT) / 7),
  };
}

/**
 * Reads a `PWV4` column: six bytes, of which the first is the height (0 to
 * 127) and the last three the red, green and blue of the column; the RGB
 * palette. The channels track the low, mid and high bands in that order:
 * in rekordbox's own files the red runs with `PWV6`'s low band at r = 0.83
 * to 0.96 on eleven tracks [OBS 7.2.11, issue #158], as Deep Symmetry's
 * sine sweep found (bytes 3, 4 and 5 the bottom, middle and top of the
 * range), so bass is red. Bytes 1 and 2 are not read [UNKNOWN].
 */
function colourColumn(data: Uint8Array, at: number): Column {
  return {
    height: (data[at] ?? 0) / 127,
    colour: rgbOf(data[at + 3] ?? 0, data[at + 4] ?? 0, data[at + 5] ?? 0),
  };
}

/**
 * Reads a `PWV5` column: sixteen bits big-endian, `rrrgggbbbhhhhh00` — three
 * bits each of red, green and blue, then five of height [DOC], and the
 * channels track the low, mid and high bands as `PWV4`'s do [OBS 7.2.11,
 * issue #158].
 */
function colourDetailColumn(data: Uint8Array, at: number): Column {
  const word = ((data[at] ?? 0) << 8) | (data[at + 1] ?? 0);
  return {
    height: ((word >> 2) & 0x1f) / 0x1f,
    colour: rgbOf((word >> 13) & 7, (word >> 10) & 7, (word >> 7) & 7),
  };
}

/**
 * A column's colour from its three channels, whatever their scale: the
 * strongest channel is drawn at full, the others in proportion, so a
 * quiet column is as saturated as a loud one and only its height differs —
 * which is how the CDJ's own drawing of these bytes reads.
 */
function rgbOf(r: number, g: number, b: number): string {
  const peak = Math.max(r, g, b);
  if (peak === 0) return "rgb(0,0,0)";
  const scale = 255 / peak;
  return `rgb(${Math.round(r * scale)},${Math.round(g * scale)},${Math.round(b * scale)})`;
}

/**
 * Draws a one-colour-per-column waveform — the BLUE and RGB palettes — the
 * way `drawBands` draws the three-band one: centred by default, or a half
 * from the bottom, with the same insets. The loudest column in each pixel's
 * span is the one drawn, so a transient survives many columns to a pixel.
 */
export function drawColumns(
  ctx: CanvasRenderingContext2D,
  data: Uint8Array,
  width: number,
  height: number,
  palette: "blue" | "rgb",
  detail: boolean,
  half: HalfWaveform = false,
  inset: { top: number; bottom: number } = { top: 0, bottom: 0 },
): void {
  ctx.clearRect(0, 0, width, height);
  const stride = strideOf(palette, detail);
  const columns = Math.floor(data.length / stride);
  if (columns === 0 || width <= 0 || height <= 0) return;
  const read = palette === "blue" ? monoColumn : detail ? colourDetailColumn : colourColumn;

  const top = Math.max(0, Math.min(inset.top, height / 2 - 1));
  const bottom = Math.max(0, Math.min(inset.bottom, height / 2 - 1));
  const usable = Math.max(1, height - top - bottom);
  const floor = height - bottom;
  const centre = top + usable / 2;
  const step = columns / width;

  for (let x = 0; x < width; x++) {
    let peak: Column | null = null;
    const first = Math.floor(x * step);
    const last = Math.max(first + 1, Math.floor((x + 1) * step));
    for (let i = first; i < last && i < columns; i++) {
      const column = read(data, i * stride);
      if (!peak || column.height > peak.height) peak = column;
    }
    if (!peak || peak.height <= 0) {
      // See the equivalent three-band path above: silence is a visible,
      // horizontal zero-amplitude line, not a hole in the file.
      ctx.fillStyle = bandColour(BAND_LOW | BAND_HIGH);
      ctx.fillRect(x, half ? floor : centre, 1, 1);
      continue;
    }
    ctx.fillStyle = peak.colour;
    if (half) {
      const tall = Math.max(1, Math.min(peak.height, 1) * usable);
      ctx.fillRect(x, floor - tall, 1, tall);
    } else {
      const reach = Math.max(0.5, Math.min(peak.height, 1) * (usable / 2));
      ctx.fillRect(x, centre - reach, 1, reach * 2);
    }
  }
}

/**
 * Draws a waveform in the palette asked for. The bytes must be the tag that
 * palette reads (`waveformKindOf`).
 */
export function drawWave(
  ctx: CanvasRenderingContext2D,
  data: Uint8Array,
  width: number,
  height: number,
  palette: WavePalette,
  detail: boolean,
  half: HalfWaveform = false,
  inset: { top: number; bottom: number } = { top: 0, bottom: 0 },
): void {
  if (palette === "3band") {
    drawBands(ctx, data, width, height, detail ? "detail" : "overview", half, inset);
  } else {
    drawColumns(ctx, data, width, height, palette, detail, half, inset);
  }
}

/** Draw a DAW-style stereo PCM peak envelope (left/right min/max i16 pairs). */
export function drawPcmWave(
  ctx: CanvasRenderingContext2D,
  data: Uint8Array,
  width: number,
  height: number,
  palette: WavePalette,
  visible: { from: number; to: number } = { from: 0, to: 1 },
): void {
  ctx.clearRect(0, 0, width, height);
  const lanes = [height * 0.25 + 0.5, height * 0.75 + 0.5] as const;
  const from = visible.from;
  const to = Math.max(from + 1e-6, visible.to);
  // Silence has a visible zero-amplitude line in each stereo lane, rather
  // than disappearing into the background. Do not clamp its endpoints: at a
  // file edge, the canvas deliberately has empty overhang beyond the source.
  const colour = pcmColour(palette);
  ctx.strokeStyle = colour;
  ctx.fillStyle = colour;
  ctx.globalAlpha = 0.65;
  for (let x = 0; x < width; x++) {
    const fraction = from + (to - from) * x / width;
    if (fraction >= 0 && fraction <= 1) {
      ctx.fillRect(x, lanes[0], 1, 1);
      ctx.fillRect(x, lanes[1], 1, 1);
    }
  }
  const columns = Math.floor(data.length / 8);
  if (columns === 0 || width <= 0 || height <= 0) return;
  const values = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const first = Math.max(0, Math.floor(from * columns));
  const last = Math.min(columns, Math.ceil(to * columns));
  // Ableton-style waveform views scale the visible envelope, rather than
  // leaving a mastered-but-sub-full-scale track as a tiny trace.
  let peak = 0;
  for (let column = first; column < last; column++) {
    for (let part = 0; part < 4; part++) {
      peak = Math.max(peak, Math.abs(values.getInt16(column * 8 + part * 2, true)));
    }
  }
  const scale = Math.max(1, height / 4 - 2) / Math.max(peak, 1);
  const sampleAt = (fraction: number, lane: number, edge: number): number => {
    const at = Math.max(0, Math.min(fraction * (columns - 1), columns - 1));
    const column = Math.floor(at);
    const next = Math.min(column + 1, columns - 1);
    const blend = at - column;
    const a = values.getInt16(column * 8 + lane * 4 + edge * 2, true);
    const b = values.getInt16(next * 8 + lane * 4 + edge * 2, true);
    return a + (b - a) * blend;
  };
  for (let lane = 0; lane < 2; lane++) {
    // Fill between the negative and positive peak edges, which gives the
    // close inspection view a solid waveform body without hiding its shape.
    let started = false;
    ctx.globalAlpha = 0.26;
    ctx.fillStyle = colour;
    ctx.beginPath();
    for (let x = 0; x < width; x++) {
      const fraction = from + (to - from) * x / width;
      if (fraction < 0 || fraction > 1) continue;
      const y = (lanes[lane] ?? height / 2) - sampleAt(fraction, lane, 1) * scale;
      if (started) ctx.lineTo(x + 0.5, y);
      else ctx.moveTo(x + 0.5, y);
      started = true;
    }
    for (let x = width - 1; x >= 0; x--) {
      const fraction = from + (to - from) * x / width;
      if (fraction < 0 || fraction > 1) continue;
      ctx.lineTo(x + 0.5, (lanes[lane] ?? height / 2) - sampleAt(fraction, lane, 0) * scale);
    }
    if (started) {
      ctx.closePath();
      ctx.fill();
    }

    ctx.globalAlpha = 1;
    ctx.strokeStyle = colour;
    for (let edge = 0; edge < 2; edge++) {
      let joined = false;
      ctx.beginPath();
      for (let x = 0; x < width; x++) {
        const fraction = from + (to - from) * x / width;
        if (fraction < 0 || fraction > 1) {
          joined = false;
          continue;
        }
        // Linear interpolation prevents staircase-shaped traces when a
        // decimation point spans more than one device pixel.
        const sample = sampleAt(fraction, lane, edge);
        const y = (lanes[lane] ?? height / 2) - sample * scale;
        if (joined) ctx.lineTo(x + 0.5, y);
        else ctx.moveTo(x + 0.5, y);
        joined = true;
      }
      ctx.stroke();
    }
  }
  ctx.globalAlpha = 1;
}

/**
 * PCM has no frequency bands of its own, so it uses one representative hue
 * for each normal waveform palette rather than pretending the samples contain
 * PWV7 spectrum data. These deliberately are not the dim grid colours:
 * `--c-beat` is #4C4C4C. Each is light/saturated enough to remain separable
 * from both that grid and the white downbeat line on the black detail band.
 */
function pcmColour(palette: WavePalette): string {
  switch (palette) {
    // Blue's familiar low-band hue, raised so it does not merge into #4C4C4C.
    case "blue": return theme.color.pcmBlue.value;
    // 3Band's amber middle band: distinct from its white high-band/downbeat.
    case "3band": return theme.color.pcm3Band.value;
    // A vivid magenta is the neutral representative of the varying RGB view.
    case "rgb": return theme.color.pcmRgb.value;
  }
}

/**
 * Draws a `PWAV` preview into a context.
 *
 * `data` is one byte per column. The canvas is scaled to fit however many
 * columns there are, so a 400-column preview fills a 120px cell.
 */
export function drawPreview(
  ctx: CanvasRenderingContext2D,
  data: Uint8Array,
  width: number,
  height: number,
  options?: {
    /**
     * The slice of the track to draw, as fractions of its length. Defaults to
     * all of it; the detail waveform passes a window around the playhead,
     * which is what makes it a *detail* rather than a second copy.
     */
    window?: { from: number; to: number };
    /** Which palette. Defaults to the overview's brighter amber. */
    band?: WaveBand;
  },
): void {
  const window = options?.window;
  const stops = bandStops(options?.band ?? "overview");
  ctx.clearRect(0, 0, width, height);
  if (data.length === 0) return;

  // Clamped and ordered, so a playhead at either end still draws something.
  const from = Math.max(0, Math.min(window?.from ?? 0, 1));
  const to = Math.max(from + 1e-6, Math.min(window?.to ?? 1, 1));
  const first = Math.floor(from * data.length);
  const last = Math.max(first + 1, Math.ceil(to * data.length));
  const span = last - first;

  const step = span / width;
  for (let x = 0; x < width; x++) {
    // Take the loudest column in this pixel's span so quiet gaps do not
    // swallow transients when many columns share a pixel.
    let peak = 0;
    let whiteness = 0;
    const columnFrom = first + Math.floor(x * step);
    const columnTo = Math.max(columnFrom + 1, first + Math.floor((x + 1) * step));
    for (let i = columnFrom; i < columnTo && i < data.length; i++) {
      const byte = data[i] ?? 0;
      const h = byte & HEIGHT_MASK;
      if (h > peak) {
        peak = h;
        whiteness = byte >> WHITENESS_SHIFT;
      }
    }
    if (peak === 0) continue;
    const columnHeight = Math.max(1, Math.round((peak / HEIGHT_MASK) * height));
    ctx.fillStyle = ramp(stops, whiteness / 7);
    ctx.fillRect(x, height - columnHeight, 1, columnHeight);
  }
}

/**
 * A hot cue on a row's preview: letter, position in ms, drawn colour.
 *
 * The `RowCue` tuple from the IPC contract, by shape rather than by import so
 * the canvas module stays free of it.
 */
export type PreviewCue = readonly [letter: string, positionMs: number, colour: string | null];

/**
 * Canvas measurements and colours come from the same theme source as CSS.
 */
/** A hot cue badge over a row's waveform, square, in CSS pixels. */
export const PREVIEW_BADGE = 7; // --s-preview-cue-badge
const PREVIEW_BADGE_FONT = 6; // --f-size-preview-cue
const CUE_HOT = theme.color.cueHot.value;
const CUE_HOT_TEXT = theme.color.cueHotText.value;
const UI_FONT = 'Arial, "Helvetica Neue", Helvetica, sans-serif'; // --f-ui

/**
 * Draws a row's hot cue badges over its preview.
 *
 * Measured off `design/reference/macos/playlist-player@2x.png`: a 7pt square
 * at the top of the band with its left edge on the cue, filled with the
 * colour rekordbox paints for the cue's `ColorTableIndex` and a bold black
 * letter centred in it. The badges sit on the waveform, not above it — the
 * band's top is where both begin — so they cover whatever peak is under
 * them, as the capture's do.
 *
 * Painted in the order given, which the backend makes slot order, so where
 * two cues share a position the later slot is on top. A badge at the far end
 * of the track is pulled back inside the strip rather than cut off: the
 * letter is the point of it.
 */
export function drawPreviewCues(
  ctx: CanvasRenderingContext2D,
  cues: readonly PreviewCue[],
  durationMs: number,
  width: number,
  dpr: number,
): void {
  if (cues.length === 0 || durationMs <= 0 || width <= 0) return;
  const size = PREVIEW_BADGE * dpr;
  ctx.font = `700 ${PREVIEW_BADGE_FONT * dpr}px ${UI_FONT}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  for (const [letter, positionMs, colour] of cues) {
    const at = Math.min(Math.max(positionMs / durationMs, 0), 1);
    // Whole device pixels, so the square has a crisp edge at every DPR.
    const x = Math.round(Math.min(at * width, width - size));
    ctx.fillStyle = colour ?? CUE_HOT;
    ctx.fillRect(x, 0, size, size);
    ctx.fillStyle = CUE_HOT_TEXT;
    ctx.fillText(letter, x + size / 2, size / 2);
  }
}

/**
 * Where a click on a row's waveform starts the preview, in milliseconds.
 *
 * rekordbox: a click on a hot cue's badge starts from the cue itself
 * (`PreviewComponent::cueRegionMouseDown`); anywhere else, from the click's
 * fraction of the width, clamped to it (`PreviewComponent::clickWave`). The
 * badges are where `drawPreviewCues` puts them, and the last drawn — the one
 * on top — wins. `x` and `y` are CSS pixels from the waveform's top left.
 */
export function previewClickMs(
  x: number,
  y: number,
  width: number,
  durationMs: number,
  cues: readonly PreviewCue[],
): number {
  if (width <= 0 || durationMs <= 0) return 0;
  if (y >= 0 && y < PREVIEW_BADGE) {
    for (let i = cues.length - 1; i >= 0; i--) {
      const cue = cues[i];
      if (!cue) continue;
      const positionMs = cue[1];
      const at = Math.min(Math.max(positionMs / durationMs, 0), 1);
      const left = Math.min(at * width, width - PREVIEW_BADGE);
      if (x >= left && x < left + PREVIEW_BADGE) return Math.min(Math.max(positionMs, 0), durationMs);
    }
  }
  const fraction = Math.min(Math.max(x / width, 0), 1);
  return fraction * durationMs;
}

/** Memory cues use centered red downward triangles, below hot-cue badges. */
export function drawPreviewMemoryCues(
  ctx: CanvasRenderingContext2D, positions: readonly number[], durationMs: number, width: number, dpr: number,
): void {
  if (durationMs <= 0 || width <= 0 || positions.length === 0) return;
  const half = 3 * dpr;
  const height = 4 * dpr;
  ctx.fillStyle = theme.color.cueHead.value;
  for (const position of positions) {
    if (position < 0 || position > durationMs || !Number.isFinite(position)) continue;
    const x = Math.round(position / durationMs * width);
    ctx.beginPath();
    ctx.moveTo(x - half, 0);
    ctx.lineTo(x + half, 0);
    ctx.lineTo(x, height);
    ctx.closePath();
    ctx.fill();
  }
}

/** Renders a preview to an offscreen bitmap at device resolution. */
export async function renderPreview(
  data: Uint8Array,
  width: number,
  height: number,
  dpr: number,
  palette: WavePalette = "3band",
): Promise<RenderedWaveform | null> {
  const w = Math.max(1, Math.round(width * dpr));
  const h = Math.max(1, Math.round(height * dpr));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  // The row preview: half height from the baseline, bands stacked.
  drawWave(ctx, data, w, h, palette, false, true);

  // An ImageBitmap blits faster than a canvas element; fall back where the
  // browser lacks it rather than failing to draw at all.
  if (typeof createImageBitmap === "function") {
    try {
      return { bitmap: await createImageBitmap(canvas), width: w, height: h };
    } catch {
      // fall through
    }
  }
  return { bitmap: canvas, width: w, height: h };
}

/** Bounded cache of rendered previews, keyed by track and size. */
export class WaveformCache {
  #entries = new Map<string, RenderedWaveform>();
  #max: number;

  constructor(max = 500) {
    this.#max = max;
  }

  static key(trackId: string, width: number, dpr: number): string {
    return `${trackId}:${Math.round(width)}:${dpr}`;
  }

  get(key: string): RenderedWaveform | undefined {
    const hit = this.#entries.get(key);
    if (hit) {
      this.#entries.delete(key);
      this.#entries.set(key, hit);
    }
    return hit;
  }

  set(key: string, value: RenderedWaveform): void {
    this.#entries.delete(key);
    this.#entries.set(key, value);
    while (this.#entries.size > this.#max) {
      const oldest = this.#entries.keys().next();
      if (oldest.done) break;
      const evicted = this.#entries.get(oldest.value);
      // Release the GPU-side copy explicitly; the GC will not do it promptly.
      if (evicted && "close" in evicted.bitmap) evicted.bitmap.close();
      this.#entries.delete(oldest.value);
    }
  }

  get size(): number {
    return this.#entries.size;
  }

  /** Drops every rendering of one track: its analysis was rewritten. */
  forget(trackId: string): void {
    const marker = `:${trackId}:`;
    for (const [key, entry] of this.#entries) {
      if (!key.includes(marker)) continue;
      if ("close" in entry.bitmap) entry.bitmap.close();
      this.#entries.delete(key);
    }
  }

  clear(): void {
    for (const entry of this.#entries.values()) {
      if ("close" in entry.bitmap) entry.bitmap.close();
    }
    this.#entries.clear();
  }
}
