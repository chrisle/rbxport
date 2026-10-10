import { describe, expect, it } from "vitest";
import { applyEditFrom, nudgeGrid, tapTempo, tapTimeout, withTap, roundEven, validateEdit, type EditableBeat } from "./gridEdit";
const grid = (): EditableBeat[] => Array.from({length: 9}, (_, i) => ({timeMs: i * 500, number: i % 4 + 1, tempoX100: 12000}));
const times = (beats: EditableBeat[]) => beats.map(b => b.timeMs);
describe("Ghidra-derived beat-grid vectors", () => {
  it("nudges a deck's grid as the saved edit will", () => {
    const beats = grid();
    const deck = { times: Uint32Array.from(times(beats)), numbers: Uint8Array.from(beats, b => b.number), tempos: Uint16Array.from(beats, b => b.tempoX100) };
    const saved = applyEditFrom(beats, null, {kind: "nudge", ms: 7}, 4500);
    const out = nudgeGrid(deck, 7, 4500);
    expect(Array.from(out.times)).toEqual(times(saved));
    expect(Array.from(out.numbers)).toEqual(saved.map(b => b.number));
  });
  it("keeps time zero and extends to the supplied duration", () => {
    expect(times(applyEditFrom(grid(), null, {kind: "nudge", ms: -1}, 4500))).toEqual([499,999,1499,1999,2499,2999,3499,3999,4499]);
  });
  it("aligns and renumbers the selected downbeat", () => {
    const out = applyEditFrom(grid(), null, {kind: "downbeat", timeMs: 1600}, 4000);
    expect(out.find(b => b.timeMs === 1600)?.number).toBe(1);
    expect(times(out)).toEqual([100,600,1100,1600,2100,2600,3100,3600]);
  });
  it("saves the presses of a held stretch as one stretch with the same grid (#196)", () => {
    // A grid over the whole track, as analysis leaves it: 120 BPM for 200 s.
    const track = Array.from({length: 400}, (_, i) => ({timeMs: i * 500, number: i % 4 + 1, tempoX100: 12000}));
    for (const [fromMs, steps] of [[null, [1, 10, 10, 10, 10, 10]], [null, [-1, -10, -10, -10, -10]], [8000, [1, 10, 10, -10, 10]]] as const) {
      const oneByOne = steps.reduce((beats, byMs) => applyEditFrom(beats, fromMs, {kind: "stretch", byMs, timeMs: 32_000}, 200_000), track);
      const batched = applyEditFrom(track, fromMs, {kind: "stretch", byMs: steps.reduce((a: number, b) => a + b, 0), timeMs: 32_000}, 200_000);
      expect(batched.length).toBe(oneByOne.length);
      expect(batched.map(b => b.tempoX100)).toEqual(oneByOne.map(b => b.tempoX100));
      // Every beat the stretch placed lands where the presses one by one put
      // it; the few beats the narrower grid frees at the end are extended at
      // the whole-millisecond interval, from a different last beat, so they
      // may be a millisecond apart.
      batched.forEach((beat, i) => expect(Math.abs(beat.timeMs - oneByOne[i]!.timeMs)).toBeLessThanOrEqual(i < track.length ? 0 : 1));
    }
  });
  it("uses target distance for stretch, including ties-to-even rounding", () => {
    const out = applyEditFrom(grid(), null, {kind: "stretch", byMs: 1, timeMs: 1000}, 4000);
    expect(times(out).slice(0,5)).toEqual([0,500,1001,1502,2002]);
    expect(out[0]?.tempoX100).toBe(11988);
    expect(roundEven(2.5)).toBe(2); expect(roundEven(3.5)).toBe(4);
  });
  it("holds the first affected beat when typing BPM", () => {
    expect(times(applyEditFrom(grid(), 2000, {kind: "tempo", bpmX100: 10000, anchorMs: 999}, 4000)))
      .toEqual([0,500,1000,1500,2000,2600,3200,3800]);
  });
  it("adds the final doubled midpoint and keeps halve phase", () => {
    expect(times(applyEditFrom(grid(), null, {kind: "double"}, 4250))).toEqual(Array.from({length:18},(_,i)=>i*250));
    const shifted = grid().map(b=>({...b, number: b.number % 4 + 1}));
    const half = applyEditFrom(shifted, null, {kind: "halve"}, 4000);
    expect(times(half)).toEqual([500,1500,2500,3500]);
    expect(half[0]?.number).toBe(4);
  });
  it("rejects out-of-range results instead of saturating", () => {
    expect(validateEdit(grid().map(b=>({...b,tempoX100:25000})), null, {kind:"double"})).not.toBeNull();
    expect(validateEdit(grid(), null, {kind:"tempo",bpmX100:3999,anchorMs:0})).not.toBeNull();
    expect(applyEditFrom([],null,{kind:"double"},4000)).toEqual([]);
  });
});
describe("TapButton",()=>{
 it("uses the full run and an adaptive timeout",()=>{
  expect(tapTempo([0,500,1020])).toBe(11765);
  expect(tapTimeout([0,500])).toBe(750);
  expect(tapTempo([0])).toBeNull();
 });
 it("rejects outliers and starts again after timeout",()=>{
  expect(withTap([0,500],1100)).toEqual([]);
  expect(withTap([0,500],1300)).toEqual([1300]);
  expect(withTap([0],100)).toEqual([]);
  expect(withTap([0,500],1010)).toEqual([0,500,1010]);
 });
 // The tempo rekordbox 7.2.19 hands BeatGridAdjustment::tapTap after each
 // tap (TapButton::clicked @0x1010b6378 / calcBpm @0x1010b64d4, arm64), and
 // the timeout it restarts; 0 / null where it sends no tempo (#137).
 it.each([
  ["steady", [0,500,1000,1500,2000], [0,120,120,120,120], [1500,750,750,750,750]],
  ["uneven", [0,469,937,1406,1875,2343], [0,127.931770,128.068303,128.022760,128,128.040973], [1500,703,702,703,703,702]],
  ["drift inside the ±16% gate", [0,500,1020,1500,2010], [0,120,117.647059,120,119.402985], [1500,750,765,750,753]],
  ["outlier resets", [0,500,1100,1600,2100], [0,120,0,0,120], [1500,750,null,1500,750]],
  ["second tap too fast", [0,100,600,1100], [0,0,0,120], [1500,null,1500,750]],
  ["gap starts a new run", [0,500,1300,1800,2300], [0,120,0,120,120], [1500,750,1500,750,750]],
  ["slowest", [0,1490,2980,4470], [0,40.268456,40.268456,40.268456], [1500,1500,1500,1500]],
  ["mean at 120 ms sends none", [0,130,240,370], [0,461.538462,0,486.486486], [1500,195,180,185]],
 ])("matches rekordbox's TapButton: %s", (_name, times, bpms, timeouts) => {
  let taps: number[] = [];
  times.forEach((now, i) => {
   taps = withTap(taps, now);
   const bpm = tapTempo(taps) === null ? 0 : 60_000 * (taps.length - 1) / (taps.at(-1)! - taps[0]!);
   expect(bpm).toBeCloseTo(bpms[i]!, 5);
   expect(taps.length ? tapTimeout(taps) : null).toBe(timeouts[i]);
  });
 });
});
