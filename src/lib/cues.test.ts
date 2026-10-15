import { describe, expect, it } from "vitest";

import type { Cue } from "@/ipc/types";
import {
  MEMORY_TOLERANCE_MS, hotCue, hotLetters, memoryCueAt, memoryCueNumber, nextMemoryCue,
  previousMemoryCue, rowCuesOf,
} from "./cues";

const memory = (id: string, positionMs: number): Cue => ({
  id, positionMs, outMs: 0, letter: "", memory: true, colour: null,
});
const hot = (id: string, letter: string, positionMs: number, colour: string | null = null): Cue => ({
  id, positionMs, outMs: 0, letter, memory: false, colour,
});

// Out of order on purpose: the list the backend hands over is sorted, but
// nothing here should depend on it.
const cues: Cue[] = [
  memory("m3", 90_000),
  hot("a", "A", 5_000),
  memory("m1", 1_000),
  memory("m2", 30_000),
  hot("b", "B", 60_000),
];

describe("nextMemoryCue", () => {
  it("is the first memory cue after the playhead, skipping hot cues", () => {
    expect(nextMemoryCue(cues, 0)?.id).toBe("m1");
    expect(nextMemoryCue(cues, 4_000)?.id).toBe("m2");
    expect(nextMemoryCue(cues, 59_000)?.id).toBe("m3");
  });

  it("moves on from the cue the playhead is standing on", () => {
    expect(nextMemoryCue(cues, 30_000)?.id).toBe("m3");
    expect(nextMemoryCue(cues, 30_000 + MEMORY_TOLERANCE_MS)?.id).toBe("m3");
    expect(nextMemoryCue(cues, 30_000 - MEMORY_TOLERANCE_MS - 1)?.id).toBe("m2");
  });

  it("is null past the last one, and with no memory cues at all", () => {
    expect(nextMemoryCue(cues, 90_000)).toBeNull();
    expect(nextMemoryCue([hot("a", "A", 1)], 0)).toBeNull();
    expect(nextMemoryCue([], 0)).toBeNull();
  });
});

describe("previousMemoryCue", () => {
  it("is the last memory cue before the playhead", () => {
    expect(previousMemoryCue(cues, 100_000)?.id).toBe("m3");
    expect(previousMemoryCue(cues, 60_000)?.id).toBe("m2");
    expect(previousMemoryCue(cues, 2_000)?.id).toBe("m1");
  });

  it("moves back from the cue the playhead is standing on", () => {
    expect(previousMemoryCue(cues, 30_000)?.id).toBe("m1");
    expect(previousMemoryCue(cues, 30_000 - MEMORY_TOLERANCE_MS)?.id).toBe("m1");
    expect(previousMemoryCue(cues, 30_000 + MEMORY_TOLERANCE_MS + 1)?.id).toBe("m2");
  });

  it("is null before the first one", () => {
    expect(previousMemoryCue(cues, 1_000)).toBeNull();
    expect(previousMemoryCue(cues, 0)).toBeNull();
    expect(previousMemoryCue([], 50_000)).toBeNull();
  });
});

describe("memoryCueAt", () => {
  it("is the memory cue within the tolerance, and nothing further", () => {
    expect(memoryCueAt(cues, 30_000)?.id).toBe("m2");
    expect(memoryCueAt(cues, 30_000 + MEMORY_TOLERANCE_MS)?.id).toBe("m2");
    expect(memoryCueAt(cues, 30_000 + MEMORY_TOLERANCE_MS + 1)).toBeNull();
    expect(memoryCueAt(cues, 45_000)).toBeNull();
  });

  it("never picks a hot cue", () => {
    expect(memoryCueAt(cues, 5_000)).toBeNull();
    expect(memoryCueAt(cues, 60_000)).toBeNull();
  });

  it("takes the nearer of two within reach", () => {
    const close = [memory("x", 1_000), memory("y", 1_010)];
    expect(memoryCueAt(close, 1_008)?.id).toBe("y");
    expect(memoryCueAt(close, 1_003)?.id).toBe("x");
  });
});

describe("hotCue", () => {
  it("is the cue in the slot, or null for an empty pad", () => {
    expect(hotCue(cues, "A")?.id).toBe("a");
    expect(hotCue(cues, "B")?.id).toBe("b");
    expect(hotCue(cues, "C")).toBeNull();
    expect(hotCue([], "A")).toBeNull();
  });

  it("never reads a memory cue as a slot", () => {
    expect(hotCue([memory("m", 1)], "")).toBeNull();
  });

  it("takes the earlier of two rows in one slot", () => {
    const doubled = [hot("late", "A", 9_000), hot("early", "A", 2_000)];
    expect(hotCue(doubled, "A")?.id).toBe("early");
  });
});

describe("hotLetters", () => {
  it("lists the letters in letter order, whatever order the cues sit in", () => {
    expect(hotLetters(cues)).toBe("AB");
    expect(hotLetters([hot("d", "D", 1), memory("m", 2), hot("a", "A", 3)])).toBe("AD");
  });

  it("is empty with no hot cues, and lists a doubled slot once", () => {
    expect(hotLetters([])).toBe("");
    expect(hotLetters([memory("m", 1)])).toBe("");
    expect(hotLetters([hot("x", "A", 1), hot("y", "A", 2)])).toBe("A");
  });
});

describe("memoryCueNumber", () => {
  it("counts memory cues from the start of the track, one-based", () => {
    expect(memoryCueNumber(cues, 1)?.id).toBe("m1");
    expect(memoryCueNumber(cues, 2)?.id).toBe("m2");
    expect(memoryCueNumber(cues, 3)?.id).toBe("m3");
  });

  it("counts by position, not by the order the list arrived in", () => {
    // `cues` is deliberately unsorted, and the hot cues at 5 s and 60 s sit
    // between the memory cues: neither may shift the numbering.
    const shuffled = [memory("c", 3_000), memory("a", 1_000), memory("b", 2_000)];
    expect([1, 2, 3].map((n) => memoryCueNumber(shuffled, n)?.id)).toEqual(["a", "b", "c"]);
  });

  it("is null off either end, which is what a pad past the last cue presses", () => {
    expect(memoryCueNumber(cues, 4)).toBeNull();
    expect(memoryCueNumber(cues, 0)).toBeNull();
    expect(memoryCueNumber(cues, -1)).toBeNull();
    expect(memoryCueNumber([], 1)).toBeNull();
    expect(memoryCueNumber([hot("a", "A", 1_000)], 1)).toBeNull();
  });
});

describe("rowCuesOf", () => {
  it("is one entry per slot in letter order, carrying the colour", () => {
    const coloured = [
      hot("b", "B", 60_000, "#00f"),
      memory("m", 30_000),
      hot("a", "A", 5_000, "#f00"),
    ];
    expect(rowCuesOf(coloured)).toEqual([
      ["A", 5_000, "#f00", false],
      ["B", 60_000, "#00f", false],
    ]);
  });

  it("leaves out memory cues and any row with no slot", () => {
    expect(rowCuesOf([memory("m", 1_000)])).toEqual([]);
    expect(rowCuesOf([hot("blank", "", 1_000)])).toEqual([]);
    expect(rowCuesOf([])).toEqual([]);
  });

  it("marks a hot loop, which an uncoloured badge draws in the loop default", () => {
    const loop = { ...hot("l", "C", 4_000), outMs: 6_000 };
    expect(rowCuesOf([loop])[0]?.[3]).toBe(true);
  });

  it("agrees with hotLetters, which is built from it", () => {
    expect(rowCuesOf(cues).map(([letter]) => letter).join("")).toBe(hotLetters(cues));
  });

  it("names a doubled slot once, taking the row the backend would list first", () => {
    // The backend hands the list back in position order, so the first row
    // for a slot is also the earliest — the same one `hotCue` gives the pad.
    const sorted = [hot("early", "A", 2_000, "#f00"), hot("late", "A", 9_000, "#0f0")];
    expect(rowCuesOf(sorted)).toEqual([["A", 2_000, "#f00", false]]);
    expect(rowCuesOf(sorted)[0]?.[1]).toBe(hotCue(sorted, "A")?.positionMs);
  });
});
