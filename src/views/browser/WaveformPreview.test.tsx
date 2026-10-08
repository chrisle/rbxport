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
  ({ WaveformPreview, previewFromClick } = await import("./WaveformPreview"));
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
