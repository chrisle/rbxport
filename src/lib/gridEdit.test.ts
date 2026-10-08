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
});
