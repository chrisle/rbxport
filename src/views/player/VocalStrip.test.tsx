/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const fixture = vi.hoisted(() => ({ bytes: new Uint8Array(), width: 5 }));
vi.mock("@/ipc/client", () => ({
  getBackend: () => Promise.resolve({ trackVocals: () => Promise.resolve(fixture.bytes) }),
}));
vi.mock("@/store/useElementSize", () => ({
  useElementSize: () => [null, { width: fixture.width, height: 2 }],
}));
vi.mock("@/lib/canvasSize", () => ({
  backingSize: (width: number, height: number) => ({ width, height }),
}));
import { VocalStrip } from "./VocalStrip";

declare global { var IS_REACT_ACT_ENVIRONMENT: boolean; }
let host: HTMLDivElement;
let root: Root;
let nextTrack = 0;
const drawn: { x: number; alpha: number }[] = [];
const context = {
  globalAlpha: 1,
  fillStyle: "",
  clearRect: vi.fn(),
  fillRect: vi.fn((x: number) => drawn.push({ x, alpha: context.globalAlpha })),
};

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("devicePixelRatio", 1);
  vi.spyOn(HTMLCanvasElement.prototype, "getContext")
    .mockReturnValue(context as unknown as CanvasRenderingContext2D);
  drawn.length = 0;
  context.globalAlpha = 1;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function render(bytes: number[], width = bytes.length) {
  fixture.bytes = new Uint8Array(bytes);
  fixture.width = width;
  await act(async () => {
    root.render(<VocalStrip trackId={`vocal-${++nextTrack}`} />);
    await Promise.resolve();
  });
}

it("draws supported 0–4 intensities instead of rejecting the whole strip", async () => {
  await render([0, 1, 2, 3, 4]);
  expect(drawn).toEqual([
    { x: 1, alpha: 0.25 }, { x: 2, alpha: 0.5 },
    { x: 3, alpha: 0.75 }, { x: 4, alpha: 1 },
  ]);
  expect(context.globalAlpha).toBe(1);
});

it("takes intensity from the low three bits", async () => {
  await render([0x80, 0x81, 0x82, 0x83, 0x84]);
  expect(drawn).toEqual([
    { x: 1, alpha: 0.25 }, { x: 2, alpha: 0.5 },
    { x: 3, alpha: 0.75 }, { x: 4, alpha: 1 },
  ]);
});

it("averages the points covered by each downsampled column", async () => {
  await render([0, 4, 4, 0], 2);
  expect(drawn).toEqual([{ x: 0, alpha: 0.5 }, { x: 1, alpha: 0.5 }]);
});

it("keeps a sparse strip aligned when the canvas is wider than the data", async () => {
  await render([0, 4], 8);
  expect(drawn).toEqual([4, 5, 6, 7].map((x) => ({ x, alpha: 1 })));
});

it("leaves a silent strip transparent", async () => {
  await render([0, 0, 0, 0]);
  expect(drawn).toEqual([]);
});
