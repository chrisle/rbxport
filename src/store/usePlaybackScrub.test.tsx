/**
 * @vitest-environment jsdom
 *
 * What the playhead does while a waveform is being dragged.
 *
 * The deck's read head is not under the pointer — it is rate-limited so a drag
 * stays audible, so it trails a fast hand and comes to rest about a block past
 * a still one. Taking a tick as the anchor mid-drag, and then extrapolating it
 * at playback speed, drew the waveform creeping forward and snapping back ten
 * times a second under a hand that was holding still. These mount the hook
 * against a stub deck and drive the ticks and the frames by hand.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, DeckEvent, Tick } from "@/ipc/types";
import { usePlayback, type Playback } from "./usePlayback";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

const RATE = 44_100;

/** A tick as the engine sends it: deck A only, deck B idle. */
function tickAt(seconds: number, generation: number, playing = true, tempo = 1): Tick {
  const deck = {
    frames: seconds * RATE,
    totalFrames: 300 * RATE,
    generation,
    playing,
    loaded: true,
    tempo,
    masterTempo: false,
    keyShift: 0,
    startInFrames: 0, loopInFrames: 0, loopOutFrames: 0, looping: false,
  };
  return {
    a: deck,
    b: {
      frames: 0, totalFrames: 0, generation: 0, playing: false, loaded: false,
      tempo: 1, masterTempo: false, keyShift: 0, startInFrames: 0, loopInFrames: 0, loopOutFrames: 0, looping: false,
    },
    sampleRate: RATE,
    peakLeft: 0,
    peakRight: 0,
    master: 1,
    reduction: 0,
    shiftsKey: true,
  };
}

let host: HTMLDivElement;
let root: Root;
let deck: Playback;
/** Every tick listener the hook has registered. */
let ticked: (tick: Tick) => void;
/** What the deck was told, in order, so a drag's commands can be read back. */
let sent: string[];
/** Pending animation frames, run only when a test says so. */
let frames: (() => void)[];

/** A resolved promise, for the commands that answer nothing. */
const done = () => Promise.resolve();

/** A deck that records what it is asked to do and reports nothing back. */
function stubBackend(): Backend {
  const nothing = done;
  return {
    deckState: () => Promise.resolve(tickAt(1, 1)),
    onDeckTick: (listener: (tick: Tick) => void) => {
      ticked = listener;
      return () => {};
    },
    onDeckEvent: (_listener: (event: DeckEvent) => void) => () => {},
    deckLoad: (_d: string, id: string) => {
      sent.push(`load:${id}`);
      return done();
    },
    deckUnload: nothing,
    deckPlay: nothing,
    deckPause: nothing,
    deckSeek: (_d: string, ms: number) => {
      sent.push(`seek:${ms}`);
      return done();
    },
    deckScrubBegin: () => {
      sent.push("begin");
      return done();
    },
    deckScrubTo: (_d: string, ms: number) => {
      sent.push(`to:${ms}`);
      return done();
    },
    deckScrubEnd: () => {
      sent.push("end");
      return done();
    },
  } as unknown as Backend;
}

/** Mounts the hook and hands the test what it returned. */
let renderPosition = true;
let renders = 0;
function Probe() {
  renders++;
  deck = usePlayback("track-1", "a", renderPosition);
  return null;
}

beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  sent = [];
  renderPosition = true;
  renders = 0;
  frames = [];
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
    frames.push(() => cb(performance.now()));
    return frames.length;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => {
    frames[id - 1] = () => {};
  });
  __setBackend(stubBackend());
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<Probe />);
    await done();
  });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
  vi.unstubAllGlobals();
});

/** Runs whatever frames are queued, the way the browser would.  */
function runFrames(count = 1) {
  for (let i = 0; i < count; i++) {
    const queued = frames;
    frames = [];
    act(() => {
      for (const frame of queued) frame();
    });
  }
}

/** A tick from the engine, delivered the way the event bridge would. */
function deliver(tick: Tick) {
  act(() => ticked(tick));
}

it("can isolate position ticks while still publishing paused seeks to subscribers", async () => {
  renderPosition = false;
  await act(async () => { root.render(<Probe />); await done(); });
  deliver(tickAt(2, 1));
  deliver(tickAt(3, 1));
  const before = renders;
  const snapshot = deck;
  deliver(tickAt(4, 1));
  deliver(tickAt(5, 1));
  expect(renders).toBe(before);
  expect(deck).toBe(snapshot);
  const listener = vi.fn();
  const unsubscribe = deck.subscribe(listener);
  deliver(tickAt(12.5, 2, false));
  expect(deck.positionRef.current).toBe(12.5);
  expect(listener).toHaveBeenCalledWith(12.5);
  deliver(tickAt(15, 2, false));
  expect(listener).toHaveBeenCalledWith(15);
  unsubscribe();
});

/** Lets the commands a drag fires off actually reach the stub deck. */
async function settle() {
  await act(async () => {
    await done();
  });
}

describe("a drag on a playing deck", () => {
  it.each([0.5, 1, 1.25])("keeps playback at %s speed on the audio timeline instead of easing behind it", (tempo) => {
    let clock = performance.now();
    const now = vi.spyOn(performance, "now").mockImplementation(() => clock);
    try {
      deliver(tickAt(1, 2, true, tempo));
      runFrames();
      for (let frame = 1; frame <= 120; frame++) {
        clock += 1000 / 60;
        if (frame % 6 === 0) deliver(tickAt(1 + frame * tempo / 60, 2, true, tempo));
        runFrames();
      }
      expect(deck.positionRef.current).toBeCloseTo(1 + 2 * tempo, 6);
    } finally {
      now.mockRestore();
    }
  });

  it("does not advance before a scheduled audio start", () => {
    let clock = performance.now();
    const now = vi.spyOn(performance, "now").mockImplementation(() => clock);
    try {
      const tick = tickAt(1, 2);
      tick.a.startInFrames = RATE / 10;
      deliver(tick);
      runFrames();
      clock += 50;
      runFrames();
      expect(deck.positionRef.current).toBeCloseTo(1, 6);
      clock += 75;
      runFrames();
      expect(deck.positionRef.current).toBeCloseTo(1.025, 6);
    } finally {
      now.mockRestore();
    }
  });
  it("leaves the playhead where the hand left it, tick after tick", () => {
    deliver(tickAt(1, 1));
    act(() => deck.scrubBegin());
    act(() => deck.scrubTo(10));
    expect(deck.positionRef.current).toBe(10);

    // The head is behind the pointer, and the drag bumped a generation of its
    // own. Neither is a reason to move what is drawn.
    for (let pass = 0; pass < 5; pass++) {
      deliver(tickAt(9.5, 2));
      runFrames(3);
      expect(deck.positionRef.current).toBeCloseTo(10, 6);
    }
  });

  it("does not creep forward between pointer moves", () => {
    // The other half, and the one a real hand feels: a pinned anchor is not
    // extrapolated, so a second of frames with no pointer move draws the same
    // position a second later. Unpinned it ran a second down the track.
    let clock = performance.now();
    const now = vi.spyOn(performance, "now").mockImplementation(() => clock);
    try {
      deliver(tickAt(1, 1));
      act(() => deck.scrubBegin());
      act(() => deck.scrubTo(30));
      for (let frame = 0; frame < 60; frame++) {
        clock += 16.7;
        runFrames(1);
      }
      expect(deck.positionRef.current).toBeCloseTo(30, 6);
    } finally {
      now.mockRestore();
    }
  });

  it("follows the pointer while it does move", async () => {
    deliver(tickAt(1, 1));
    act(() => deck.scrubBegin());
    for (const at of [2, 4, 6]) {
      act(() => deck.scrubTo(at));
      runFrames(2);
      await settle();
      expect(deck.positionRef.current).toBeCloseTo(at, 6);
    }
    // And the moves reached the deck, coalesced to the frames they were sent in.
    expect(sent.filter((s) => s.startsWith("to:"))).toEqual(["to:2000", "to:4000", "to:6000"]);
  });
});

describe("letting go", () => {
  it("holds where the drag ended until the seek it asked for lands", async () => {
    deliver(tickAt(1, 1));
    act(() => deck.scrubBegin());
    act(() => deck.scrubTo(20));
    act(() => deck.scrubEnd());
    await settle();
    expect(sent).toContain("end");

    // A tick sent before the seek was processed still carries the mid-drag
    // head. Taking it would spring the playhead back at the end of every drag.
    deliver(tickAt(19.4, 2));
    runFrames(2);
    expect(deck.positionRef.current).toBeCloseTo(20, 6);

    // The tick that agrees with where the drag ended is the seek landing, and
    // the deck is believed again from there.
    deliver(tickAt(20, 3));
    expect(deck.positionRef.current).toBeCloseTo(20, 2);
  });

  it("gives up holding rather than freezing if no seek ever lands", async () => {
    // A deck unloaded mid-drag never seeks, so the generation never moves. The
    // hold is bounded in time as well, or the playhead would stop for good.
    let clock = performance.now();
    const now = vi.spyOn(performance, "now").mockImplementation(() => clock);
    try {
      deliver(tickAt(1, 1));
      act(() => deck.scrubBegin());
      act(() => deck.scrubTo(20));
      act(() => deck.scrubEnd());
      await settle();
      deliver(tickAt(19.4, 2));
      expect(deck.positionRef.current).toBeCloseTo(20, 6);
      clock += 2_000;
      // Believed again: the readout follows the deck rather than the drag that
      // never finished.
      deliver(tickAt(5, 2, false));
      expect(deck.position).toBeCloseTo(5, 6);
    } finally {
      now.mockRestore();
    }
  });
});


it("holds a negative scrub position, clamps at minus five seconds, and sends the landing", async () => {
  deliver(tickAt(1, 2, false));
  act(() => { deck.scrubBegin(); deck.scrubTo(-2.5); });
  expect(deck.positionRef.current).toBe(-2.5);
  deliver(tickAt(0, 3, false));
  expect(deck.positionRef.current).toBe(-2.5);
  act(() => deck.scrubTo(-9));
  expect(deck.positionRef.current).toBe(-5);
  act(() => deck.scrubEnd());
  await settle();
  expect(sent).toContain("to:-5000");
  expect(sent.at(-1)).toBe("end");
  deliver(tickAt(-5, 4, false));
  expect(deck.positionRef.current).toBe(-5);
});

it("lands a drag where the snap puts it, and sends that landing", async () => {
  deliver(tickAt(10, 2, true));
  act(() => { deck.scrubBegin(); deck.scrubTo(20); });
  let given: number | null = null;
  act(() => deck.scrubEnd((at) => { given = at; return at + 0.25; }));
  await settle();
  expect(given).toBe(20);
  expect(deck.positionRef.current).toBe(20.25);
  expect(sent).toContain("to:20250");
  expect(sent).not.toContain("to:20000");
  expect(sent.at(-1)).toBe("end");
});
