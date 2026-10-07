/**
 * The player's arithmetic, kept out of the component so it can be tested.
 *
 * The detail waveform shows a fixed number of *bars*, not a fixed fraction of
 * the track. That is the difference between a zoom level and a scale: at a
 * fixed fraction a three-minute track and a ninety-minute mix show wildly
 * different amounts of music, and neither matches what a CDJ shows.
 */
import type { SortColumn } from "@/ipc/types";

/** Bars across the detail waveform. Matches rekordbox's default zoom. */
export const DETAIL_BARS = 12;

/** Beats in a bar. Everything here assumes 4/4, as rekordbox's grid does. */
export const BEATS_PER_BAR = 4;

/**
 * How much wider than the strip the scrolling layer is drawn.
 *
 * The detail waveform scrolls under a fixed playhead, and redrawing it on
 * every tick is what made it step ten times a second instead of moving. So it
 * is drawn once across twice the visible span and slid by a transform, which
 * the compositor does every frame for nothing; a redraw happens only when the
 * playhead has travelled far enough to see the end of what was drawn.
 */
export const OVERDRAW = 2;

/**
 * Zoom levels, in bars across, that the +/- buttons and the wheel step
 * through. Rekordbox's closest view is half a bar. It is rendered from PCM
 * here, with one additional quarter-bar inspection step beyond it.
 */
export const ZOOM_STEPS = [0.25, 0.5, 1, 2, 4, 8, 12, 16, 32, 64] as const;

/**
 * Whether the grid draws every beat, or only the bar lines.
 *
 * At the widest step the beats are a few pixels apart and the grid stops being
 * a grid: it is a picket fence over the waveform, and the downbeats that make
 * it readable are lost among them. Bars alone still say where the phrase is.
 */
export function showsEveryBeat(bars: number): boolean {
  return bars < (ZOOM_STEPS.at(-1) ?? 64);
}

/** A beat-jump size as the size menu lists it. */
export interface JumpSize {
  id: string;
  /** As printed, with no space — "8Beats", "16Bars". */
  label: string;
  /** Beats per press. Zero is the fine nudge, which is a time, not a length. */
  beats: number;
}

/**
 * The sizes rekordbox's beat-jump menu offers, in its order.
 *
 * Transcribed from the menu itself rather than derived: it skips 1 and 2 beats
 * and changes unit at 8 bars, so a generated list of powers of two would be
 * neither its wording nor its contents.
 */
export const JUMP_SIZES: readonly JumpSize[] = [
  { id: "fine", label: "Fine", beats: 0 },
  { id: "4beats", label: "4Beats", beats: 4 },
  { id: "8beats", label: "8Beats", beats: 8 },
  { id: "16beats", label: "16Beats", beats: 16 },
  { id: "8bars", label: "8Bars", beats: 32 },
  { id: "16bars", label: "16Bars", beats: 64 },
  { id: "32bars", label: "32Bars", beats: 128 },
];

/** The default, and what the button reads before anyone touches it. */
export const JUMP_SIZE_ID = "4beats";

/**
 * How far a fine press moves, in seconds.
 *
 * [ASSUME] Fine is the one size in the menu that is not a musical length, and
 * what rekordbox moves by has not been measured. Ten milliseconds is the step
 * a CDJ's fine search uses and is small enough to be a nudge at any tempo;
 * replace it with a measured figure rather than treating this as settled.
 */
export const FINE_JUMP_SECONDS = 0.01;

/** The size with that id, or the default when the id means nothing. */
export function jumpSizeById(id: string): JumpSize {
  return (
    JUMP_SIZES.find((size) => size.id === id) ??
    JUMP_SIZES.find((size) => size.id === JUMP_SIZE_ID) ??
    { id: JUMP_SIZE_ID, label: "4Beats", beats: 4 }
  );
}

/** The next size along, wrapping — for cycling without opening the menu. */
export function nextJumpSize(current: string): string {
  const at = JUMP_SIZES.findIndex((size) => size.id === current);
  return JUMP_SIZES[(at + 1) % JUMP_SIZES.length]?.id ?? JUMP_SIZE_ID;
}

/** How long a jump of `beats` lasts at a tempo. Zero when there is no tempo. */
export function jumpSeconds(beats: number, bpmX100: number): number {
  if (bpmX100 <= 0 || beats <= 0) return 0;
  return (beats * 60) / (bpmX100 / 100);
}

/** How far one press of the jump buttons moves at the chosen size. */
export function jumpStepSeconds(size: JumpSize, bpmX100: number): number {
  return size.beats > 0 ? jumpSeconds(size.beats, bpmX100) : FINE_JUMP_SECONDS;
}

/** Pixels of wheel travel that make one zoom step: about one mouse notch. */
export const WHEEL_STEP_PX = 100;
/** After a step, further wheel input is ignored this long (ms), so a trackpad's
 * stream and its inertia tail do not run through every zoom level. */
export const WHEEL_COOLDOWN_MS = 150;
/** A pause this long (ms) starts a new gesture and drops any partial travel. */
export const WHEEL_IDLE_MS = 250;

/**
 * Turns a stream of wheel events into zoom steps.
 *
 * A mouse notch is one event of about a hundred pixels; a trackpad swipe is
 * dozens of small events plus an inertia tail. Distance is accumulated to a
 * step and each step is followed by a short cooldown, so a swipe is a step or
 * two rather than the whole zoom range. Returns -1 (zoom in), 1 (zoom out)
 * or 0 (no step yet).
 */
export function createWheelZoomGate() {
  let travel = 0;
  let lastEvent = Number.NEGATIVE_INFINITY;
  let lastStep = Number.NEGATIVE_INFINITY;
  return (deltaPx: number, now: number): -1 | 0 | 1 => {
    if (deltaPx === 0 || !Number.isFinite(deltaPx)) return 0;
    if (now - lastEvent > WHEEL_IDLE_MS) travel = 0;
    lastEvent = now;
    // Reversing direction discards travel in the old one.
    if (travel !== 0 && Math.sign(travel) !== Math.sign(deltaPx)) travel = 0;
    if (now - lastStep < WHEEL_COOLDOWN_MS) return 0;
    travel += deltaPx;
    if (Math.abs(travel) < WHEEL_STEP_PX) return 0;
    const direction = travel > 0 ? 1 : -1;
    travel = 0;
    lastStep = now;
    return direction;
  };
}

/**
 * The zoom a wheel gesture lands on.
 *
 * Wheels differ wildly — a mouse notch is 100-odd pixels and a trackpad emits
 * a stream of ones — so this steps one level per call and lets the caller
 * decide what counts as a gesture. Up zooms in, which is the direction every
 * map and every waveform editor uses.
 */
export function zoomBy(bars: number, direction: number): number {
  const at = ZOOM_STEPS.indexOf(bars as (typeof ZOOM_STEPS)[number]);
  const from = at === -1 ? ZOOM_STEPS.indexOf(DETAIL_BARS) : at;
  const to = Math.min(Math.max(from + direction, 0), ZOOM_STEPS.length - 1);
  return ZOOM_STEPS[to] ?? DETAIL_BARS;
}

/**
 * What fraction of a track `bars` covers at a given tempo.
 *
 * Falls back to a fraction of the whole track when the tempo or length is
 * unknown, so an unanalysed track still draws something rather than dividing
 * by zero.
 */
export function detailSpan(bars: number, bpmX100: number, durationSec: number): number {
  if (bpmX100 <= 0 || durationSec <= 0) return 0.08;
  const bpm = bpmX100 / 100;
  const seconds = (bars * BEATS_PER_BAR * 60) / bpm;
  // Never more than the whole track, and never so small that a rounding error
  // collapses the window to nothing.
  return Math.min(1, Math.max(seconds / durationSec, 1e-4));
}

/** Rekordbox's scrolling waveform tags (`PWV3`, `PWV5`, `PWV7`) run at 150 columns/second. */
export const DETAIL_WAVE_COLUMNS_PER_SECOND = 150;

/**
 * Converts an audio-player window to the fixed clock of a scrolling waveform.
 *
 * The decoded audio duration and the tag's column count commonly differ by a
 * few dozen milliseconds. Normalising both to the audio duration stretches
 * the waveform, so its attacks drift away from the timestamped beat grid.
 */
export function detailWaveWindow(
  progress: number,
  span: number,
  audioDurationMs: number,
  columns: number,
  originMs = 0,
): { progress: number; span: number } {
  if (audioDurationMs <= 0 || columns <= 0) return { progress, span };
  const waveformDurationMs = columns / DETAIL_WAVE_COLUMNS_PER_SECOND * 1000;
  return {
    progress: (progress * audioDurationMs + originMs) / waveformDurationMs,
    span: span * audioDurationMs / waveformDurationMs,
  };
}

/**
 * Clock correction for an analysis waveform whose first attack includes an
 * encoder's leading samples while the beat grid and decoder do not.
 *
 * Only infer it for the unambiguous start-of-file case: a near-zero first
 * beat and a short run of genuinely silent waveform columns followed by an
 * attack. The column timestamp is its centre, not its left edge. Tracks with
 * an intro before their first beat deliberately return zero.
 */
export function detailWaveOriginMs(
  firstBeatMs: number | undefined,
  bytes: Uint8Array,
  stride: number,
): number {
  if (firstBeatMs === undefined || firstBeatMs < 0 || firstBeatMs > 100 || stride <= 0) return 0;
  const columns = Math.floor(bytes.length / stride);
  let first = -1;
  for (let column = 0; column < Math.min(columns, 16); column += 1) {
    const at = column * stride;
    if (bytes.subarray(at, at + stride).some((value) => value !== 0)) {
      first = column;
      break;
    }
  }
  if (first <= 0) return 0;
  const attackMs = (first + 0.5) / DETAIL_WAVE_COLUMNS_PER_SECOND * 1000;
  const correction = attackMs - firstBeatMs;
  return correction > 0 && correction <= 50 ? correction : 0;
}

/**
 * The slice of the track a window of `span` centred on `at` covers.
 *
 * Not clamped to the track. The head stays in the middle and the waveform
 * moves under it, so at the start and the end the window hangs off the edge
 * and the empty half is drawn empty — which is what a CDJ shows, and the only
 * way the head can mean "here" rather than "somewhere in this strip".
 */
export function windowAround(at: number, span: number): { from: number; to: number } {
  const half = Math.max(span, 0) / 2;
  return { from: at - half, to: at + half };
}

/**
 * Where the playhead sits inside that window, as a percentage.
 *
 * Always the middle. The window moves instead — see `windowAround`.
 */
export function headPercent(): number {
  return 50;
}

/**
 * How far a drag of `dx` pixels moves the playhead, in seconds.
 *
 * Negated: dragging the waveform to the right pulls earlier music into view,
 * so the playhead goes back. The window's own span sets the rate, which is
 * what makes a zoomed-in drag fine and a zoomed-out one coarse rather than
 * both moving by the same amount of music per pixel.
 */
export function dragSeconds(
  dx: number,
  width: number,
  span: number,
  durationSec: number,
): number {
  if (width <= 0 || durationSec <= 0 || !Number.isFinite(dx)) return 0;
  return -(dx / width) * Math.max(span, 0) * durationSec;
}

/**
 * `-05:38.1` — time remaining, tenths in a smaller face.
 *
 * Minutes are padded to two digits, as rekordbox prints them everywhere in the
 * player: unpadded, the readout shifts by a character as a track crosses ten
 * minutes and the columns beside it move with it.
 *
 * Returned split so the caller can size the fraction differently.
 */
export function splitTime(seconds: number): { main: string; tenths: string } {
  const safe = Number.isFinite(seconds) ? Math.abs(seconds) : 0;
  const whole = Math.floor(safe);
  const tenths = Math.floor((safe - whole) * 10);
  const minutes = Math.floor(whole / 60);
  const rest = whole % 60;
  return {
    main: `${seconds < 0 ? "−" : ""}${String(minutes).padStart(2, "0")}:${String(rest).padStart(2, "0")}`,
    tenths: String(tenths),
  };
}

/**
 * `00:00:046` — how the memory list prints a position, to the millisecond.
 *
 * Built on `splitTime` so the minute padding is decided in one place; the two
 * lists disagreeing about that is exactly the sort of thing nobody notices
 * until the columns stop lining up.
 */
export function memoryTime(positionMs: number): string {
  const safe = Number.isFinite(positionMs) ? Math.max(positionMs, 0) : 0;
  const ms = Math.round(safe) % 1000;
  return `${splitTime(safe / 1000).main}:${String(ms).padStart(3, "0")}`;
}

/** Phrase kinds, as the colour tokens name them. */
export type PhraseKind =
  | "intro" | "verse" | "bridge" | "chorus"
  | "up" | "up2" | "up3" | "down" | "outro";

/**
 * Which colour token a phrase label takes.
 *
 * The labels come from the analysis, so this matches on what rekordbox writes
 * rather than on a numeric kind: `UP 1`, `UP 2` and `UP 3` are separate
 * colours, and everything else keys off its first word.
 */
export function phraseKind(label: string): PhraseKind {
  const text = label.trim().toUpperCase();
  if (text.startsWith("UP")) {
    if (text.includes("3")) return "up3";
    if (text.includes("2")) return "up2";
    return "up";
  }
  if (text.startsWith("DOWN")) return "down";
  if (text.startsWith("CHORUS")) return "chorus";
  if (text.startsWith("INTRO")) return "intro";
  if (text.startsWith("OUT")) return "outro";
  if (text.startsWith("VERSE")) return "verse";
  if (text.startsWith("BRIDGE")) return "bridge";
  // An unknown label still gets a block rather than a gap: a hole in the
  // phrase bar reads as missing analysis, which is a different problem.
  return "verse";
}

/** A phrase laid out as a fraction of the track. */
export interface PhraseSpan {
  label: string;
  kind: PhraseKind;
  from: number;
  to: number;
}

/**
 * Phrases as spans, each running to the start of the next.
 *
 * The analysis gives a start time per phrase and nothing else, so the end of
 * one is the start of the next and the last runs to the end of the track.
 */
export function phraseSpans(
  phrases: readonly { timeMs: number | null; beat: number; label: string }[],
  totalMs: number,
  /** Milliseconds a beat lasts, for phrases the beat grid did not reach. */
  beatMs = 0,
): PhraseSpan[] {
  if (totalMs <= 0) return [];
  // A phrase with no resolved time is placed from its beat where the tempo is
  // known, and dropped where it is not — a phrase at zero would stack every
  // unresolved block on the left edge and read as a mangled track.
  const at = (phrase: { timeMs: number | null; beat: number }): number | null =>
    phrase.timeMs ?? (beatMs > 0 ? (phrase.beat - 1) * beatMs : null);

  const placed = phrases
    .map((phrase) => ({ label: phrase.label, ms: at(phrase) }))
    .filter((phrase): phrase is { label: string; ms: number } => phrase.ms !== null)
    .sort((a, b) => a.ms - b.ms);

  return placed
    .map((phrase, i) => {
      const next = placed[i + 1];
      return {
        label: phrase.label,
        kind: phraseKind(phrase.label),
        from: Math.min(Math.max(phrase.ms / totalMs, 0), 1),
        to: Math.min(Math.max((next ? next.ms : totalMs) / totalMs, 0), 1),
      };
    })
    .filter((span) => span.to > span.from);
}

/** Column keys the player's readouts correspond to, for the info panel. */
export const READOUT_COLUMNS: readonly SortColumn[] = ["key", "bpm"];

/** Which set of controls the pad row is showing. */
export type PadMode = "cue" | "grid";

/** Which list the panel beside the deck is showing. */
export type CuePanel = "memory" | "hotCue" | "info";

/**
 * The cues one panel tab lists.
 *
 * Memory cues and hot cues are the same rows told apart by a flag, so the two
 * tabs are one filter rather than two fetches. Ordered by position, because a
 * cue list read out of order is unusable for finding a section.
 */
export function cuesFor<T extends { memory: boolean; positionMs: number }>(
  cues: readonly T[],
  panel: CuePanel,
): T[] {
  if (panel === "info") return [];
  const want = panel === "memory";
  return cues.filter((cue) => cue.memory === want).sort((a, b) => a.positionMs - b.positionMs);
}

/**
 * A track's beat grid, held as typed arrays rather than objects.
 *
 * Fetched whole, once per track: windowing it meant re-reading and re-parsing
 * the analysis file every time the playhead moved on. A three-hour mix is
 * about 23,000 beats, which is 23,000 small objects if this were a list and
 * 115 KB of typed array if it is not.
 */
export interface BeatGrid {
  /** Each beat's position in milliseconds, ascending. */
  times: Uint32Array;
  /** Each beat's number within its bar, 1 to 4. */
  numbers: Uint8Array;
  /**
   * The tempo x100 at each beat. A grid may change tempo partway through —
   * an edit from a selected beat, or a track that simply speeds up — so this is
   * per beat rather than one number for the track.
   */
  tempos: Uint16Array;
}

/**
 * The grid with each beat split into `divisions` equal steps, for a quantize
 * beat value finer than a beat: 1/2 gives the off-beats too, 1/8 every
 * thirty-second. Each step carries the number of the beat it belongs to. One
 * or fewer divisions, or an empty grid, is the grid itself.
 */
export function subdivideGrid(grid: BeatGrid, divisions: number): BeatGrid {
  const steps = Math.floor(divisions);
  if (steps <= 1 || grid.times.length < 2) return grid;
  const beats = grid.times.length;
  const times = new Uint32Array((beats - 1) * steps + 1);
  const numbers = new Uint8Array(times.length);
  const tempos = new Uint16Array(times.length);
  let at = 0;
  for (let i = 0; i < beats - 1; i++) {
    const from = grid.times[i] ?? 0;
    const to = grid.times[i + 1] ?? from;
    const number = grid.numbers[i] ?? 1;
    const tempo = grid.tempos[i] ?? 0;
    for (let step = 0; step < steps; step++) {
      times[at] = Math.round(from + ((to - from) * step) / steps);
      numbers[at] = number;
      tempos[at] = tempo;
      at++;
    }
  }
  times[at] = grid.times[beats - 1] ?? 0;
  numbers[at] = grid.numbers[beats - 1] ?? 1;
  tempos[at] = grid.tempos[beats - 1] ?? 0;
  return { times, numbers, tempos };
}

/**
 * Bytes one beat takes on the wire: a little-endian `u32` of milliseconds,
 * its number in the bar, then a little-endian `u16` of the tempo there x100.
 */
const BEAT_BYTES = 7;

/** An empty grid, so a track without analysis is still a `BeatGrid`. */
export const NO_BEATS: BeatGrid = {
  times: new Uint32Array(),
  numbers: new Uint8Array(),
  tempos: new Uint16Array(),
};

/**
 * Reads the backend's beat bytes.
 *
 * A trailing partial record is dropped rather than read past the end: the
 * backend never writes one, and a truncated read should draw fewer beats
 * rather than a beat at a garbage position.
 */
export function parseBeatGrid(bytes: Uint8Array): BeatGrid {
  const count = Math.floor(bytes.length / BEAT_BYTES);
  if (count === 0) return NO_BEATS;
  const times = new Uint32Array(count);
  const numbers = new Uint8Array(count);
  const tempos = new Uint16Array(count);
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  for (let i = 0; i < count; i++) {
    times[i] = view.getUint32(i * BEAT_BYTES, true);
    numbers[i] = view.getUint8(i * BEAT_BYTES + 4);
    tempos[i] = view.getUint16(i * BEAT_BYTES + 5, true);
  }
  return { times, numbers, tempos };
}

/**
 * The most beats one window draws.
 *
 * The widest zoom is 64 bars, so 256 beats; this is generous for that and
 * stops a nonsense window from asking for thousands of spans.
 */
const MAX_DRAWN = 1024;

/** The index of the first beat at or after `ms`. */
function lowerBound(times: Uint32Array, ms: number): number {
  let lo = 0;
  let hi = times.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((times[mid] ?? 0) < ms) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/**
 * The beats inside a window, as the grid component draws them.
 *
 * A binary search rather than a filter over the whole grid: this runs on every
 * tick of the playhead, and scanning 23,000 beats to draw forty-eight of them
 * is the sort of thing that shows up as a dropped frame.
 */
export function beatsIn(
  grid: BeatGrid,
  fromMs: number,
  toMs: number,
): { timeMs: number; downbeat: boolean }[] {
  const out: { timeMs: number; downbeat: boolean }[] = [];
  if (toMs < fromMs) return out;
  for (let i = lowerBound(grid.times, fromMs); i < grid.times.length; i++) {
    const timeMs = grid.times[i] ?? 0;
    if (timeMs > toMs || out.length >= MAX_DRAWN) break;
    out.push({ timeMs, downbeat: grid.numbers[i] === 1 });
  }
  return out;
}

/**
 * The tempo x100 the grid holds at `ms`, or 0 when there is no grid.
 *
 * The tempo of the last beat at or before `ms`, so a grid that changes tempo
 * partway through reads as the new one from the beat it changes at. Before
 * the first beat it is the first beat's: the run-in to a track belongs to the
 * tempo it starts at.
 */
export function tempoAtMs(grid: BeatGrid, ms: number): number {
  const { times, tempos } = grid;
  if (times.length === 0) return 0;
  const at = lowerBound(times, ms);
  // `lowerBound` gives the first beat at or after `ms`; the tempo runs from
  // the beat before it, except at or before the first beat.
  const beat = (times[at] ?? Number.POSITIVE_INFINITY) <= ms ? at : Math.max(at - 1, 0);
  return tempos[beat] ?? 0;
}

/** BPM x100 at the first grid beat or an exact tempo boundary, independent of cues. */
export function tempoChangeAtMs(grid: BeatGrid, ms: number): number | null {
  const at = lowerBound(grid.times, ms);
  if (grid.times[at] !== ms) return null;
  const tempo = grid.tempos[at] ?? 0;
  return tempo > 0 && (at === 0 || tempo !== grid.tempos[at - 1]) ? tempo : null;
}

/**
 * The beat nearest `ms`, or `ms` itself when there is no grid.
 *
 * What the Q button does. A cue set by hand lands a few tens of milliseconds
 * off the beat and every loop and every mix from it inherits that; quantising
 * is why a CDJ's cues sit on the grid whatever the finger did.
 */
export function nearestBeatMs(grid: BeatGrid, ms: number): number {
  const { times } = grid;
  if (times.length === 0) return ms;
  const at = lowerBound(times, ms);
  const after = times[Math.min(at, times.length - 1)] ?? ms;
  // `lowerBound` gives the first beat at or after; the one before it is the
  // only other candidate, so this is two comparisons rather than a scan.
  const before = times[Math.max(at - 1, 0)] ?? after;
  return Math.abs(ms - before) <= Math.abs(after - ms) ? before : after;
}

/** What the deck should do, decided by the CUE button. */
export interface CueAction {
  /** Where to move the playhead, or `null` to leave it. */
  seekTo: number | null;
  /** Whether audio should be running after this. */
  playing: boolean;
  /** The cue point afterwards, which a press away from it moves. */
  cuePoint: number;
}

/** How close to the cue point still counts as being on it, in seconds. */
export const CUE_TOLERANCE = 0.02;

/**
 * Pressing CUE, as a CDJ does it.
 *
 * Three cases, and they are not variations of one thing:
 *
 * - **Playing.** Stop, and jump back to the cue point. This is the one people
 *   use mid-mix, and it is why the button is where it is.
 * - **Paused on the cue point.** Play for as long as the button is held, then
 *   snap back — `releaseCue` is the other half. Previewing the drop without
 *   losing your place is the whole point of the control.
 * - **Paused anywhere else.** Set the cue point here. A CDJ does not need a
 *   separate "set cue" button because this is it.
 */
export function pressCue(
  position: number,
  cuePoint: number,
  playing: boolean,
  /** The grid to snap a new cue point to, when Q is on. */
  quantiseTo: BeatGrid | null = null,
): CueAction {
  if (playing) return { seekTo: cuePoint, playing: false, cuePoint };
  if (Math.abs(position - cuePoint) <= CUE_TOLERANCE) {
    return { seekTo: null, playing: true, cuePoint };
  }
  const at = Math.max(position, 0);
  // With Q on the new cue lands on the nearest beat, and the playhead goes
  // there with it — a cue point the head is not standing on would immediately
  // read as "paused somewhere else" and the next press would move it again.
  const set = quantiseTo ? Math.max(nearestBeatMs(quantiseTo, at * 1000) / 1000, 0) : at;
  return { seekTo: set === at ? null : set, playing: false, cuePoint: set };
}

/**
 * Letting CUE go.
 *
 * Only a held preview does anything: the button was pressed on the cue point
 * and audio has been running since, so releasing it stops and rewinds. A
 * release that follows any other press is not a rewind — that would undo the
 * jump the press just made.
 */
export function releaseCue(previewing: boolean, cuePoint: number): CueAction | null {
  if (!previewing) return null;
  return { seekTo: cuePoint, playing: false, cuePoint };
}

/**
 * How far the drawn layer must slide, in pixels, to keep the head centred.
 *
 * The layer covers `OVERDRAW` spans centred on `anchor`, laid out so that with
 * no transform its middle is the middle of the strip. Sliding it by this puts
 * `progress` there instead.
 */
export function scrollOffset(
  progress: number,
  anchor: number,
  span: number,
  width: number,
): number {
  if (span <= 0 || width <= 0) return 0;
  return -((progress - anchor) / span) * width;
}

/**
 * Whether the playhead has run far enough that the layer must be redrawn.
 *
 * The layer reaches half its width either side of the anchor, and the strip
 * shows half a span either side of the head, so the hard limit is half a span.
 * Redrawing at half of that leaves a margin: a redraw that lands exactly as
 * the edge arrives is a redraw that sometimes arrives late.
 */
export function needsRedraw(progress: number, anchor: number, span: number): boolean {
  if (span <= 0) return false;
  return Math.abs(progress - anchor) > (span * (OVERDRAW - 1)) / 4;
}

/** Where a window into the waveform bytes lands on the canvas. */
export interface WaveSlice {
  /** Byte offsets into the tag, so the window is a slice rather than a fetch. */
  first: number;
  last: number;
  /** Where that slice starts on the canvas, in whole device pixels. */
  x0: number;
  /** How wide it is there, in whole device pixels. */
  width: number;
}

/**
 * The slice of a waveform tag a window shows, and where it sits on the canvas.
 *
 * `x0` and `width` are whole device pixels on purpose. The bars are one pixel
 * wide, and translating the canvas by a fraction splits every one of them
 * across two columns at partial alpha — measured, an offset of 285.6 left not
 * one pure pixel in the strip, which over black reads as the waveform going
 * pale. Position the slice by the timestamps of the columns actually read,
 * including the partial columns outside the requested window. Stretching a
 * rounded slice to the unrounded window moves transients relative to the grid
 * by up to a column as the window changes. Only the final pixel edges round.
 */
export function waveSlice(
  progress: number,
  span: number,
  bytes: number,
  canvasWidth: number,
  /** Bytes per column of the tag: three for the bands, one or two or six for the others. */
  stride = 3,
): WaveSlice {
  const reach = Math.max(span, 0) / 2;
  const from = progress - reach;
  const to = progress + reach;
  const width_ = Math.max(to - from, 1e-9);
  const columns = Math.floor(bytes / stride);
  if (columns === 0) return { first: 0, last: 0, x0: 0, width: 0 };
  const shownFrom = Math.min(Math.max(from, 0), 1);
  const shownTo = Math.min(Math.max(to, 0), 1);
  const first = Math.floor(shownFrom * columns);
  const last = Math.ceil(shownTo * columns);
  const x0 = Math.round(((first / columns - from) / width_) * canvasWidth);
  const x1 = Math.round(((last / columns - from) / width_) * canvasWidth);
  return {
    first: first * stride,
    last: last * stride,
    x0,
    width: x1 - x0,
  };
}

/**
 * The number beside the playhead: View › Display Type › Beat Count Display.
 *
 * `position` is bars and beats from the start, `12.3` being the third beat
 * of the twelfth bar, so the figure after the point only ever reads 1 to 4.
 * With analysis, its boundaries come from the same grid as the beat lines;
 * elapsed time times BPM is only a fallback while no grid is available.
 * The other two count down to the next memory cue at or after the
 * playhead, in bars and beats or in whole beats, the way a CDJ's count-down
 * does; with no cue ahead there is nothing to count, and nothing is shown.
 * Four beats to the bar, which is what the grid gives.
 */
export function beatCountText(
  seconds: number,
  bpm: number,
  mode: "position" | "toMemoryBars" | "toMemoryBeats",
  /** Memory cue positions in seconds, in any order. */
  memorySeconds: readonly number[],
  /** The same timestamped grid used to draw the beat lines. */
  grid: BeatGrid = NO_BEATS,
): string {
  if (!Number.isFinite(seconds)) return "";
  if (mode === "position" && grid.times.length > 0) {
    const ms = seconds * 1000;
    const next = lowerBound(grid.times, ms);
    // A beat becomes current at its timestamp, never when it is merely the
    // nearest beat. The grid may start after zero or change tempo mid-track.
    const index = grid.times[next] === ms ? next : next - 1;
    const firstNumber = grid.numbers[0] || 1;
    const ordinal = index + firstNumber - 1;
    const bar = Math.floor(ordinal / 4);
    const number = index >= 0 ? (grid.numbers[index] || 1) : ((ordinal % 4 + 4) % 4) + 1;
    // The beat just before 1.1 is -1.4; there is no bar zero.
    return `${bar < 0 ? bar : bar + 1}.${number} Bars`;
  }
  if (!(bpm > 0)) return "";
  const beatsPerSecond = bpm / 60;
  if (mode === "position") {
    const elapsed = Math.max(0, Math.floor(seconds * beatsPerSecond + 1e-9));
    return `${Math.floor(elapsed / 4) + 1}.${(elapsed % 4) + 1} Bars`;
  }
  let next = Number.POSITIVE_INFINITY;
  for (const at of memorySeconds) {
    if (at >= seconds && at < next) next = at;
  }
  if (!Number.isFinite(next)) return "";
  const beats = (next - seconds) * beatsPerSecond;
  // Whole beats left, rounded up, then split into bars and beats.
  const whole = Math.ceil(beats - 1e-9);
  return mode === "toMemoryBars" ? `-${Math.floor(whole / 4)}.${whole % 4} Bars` : `-${whole}Beats`;
}

/**
 * Where a click on the enlarged waveform lands, in seconds: the playhead is
 * at the middle, and the music at `x` is as far from it as the window's
 * span puts it. `dragSeconds` is the same rate the other way round — a drag
 * moves the record, a click moves the head.
 */
export function clickSeconds(
  x: number,
  width: number,
  positionSec: number,
  span: number,
  durationSec: number,
): number {
  const head = (headPercent() / 100) * width;
  const target = positionSec - dragSeconds(x - head, width, span, durationSec);
  return Math.min(Math.max(target, 0), Math.max(durationSec, 0));
}

/**
 * Whether a press and release on the waveform was a click rather than a
 * drag: the pointer stayed within a few pixels. A drag that went nowhere is
 * a click too, which is what a person who pressed and let go meant.
 */
export const CLICK_SLOP_PX = 3;
export function isClick(dx: number, dy: number): boolean {
  return Math.abs(dx) <= CLICK_SLOP_PX && Math.abs(dy) <= CLICK_SLOP_PX;
}

/**
 * The tempo slider's ranges, as a CDJ offers them: ±6, ±10, ±20 and WIDE.
 * WIDE is everything the engine can play — half speed to double — so its
 * two halves are not alike: the fader's lower half spans −50 % and its upper
 * +100 %, with the file's own speed still at the middle.
 */
export type TempoRange = 6 | 10 | 16 | "wide";
export const TEMPO_RANGES: readonly TempoRange[] = [6, 10, 16, "wide"];

/** What a range's ends are, as a percentage either side of the file's speed. */
export function tempoRangeEnds(range: TempoRange): { down: number; up: number } {
  return range === "wide" ? { down: 50, up: 100 } : { down: range, up: range };
}

/**
 * The fader's position for a tempo, −1 at the slow end to +1 at the fast
 * end, 0 at the file's own speed. Past the end is the end.
 */
export function tempoToFader(tempo: number, range: TempoRange): number {
  const { down, up } = tempoRangeEnds(range);
  const pct = (tempo - 1) * 100;
  const at = pct >= 0 ? pct / up : pct / down;
  return Math.min(Math.max(at, -1), 1);
}

/** The tempo at a fader position, the inverse of `tempoToFader`. */
export function faderToTempo(at: number, range: TempoRange): number {
  const { down, up } = tempoRangeEnds(range);
  const clamped = Math.min(Math.max(at, -1), 1);
  const pct = clamped >= 0 ? clamped * up : clamped * down;
  return 1 + pct / 100;
}

/**
 * The tempo for a BPM somebody typed, against the track's own: 130 typed
 * over a 128 BPM track is 1.5625 % up. Nothing to type against — no grid,
 * or a number that is not one — leaves the tempo alone.
 */
export function tempoForTypedBpm(typed: string, trackBpmX100: number): number | null {
  const bpm = Number.parseFloat(typed.trim().replace(",", "."));
  if (!(bpm > 0) || !(trackBpmX100 > 0)) return null;
  const tempo = bpm / (trackBpmX100 / 100);
  return Math.min(Math.max(tempo, 0.5), 2);
}

/**
 * A beat loop of `beats` beats from `atMs`: the in point snapped to
 * `snapTo` when quantize is on, the out point `beats` beats later on the
 * track's own grid. Past the grid's end the average beat carries on. Null
 * with no grid to count on, or a length that is not positive.
 */
export function beatLoopRange(
  grid: BeatGrid,
  snapTo: BeatGrid | null,
  atMs: number,
  beats: number,
): [number, number] | null {
  const { times } = grid;
  if (times.length < 2 || !(beats > 0)) return null;
  const start = snapTo ? nearestBeatMs(snapTo, atMs) : atMs;
  const first = times[0] ?? 0;
  const last = times[times.length - 1] ?? 0;
  const period = (last - first) / (times.length - 1);
  if (!(period > 0)) return null;
  const at = lowerBound(times, start);
  const onBeat = times[at] === start;
  const target = at + beats;
  const end = onBeat && Number.isInteger(beats) && target < times.length ? (times[target] ?? start) : start + beats * period;
  return end > start ? [start, end] : null;
}

/**
 * The beat the head is on, 1-based on the grid as `PQTZ` numbers them: the
 * last beat at or before `ms`, or the first when the head is before it.
 */
export function beatAtMs(grid: BeatGrid, ms: number): number {
  const { times } = grid;
  if (times.length === 0) return 1;
  const at = lowerBound(times, ms);
  const exact = times[at] === ms;
  return Math.max(1, exact ? at + 1 : at);
}

export interface TempoAnnotation {
  fromMs: number;
  toMs: number;
  fromBpmX100: number;
  toBpmX100: number;
}

/** Presentation only: collapse successive short tempo runs into one ramp.
 * Four equal-tempo beats establish a settled section. The actual grid is
 * untouched, including the individually measured intervals inside a ramp.
 */
export function tempoAnnotations(grid: BeatGrid): TempoAnnotation[] {
  const runs: { start: number; end: number; bpm: number }[] = [];
  for (let i = 0; i < grid.times.length; i++) {
    const bpm = grid.tempos[i] ?? 0;
    if (bpm <= 0) continue;
    const last = runs.at(-1);
    if (last?.bpm === bpm) last.end = i + 1;
    else runs.push({ start: i, end: i + 1, bpm });
  }
  const first = runs[0];
  if (!first) return [];
  const at = (i: number) => grid.times[i] ?? 0;
  const out: TempoAnnotation[] = [{ fromMs: at(first.start), toMs: at(first.start), fromBpmX100: first.bpm, toBpmX100: first.bpm }];
  for (let i = 1; i < runs.length; i++) {
    const start = runs[i];
    if (!start) continue;
    let end = i;
    while (end < runs.length - 1) {
      const run = runs[end];
      if (!run || run.end - run.start >= 4) break;
      end++;
    }
    const finish = runs[end] ?? start;
    out.push({ fromMs: at(start.start), toMs: at(finish.start),
      fromBpmX100: end > i ? (runs[i - 1]?.bpm ?? start.bpm) : start.bpm,
      toBpmX100: finish.bpm });
    i = end;
  }
  return out;
}
