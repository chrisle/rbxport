/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useAnalysis, type Analysis } from "./useAnalysis";
import type { AnalysisPreferences } from "@/lib/preferences";
import type { AnalysisResult } from "@/ipc/types";

const held = vi.hoisted(() => ({ analyseTrack: vi.fn() }));
vi.mock("@/ipc/client", () => ({ getBackend: () => Promise.resolve(held) }));
let host: HTMLDivElement;
let root: Root;
let analysis: Analysis;
function Harness({ preferences }: { preferences: AnalysisPreferences }) {
  analysis = useAnalysis(undefined, undefined, preferences);
  return null;
}
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  held.analyseTrack.mockReset().mockImplementation(() => new Promise(() => {}));
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});

it("appends to a running batch, stops waiting work, and starts the next run fresh", async () => {
  const finish = new Map<string, (result: AnalysisResult) => void>();
  held.analyseTrack.mockImplementation((id: string) => new Promise<AnalysisResult>(resolve => { finish.set(id, resolve); }));
  await act(async () => { root.render(<Harness preferences={{ mode: "rbxport", concurrentTracks: 1, auto: true, firstBeatCue: false }} />); await Promise.resolve(); });
  await act(async () => { analysis.add([{ id: "a", title: "First" }, { id: "b", title: "Second" }]); await Promise.resolve(); });
  await act(async () => { analysis.add([{ id: "a", title: "First" }, { id: "c", title: "Added" }]); await Promise.resolve(); });
  expect(analysis.total).toBe(3);
  expect(analysis.state.pending.map(item => item.id)).toEqual(["b", "c"]);
  const result = (id: string): AnalysisResult => ({ trackId: id, bpmX100: 12800, key: "Am", beats: 100,
    peak: 1, durationSec: 60, elapsedMs: 10, analysisPath: "" });
  await act(async () => { finish.get("a")!(result("a")); await Promise.resolve(); });
  expect(held.analyseTrack.mock.calls.map(call => String(call[0]))).toEqual(["a", "b"]);
  await act(async () => { analysis.cancel(); await Promise.resolve(); });
  expect(analysis.state.pending).toEqual([]);
  await act(async () => { finish.get("b")!(result("b")); await Promise.resolve(); });
  expect(analysis.running).toBe(false);
  expect(held.analyseTrack.mock.calls.map(call => String(call[0]))).toEqual(["a", "b"]);
  await act(async () => { analysis.add([{ id: "c", title: "Added again" }]); await Promise.resolve(); });
  expect(analysis.total).toBe(1);
  expect(analysis.state.done).toBe(0);
  expect(held.analyseTrack.mock.calls.map(call => String(call[0]))).toEqual(["a", "b", "c"]);
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.unstubAllGlobals(); });

it("captures each batch's settings before preference changes or another batch", async () => {
  await act(async () => { root.render(<Harness preferences={{ mode: "rekordbox", concurrentTracks: 1, auto: true, firstBeatCue: false }} />); await Promise.resolve(); });
  await act(async () => { analysis.add([{ id: "a", title: "First" }, { id: "b", title: "Waiting" }]); await Promise.resolve(); });
  const defaults = { mode: "rekordbox", bpmGrid: true, key: true, highPrecision: true, minBpm: 70, maxBpm: 180, firstBeatCue: false };
  expect(held.analyseTrack).toHaveBeenCalledWith("a", "rekordbox", defaults);
  const chosen = { mode: "rbxport" as const, bpmGrid: false, key: true, highPrecision: false, minBpm: 98, maxBpm: 195, firstBeatCue: true };
  await act(async () => {
    root.render(<Harness preferences={{ mode: "rbxport", concurrentTracks: 1, auto: true, firstBeatCue: false }} />);
    analysis.add([{ id: "c", title: "Key only" }], chosen);
    await Promise.resolve();
  });
  chosen.key = false;
  expect(analysis.state.pending.map(item => item.analysis)).toEqual([defaults, { ...chosen, key: true }]);
  // Once more slots open, both waiting tracks use their own saved choices.
  await act(async () => { root.render(<Harness preferences={{ mode: "rbxport", concurrentTracks: 3, auto: true, firstBeatCue: false }} />); await Promise.resolve(); });
  expect(held.analyseTrack).toHaveBeenCalledWith("b", "rekordbox", defaults);
  expect(held.analyseTrack).toHaveBeenCalledWith("c", "rbxport", { ...chosen, key: true });
});

it("queues automatic batches with the first-beat cue preference captured", async () => {
  await act(async () => { root.render(<Harness preferences={{ mode: "rbxport", concurrentTracks: 1, auto: true, firstBeatCue: true }} />); await Promise.resolve(); });
  await act(async () => { analysis.add([{ id: "a", title: "Imported" }, { id: "b", title: "Waiting" }]); await Promise.resolve(); });
  expect(held.analyseTrack).toHaveBeenCalledWith("a", "rbxport", expect.objectContaining({ firstBeatCue: true }));
  await act(async () => { root.render(<Harness preferences={{ mode: "rbxport", concurrentTracks: 1, auto: true, firstBeatCue: false }} />); await Promise.resolve(); });
  expect(analysis.state.pending[0]?.analysis?.firstBeatCue).toBe(true);
});
