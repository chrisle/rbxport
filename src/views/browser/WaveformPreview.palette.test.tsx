/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Backend, WaveformKind } from "@/ipc/types";

let root: Root;
let host: HTMLDivElement;
let drawn: number | null;
let changed: (id: string) => void;
let show: (palette: "3band" | "blue", id?: string) => void;
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
  drawn = null;
  read.mockReset().mockImplementation((_id, kind) => Promise.resolve(new Uint8Array([kind === "bands" ? 11 : 22])));
  const bitmaps = new WeakMap<HTMLCanvasElement, number>();
  vi.doMock("@/canvas", async importOriginal => ({
    ...await importOriginal<typeof import("@/canvas")>(),
    renderPreview: (bytes: Uint8Array, width: number, height: number) => {
      const bitmap = document.createElement("canvas"); bitmaps.set(bitmap, bytes[0]!);
      return Promise.resolve({ bitmap, width, height });
    },
  }));
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect: () => { drawn = null; },
    drawImage: (bitmap: HTMLCanvasElement) => { drawn = bitmaps.get(bitmap)!; },
  } as unknown as CanvasRenderingContext2D);
  const { __setBackend } = await import("@/ipc/client");
  __setBackend({ trackWaveform: read, onAnalysisChanged: (listener: (id: string) => void) => { changed = listener; return () => {}; } } as unknown as Backend);
  resetBackend = () => __setBackend(null);
  const { WaveformPreview } = await import("./WaveformPreview");
  const { PreferencesProvider } = await import("@/store/usePreferences");
  const { DEFAULT_PREFERENCES } = await import("@/lib/preferences");
  host = document.createElement("div"); root = createRoot(host);
  show = (waveformColor, id = "palette-fixture") => act(() => root.render(
    <PreferencesProvider value={{ preferences: { ...DEFAULT_PREFERENCES, view: { ...DEFAULT_PREFERENCES.view, waveformColor } }, update: vi.fn(), reset: vi.fn() }}>
      <WaveformPreview trackId={id} width={120} height={20} hotCues={[]} durationSec={300} />
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
