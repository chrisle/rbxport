// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMockBackend } from "@/ipc/backend-mock";
import type { Backend, ImportReport } from "@/ipc/types";
import { PREFERENCES_KEY } from "@/lib/preferences";
import { App } from "./App";

const held = vi.hoisted(() => ({
  backend: null as Backend | null,
  drop: null as ((target: string, files: File[]) => void) | null,
}));
vi.mock("@/ipc/client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/ipc/client")>();
  const { createMockBackend } = await import("@/ipc/backend-mock");
  held.backend = createMockBackend({ trackCount: 1 });
  return { ...original, getBackend: () => Promise.resolve(held.backend),
    droppedFilePaths: () => Promise.resolve(["/tmp/fresh.mp3"]) };
});
vi.mock("@/views/tree/TreeView", () => ({
  TreeView: (props: { onDropFiles?: (target: string, files: File[]) => void }) => {
    if (props.onDropFiles) held.drop = props.onDropFiles;
    return null;
  },
}));
vi.mock("@/views/player/Player", () => ({ Player: () => null }));
vi.mock("@/views/browser/TrackTable", () => ({ TrackTable: () => null }));
vi.mock("@/views/statusbar/StatusBar", () => ({
  StatusBar: ({ error }: { error: string | null }) => <span role="alert">{error}</span>,
}));
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  localStorage.setItem(PREFERENCES_KEY, JSON.stringify({
    advanced: { protectLibrary: false, duplicateTracks: "skip" }, analysis: { auto: true },
  }));
  window.history.replaceState(null, "", "/?dropFixture=1");
  held.drop = null;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  localStorage.clear();
  window.history.replaceState(null, "", "/");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
it.each([
  { refusal: true, auto: true },
  { refusal: false, auto: true },
  { refusal: true, auto: false },
])("preserves completed imports after playlist refusal=$refusal, auto=$auto", async ({ refusal, auto }) => {
  localStorage.setItem(PREFERENCES_KEY, JSON.stringify({
    advanced: { protectLibrary: false, duplicateTracks: "skip" }, analysis: { auto },
  }));
  const backend = createMockBackend({ trackCount: 1, writable: true });
  held.backend = backend;
  backend.unanalysedTracks = () => Promise.resolve({ tracks: [], next: null });
  await backend.edits.createPlaylist("Drop target", "root");
  const playlist = (await backend.playlistTree()).find(node => node.name === "Drop target");
  expect(playlist).toBeDefined();
  const reload = vi.spyOn(backend, "reloadLibrary");
  const imported = vi.spyOn(backend, "importPathsSlice");
  const analysed = vi.spyOn(backend, "analyseTrack");
  const add = vi.spyOn(backend.edits, "addTracksToPlaylist");
  if (refusal) add.mockRejectedValue(new Error("Fixture playlist refusal"));
  await act(async () => { root.render(<App />); await new Promise(resolve => setTimeout(resolve, 50)); });
  expect(held.drop).not.toBeNull();
  await act(async () => {
    held.drop?.(playlist!.id, [new File([], "fresh.mp3")]);
    await new Promise(resolve => setTimeout(resolve, 100));
  });
  expect(imported).toHaveBeenCalledTimes(1);
  // One refresh for the import, plus the existing analysis-drained refresh.
  expect(reload).toHaveBeenCalledTimes(auto ? 2 : 1);
  expect(add).toHaveBeenCalledTimes(1);
  expect(host.querySelector('[role="alert"]')?.textContent).toBe(refusal ? "Fixture playlist refusal" : "");
  const report = await imported.mock.results[0]!.value as ImportReport;
  expect(report.tracks).toHaveLength(1);
  const view = await backend.openView({ source: { kind: "collection" }, sort: "title", descending: false, query: "fresh" });
  expect(view.len).toBe(1);
  expect(analysed.mock.calls.map(call => call[0])).toEqual(auto ? [report.tracks[0]!.id] : []);
});

it("analyses only the committed import prefix when a later slice refuses", async () => {
  const backend = createMockBackend({ trackCount: 1, writable: true });
  held.backend = backend;
  backend.unanalysedTracks = () => Promise.resolve({ tracks: [], next: null });
  await backend.edits.createPlaylist("Drop target", "root");
  const playlist = (await backend.playlistTree()).find(node => node.name === "Drop target")!;
  const paths = Array.from({ length: 101 }, (_, index) => `/tmp/fresh-${index}.mp3`);
  backend.listImportFiles = () => Promise.resolve(paths);
  const importSlice = backend.importPathsSlice.bind(backend);
  const imported = vi.spyOn(backend, "importPathsSlice")
    .mockImplementationOnce(importSlice)
    .mockRejectedValue(new Error("Fixture later import refusal"));
  const analysed = vi.spyOn(backend, "analyseTrack").mockImplementation(trackId => Promise.resolve({
    trackId, bpmX100: 12000, key: "Am", beats: 120, peak: 0.5,
    durationSec: 60, elapsedMs: 1, analysisPath: "fixture",
  }));
  const add = vi.spyOn(backend.edits, "addTracksToPlaylist");
  await act(async () => { root.render(<App />); await new Promise(resolve => setTimeout(resolve, 50)); });
  await act(async () => {
    held.drop?.(playlist.id, [new File([], "fresh.mp3")]);
    await new Promise(resolve => setTimeout(resolve, 100));
  });
  await vi.waitFor(async () => {
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
    expect(analysed).toHaveBeenCalledTimes(100);
  });
  expect(imported).toHaveBeenCalledTimes(2);
  expect(add).not.toHaveBeenCalled();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("Fixture later import refusal");
  const report = await imported.mock.results[0]!.value as ImportReport;
  expect(new Set(analysed.mock.calls.map(call => call[0]))).toEqual(new Set(report.tracks.map(track => track.id)));
  const view = await backend.openView({ source: { kind: "collection" }, sort: "title", descending: false, query: "fresh-" });
  expect(view.len).toBe(100);
});
