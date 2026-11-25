/**
 * @vitest-environment jsdom
 *
 * What a row costs the backend while a scroll goes past it.
 *
 * Each of these mounts a preview and asks one question: did it call
 * `track_waveform`? A flick through a big playlist mounts thousands of rows,
 * and the answer used to be yes for every one of them — an IPC round trip and
 * an analysis file read on the same blocking pool `fetch_rows` uses, for a row
 * nobody saw. That is what made the list come up blank after a fast scroll.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Backend } from "@/ipc/types";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let asked: string[];
let previews: Array<[string, number]>;
let stops: number;
let previewRefusal: Error | null;
let WaveformPreview: typeof import("./WaveformPreview").WaveformPreview;
let previewFromClick: typeof import("./WaveformPreview").previewFromClick;
let clearWaveformPreviewCache: typeof import("./WaveformPreview").clearWaveformPreviewCache;
/**
 * Taken from the same module graph the component imports.
 *
 * `vi.resetModules()` gives the dynamic import below a fresh `ipc/client`, and
 * a `__setBackend` held from the outer import would be setting the backend on
 * a different copy of that module — which is how this first read as "the row
 * never asks".
 */
let setBackend: typeof import("@/ipc/client").__setBackend;

/**
 * Drains the microtask queue. Several passes, because the request is behind a
 * concurrency gate and then behind `getBackend()`, so one tick is not enough.
 */
const settle = () =>
  act(async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  });

beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  vi.resetModules();
  asked = [];
  previews = [];
  stops = 0;
  previewRefusal = null;
  ({ __setBackend: setBackend } = await import("@/ipc/client"));
  // A backend that records the request and never answers: what is under test
  // is whether the call is made at all.
  setBackend({
    trackWaveform: (trackId: string) => {
      asked.push(trackId);
      return new Promise<Uint8Array>(() => {});
    },
    // The module subscribes once on load; nothing is re-analysed here.
    onAnalysisChanged: () => () => undefined,
    previewPlay: (trackId: string, positionMs: number) => {
      previews.push([trackId, positionMs]);
      return previewRefusal ? Promise.reject(previewRefusal) : Promise.resolve();
    },
    previewStop: () => {
      stops += 1;
      return Promise.resolve();
    },
    previewState: () => new Promise(() => {}),
  } as unknown as Backend);
  ({ WaveformPreview, previewFromClick, clearWaveformPreviewCache } = await import("./WaveformPreview"));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setBackend(null);
  vi.useRealTimers();
});

function mount(trackId: string) {
  act(() => root.render(<WaveformPreview trackId={trackId} width={120} height={20} hotCues={[]} durationSec={300} />));
}

describe("WaveformPreview", () => {
  it("asks for nothing while the row is only being scrolled past", async () => {
    mount("1");
    // Two frames' worth: far less than anyone can read, and the case a flick
    // produces thousands of.
    void act(() => vi.advanceTimersByTime(33));
    void act(() => root.render(<></>));
    void act(() => vi.advanceTimersByTime(1000));
    await settle();
    expect(asked).toEqual([]);
  });

  it("asks once the row has settled", async () => {
    mount("2");
    void act(() => vi.advanceTimersByTime(200));
    await settle();
    expect(asked).toEqual(["2"]);
  });

  it("still loads a mounted row after the startup cache is cleared", async () => {
    mount("live-row");
    await act(() => vi.advanceTimersByTimeAsync(200));
    await settle();
    expect(asked).toEqual(["live-row"]);
    clearWaveformPreviewCache();
    await act(() => vi.advanceTimersByTimeAsync(200));
    await settle();
    expect(asked).toEqual(["live-row", "live-row"]);
  });

  it("keeps a screenful of settled rows off the command channel at once", async () => {
    // Twenty rows land together when a scroll stops. Letting all twenty go
    // puts twenty file reads in front of the next `fetch_rows`.
    act(() => {
      root.render(
        <>
          {Array.from({ length: 20 }, (_, i) => (
            <WaveformPreview key={i} trackId={`row-${i}`} width={120} height={20} hotCues={[]} durationSec={300} />
          ))}
        </>,
      );
    });
    void act(() => vi.advanceTimersByTime(200));
    await settle();
    // Some go, so this is a gate and not a mistake; not all twenty at once,
    // which is the point. Asserted against the screenful rather than the exact
    // cap so tuning the cap does not rewrite the test — what matters is that a
    // bound below a screen's worth still holds.
    expect(asked.length).toBeGreaterThan(0);
    expect(asked.length).toBeLessThan(20);
  });
});

describe("a click on a row's waveform", () => {
  /** A preview cell the way the row lays it out, clicked the way the row hands it on. */
  function cell(cues: readonly (readonly [string, number, string | null])[] = []) {
    act(() =>
      root.render(
        <div
          data-col="preview"
          onClick={(e) => previewFromClick(e, "7", 200, cues)}
        >
          <WaveformPreview trackId="7" width={100} height={15} hotCues={cues} durationSec={200} />
        </div>,
      ),
    );
    const canvas = host.querySelector("canvas")!;
    // jsdom lays nothing out: the waveform is put 100 px wide at x = 20.
    canvas.getBoundingClientRect = () => ({ left: 20, top: 5, width: 100, height: 15, right: 120, bottom: 20, x: 20, y: 5, toJSON: () => ({}) });
    return canvas;
  }

  const click = (target: Element, init: MouseEventInit = {}) =>
    act(() => {
      target.dispatchEvent(new MouseEvent("click", { bubbles: true, button: 0, detail: 1, clientX: 45, clientY: 15, ...init }));
    });

  it("previews the track from where it was clicked", async () => {
    const canvas = cell();
    click(canvas);
    await settle();
    // A quarter of the way across a 200-second track.
    expect(previews).toEqual([["7", 50_000]]);
  });

  it("starts at a hot cue when its badge is clicked", async () => {
    const canvas = cell([["A", 120_000, null]]);
    // The badge sits at 60 % across, at the top of the waveform.
    click(canvas, { clientX: 20 + 61, clientY: 5 + 2 });
    await settle();
    expect(previews).toEqual([["7", 120_000]]);
  });

  it("leaves a Shift or Command click, and a double click, to the selection and the deck", async () => {
    const canvas = cell();
    click(canvas, { shiftKey: true });
    click(canvas, { metaKey: true });
    click(canvas, { ctrlKey: true });
    click(canvas, { detail: 2 });
    click(canvas, { button: 2 });
    await settle();
    expect(previews).toEqual([]);
  });

  it("shows a stop button on the previewing row, which stops it", async () => {
    const canvas = cell();
    click(canvas);
    await settle();
    const stop = host.querySelector("button");
    expect(stop).not.toBeNull();
    click(stop!);
    await settle();
    expect(stops).toBe(1);
    // Stopping is not another preview, and the button goes with it.
    expect(previews).toHaveLength(1);
    expect(host.querySelector("button")).toBeNull();
  });

  it("reports a refusal and draws nothing", async () => {
    previewRefusal = Object.assign(new Error("gone"), { message: "That track's file could not be found." });
    const { onPreviewError } = await import("@/store/usePreview");
    const told: string[] = [];
    onPreviewError((message) => told.push(message));
    const canvas = cell();
    click(canvas);
    await settle();
    expect(told).toEqual(["That track's file could not be found."]);
    expect(host.querySelector("button")).toBeNull();
  });
});

describe("analysis completion", () => {
  afterEach(() => { vi.doUnmock("@/canvas"); vi.restoreAllMocks(); });

  it.each([false, true])("refreshes only the changed row with an older read held=%s", async (held) => {
    vi.resetModules();
    const rendered: number[] = [];
    const painted: number[] = [];
    const requests: string[] = [];
    const pixels = new WeakMap<HTMLCanvasElement, number>();
    let changed!: (id: string) => void;
    let releaseOld!: (value: Uint8Array) => void;
    const older = new Promise<Uint8Array>(resolve => { releaseOld = resolve; });
    let current = 11;
    vi.doMock("@/canvas", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@/canvas")>()),
      renderPreview: (bytes: Uint8Array, width: number, height: number) => {
        const bitmap = document.createElement("canvas");
        const colour = bytes[0]!;
        pixels.set(bitmap, colour);
        rendered.push(colour);
        return Promise.resolve({ bitmap, width, height });
      },
    }));
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      clearRect: () => undefined,
      drawImage: (bitmap: HTMLCanvasElement) => { painted.push(pixels.get(bitmap)!); },
    } as unknown as CanvasRenderingContext2D);
    const { __setBackend } = await import("@/ipc/client");
    __setBackend({
      trackWaveform: (id: string) => {
        requests.push(id);
        if (held && current === 11 && id === "120") return older;
        return Promise.resolve(new Uint8Array([id === "120" ? current : 77]));
      },
      onAnalysisChanged: (listener: (id: string) => void) => { changed = listener; return () => undefined; },
    } as unknown as Backend);
    const { WaveformPreview: Preview } = await import("./WaveformPreview");
    // The other track's width equals the target track's numeric ID.
    const rows = () => <>{["120", "111"].map(id => (
      <Preview key={id} trackId={id} width={120} height={20} hotCues={[]} durationSec={300} />
    ))}</>;
    const drain = async () => {
      await act(() => vi.advanceTimersByTimeAsync(200));
      await settle();
    };
    act(() => root.render(rows()));
    await drain();
    expect(requests).toEqual(["120", "111"]);
    expect(painted).toContain(77);
    if (!held) expect(painted).toContain(11);

    const before = painted.length;
    current = 22;
    act(() => changed("120"));
    await drain();
    expect(requests).toEqual(["120", "111", "120"]);
    expect(painted.slice(before)).toEqual([22]);

    releaseOld(new Uint8Array([11]));
    await settle();
    expect(painted.slice(before)).toEqual([22]);
    expect(rendered.filter(value => value === 11)).toHaveLength(held ? 0 : 1);
    act(() => root.render(null));
    act(() => root.render(rows()));
    await drain();
    expect(requests).toEqual(["120", "111", "120"]);
    expect(painted.slice(-2)).toEqual([22, 77]);
    __setBackend(null);
  });
});

describe("the row preview's shape", () => {
  afterEach(() => {
    vi.doUnmock("@/canvas");
  });

  it("follows View › Full/Preview Waveform, as rekordbox's Preview column does", async () => {
    const drawnHalf: boolean[] = [];
    vi.resetModules();
    vi.doMock("@/canvas", async (importOriginal) => ({
      ...(await importOriginal<typeof import("@/canvas")>()),
      // A real rendering, so the first shape is cached: a cache key that left
      // the shape out would hand the second setting the first one's bitmap.
      renderPreview: (...[, width, height, , , half]: Parameters<typeof import("@/canvas").renderPreview>) => {
        drawnHalf.push(half ?? true);
        return Promise.resolve({ bitmap: document.createElement("canvas"), width, height });
      },
    }));
    const { __setBackend } = await import("@/ipc/client");
    __setBackend({
      trackWaveform: () => Promise.resolve(new Uint8Array([60, 30, 10])),
      onAnalysisChanged: () => () => undefined,
    } as unknown as Backend);
    const { WaveformPreview: Preview } = await import("./WaveformPreview");
    const { PreferencesProvider } = await import("@/store/usePreferences");
    const { DEFAULT_PREFERENCES } = await import("@/lib/preferences");

    for (const overviewWaveform of ["half", "full"] as const) {
      const preferences = { ...DEFAULT_PREFERENCES, view: { ...DEFAULT_PREFERENCES.view, overviewWaveform } };
      act(() =>
        root.render(
          <PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
            <Preview trackId="7" width={120} height={20} hotCues={[]} durationSec={300} />
          </PreferencesProvider>,
        ),
      );
      await act(() => vi.advanceTimersByTimeAsync(200));
      await settle();
    }
    // The same row, drawn again when the setting changes.
    expect(drawnHalf).toEqual([true, false]);
  });
});


it("shows a hover target without starting preview or requesting another waveform", () => {
  mount("hover");
  const wave = host.querySelector("canvas")!.parentElement!;
  wave.getBoundingClientRect = () => ({ left: 20, top: 5, width: 120, height: 20, right: 140, bottom: 25, x: 20, y: 5, toJSON: () => ({}) });
  act(() => { wave.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, clientX: 80, clientY: 20 })); });
  const target = wave.querySelector<HTMLElement>("[data-hover-head]");
  expect(target).not.toBeNull();
  expect(target!.hidden).toBe(false);
  expect(target!.style.left).toBe("60px");
  expect(previews).toEqual([]);
  expect(asked).toEqual([]);
  act(() => { wave.dispatchEvent(new MouseEvent("pointerleave")); });
  expect(target!.hidden).toBe(true);
});


it("points at the same hot-cue target as a preview click and resets a recycled row", async () => {
  act(() => root.render(<WaveformPreview trackId="cue" width={120} height={20} hotCues={[["A", 100_000, null]]} durationSec={300} />));
  const surface = host.querySelector("canvas")!.parentElement!;
  surface.getBoundingClientRect = () => ({ left: 20, top: 5, width: 120, height: 20, right: 140, bottom: 25, x: 20, y: 5, toJSON: () => ({}) });
  act(() => { surface.dispatchEvent(new MouseEvent("pointermove", { bubbles: true, clientX: 65, clientY: 7 })); });
  const line = surface.querySelector<HTMLElement>("[data-hover-head]")!;
  expect(line.hidden).toBe(false);
  expect(line.style.left).toBe("40px"); // Mouse x45 hits A's badge, whose click target is x40.
  expect(previews).toEqual([]);
  mount("replacement");
  expect(line.hidden).toBe(true);
  void act(() => vi.advanceTimersByTime(200)); await settle();
  const askedBefore = [...asked];
  for (let at = 20; at < 100; at++) act(() => { surface.dispatchEvent(new MouseEvent("pointermove", { clientX: at, clientY: 20 })); });
  expect(asked).toEqual(askedBefore);
  expect(previews).toEqual([]);
});
