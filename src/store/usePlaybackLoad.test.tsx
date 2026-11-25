/**
 * @vitest-environment jsdom
 *
 * Loading is asynchronous: selection, cue and play can all outrun disk I/O.
 * These tests hold completions back so superseded tracks and an audio-engine
 * rebuild cannot reset the deck after the newest action has been accepted.
 */
import { act, startTransition, StrictMode, Suspense, useLayoutEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, DeckEvent, Tick } from "@/ipc/types";
import { confirmedLoads, useLoadedTracks } from "./useLoadedTracks";
import { usePlayback, type Playback } from "./usePlayback";
import { registerDeck, setPlaying as setScriptPlaying } from "@/lib/scripting";

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
let track: string | null = "track-a";
let engineState: Tick = emptyTick;
let intent = 0;
let loaded: (event: DeckEvent) => void;
let tick: (value: Tick) => void;
let reset: () => void;
let loads: { track: string; id: number; deck: "a" | "b" }[];
let sent: string[];
let tempoWrites: number[];
let tempoResult: (value: number) => Promise<void>;
let unloads = 0;
let tickSubscriptions = 0;
let resetListeners: Set<() => void>;

const done = () => Promise.resolve();

function backend(): Backend {
  return {
    deckState: () => Promise.resolve(engineState),
    onDeckTick: (listener: (tick: Tick) => void) => {
      tickSubscriptions++;
      tick = listener;
      return () => undefined;
    },
    onDeckEvent: (listener: (event: DeckEvent) => void) => {
      loaded = listener;
      return () => undefined;
    },
    onDeckReset: (listener: () => void) => {
      resetListeners.add(listener);
      return () => { resetListeners.delete(listener); };
    },
    deckLoad: (_deck: "a" | "b", next: string, loadId: number) => {
      loads.push({ track: next, id: loadId, deck: _deck });
      return done();
    },
    deckUnload: () => { unloads++; return done(); },
    deckPlay: () => { sent.push("play"); return done(); },
    deckPlayAfter: (_deck: "a" | "b", ms: number) => { sent.push(`play-after:${ms}`); return done(); },
    deckPause: () => { sent.push("pause"); return done(); },
    deckSeek: (_deck: "a" | "b", ms: number) => { sent.push(`seek:${ms}`); return done(); },
    deckTempo: (_deck: "a" | "b", tempo: number) => { tempoWrites.push(tempo); return tempoResult(tempo); },
    deckMasterTempo: () => done(),
    deckKeyShift: () => done(),
    deckSetLoop: () => done(),
    deckLoopActive: () => done(),
  } as unknown as Backend;
}

function Probe() {
  deck = usePlayback(track, "b", true, intent);
  return null;
}

async function settle() {
  await act(async () => { await done(); await done(); });
}

async function select(next: string | null) {
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
  engineState = emptyTick;
  loads = [];
  unloads = 0;
  tickSubscriptions = 0;
  intent = 0;
  sent = [];
  tempoWrites = [];
  tempoResult = done;
  resetListeners = new Set();
  reset = () => { for (const listener of resetListeners) listener(); };
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

it("keeps the requested tempo across load when a tick still reports the old tempo", async () => {
  const first = loads[0]!;
  finish(first);
  await settle();
  const halfSpeed = {
    ...emptyTick,
    b: { ...emptyDeck, loadId: first.id, loaded: true, totalFrames: RATE * 300, tempo: 0.5 },
  };
  act(() => deck.setTempo(0.5));
  await settle();
  act(() => tick(halfSpeed));
  expect(deck.tempo).toBe(0.5);

  act(() => deck.setTempo(1));
  expect(deck.tempo).toBe(1);
  // An already queued tick can arrive before the engine reports the reset.
  act(() => tick(halfSpeed));
  await select("track-b");
  finish(loads[1]!);
  await settle();

  expect(tempoWrites.slice(-2)).toEqual([1, 1]);
  expect(deck.tempo).toBe(1);
});

it("carries the observed native tempo when there is no local request", async () => {
  finish(loads[0]!);
  await settle();
  act(() => tick({
    ...emptyTick,
    b: { ...emptyDeck, loadId: loads[0]!.id, loaded: true, totalFrames: RATE * 300, tempo: 0.75 },
  }));
  expect(deck.tempo).toBe(0.75);
  await select("track-b");
  finish(loads[1]!);
  await settle();
  expect(tempoWrites.at(-1)).toBe(0.75);
});

it("continues to adopt native tempo changes after the local request lands", async () => {
  finish(loads[0]!);
  await settle();
  act(() => deck.setTempo(1.001));
  await settle();
  const reportTempo = (tempo: number) => tick({
    ...emptyTick,
    b: { ...emptyDeck, loadId: loads[0]!.id, loaded: true, totalFrames: RATE * 300, tempo },
  });
  act(() => reportTempo(Math.fround(1.001)));
  act(() => reportTempo(0.75));
  expect(deck.tempo).toBe(0.75);
  await select("track-b");
  finish(loads[1]!);
  await settle();
  expect(tempoWrites.at(-1)).toBe(0.75);
});

it("does not acknowledge a matching tick while the newest request is still being sent", async () => {
  finish(loads[0]!);
  await settle();
  let finishReset!: () => void;
  const dispatch = new Promise<void>((resolve) => { finishReset = resolve; });
  tempoResult = (value) => value === 1 ? dispatch : done();
  const reportTempo = (tempo: number) => tick({
    ...emptyTick,
    b: { ...emptyDeck, loadId: loads[0]!.id, loaded: true, totalFrames: RATE * 300, tempo },
  });
  act(() => deck.setTempo(0.5));
  await settle();
  act(() => deck.setTempo(1));
  await settle();
  act(() => { reportTempo(1); reportTempo(0.5); });
  expect(deck.tempo).toBe(1);
  await select("track-b");
  finish(loads[1]!);
  await settle();
  expect(tempoWrites.at(-1)).toBe(1);
  await act(async () => { finishReset(); await done(); });
});

it("leaves a newer tempo request in place when an older command fails", async () => {
  finish(loads[0]!);
  await settle();
  let rejectOlder!: (error: Error) => void;
  const older = new Promise<void>((_resolve, reject) => { rejectOlder = reject; });
  tempoResult = (value) => value === 0.5 ? older : done();
  act(() => deck.setTempo(0.5));
  await settle();
  act(() => deck.setTempo(1.25));
  await settle();
  await act(async () => { rejectOlder(new Error("older tempo failed")); await done(); });
  expect(deck.tempo).toBe(1.25);
  expect(deck.error).toBeNull();
  await select("track-b");
  finish(loads[1]!);
  await settle();
  expect(tempoWrites.at(-1)).toBe(1.25);
});

it("restores the observed tempo when the current tempo command fails", async () => {
  finish(loads[0]!);
  await settle();
  tempoResult = () => Promise.reject(new Error("tempo was refused"));
  act(() => deck.setTempo(0.5));
  await settle();
  expect(deck.tempo).toBe(1);
  expect(deck.error).toBe("tempo was refused");
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

  // A zero-frame playing tick must not start wall-clock interpolation. The
  // next paint can arrive before the deck has produced its first buffer.
  now += 400;
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
  // Track skipping asks for Play again while the new file opens. That must
  // preserve the pending intent rather than toggling it off with the UI.
  const unregister = registerDeck("b", {
    track: () => track, idle: () => deck.idle, playing: () => deck.wantsPlaying(),
    error: () => deck.error, togglePlay: () => deck.toggle(),
  });
  act(() => setScriptPlaying("b", true));
  unregister();
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


it("publishes only confirmed audio owners, retaining pause and refusing stale completions", async () => {
  expect(deck.loadedTrack?.[0] ?? null).toBeNull();
  const first = loads[0]!;
  finish(first); await settle();
  expect(deck.loadedTrack?.[0] ?? null).toBe(first.track);
  act(() => tick({ ...emptyTick, b: { ...emptyDeck, loadId: first.id, loaded: true, playing: false } }));
  expect(deck.loadedTrack?.[0] ?? null).toBe(first.track);
  await select("replacement");
  expect(deck.loadedTrack).toBeNull();
  finish(first); await settle();
  expect(deck.loadedTrack).toBeNull();
  const next = loads.at(-1)!;
  finish(next); await settle();
  expect(deck.loadedTrack?.[0] ?? null).toBe("replacement");
  act(() => loaded({ deck: "b", loadId: next.id, totalFrames: 0, sampleRate: 0, message: "decoder refused" }));
  expect(deck.loadedTrack?.[0] ?? null).toBeNull();
});


it("keeps the installed owner after an early missing-file refusal and clears it on unload/reset", async () => {
  const first = loads[0]!; finish(first); await settle();
  __setBackend({ ...backend(), deckLoad: () => Promise.reject(new Error("missing file")) });
  await select("missing"); await settle();
  expect(deck.loadedTrack?.[0] ?? null).toBe(first.track);
  act(() => tick({ ...emptyTick, b: { ...emptyDeck, loadId: first.id, loaded: true, playing: true } }));
  expect(deck.loadedTrack?.[0] ?? null).toBe(first.track);
  act(() => reset()); await settle();
  expect(deck.loadedTrack?.[0] ?? null).toBeNull();
});


it("clears a confirmed owner when the track is ejected", async () => {
  const first = loads[0]!; finish(first); await settle();
  expect(deck.loadedTrack?.[0] ?? null).toBe(first.track);
  await select(null); await settle();
  expect(deck.loadedTrack?.[0] ?? null).toBeNull();
});


it("publishes a new request identity when the same numeric row is loaded again", async () => {
  const first = loads[0]!; finish(first); await settle();
  expect(deck.loadedTrack?.[1]).toBe(first.id);
  act(() => reset()); await settle();
  expect(deck.loadedTrack).toBeNull();
  const restored = loads.at(-1)!; finish(restored); await settle();
  expect(deck.loadedTrack).toEqual([first.track, restored.id]);
  expect(restored.id).not.toBe(first.id);
});


it("retains the installed owner when a refused selection is retried after remount", async () => {
  const first = loads[0]!; finish(first); await settle();
  const actual: Tick = { ...emptyTick, b: { ...emptyDeck, loadId: first.id, loaded: true } };
  __setBackend({ ...backend(), deckState: () => Promise.resolve(actual), deckLoad: () => Promise.reject(new Error("missing file")) });
  await select("missing-remount"); await settle();
  expect(deck.loadedTrack).toEqual([first.track, first.id]);
  act(() => root.unmount());
  root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  await settle();
  expect(deck.loadedTrack).toEqual([first.track, first.id]);
  act(() => tick(actual));
  expect(deck.loadedTrack).toEqual([first.track, first.id]);
});


it("cannot restore an owner from a refused request arriving after a hidden-player reset", async () => {
  const first = loads[0]!; finish(first); await settle();
  let reject: (error: Error) => void = () => {};
  const actual: Tick = { ...emptyTick, b: { ...emptyDeck, loadId: first.id, loaded: true } };
  __setBackend({ ...backend(), deckState: () => Promise.resolve(actual),
    deckLoad: () => new Promise<void>((_, no) => { reject = no; }),
  });
  let visible = true;
  function Shell() { useLoadedTracks(); return visible ? <Probe /> : null; }
  await act(async () => { root.render(<Shell />); await Promise.resolve(); });
  track = "late-missing";
  await act(async () => { root.render(<Shell />); await Promise.resolve(); });
  visible = false;
  await act(async () => { root.render(<Shell />); await Promise.resolve(); });
  await act(async () => { reset(); await Promise.resolve(); });
  await act(async () => { reject(new Error("missing file")); await Promise.resolve(); });
  expect(confirmedLoads.get("b") ?? null).toBeNull();
});


it("ignores a deck snapshot that arrives after its Player unmounts and resets", async () => {
  const first = loads[0]!; finish(first); await settle();
  let answer: (value: Tick) => void = () => {};
  __setBackend({ ...backend(), deckState: () => new Promise<Tick>(done => { answer = done; }) });
  let visible = true;
  function Shell() { useLoadedTracks(); return visible ? <Probe /> : null; }
  await act(async () => { root.render(<Shell />); await done(); });
  visible = false;
  await act(async () => { root.render(<Shell />); await done(); });
  await act(async () => { reset(); await done(); });
  await act(async () => {
    answer({ ...emptyTick, b: { ...emptyDeck, loadId: first.id, loaded: true } });
    await done();
  });
  expect(confirmedLoads.get("b") ?? null).toBeNull();
});


it("restarts a deliberate same-track load while an unchanged render keeps its position", async () => {
  const first = loads[0]!; finish(first); await settle();
  act(() => deck.seek(19)); await settle();
  expect(deck.positionRef.current).toBe(19);
  await select(track); await settle();
  expect(loads).toHaveLength(1);
  expect(deck.positionRef.current).toBe(19);
  intent++;
  await select(track); await settle();
  expect(loads).toHaveLength(2);
  expect(tickSubscriptions).toBe(1);
  expect(loads[1]!.track).toBe(first.track);
  expect(loads[1]!.id).not.toBe(first.id);
  expect(deck.positionRef.current).toBe(0);
  finish(first); await settle();
  expect(deck.loadedTrack).toBeNull();
  finish(loads[1]!); await settle();
  expect(deck.loadedTrack).toEqual([first.track, loads[1]!.id]);
  expect(deck.playing).toBe(false);
});

it("keeps ordinary playing-load behavior on explicit reload and does not repeat it on remount", async () => {
  finish(loads[0]!); await settle();
  act(() => deck.toggle()); await settle();
  intent++;
  await select(track); await settle();
  const reloaded = loads.at(-1)!;
  expect(loads).toHaveLength(2);
  sent = [];
  finish(reloaded); await settle();
  expect(sent).toEqual(["play"]);
  const actual: Tick = { ...emptyTick, b: { ...emptyDeck, loadId: reloaded.id, loaded: true, frames: RATE * 23, totalFrames: RATE * 300 } };
  __setBackend({ ...backend(), deckState: () => Promise.resolve(actual) });
  act(() => root.unmount()); root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  await settle();
  expect(loads).toHaveLength(2);
  expect(deck.positionRef.current).toBe(23);
});


it("does not unload an already-ejected deck again when its view remounts after explicit loading", async () => {
  finish(loads[0]!); await settle();
  intent++; await select(track); await settle();
  finish(loads.at(-1)!); await settle();
  await select(null); await settle();
  expect(unloads).toBe(1);
  act(() => root.unmount()); root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  await settle();
  expect(unloads).toBe(1);
});


it("ignores an initial snapshot superseded by a newer explicit load in the mounted Player", async () => {
  const first = loads[0]!; finish(first); await settle();
  let answer: (value: Tick) => void = () => {};
  __setBackend({ ...backend(), deckState: () => new Promise<Tick>(done => { answer = done; }) });
  act(() => root.unmount()); root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  intent++; await select(track); await settle();
  const second = loads.at(-1)!; finish(second); await settle();
  await act(async () => {
    answer({ ...emptyTick, b: { ...emptyDeck, loadId: first.id, loaded: true } });
    await done();
  });
  expect(loads).toHaveLength(2);
  expect(deck.loadedTrack).toEqual([second.track, second.id]);
});

it("ignores a pre-completion snapshot when a pending explicit reload has already completed", async () => {
  finish(loads[0]!); await settle();
  let answer: (value: Tick) => void = () => {};
  __setBackend({ ...backend(), deckState: () => new Promise<Tick>(done => { answer = done; }) });
  intent++;
  act(() => root.unmount()); root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  const second = loads.at(-1)!; finish(second); await settle();
  await act(async () => { answer(emptyTick); await done(); });
  expect(loads).toHaveLength(2);
  expect(deck.loadedTrack).toEqual([second.track, second.id]);
});

it.each([0.8, 1, 1.2])("keeps pause/resume continuous between ticks at tempo %s", async (tempo) => {
  let now = 1_000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frame = callback;
    return 1;
  });
  finish(loads[0]!);
  await settle();
  const state = { ...emptyDeck, loadId: loads[0]!.id, loaded: true, playing: true,
    frames: 10 * RATE, totalFrames: 300 * RATE, generation: 1, tempo };
  act(() => tick({ ...emptyTick, b: state }));
  now += 80;
  act(() => frame?.(now));
  const before = deck.positionRef.current;
  expect(before).toBeCloseTo(10 + 0.08 * tempo);

  act(() => deck.toggle());
  await settle();
  expect(deck.playing).toBe(false);
  expect(deck.positionNow()).toBeCloseTo(before);
  now += 20;
  expect(deck.positionRef.current).toBe(before);
  act(() => deck.toggle());
  await settle();
  now += 16;
  act(() => frame?.(now));
  expect(sent).toEqual(["pause", "play"]);
  expect(deck.positionRef.current).toBeGreaterThanOrEqual(before);
  expect(deck.positionNow()).toBeCloseTo(before + 0.016 * tempo);

  // An intentional native seek remains authoritative, including backwards.
  act(() => tick({ ...emptyTick, b: { ...state, frames: 2 * RATE, generation: 2 } }));
  expect(deck.positionRef.current).toBe(2);
  expect(deck.positionNow()).toBe(2);
  // A later stopped tick owns the final paused position, rather than our
  // optimistic local transport anchor.
  act(() => tick({ ...emptyTick, b: { ...state, frames: 2.1 * RATE, generation: 2, playing: false } }));
  now += 100;
  expect(deck.playing).toBe(false);
  expect(deck.positionRef.current).toBeCloseTo(2.1);
  expect(deck.positionNow()).toBeCloseTo(2.1);
});


it("pins a confirmed-ready zero-frame start until its first advancing tick", async () => {
  let now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frame = callback; return 1; });
  finish(loads[0]!);
  await settle();
  const state = { ...emptyDeck, loadId: loads[0]!.id, loaded: true, frames: 0,
    generation: 1, totalFrames: RATE * 300 };
  act(() => tick({ ...emptyTick, b: state }));
  act(() => deck.toggle());
  await settle();
  expect(sent).toEqual(["play"]);
  now += 80;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);
  act(() => tick({ ...emptyTick, b: { ...state, playing: true } }));
  now += 16;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);
  act(() => tick({ ...emptyTick, b: { ...state, playing: true, frames: RATE / 10 } }));
  expect(deck.positionNow()).toBeCloseTo(0.1);
  now += 16;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBeGreaterThan(0);
});

it.each(["seek", "native generation", "stop", "scheduled play", "loop", "EOF"] as const)(
  "keeps %s authoritative around a ready start", async (control) => {
    let now = 1000;
    vi.spyOn(performance, "now").mockImplementation(() => now);
    let frame: FrameRequestCallback | undefined;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frame = callback; return 1; });
    finish(loads[0]!);
    await settle();
    const state = { ...emptyDeck, loadId: loads[0]!.id, loaded: true, generation: 1,
      totalFrames: RATE * 300 };
    act(() => tick({ ...emptyTick, b: state }));
    if (control === "scheduled play") {
      act(() => deck.playAfter(80));
      await settle();
      act(() => tick({ ...emptyTick, b: { ...state, playing: true, startInFrames: RATE * 0.08 } }));
      now += 40;
      act(() => frame?.(now));
      expect(deck.positionRef.current).toBe(0);
      now += 80;
      expect(deck.positionNow()).toBeCloseTo(0.04);
      expect(sent).toEqual(["play-after:80"]);
      return;
    }
    if (control === "loop") {
      act(() => deck.setLoop(0, 2));
      await settle();
      act(() => tick({ ...emptyTick, b: { ...state, loopOutFrames: RATE * 2, looping: true } }));
    } else if (control === "EOF") {
      act(() => tick({ ...emptyTick, b: { ...state, frames: RATE * 300 } }));
    }
    act(() => deck.toggle());
    await settle();
    if (control === "stop") {
      // Held CUE release seeks back and stops while no PCM has advanced.
      act(() => deck.seek(0));
      act(() => deck.toggle());
      await settle();
      act(() => tick({ ...emptyTick, b: state }));
      now += 100;
      act(() => frame?.(now));
      expect(deck.playing).toBe(false);
      expect(deck.positionRef.current).toBe(0);
      expect(sent).toEqual(["play", "seek:0", "pause"]);
      return;
    }
    if (control === "seek") {
      act(() => deck.seek(2));
      await settle();
      act(() => tick({ ...emptyTick, b: { ...state, frames: RATE * 2, generation: 2, playing: true } }));
      now += 80;
      expect(deck.positionNow()).toBeCloseTo(2.08);
      expect(sent).toEqual(["play", "seek:2000"]);
      return;
    }
    // A native seek/EOF restart is an intentional generation, not startup.
    act(() => tick({ ...emptyTick, b: { ...state, generation: control === "loop" ? 1 : 2,
      playing: true, loopOutFrames: control === "loop" ? RATE * 2 : 0, looping: control === "loop" } }));
    now += 80;
    expect(deck.positionNow()).toBeCloseTo(0.08);
    expect(sent).toEqual(["play"]);
  },
);


it("binds an event-first ready start to the new load's first tick generation", async () => {
  let now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frame = callback; return 1; });
  finish(loads[0]!);
  await settle();
  act(() => tick({ ...emptyTick, b: { ...emptyDeck, loadId: loads[0]!.id, loaded: true,
    generation: 7, totalFrames: RATE * 300 } }));
  await select("new-event-first-track");
  const request = loads[1]!;
  finish(request);
  await settle();
  act(() => deck.toggle());
  await settle();
  expect(sent).toEqual(["play"]);
  now += 80;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);
  // A late tick from the previous load cannot bind the new start's generation.
  act(() => tick({ ...emptyTick, b: { ...emptyDeck, loadId: loads[0]!.id, loaded: true,
    playing: true, generation: 7, frames: RATE * 12, totalFrames: RATE * 300 } }));
  expect(deck.positionNow()).toBe(0);
  const state = { ...emptyDeck, loadId: request.id, loaded: true, playing: true,
    generation: 8, totalFrames: RATE * 300 };
  act(() => tick({ ...emptyTick, b: state }));
  now += 16;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);
  act(() => tick({ ...emptyTick, b: { ...state, frames: RATE / 10 } }));
  expect(deck.positionNow()).toBeCloseTo(0.1);
  // Once bound, a genuinely newer generation is still an intentional seek.
  act(() => tick({ ...emptyTick, b: { ...state, generation: 9, frames: 0 } }));
  now += 80;
  expect(deck.positionNow()).toBeCloseTo(0.08);
});

it.each(["unloaded tick", "deck error"] as const)("releases a ready-start gate after %s", async (failure) => {
  finish(loads[0]!);
  await settle();
  const state = { ...emptyDeck, loadId: loads[0]!.id, loaded: true,
    generation: 1, totalFrames: RATE * 300 };
  act(() => tick({ ...emptyTick, b: state }));
  act(() => deck.toggle());
  await settle();
  if (failure === "deck error") {
    act(() => loaded({ deck: "b", loadId: loads[0]!.id, totalFrames: 0,
      sampleRate: RATE, message: "Generated decoder refusal" }));
    expect(deck.error).toBe("Generated decoder refusal");
  }
  act(() => tick({ ...emptyTick, b: { ...state, loaded: false, totalFrames: 0 } }));
  expect(deck.playing).toBe(false);
  expect(deck.duration).toBe(0);
});

it.each(["play", "pause", "scheduled"] as const)("ignores an older %s rejection after a newer transport intent", async (command) => {
  finish(loads[0]!);
  await settle();
  let rejectOld: (error: Error) => void = () => {};
  const old = new Promise<void>((_, reject) => { rejectOld = reject; });
  const next = backend();
  if (command === "pause") {
    act(() => deck.toggle());
    await settle();
    next.deckPause = () => old;
  } else if (command === "play") next.deckPlay = () => old;
  else next.deckPlayAfter = () => old;
  __setBackend(next);
  act(() => command === "scheduled" ? deck.playAfter(200) : deck.toggle());
  await settle();
  __setBackend(backend());
  if (command !== "pause") {
    act(() => deck.toggle());
    await settle();
  }
  act(() => deck.toggle());
  await settle();
  expect(deck.playing).toBe(true);
  await act(async () => { rejectOld(new Error("obsolete transport failure")); await done(); });
  expect(deck.playing).toBe(true);
  expect(deck.error).toBeNull();
});

it.each([false, true])("ignores an old load rejection with replacement Play=%s and preserves its first-frame wait", async (playFirst) => {
  let now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frame = callback; return 1; });
  finish(loads[0]!);
  await settle();
  let rejectOld: (error: Error) => void = () => {};
  __setBackend({ ...backend(), deckPlay: () => new Promise<void>((_, reject) => { rejectOld = reject; }) });
  act(() => deck.toggle());
  await settle();
  // Unload resets playback intent without a newer Play/Pause version.
  __setBackend(backend());
  await select(null);
  await settle();
  await select("replacement-transport-owner");
  finish(loads.at(-1)!);
  await settle();
  if (playFirst) {
    act(() => deck.toggle());
    await settle();
  }
  await act(async () => { rejectOld(new Error("old load play failed")); await done(); });
  expect(deck.playing).toBe(playFirst);
  expect(deck.error).toBeNull();
  if (!playFirst) {
    act(() => deck.toggle());
    await settle();
  }
  now += 80;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);
  act(() => tick({ ...emptyTick, b: { ...emptyDeck, loadId: loads.at(-1)!.id, loaded: true, playing: true, totalFrames: RATE * 300 } }));
  now += 80;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBe(0);
  act(() => tick({ ...emptyTick, b: { ...emptyDeck, loadId: loads.at(-1)!.id, loaded: true, playing: true, frames: RATE / 10, totalFrames: RATE * 300 } }));
  expect(deck.positionNow()).toBeCloseTo(0.1);
});

it.each(["play", "pause", "scheduled"] as const)("still reports the current %s failure", async (command) => {
  finish(loads[0]!);
  await settle();
  if (command === "pause") {
    act(() => deck.toggle());
    await settle();
  }
  const next = backend();
  const fail = () => Promise.reject(new Error("current transport failure"));
  if (command === "pause") next.deckPause = fail;
  else if (command === "play") next.deckPlay = fail;
  else next.deckPlayAfter = fail;
  __setBackend(next);
  act(() => command === "scheduled" ? deck.playAfter(200) : deck.toggle());
  await settle();
  expect(deck.playing).toBe(false);
  expect(deck.error).toBe("current transport failure");
});


it.each([0, 200])("honors a ready scheduled start of %d ms before its first native tick", async (wait) => {
  let now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frame = callback; return 1; });
  finish(loads[0]!);
  await settle();
  const state = { ...emptyDeck, loadId: loads[0]!.id, loaded: true, generation: 8,
    frames: RATE * 10, totalFrames: RATE * 300 };
  act(() => tick({ ...emptyTick, b: state }));
  expect(deck.positionRef.current).toBe(10);
  act(() => deck.playAfter(wait));
  await settle();
  expect(sent.at(-1)).toBe(`play-after:${wait}`);
  now += 80;
  act(() => frame?.(now));
  if (wait === 0) expect(deck.positionRef.current).toBeGreaterThan(10);
  else expect(deck.positionRef.current).toBe(10);
});

it("honors native remaining-start frames, then accepts stalled and advancing authority", async () => {
  let now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frame = callback; return 1; });
  finish(loads[0]!);
  await settle();
  const state = { ...emptyDeck, loadId: loads[0]!.id, loaded: true, generation: 8,
    frames: RATE * 10, totalFrames: RATE * 300 };
  act(() => tick({ ...emptyTick, b: state }));
  act(() => deck.playAfter(200));
  await settle();
  act(() => tick({ ...emptyTick, b: { ...state, playing: true, startInFrames: RATE / 5 } }));
  for (const instant of [1080, 1160, 1199]) {
    now = instant;
    act(() => frame?.(now));
    expect(deck.positionRef.current).toBe(10);
  }
  // Once the scheduled wait expires the existing clock may interpolate;
  // this is intentionally not the newly-loaded first-audio-frame gate.
  now = 1216;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBeGreaterThan(10);
  act(() => tick({ ...emptyTick, b: { ...state, playing: true } }));
  expect(deck.positionNow()).toBe(10);
  act(() => tick({ ...emptyTick, b: { ...state, playing: true, frames: RATE * 10.04 } }));
  expect(deck.positionNow()).toBeCloseTo(10.04);
});

it("cancels a scheduled deadline when Pause is followed by immediate ordinary Play", async () => {
  let now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let frame: FrameRequestCallback | undefined;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frame = callback; return 1; });
  finish(loads[0]!);
  await settle();
  const state = { ...emptyDeck, loadId: loads[0]!.id, loaded: true, generation: 8,
    frames: RATE * 10, totalFrames: RATE * 300 };
  act(() => tick({ ...emptyTick, b: state }));
  act(() => deck.playAfter(200));
  await settle();
  now = 1050;
  act(() => deck.toggle());
  await settle();
  now = 1070;
  act(() => deck.toggle());
  await settle();
  expect(sent.slice(-3)).toEqual(["play-after:200", "pause", "play"]);
  now = 1086;
  act(() => frame?.(now));
  expect(deck.positionRef.current).toBeCloseTo(10.016, 8);
});


it.each(["another", "empty", "intent"] as const)("resets the committed selection during a suspended %s transition", async (attempt) => {
  const original = track!;
  const blocked = new Promise<void>(() => undefined);
  let attempted = false;
  function ConcurrentProbe({ id, revision, suspend = false }: { id: string | null; revision: number; suspend?: boolean }) {
    const current = usePlayback(id, "b", true, revision);
    if (suspend) {
      attempted = true;
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- React Suspense throws a pending promise.
      throw blocked;
    }
    deck = current;
    return <span>{id ?? "empty"}</span>;
  }
  await act(async () => { root.render(<Suspense fallback={<span>Waiting</span>}><ConcurrentProbe id={original} revision={0} /></Suspense>); await done(); });
  finish(loads.at(-1)!);
  await settle();
  const before = loads.length;
  await act(async () => {
    startTransition(() => root.render(<Suspense fallback={<span>Waiting</span>}><ConcurrentProbe id={attempt === "another" ? "uncommitted-track" : attempt === "empty" ? null : original} revision={1} suspend /></Suspense>));
    await done();
  });
  expect(attempted).toBe(true);
  expect(host.textContent).toBe(original);
  act(() => reset());
  await settle();
  expect(loads.slice(before).map(request => request.track)).toEqual([original]);
  await act(async () => { root.render(<Suspense fallback={<span>Waiting</span>}><ConcurrentProbe id={original} revision={0} /></Suspense>); await done(); });
  expect(loads).toHaveLength(before + 1);
  const restored = loads.at(-1)!;
  finish(restored);
  await settle();
  engineState = { ...emptyTick, b: { ...emptyDeck, loaded: true, loadId: restored.id, totalFrames: RATE * 300 } };
  await act(async () => { root.render(<span>Hidden</span>); await done(); });
  const hiddenCount = loads.length;
  act(() => reset());
  await settle();
  expect(loads).toHaveLength(hiddenCount);
  await act(async () => { root.render(<Suspense fallback={<span>Waiting</span>}><ConcurrentProbe id={original} revision={0} /></Suspense>); await done(); });
  expect(loads).toHaveLength(before + 1);

});


it("does not reset the previous deck owner during a committed deck-route change", async () => {
  const original = track!;
  function RouteProbe({ id, channel, fire = false }: { id: string | null; channel: "a" | "b"; fire?: boolean }) {
    deck = usePlayback(id, channel);
    useLayoutEffect(() => { if (fire) reset(); }, [fire]);
    return <span>{id ?? "empty"}</span>;
  }
  await act(async () => { root.render(<RouteProbe id={original} channel="a" />); await done(); });
  const before = loads.length;
  await act(async () => { root.render(<RouteProbe id="route-B" channel="b" fire />); await done(); });
  expect(loads.slice(before).map(({ deck, track }) => ({ deck, track }))).toEqual([{ deck: "b", track: "route-B" }]);
  await act(async () => { root.render(<RouteProbe id={null} channel="b" />); await done(); });
  const removedCount = loads.length;
  act(() => reset());
  await settle();
  expect(loads).toHaveLength(removedCount);
  const retired = [...resetListeners];
  await act(async () => { root.render(<span>Hidden</span>); await done(); });
  act(() => { for (const listener of retired) listener(); });
  await settle();
  expect(loads).toHaveLength(removedCount);
});

it("restores the real committed track and intent once after a StrictMode reset", async () => {
  await act(async () => { root.render(<StrictMode><Probe /></StrictMode>); await done(); });
  const before = loads.length;
  expect(resetListeners.size).toBe(1);
  act(() => reset());
  await settle();
  expect(loads.slice(before).map(request => request.track)).toEqual([track]);
  const restored = loads.at(-1)!;
  finish(restored);
  await settle();
  engineState = { ...emptyTick, b: { ...emptyDeck, loaded: true, loadId: restored.id, totalFrames: RATE * 300 } };
  await act(async () => { root.render(<span>Hidden</span>); await done(); });
  await act(async () => { root.render(<StrictMode><Probe /></StrictMode>); await done(); });
  expect(loads).toHaveLength(before + 1);
});

it("reconciles a real held-load mismatch for a newly committed selection and keeps its intent", async () => {
  const first = loads[0]!;
  finish(first); await settle();
  let answer: (value: Tick) => void = () => {};
  __setBackend({ ...backend(), deckState: () => new Promise<Tick>(resolve => { answer = resolve; }) });
  act(() => root.unmount()); root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  intent++;
  await select("committed-B");
  const second = loads.at(-1)!;
  finish(second); await settle();
  const oldSnapshot: Tick = { ...emptyTick, b: { ...emptyDeck, loaded: true, loadId: first.id } };
  await act(async () => { answer(oldSnapshot); await done(); });
  expect(loads).toHaveLength(2);
  expect(deck.loadedTrack).toEqual(["committed-B", second.id]);

  // A real remount whose current engine holds another load must still
  // restore the newly committed B; the old snapshot above was only stale.
  __setBackend({ ...backend(), deckState: () => Promise.resolve(oldSnapshot) });
  act(() => root.unmount()); root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  expect(loads).toHaveLength(3);
  const restored = loads.at(-1)!;
  expect(restored.track).toBe("committed-B");
  finish(restored); await settle();
  engineState = { ...emptyTick, b: { ...emptyDeck, loaded: true, loadId: restored.id } };
  __setBackend(backend());
  act(() => root.unmount()); root = createRoot(host);
  await act(async () => { root.render(<Probe />); await done(); });
  expect(loads).toHaveLength(3);
  expect(deck.loadedTrack).toEqual(["committed-B", restored.id]);
});
