import { describe, expect, it, vi } from "vitest";

import { jumpSizeById, shiftByGridBeats, type BeatGrid } from "@/lib/player";
import { jumpHead } from "./beatJump";

describe("no-loop beat jump dispatch", () => {
  it("keeps a synced, quantized jump on the engine grid after a delayed tempo-change command", () => {
    const times = Uint32Array.from({ length: 45 }, (_, i) =>
      i <= 24 ? i * 500 : 12_000 + (i - 24) * 400);
    const grid: BeatGrid = {
      times,
      numbers: new Uint8Array(times.length),
      tempos: new Uint16Array(times.length),
    };
    const displayedHead = 10.75;
    let engineHead = 11.75; // Audio advanced while the displayed tick was delayed.
    const seekInPhase = vi.fn((seconds: number) => { engineHead = seconds; });
    const moveBy = vi.fn((seconds: number) => { engineHead += seconds; });
    const moveByGridBeats = vi.fn((gridTimes: number[], beats: number) => {
      const target = shiftByGridBeats(grid, engineHead * 1000, beats);
      if (target !== null) engineHead = target / 1000;
      expect(gridTimes).toEqual(Array.from(times));
    });

    jumpHead({ moveByGridBeats, moveBy }, grid, jumpSizeById("4beats"),
      12_000, displayedHead, 1, true, seekInPhase);

    expect(moveByGridBeats).toHaveBeenCalledOnce();
    expect(moveByGridBeats).toHaveBeenCalledWith(Array.from(times), 4);
    expect(seekInPhase).not.toHaveBeenCalled();
    expect(moveBy).not.toHaveBeenCalled();
    expect(engineHead).toBeCloseTo(13.4, 6);
    // 11.75 is halfway through a 500 ms beat; 13.4 is halfway through
    // the corresponding 400 ms beat. Beat Sync's fractional phase survives.
    expect((engineHead - 13.2) / 0.4).toBeCloseTo(0.5, 6);
  });

  it("keeps fine jumps on the phase-lock seek path", () => {
    const grid: BeatGrid = { times: new Uint32Array(), numbers: new Uint8Array(), tempos: new Uint16Array() };
    const moveByGridBeats = vi.fn();
    const moveBy = vi.fn();
    const seekInPhase = vi.fn();
    jumpHead({ moveByGridBeats, moveBy }, grid, jumpSizeById("fine"),
      12_000, 10.75, 1, true, seekInPhase);
    expect(seekInPhase).toHaveBeenCalledWith(10.76);
    expect(moveByGridBeats).not.toHaveBeenCalled();
  });
});
