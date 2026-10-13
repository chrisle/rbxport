/** @vitest-environment jsdom */
/**
 * A wheel can cross the PCM/PWV7 boundary before the passive fetch effect
 * runs. The previous render's bytes must never be decoded by the new path in
 * that intervening layout frame.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const drawn = vi.hoisted(() => ({ wave: vi.fn(), pcm: vi.fn() }));

vi.mock("@/canvas", () => ({
  drawWave: drawn.wave,
  drawPcmWave: drawn.pcm,
  strideOf: (palette: string, detail: boolean) => palette === "3band" ? 3 : palette === "blue" ? 1 : detail ? 2 : 6,
  waveformKindOf: (palette: string, detail: boolean) => ({
    "3band": detail ? "bandsDetail" : "bands",
    blue: detail ? "monoDetail" : "mono",
    rgb: detail ? "colourDetail" : "colour",
  })[palette],
}));

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { WaveformDetail } from "./WaveformDetail";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const settle = () => act(async () => {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
});

let host: HTMLDivElement;
let root: Root;
let pcm: Deferred<Uint8Array>;
let pwv7: Deferred<Uint8Array>;
let fetchPcm: ReturnType<typeof vi.fn>;

function render(pcmWindow?: { fromMs: number; toMs: number; drawFromMs: number; drawToMs: number }) {
  act(() => root.render(
    <WaveformDetail
      trackId="wheel-race-track"
      progress={0.5}
      span={0.02}
      width={400}
      height={80}
      detail
      pcmWindow={pcmWindow}
    />,
  ));
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  drawn.wave.mockReset();
  drawn.pcm.mockReset();
  pcm = deferred<Uint8Array>();
  pwv7 = deferred<Uint8Array>();
  fetchPcm = vi.fn(() => pcm.promise);
  __setBackend({
    trackPcmWaveform: fetchPcm,
    trackWaveform: () => pwv7.promise,
    onAnalysisChanged: () => () => {},
  } as unknown as Backend);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
  clearRect: vi.fn(),
    save: vi.fn(),
    translate: vi.fn(),
    restore: vi.fn(),
  } as unknown as CanvasRenderingContext2D);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  __setBackend(null);
});

describe("wheel crossing from PCM back to PWV7", () => {
  it("never gives the normal renderer the previous PCM byte layout", async () => {
    const pcmBytes = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
    const pwvBytes = new Uint8Array([90, 50, 20, 80, 40, 10]);

    render({ fromMs: 0, toMs: 4_000, drawFromMs: 1_000, drawToMs: 3_000 });
    await settle();
    pcm.resolve(pcmBytes);
    await settle();
    expect(drawn.pcm).toHaveBeenCalledWith(expect.anything(), pcmBytes, expect.any(Number), expect.any(Number), "3band", expect.anything());

    // This is the render a wheel event produces on its first tick out of the
    // PCM range. Layout effects run before the normal PWV7 fetch can settle.
    drawn.wave.mockClear();
    render();
    expect(drawn.wave).not.toHaveBeenCalled();

    pwv7.resolve(pwvBytes);
    await settle();
    expect(drawn.wave).toHaveBeenLastCalledWith(
      expect.anything(), pwvBytes, expect.any(Number), expect.any(Number), "3band", true, false, expect.anything(),
    );
  });
});

describe("PCM read-ahead", () => {
  it("reuses guard audio and keeps correctly positioned samples visible during refresh", async () => {
    const first = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
    const next = new Uint8Array([9, 10, 11, 12, 13, 14, 15, 16]);
    render({ fromMs: 0, toMs: 6000, drawFromMs: 2000, drawToMs: 4000 });
    await settle();
    pcm.resolve(first);
    await settle();
    render({ fromMs: 100, toMs: 6100, drawFromMs: 2100, drawToMs: 4100 });
    await settle();
    expect(fetchPcm).toHaveBeenCalledTimes(1);

    pcm = deferred<Uint8Array>();
    drawn.pcm.mockClear();
    render({ fromMs: 1500, toMs: 7500, drawFromMs: 3500, drawToMs: 5500 });
    await settle();
    expect(fetchPcm).toHaveBeenCalledTimes(2);
    expect(fetchPcm).toHaveBeenLastCalledWith("wheel-race-track", 1500, 7500, 7500);
    expect(drawn.pcm).toHaveBeenLastCalledWith(expect.anything(), first, expect.any(Number), expect.any(Number), "3band", {
      from: 3500 / 6000, to: 5500 / 6000,
    });
    render({ fromMs: 1600, toMs: 7600, drawFromMs: 3600, drawToMs: 5600 });
    await settle();
    expect(fetchPcm).toHaveBeenCalledTimes(2);
    expect(drawn.pcm.mock.calls.every(call => call[1] === first)).toBe(true);
    pcm.resolve(next);
    await settle();
    expect(drawn.pcm).toHaveBeenLastCalledWith(expect.anything(), next, expect.any(Number), expect.any(Number), "3band", {
      from: (3600 - 1500) / 6000, to: (5600 - 1500) / 6000,
    });
  });

  it("ignores a late read after a seek outside its window", async () => {
    const stale = pcm;
    render({ fromMs: 0, toMs: 6000, drawFromMs: 2000, drawToMs: 4000 });
    await settle();
    pcm = deferred<Uint8Array>();
    render({ fromMs: 30000, toMs: 36000, drawFromMs: 32000, drawToMs: 34000 });
    await settle();
    const current = new Uint8Array(8).fill(5);
    pcm.resolve(current);
    await settle();
    drawn.pcm.mockClear();
    stale.resolve(new Uint8Array(8).fill(9));
    await settle();
    expect(drawn.pcm).not.toHaveBeenCalled();
    render({ fromMs: 30100, toMs: 36100, drawFromMs: 32100, drawToMs: 34100 });
    expect(drawn.pcm).toHaveBeenLastCalledWith(expect.anything(), current, expect.any(Number), expect.any(Number), "3band", {
      from: 2100 / 6000, to: 4100 / 6000,
    });
  });
});

describe("backing store (#346)", () => {
  it("is exactly the device pixels of the strip, so the bars are not squeezed onto it", async () => {
    const ratio = Object.getOwnPropertyDescriptor(window, "devicePixelRatio");
    Object.defineProperty(window, "devicePixelRatio", { value: 2, configurable: true });
    try {
      // The 1 PLAYER detail on a 1470pt window: 1,177pt wide, drawn two spans across.
      act(() => root.render(
        <WaveformDetail trackId="wheel-race-track" progress={0.5} span={0.02} width={2354} height={133} detail />,
      ));
      pwv7.resolve(new Uint8Array(3 * 2000).fill(40));
      await settle();
      const canvas = host.querySelector("canvas");
      // A 16pt-quantised store was 2,368pt (4,736 px) squeezed onto 4,708.
      expect(canvas?.width).toBe(4708);
      expect(canvas?.height).toBe(266);
    } finally {
      if (ratio) Object.defineProperty(window, "devicePixelRatio", ratio);
      else Reflect.deleteProperty(window, "devicePixelRatio");
    }
  });
});

describe("overview without the palette's tag (#372)", () => {
  // A stick exported before 3Band data existed has no .2EX: rekordbox's deck
  // overview then shows the .DAT's PWAV in blue (UiPlayer::updateTrackData).
  const pwav = new Uint8Array(400).fill(0x35);

  function overview(tags: Partial<Record<string, Uint8Array>>) {
    const asked: string[] = [];
    __setBackend({
      trackPcmWaveform: fetchPcm,
      trackWaveform: (_track: string, kind: string) => {
        asked.push(kind);
        return Promise.resolve(tags[kind] ?? new Uint8Array());
      },
      onAnalysisChanged: () => () => {},
    } as unknown as Backend);
    return asked;
  }

  it("draws the .DAT's PWAV in blue when the 3Band PWV6 is missing", async () => {
    const asked = overview({ mono: pwav });
    act(() => root.render(<WaveformDetail trackId="no-2ex-track" progress={0.5} span={1} width={400} height={40} />));
    await settle();
    expect(asked).toEqual(["bands", "mono"]);
    expect(drawn.wave).toHaveBeenLastCalledWith(
      expect.anything(), pwav, expect.any(Number), expect.any(Number), "blue", false, false, expect.anything(),
    );
  });

  it("draws the PWV6 when it is there, without asking for the PWAV", async () => {
    const pwv6 = new Uint8Array(1_200 * 3).fill(20);
    const asked = overview({ bands: pwv6, mono: pwav });
    act(() => root.render(<WaveformDetail trackId="with-2ex-track" progress={0.5} span={1} width={400} height={40} />));
    await settle();
    expect(asked).toEqual(["bands"]);
    expect(drawn.wave).toHaveBeenLastCalledWith(
      expect.anything(), pwv6, expect.any(Number), expect.any(Number), "3band", false, false, expect.anything(),
    );
  });

  it("does not put the PWAV in the detail, which has no such fallback", async () => {
    const asked = overview({ mono: pwav });
    act(() => root.render(<WaveformDetail trackId="no-2ex-detail" progress={0.5} span={0.02} width={400} height={80} detail />));
    await settle();
    expect(asked).toEqual(["bandsDetail"]);
    expect(drawn.wave).not.toHaveBeenCalled();
  });
});
