/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Backend, WaveformKind } from "@/ipc/types";

let root: Root;
let host: HTMLDivElement;
let drawn: number | null;
let drawnHeight: number | null;
let renderedHeights: number[];
let showHeights: (heights: readonly number[]) => void;
let changed: (id: string) => void;
let show: (palette: "3band" | "blue", id?: string, height?: number, startupCache?: boolean) => void;
let resetBackend: () => void;
const read = vi.fn<(id: string, kind: WaveformKind) => Promise<Uint8Array>>();
const drain = () => act(async () => { await vi.advanceTimersByTimeAsync(100); });
function deferred() {
  let resolve!: (data: Uint8Array) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<Uint8Array>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers(); vi.resetModules();
  drawn = null; drawnHeight = null; renderedHeights = [];
  read.mockReset().mockImplementation((_id, kind) => Promise.resolve(new Uint8Array([kind === "bands" ? 11 : 22])));
  const bitmaps = new WeakMap<HTMLCanvasElement, number>();
  const heights = new WeakMap<HTMLCanvasElement, number>();
  vi.doMock("@/canvas", async importOriginal => ({
    ...await importOriginal<typeof import("@/canvas")>(),
    renderPreview: (bytes: Uint8Array, width: number, height: number) => {
      const bitmap = document.createElement("canvas"); bitmaps.set(bitmap, bytes[0]!);
      heights.set(bitmap, height); renderedHeights.push(height);
      return Promise.resolve({ bitmap, width, height });
    },
  }));
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect: () => { drawn = null; },
    drawImage: (bitmap: HTMLCanvasElement) => { drawn = bitmaps.get(bitmap)!; drawnHeight = heights.get(bitmap)!; },
  } as unknown as CanvasRenderingContext2D);
  const { __setBackend } = await import("@/ipc/client");
  __setBackend({ trackWaveform: read, onAnalysisChanged: (listener: (id: string) => void) => { changed = listener; return () => {}; } } as unknown as Backend);
  resetBackend = () => __setBackend(null);
  const { WaveformPreview } = await import("./WaveformPreview");
  const { PreferencesProvider } = await import("@/store/usePreferences");
  const { DEFAULT_PREFERENCES } = await import("@/lib/preferences");
  host = document.createElement("div"); root = createRoot(host);
  show = (waveformColor, id = "palette-fixture", height = 20, startupCache = false) => act(() => root.render(
    <PreferencesProvider value={{ preferences: { ...DEFAULT_PREFERENCES, view: { ...DEFAULT_PREFERENCES.view, waveformColor } }, update: vi.fn(), reset: vi.fn() }}>
      <WaveformPreview trackId={id} width={120} height={height} hotCues={[]} durationSec={300} startupCache={startupCache} />
    </PreferencesProvider>,
  ));
  showHeights = sizes => act(() => root.render(
    <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: vi.fn(), reset: vi.fn() }}>
      {sizes.map((height, i) => <WaveformPreview key={i} trackId="same-source" width={120} height={height} hotCues={[]} durationSec={300} />)}
    </PreferencesProvider>,
  ));
});
afterEach(() => {
  act(() => root.unmount()); resetBackend(); vi.doUnmock("@/canvas"); vi.restoreAllMocks(); vi.useRealTimers();
});

it.each(["empty", "rejected"] as const)("clears the previous palette while its replacement waits and after %s", async result => {
  show("3band"); await drain(); expect(drawn).toBe(11);
  const pending = deferred(); read.mockReturnValueOnce(pending.promise);
  show("blue"); expect(drawn).toBeNull(); await drain();
  if (result === "empty") pending.resolve(new Uint8Array());
  else pending.reject(new Error("missing selected tag"));
  await drain(); expect(drawn).toBeNull();
});

it.each([false, true])("an old palette result cannot erase or redraw the current palette (empty=%s)", async empty => {
  const old = deferred(); read.mockReturnValueOnce(old.promise);
  show("3band"); await drain();
  show("blue"); await drain(); expect(drawn).toBe(22);
  old.resolve(empty ? new Uint8Array() : new Uint8Array([11])); await drain();
  expect(drawn).toBe(22);
});

it("clears a completed same-format miss and can reuse the unaffected palette", async () => {
  show("blue"); await drain(); expect(drawn).toBe(22);
  show("3band"); await drain(); expect(drawn).toBe(11);
  read.mockResolvedValueOnce(new Uint8Array());
  act(() => changed("palette-fixture")); await drain(); expect(drawn).toBeNull();
  show("3band", "other-fixture"); await drain(); expect(drawn).toBe(11);
  show("blue", "other-fixture"); await drain(); expect(drawn).toBe(22);
  const calls = read.mock.calls.length;
  show("3band", "other-fixture"); await drain(); expect(drawn).toBe(11);
  expect(read).toHaveBeenCalledTimes(calls);
});

it("does not borrow a previous track's pixels while the new track is unavailable", async () => {
  show("3band", "first-track"); await drain(); expect(drawn).toBe(11);
  const pending = deferred(); read.mockReturnValueOnce(pending.promise);
  show("3band", "second-track"); expect(drawn).toBeNull(); await drain();
  pending.resolve(new Uint8Array()); await drain(); expect(drawn).toBeNull();
});

it.each([false, true])("retains same-key pixels during incidental rerenders until the refreshed result (missing=%s)", async missing => {
  show("3band"); await drain(); expect(drawn).toBe(11);
  const pending = deferred(); read.mockReturnValueOnce(pending.promise);
  act(() => changed("palette-fixture")); await drain();
  expect(drawn).toBe(11);
  // New cue-prop array, same track/palette/size: do not blank while joining
  // the refresh that has already invalidated its cached bitmap.
  show("3band"); expect(drawn).toBe(11); await drain();
  expect(read).toHaveBeenCalledTimes(2);
  pending.resolve(missing ? new Uint8Array() : new Uint8Array([33])); await drain();
  expect(drawn).toBe(missing ? null : 33);
});


it.each([false, true])("keeps native-height bitmaps and return hits for each height (startup=%s)", async startup => {
  show("3band", "height-source", 20, startup); await drain(); expect(drawnHeight).toBe(20);
  show("3band", "height-source", 40, startup); await drain(); expect(drawnHeight).toBe(40);
  expect(renderedHeights).toEqual([20, 40]);
  show("3band", "height-source", 20, startup); await drain(); expect(drawnHeight).toBe(20);
  expect(read).toHaveBeenCalledTimes(2);
});

it("deduplicates equal-height pending requests without merging different heights", async () => {
  const pending = deferred(); read.mockReturnValue(pending.promise);
  showHeights([20, 40, 20]); await drain(); expect(read).toHaveBeenCalledTimes(2);
  pending.resolve(new Uint8Array([11])); await drain(); expect(renderedHeights).toEqual([20, 40]);
});

it("separates startup and live sizes and invalidates all heights for a changed track", async () => {
  for (const startup of [true, false]) for (const height of [20, 40]) { show("3band", "height-source", height, startup); await drain(); }
  expect(read).toHaveBeenCalledTimes(4); expect(renderedHeights).toEqual([20, 40, 20, 40]);
  show("3band", "height-source", 20, true); await drain(); expect(read).toHaveBeenCalledTimes(4);
  act(() => changed("height-source")); await drain(); expect(read).toHaveBeenCalledTimes(5);
  show("3band", "height-source", 40, false); await drain(); expect(read).toHaveBeenCalledTimes(6);
  expect(drawnHeight).toBe(40);
});
