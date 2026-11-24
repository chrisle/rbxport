/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend, RowDto } from "@/ipc/types";
import { useTrackGrid, type TrackGrid } from "./useTrackGrid";

let host: HTMLDivElement;
let root: Root;
let latest: TrackGrid;
let changed: (id: string) => void;
const stop = vi.fn();
const trackBeats = vi.fn<Backend["trackBeats"]>();
const gridState = vi.fn<Backend["gridState"]>();
const track = { id: "loaded", analysed: true } as unknown as RowDto;
function Probe({ row = track }: { row?: RowDto }) {
  latest = useTrackGrid(row);
  return null;
}
function bytes(time: number, bpm = 12800) {
  const data = new Uint8Array(7);
  const view = new DataView(data.buffer);
  view.setUint32(0, time, true);
  view.setUint8(4, 1);
  view.setUint16(5, bpm, true);
  return data;
}
const settle = () => act(async () => { await Promise.resolve(); });
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  stop.mockReset();
  trackBeats.mockReset().mockResolvedValue(bytes(467));
  gridState.mockReset().mockResolvedValue({ bpmX100: 12800, beats: 1, canUndo: false, canRedo: false, locked: false });
  __setBackend({ trackBeats, gridState,
    onAnalysisChanged: (listener: (id: string) => void) => { changed = listener; return stop; },
    onGridChanged: () => () => {}, onLibraryChanged: () => () => {},
  } as unknown as Backend);
  host = document.createElement("div");
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  __setBackend(null);
});
it("refreshes the loaded grid and BPM after analysis without reloading the track", async () => {
  act(() => root.render(<Probe />));
  await settle();
  expect(Array.from(latest.grid.times)).toEqual([467]);
  changed("other-track");
  await settle();
  expect(trackBeats).toHaveBeenCalledTimes(1);
  trackBeats.mockResolvedValue(bytes(0, 17400));
  gridState.mockResolvedValue({ bpmX100: 17400, beats: 1, canUndo: false, canRedo: false, locked: false });
  changed("loaded");
  await settle();
  expect(Array.from(latest.grid.times)).toEqual([0]);
  expect(latest.state?.bpmX100).toBe(17400);
  act(() => root.render(null));
  expect(stop).toHaveBeenCalledTimes(1);
});
it("does not let a pre-analysis read overwrite the refreshed grid", async () => {
  let resolveOld!: (value: Uint8Array) => void;
  trackBeats.mockReturnValueOnce(new Promise((resolve) => { resolveOld = resolve; }));
  act(() => root.render(<Probe />));
  await settle();
  trackBeats.mockResolvedValue(bytes(0));
  changed("loaded");
  await settle();
  expect(Array.from(latest.grid.times)).toEqual([0]);
  resolveOld(bytes(467));
  await settle();
  expect(Array.from(latest.grid.times)).toEqual([0]);
});
it("names the track its grid was read for, so a switch is not taken for the new grid", async () => {
  let resolveNext!: (value: Uint8Array) => void;
  act(() => root.render(<Probe />));
  await settle();
  expect(latest.gridTrackId).toBe("loaded");
  const next = { id: "next", analysed: true } as unknown as RowDto;
  trackBeats.mockReturnValueOnce(new Promise((resolve) => { resolveNext = resolve; }));
  act(() => root.render(<Probe row={next} />));
  await settle();
  // The old grid is still on show while the new one is read.
  expect(Array.from(latest.grid.times)).toEqual([467]);
  expect(latest.gridTrackId).toBe("loaded");
  resolveNext(bytes(0));
  await settle();
  expect(latest.gridTrackId).toBe("next");
});

it("reads grid bytes and state from the loaded device row when audio is shared", async () => {
  const source = { path: "/Volumes/TEST", format: "oneLibrary" as const, trackId: 9 };
  const row = { ...track, id: "file:/Volumes/TEST/a.mp3", deviceSource: source };
  act(() => root.render(<Probe row={row} />));
  await settle();
  expect(trackBeats).toHaveBeenLastCalledWith(row.id, source);
  expect(gridState).toHaveBeenLastCalledWith(row.id, source);
  const other = { ...row, deviceSource: { ...source, format: "deviceLibrary" as const, trackId: 1 } };
  act(() => root.render(<Probe row={other} />));
  await settle();
  expect(trackBeats).toHaveBeenLastCalledWith(row.id, other.deviceSource);
  expect(gridState).toHaveBeenLastCalledWith(row.id, other.deviceSource);
});

it("disables edits while the same audio switches to another device source", async () => {
  const first = { ...track, id: "file:/Volumes/TEST/a.mp3", deviceSource: { path: "/Volumes/TEST", format: "deviceLibrary" as const, trackId: 1 } };
  act(() => root.render(<Probe row={first} />)); await settle();
  expect(latest.state).not.toBeNull();
  trackBeats.mockReturnValueOnce(new Promise(() => {}));
  act(() => root.render(<Probe row={{ ...first, deviceSource: { ...first.deviceSource, format: "oneLibrary", trackId: 9 } }} />));
  expect(latest.state).toBeNull();
});

it("does not confuse pipe characters in device paths with source field boundaries", async () => {
  // Both paths are below their respective mounts. Their pipe-joined keys
  // used to collide, leaving the first source editable during the new read.
  const first = { ...track, id: "file:/Volumes/A/b.mp3|/Volumes/A/Y.mp3|/Volumes/A/b.mp3",
    deviceSource: { path: "/Volumes/A", format: "deviceLibrary" as const, trackId: 7 } };
  const second = { ...track, id: "file:/Volumes/A/b.mp3|/Volumes/A/Y.mp3",
    deviceSource: { path: "/Volumes/A/b.mp3|/Volumes/A", format: "deviceLibrary" as const, trackId: 7 } };
  act(() => root.render(<Probe row={first} />)); await settle();
  expect(latest.state).not.toBeNull();
  trackBeats.mockReturnValueOnce(new Promise(() => {}));
  act(() => root.render(<Probe row={second} />));
  expect(latest.state).toBeNull();
});

it("disables a source that disappeared instead of leaving an editable stale grid", async () => {
  trackBeats.mockRejectedValue(new Error("device row disappeared"));
  gridState.mockRejectedValue(new Error("device row disappeared"));
  act(() => root.render(<Probe />)); await settle(); await settle();
  expect(latest.state).toBeNull();
  expect(latest.grid.times.length).toBe(0);
});
