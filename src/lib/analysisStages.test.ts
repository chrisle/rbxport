import { expect, it } from "vitest";
import { analysisDependencyError, BUILTIN_ANALYSIS_STAGES, selectedStageOrder } from "./analysisStages";
const extension = { id: "example.spectrum", label: "Spectrum", description: "", requires: ["core.beat_grid"], produces: ["example.spectrum"], defaultEnabled: false, requiresFullTrack: true, revision: "1" };
const stages = [...BUILTIN_ANALYSIS_STAGES, extension];
const settings = { bpmGrid: true, key: true, highPrecision: true, minBpm: 70, maxBpm: 180, firstBeatCue: false };
it("preserves the user's order and appends a newly selected extension", () => {
  expect(selectedStageOrder({ ...settings, stageOrder: ["key", "grid", "waveform"], extensionStages: [extension.id] }, stages)).toEqual(["key", "grid", "waveform", extension.id]);
});
it("reports prerequisites without silently changing the order", () => {
  const order = [extension.id, "grid", "key", "waveform"];
  expect(analysisDependencyError(order, stages)).toEqual({ stage: "Spectrum", requirement: "BPM / Grid" });
  expect(order[0]).toBe(extension.id);
  expect(analysisDependencyError(["key", "grid", extension.id], stages)).toBeNull();
});
it("removes deselected built-ins and accepts an audio-only extension", () => {
  expect(selectedStageOrder({ ...settings, bpmGrid: false }, stages)).toEqual(["key"]);
  expect(analysisDependencyError(["key"], stages)).toBeNull();
});
