/**
 * @vitest-environment jsdom
 *
 * Loading is asynchronous: selection, cue and play can all outrun disk I/O.
 * These tests hold completions back so superseded tracks and an audio-engine
 * rebuild cannot reset the deck after the newest action has been accepted.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, DeckEvent, Tick } from "@/ipc/types";
import { usePlayback, type Playback } from "./usePlayback";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

const RATE = 44_100;
const emptyDeck = {
  frames: 0, totalFrames: 0, generation: 0, playing: false, loaded: false, loadId: 0,
  tempo: 1, masterTempo: false, keyShift: 0, startInFrames: 0,
  loopInFrames: 0, loopOutFrames: 0, looping: false,
};
const emptyTick: Tick = {
  a: { ...emptyDeck }, b: { ...emptyDeck }, sampleRate: RATE,
  peakLeft: 0, peakRight: 0, master: 1, reduction: 0, shiftsKey: true,
};

let host: HTMLDivElement;
let root: Root;
let deck: Playback;
let track = "track-a";
let loaded: (event: DeckEvent) => void;
let tick: (value: Tick) => void;
let reset: () => void;
let loads: { track: string; id: number }[];
let sent: string[];

const done = () => Promise.resolve();

function backend(): Backend {
  return {
    deckState: () => Promise.resolve(emptyTick),
    onDeckTick: (listener: (tick: Tick) => void) => {
      tick = listener;
      return () => undefined;
    },
    onDeckEvent: (listener: (event: DeckEvent) => void) => {
      loaded = listener;
      return () => undefined;
    },
    onDeckReset: (listener: () => void) => {
      reset = listener;
      return () => undefined;
    },
    deckLoad: (_deck: "a" | "b", next: string, loadId: number) => {
      loads.push({ track: next, id: loadId });
      return done();
    },
    deckUnload: done,
    deckPlay: () => { sent.push("play"); return done(); },
    deckPlayAfter: (_deck: "a" | "b", ms: number) => { sent.push(`play-after:${ms}`); return done(); },
    deckPause: () => { sent.push("pause"); return done(); },
    deckSeek: (_deck: "a" | "b", ms: number) => { sent.push(`seek:${ms}`); return done(); },
    deckTempo: () => done(),
    deckMasterTempo: () => done(),
    deckKeyShift: () => done(),
    deckSetLoop: () => done(),
    deckLoopActive: () => done(),
  } as unknown as Backend;
}

function Probe() {
  deck = usePlayback(track, "b");
  return null;
}

async function settle() {
  await act(async () => { await done(); await done(); });
}

async function select(next: string) {
  track = next;
  await act(async () => { root.render(<Probe />); await done(); });
}

function finish(request: { id: number }) {
  act(() => loaded({
    deck: "b", loadId: request.id, totalFrames: RATE * 300, sampleRate: RATE, message: null,
  }));
}

beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  loads = [];
  sent = [];
  track = `track-a-${Math.random()}`;
  vi.stubGlobal("requestAnimationFrame", () => 1);
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
  __setBackend(backend());
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("defers cue and play until the newest rapidly selected track is ready", async () => {
  await select("track-b");
  await select("track-c");
  expect(loads).toHaveLength(3);

  act(() => {
    deck.seek(12);
    deck.toggle();
  });
  await settle();
  expect(sent).toEqual([]);

  finish(loads[0]!);
  finish(loads[1]!);
  await settle();
  expect(sent).toEqual([]);

  finish(loads[2]!);
  await settle();
  expect(sent).toEqual(["seek:12000", "play"]);
});

it("pins queued playback until an authoritative advancing deck tick", async () => {
  let now = 1_000;
  const clock = vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frame = callback;
    return 1;
  });
  act(() => deck.toggle());
  finish(loads[0]!);
  await settle();
  expect(sent).toEqual(["play"]);

  now += 400;
  expect(deck.positionNow()).toBe(0);
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);

  act(() => tick({
    ...emptyTick,
    b: { ...emptyDeck, loadId: loads[0]!.id, loaded: true, playing: true, totalFrames: RATE * 300 },
  }));
  expect(deck.positionNow()).toBe(0);
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);

  act(() => tick({
    ...emptyTick,
    b: { ...emptyDeck, loadId: loads[0]!.id, loaded: true, playing: true, frames: RATE / 10, totalFrames: RATE * 300 },
  }));
  expect(deck.positionNow()).toBeCloseTo(0.1);
  now += 16;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBeGreaterThan(0);
  clock.mockRestore();
});

it("keeps an overview click made while the track opens, and starts from there", async () => {
  // The continuous-play e2e clicks the overview's end and presses PLAY right
  // after loading; a click before the load was ready used to be dropped, so
  // the deck played from the top and never reached the end.
  act(() => {
    deck.scrubBegin();
    deck.scrubTo(290);
    deck.scrubEnd();
    deck.toggle();
  });
  await settle();
  expect(sent).toEqual([]);
  expect(deck.positionRef.current).toBe(290);

  finish(loads[0]!);
  await settle();
  expect(sent).toEqual(["seek:290000", "play"]);
});

it("lets a cue release cancel play that was queued during loading", async () => {
  act(() => deck.toggle());
  expect(deck.playing).toBe(true);
  act(() => deck.toggle());
  await settle();

  finish(loads[0]!);
  await settle();
  expect(sent).toEqual([]);
  expect(deck.playing).toBe(false);
});

it("keeps the deck playing when its track is switched", async () => {
  finish(loads[0]!);
  await settle();
  act(() => deck.toggle());
  await settle();
  expect(sent).toEqual(["play"]);

  sent = [];
  await select("replacement-track");
  const replacement = loads.at(-1)!;
  expect(replacement.track).toBe("replacement-track");
  expect(deck.playing).toBe(false);
  finish(replacement);
  await settle();

  expect(sent).toEqual(["play"]);
  expect(deck.playing).toBe(true);
});

it("leaves a newly selected track cued when the deck was stopped", async () => {
  finish(loads[0]!);
  await settle();
  await select("replacement-track");
  finish(loads.at(-1)!);
  await settle();

  expect(sent).toEqual([]);
  expect(deck.playing).toBe(false);
});

it("reloads and restores the current deck after its audio engine is replaced", async () => {
  finish(loads[0]!);
  await settle();
  act(() => deck.seek(8.5));
  await settle();
  act(() => deck.toggle());
  await settle();
  sent = [];

  act(() => reset());
  await settle();
  expect(loads.at(-1)?.track).toBe(track);
  const replacement = loads.at(-1)!;
  finish(replacement);
  await settle();
  expect(sent).toEqual(["seek:8500", "play"]);
});
