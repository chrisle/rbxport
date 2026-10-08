/** Grid arithmetic mirrored from rbl-anlz; see the Ghidra pre-release audit. */
import type { GridEdit } from "@/ipc/types";
import type { BeatGrid } from "./player";
export interface EditableBeat { number: number; tempoX100: number; timeMs: number }
export const SHIFT_MS = 1;
export const HELD_SHIFT_MS = 10;
export const TAP_GAP_MS = 1500;
export const MIN_BPM_X100 = 4000;
export const MAX_BPM_X100 = 49900;
const MAX_BEATS = 1 << 20;

export function tapTempo(taps: readonly number[]): number | null {
  if (taps.length < 2) return null;
  const interval = ((taps.at(-1) ?? 0) - (taps[0] ?? 0)) / (taps.length - 1);
  return interval > 120 && interval <= 1500 ? roundEven(6_000_000 / interval) : null;
}
export function tapTimeout(taps: readonly number[]): number {
  if (taps.length < 2) return TAP_GAP_MS;
  return Math.min(TAP_GAP_MS, Math.floor(1.5 * ((taps.at(-1) ?? 0) - (taps[0] ?? 0)) / (taps.length - 1)));
}
/** TapButton::clicked: cumulative mean, ±16% outlier gate, no rolling window. */
export function withTap(taps: readonly number[], now: number): number[] {
  const last = taps.at(-1);
  if (last === undefined || now < last || now - last >= tapTimeout(taps)) return [now];
  const interval = now - last;
  if (taps.length === 1) return interval > 120 && interval <= 1500 ? [...taps, now] : [];
  const mean = (last - (taps[0] ?? last)) / (taps.length - 1);
  const checked = Math.max(120, Math.min(1500, interval));
  return checked >= mean * 0.84 && checked <= mean * 1.16 ? [...taps, now] : [];
}
export function roundEven(value: number): number {
  const low = Math.floor(value);
  return value - low === 0.5 ? low + (low % 2) : Math.round(value);
}
const number = (offset: number) => ((offset % 4) + 4) % 4 + 1;
const clone = (beats: readonly EditableBeat[]) => beats.map(b => ({ ...b }));
function nearest(beats: readonly EditableBeat[], time: number): number {
  let best = 0;
  beats.forEach((b, i) => { if (Math.abs(b.timeMs - time) < Math.abs((beats[best]?.timeMs ?? 0) - time)) best = i; });
  return best;
}
function renumber(beats: EditableBeat[], at: number, value: number) {
  beats.forEach((b, i) => { b.number = number(i - at + value - 1); });
}
export function tempoX100(beats: readonly EditableBeat[]): number { return beats[0]?.tempoX100 ?? 0; }
export function isDynamicFrom(beats: readonly EditableBeat[], fromMs: number | null): boolean {
  const tail = beats.slice(fromMs === null ? 0 : nearest(beats, fromMs));
  return tail.some(b => b.tempoX100 !== tail[0]?.tempoX100);
}
function stretchInterval(beats: readonly EditableBeat[], start: number, time: number, by: number): number | null {
  const first = beats[start];
  if (!first) return null;
  let target = nearest(beats, time);
  if (target === start && time >= first.timeMs) target++;
  const beat = beats[target];
  return target > start && beat ? (beat.timeMs + Math.max(-120, Math.min(120, by)) - first.timeMs) / (target - start) : null;
}
export function validateEdit(beats: readonly EditableBeat[], fromMs: number | null, edit: GridEdit): string | null {
  const start = fromMs === null ? 0 : nearest(beats, fromMs);
  const tail = beats.slice(start);
  let valid = true;
  switch (edit.kind) {
    case "double": valid = tail.every(b => b.tempoX100 >= 2000 && b.tempoX100 <= 24950); break;
    case "halve": valid = tail.every(b => b.tempoX100 >= 8000); break;
    case "tempo": valid = edit.bpmX100 >= 4000 && edit.bpmX100 <= 49900; break;
    case "tap": valid = edit.bpm >= 40 && edit.bpm < 500; break;
    case "stretch": {
      const interval = stretchInterval(beats, start, edit.timeMs, edit.byMs);
      valid = interval === null || (interval >= 60_000 / 499 && interval <= 1500);
    }
  }
  return valid ? null : "The beat-grid tempo must be between 40 and 499 BPM.";
}
function fit(beats: EditableBeat[], end: number): EditableBeat[] {
  const out = beats.filter(b => b.timeMs >= 0 && b.timeMs <= end);
  if (out.length === 0) return out;
  if (out.length === 1) {
    const first = out[0]!;
    if (first.timeMs === 0 || first.timeMs * 2 > end) return out;
    while (out.length < MAX_BEATS && out.at(-1)!.timeMs + first.timeMs <= end) {
      const last = out.at(-1)!;
      out.push({...last, timeMs: last.timeMs + first.timeMs, number: number(last.number)});
    }
    return out;
  }
  const first = out[0]!;
  const interval = out[1]!.timeMs - first.timeMs;
  if (interval > 0 && first.timeMs >= interval) out.unshift({ ...first, timeMs: first.timeMs - interval, number: number(first.number - 2) });
  while (out.length < MAX_BEATS) {
    const last = out.at(-1)!;
    const interval = last.timeMs - out.at(-2)!.timeMs;
    if (interval <= 0 || last.timeMs + interval > end) break;
    out.push({ ...last, timeMs: last.timeMs + interval, number: number(last.number) });
  }
  return out;
}
function move(beats: readonly EditableBeat[], start: number, by: number, end: number) {
  return fit(beats.map((b, i) => ({ ...b, timeMs: b.timeMs + (i >= start ? by : 0) })), end);
}
function respace(beats: readonly EditableBeat[], start: number, interval: number, bpm: number, end: number) {
  return fit(beats.map((b, i) => i < start ? { ...b } : {
    ...b, tempoX100: bpm, timeMs: beats[start]!.timeMs + roundEven((i - start) * interval),
  }), end);
}
export function applyEdit(beats: readonly EditableBeat[], edit: GridEdit): EditableBeat[] {
  return applyEditFrom(beats, null, edit);
}
export function applyEditFrom(beats: readonly EditableBeat[], fromMs: number | null, edit: GridEdit,
  endMs = beats.at(-1)?.timeMs ?? 0): EditableBeat[] {
  if (!beats.length || validateEdit(beats, fromMs, edit)) return clone(beats);
  const start = fromMs === null ? 0 : nearest(beats, fromMs);
  switch (edit.kind) {
    case "nudge": return move(beats, start, edit.ms, endMs);
    case "align":
    case "downbeat": {
      const at = nearest(beats, edit.timeMs);
      const out = move(beats, start, edit.timeMs - beats[at]!.timeMs, endMs);
      if (edit.kind === "downbeat") renumber(out, nearest(out, edit.timeMs), 1);
      return out;
    }
    case "stretch": {
      const interval = stretchInterval(beats, start, edit.timeMs, edit.byMs);
      return interval === null ? clone(beats) : respace(beats, start, interval, roundEven(6_000_000 / interval), endMs);
    }
    case "tempo": return respace(beats, start, 6_000_000 / edit.bpmX100, edit.bpmX100, endMs);
    case "tap": {
      const out = respace(beats, start, 60_000 / edit.bpm, roundEven(edit.bpm * 100), endMs);
      return applyEditFrom(out, fromMs, { kind: "downbeat", timeMs: edit.anchorMs }, endMs);
    }
    case "double": {
      const out = clone(beats.slice(0, start));
      beats.forEach((b, i) => {
        if (i < start) return;
        const tempoX100 = b.tempoX100 * 2;
        out.push({ ...b, tempoX100 });
        const interval = beats[i + 1] ? beats[i + 1]!.timeMs - b.timeMs : i > start ? b.timeMs - beats[i - 1]!.timeMs : endMs - b.timeMs;
        if (interval > 1) out.push({ ...b, tempoX100, timeMs: b.timeMs + Math.floor(interval / 2) });
      });
      renumber(out, start, beats[start]!.number % 2 === 1 ? 1 : 3);
      return fit(out, endMs);
    }
    case "halve": {
      const out = clone(beats.slice(0, start));
      const original = beats[start]!.number;
      const skip = original % 2 === 0 ? 1 : 0;
      out.push(...beats.slice(start + skip).filter((_, i) => i % 2 === 0).map(b => ({ ...b, tempoX100: Math.floor(b.tempoX100 / 2) })));
      renumber(out, start, original === 1 || original === 4 ? 1 : 4);
      return fit(out, endMs);
    }
  }
}
/**
 * The grid shifted by `ms`, as the saved nudge will make it. The deck plays
 * this grid while the save runs, so the shift sounds on the press.
 */
export function nudgeGrid(grid: BeatGrid, ms: number, endMs: number): BeatGrid {
  const beats = Array.from(grid.times, (timeMs, i) => ({ timeMs, number: grid.numbers[i] ?? 1, tempoX100: grid.tempos[i] ?? 0 }));
  const out = applyEditFrom(beats, null, { kind: "nudge", ms }, endMs);
  return {
    times: Uint32Array.from(out, b => b.timeMs),
    numbers: Uint8Array.from(out, b => b.number),
    tempos: Uint16Array.from(out, b => b.tempoX100),
  };
}
