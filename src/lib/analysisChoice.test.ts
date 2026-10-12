// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { lastAnalysisChoice, rememberAnalysisChoice } from "./analysisChoice";

describe("analysis choice memory", () => {
  beforeEach(() => localStorage.clear());

  it("starts empty and returns what was last confirmed", () => {
    expect(lastAnalysisChoice()).toEqual({});
    const choice = { mode: "rbxport", bpmGrid: false, key: true, highPrecision: false, minBpm: 98, maxBpm: 195, firstBeatCue: true } as const;
    rememberAnalysisChoice(choice);
    const { firstBeatCue: _preference, ...boxes } = choice;
    expect(lastAnalysisChoice()).toEqual(boxes);
  });

  it("neither keeps nor restores the first-beat cue, which is a preference (#343)", () => {
    rememberAnalysisChoice({ mode: "rekordbox", bpmGrid: true, key: true, highPrecision: true, minBpm: 70, maxBpm: 180, firstBeatCue: false });
    expect(JSON.parse(localStorage.getItem("rbxport.analysisChoice") ?? "{}")).not.toHaveProperty("firstBeatCue");
    // What 1.6.0 and earlier saved, the preference's value at that moment.
    localStorage.setItem("rbxport.analysisChoice", '{"mode":"rbxport","bpmGrid":true,"key":true,"highPrecision":true,"minBpm":70,"maxBpm":180,"firstBeatCue":false}');
    expect(lastAnalysisChoice()).not.toHaveProperty("firstBeatCue");
  });

  it("ignores a malformed entry", () => {
    localStorage.setItem("rbxport.analysisChoice", '{"bpmGrid":"yes","mode":"x"}');
    expect(lastAnalysisChoice()).toEqual({});
  });
});
