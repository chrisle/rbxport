/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const recordPlay = vi.fn((_id: string) => Promise.resolve(1));
vi.mock("@/ipc/client", () => ({ getBackend: () => Promise.resolve({ edits: { recordPlay } }) }));

import { usePlayRecorder, type PlayRecorderInput } from "./usePlayRecorder";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

function Probe(props: PlayRecorderInput) {
  usePlayRecorder(props);
  return null;
}

const deck = (over: Partial<PlayRecorderInput> = {}): PlayRecorderInput => ({
  trackId: "7",
  playing: true,
  enabled: true,
  seconds: 5,
  ...over,
});

function render(props: PlayRecorderInput) {
  act(() => root.render(<Probe {...props} />));
}

/** Advances the clock and lets the backend promise settle. */
async function play(seconds: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(seconds * 1000);
  });
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  recordPlay.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe("usePlayRecorder (#540)", () => {
  it("records one play at the Playback time setting's seconds and not before", async () => {
    render(deck());
    await play(4);
    expect(recordPlay).not.toHaveBeenCalled();
    await play(1);
    expect(recordPlay).toHaveBeenCalledTimes(1);
    expect(recordPlay).toHaveBeenCalledWith("7");
    await play(60);
    expect(recordPlay).toHaveBeenCalledTimes(1);
  });

  it("counts the seconds played before a pause", async () => {
    render(deck());
    await play(3);
    render(deck({ playing: false }));
    await play(30);
    expect(recordPlay).not.toHaveBeenCalled();
    render(deck());
    await play(1);
    expect(recordPlay).not.toHaveBeenCalled();
    await play(1);
    expect(recordPlay).toHaveBeenCalledTimes(1);
  });

  it("starts again for another track, and records nothing when history is off", async () => {
    render(deck());
    await play(3);
    render(deck({ trackId: "8" }));
    await play(4);
    expect(recordPlay).not.toHaveBeenCalled();
    await play(1);
    expect(recordPlay).toHaveBeenCalledWith("8");

    recordPlay.mockClear();
    render(deck({ trackId: "9", enabled: false }));
    await play(10);
    expect(recordPlay).not.toHaveBeenCalled();
  });
});
