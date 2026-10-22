/**
 * @vitest-environment jsdom
 *
 * The deck's loop: what the controls send down, and what a tick draws.
 *
 * The loop is the engine's, not the interface's — `loop` is read back off
 * the tick, so what is drawn is what sounds. That makes two things worth
 * pinning here. One is the guard on `setLoop`: an out point at or before
 * the in point is not a loop, and the engine is never asked for one. The
 * other is that an unchanged loop must come back as the same object,
 * because a tick arrives thirty times a second and every reader of `loop`
 * would re-render on each one otherwise.
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

/** An idle deck, for the side of the tick a test is not looking at. */
const IDLE = {
  frames: 0, totalFrames: 0, generation: 0, playing: false, loaded: false,
  tempo: 1, masterTempo: false, keyShift: 0,
  startInFrames: 0, loopInFrames: 0, loopOutFrames: 0, looping: false,
};

/** A tick with deck A holding the given loop, in seconds. */
function tickWithLoop(loop: { inSeconds: number; outSeconds: number; active: boolean } | null): Tick {
  return {
    a: {
      ...IDLE,
      loaded: true,
      playing: true,
      totalFrames: 300 * RATE,
      generation: 1,
      loopInFrames: (loop?.inSeconds ?? 0) * RATE,
      loopOutFrames: (loop?.outSeconds ?? 0) * RATE,
      looping: loop?.active ?? false,
    },
    b: { ...IDLE },
    sampleRate: RATE,
    peakLeft: 0, peakRight: 0, master: 1, reduction: 0, shiftsKey: true,
  };
}

let host: HTMLDivElement;
let root: Root;
let deck: Playback;
/** Every tick listener the hook has registered. */
let ticked: (tick: Tick) => void;
/** What the deck was told, in order. */
let sent: string[];
/** The track the probe mounts with, so a test can mount an empty deck. */
let mountedTrack: string | null;
/** What the loop commands answer with, so a test can make one fail. */
let answer: () => Promise<void>;

const done = () => Promise.resolve();

function stubBackend(): Backend {
  return {
    deckState: () => Promise.resolve(tickWithLoop(null)),
    onDeckTick: (listener: (tick: Tick) => void) => {
      ticked = listener;
      return () => {};
    },
    onDeckEvent: (_listener: (event: DeckEvent) => void) => () => {},
    deckLoad: () => done(),
    deckUnload: () => done(),
    deckPlay: () => done(),
    deckPause: () => done(),
    deckSeek: () => done(),
    deckSetLoop: (_d: string, inMs: number, outMs: number) => {
      sent.push(`set:${inMs}:${outMs}`);
      return answer();
    },
    deckMoveLoop: (_d: string, inMs: number, outMs: number, byMs: number, grid: number[], beats: number) => {
      sent.push(`move:${inMs}:${outMs}:${byMs}:${grid.join(",")}:${beats}`);
      return answer();
    },
    deckLoopActive: (_d: string, on: boolean) => {
      sent.push(`active:${on}`);
      return answer();
    },
    deckClearLoop: () => {
      sent.push("clear");
      return answer();
    },
  } as unknown as Backend;
}

function Probe() {
  deck = usePlayback(mountedTrack, "a");
  return null;
}

async function mount() {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<Probe />);
    await done();
  });
}

beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  sent = [];
  mountedTrack = "track-1";
  answer = done;
  vi.stubGlobal("requestAnimationFrame", () => 1);
  vi.stubGlobal("cancelAnimationFrame", () => {});
  __setBackend(stubBackend());
  await mount();
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
  vi.unstubAllGlobals();
});

/** Lets a command actually reach the stub deck. */
async function settle() {
  await act(async () => {
    await done();
  });
}

const deliver = (tick: Tick) => act(() => ticked(tick));

describe("setting a loop", () => {
  it("sends one coordinated move for a playing loop and leaves it active", async () => {
    act(() => deck.setLoop(10, 12));
    await settle();
    act(() => deck.moveLoop(12, 14, 2, [10_000, 10_500], 4));
    await settle();
    expect(sent).toEqual(["set:10000:12000", "move:12000:14000:2000:10000,10500:4"]);
    expect(deck.loop).toEqual({ inSeconds: 12, outSeconds: 14, active: true });
  });

  it("delivers repeated loop moves in order", async () => {
    act(() => deck.setLoop(10, 12));
    act(() => deck.moveLoop(12, 14, 2, [10_000, 10_500], 4));
    act(() => deck.moveLoop(14, 16, 2, [12_000, 12_500], 4));
    await settle();
    expect(sent).toEqual([
      "set:10000:12000",
      "move:12000:14000:2000:10000,10500:4",
      "move:14000:16000:2000:12000,12500:4",
    ]);
    expect(deck.loop).toEqual({ inSeconds: 14, outSeconds: 16, active: true });
  });
  it("sends the two points down in milliseconds", async () => {
    act(() => deck.setLoop(1.5, 3.25));
    await settle();
    expect(sent).toEqual(["set:1500:3250"]);
  });

  it("clamps an in point before the start of the track", async () => {
    // A loop taken from a memory cue at 0 with a hand-dragged in point can
    // run negative; the engine has no frames there.
    act(() => deck.setLoop(-2, 4));
    await settle();
    expect(sent).toEqual(["set:0:4000"]);
  });

  it("refuses a loop that is not one, without troubling the engine", async () => {
    act(() => deck.setLoop(4, 4));
    act(() => deck.setLoop(5, 2));
    await settle();
    expect(sent).toEqual([]);
  });

  it("refuses points that are not numbers", async () => {
    // A loop length divided by a zero tempo, and an unparsed field.
    act(() => deck.setLoop(1, Number.POSITIVE_INFINITY));
    act(() => deck.setLoop(Number.NaN, 4));
    await settle();
    expect(sent).toEqual([]);
  });
});

describe("RELOOP, EXIT and clearing", () => {
  it("turns the loop on and off without changing its points", async () => {
    act(() => deck.setLoopActive(true));
    act(() => deck.setLoopActive(false));
    await settle();
    expect(sent).toEqual(["active:true", "active:false"]);
  });

  it("clears the loop outright", async () => {
    act(() => deck.clearLoop());
    await settle();
    expect(sent).toEqual(["clear"]);
  });
});

describe("an empty deck", () => {
  it("sends nothing, because there is nothing to loop", async () => {
    act(() => root.unmount());
    host.remove();
    mountedTrack = null;
    await mount();

    act(() => deck.setLoop(1, 2));
    act(() => deck.setLoopActive(true));
    act(() => deck.clearLoop());
    await settle();
    expect(sent).toEqual([]);
  });
});

describe("a loop command that fails", () => {
  it("is reported as a deck error rather than thrown", async () => {
    answer = () => Promise.reject(new Error("no loop past the end of the track"));
    act(() => deck.setLoop(1, 2));
    await settle();
    expect(deck.error).toBe("no loop past the end of the track");
  });
});

describe("the loop the tick reports", () => {
  it("is what the engine says, in seconds", () => {
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 6, active: true }));
    expect(deck.loop).toEqual({ inSeconds: 2, outSeconds: 6, active: true });
  });

  it("is null when the engine holds no loop", () => {
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 6, active: true }));
    deliver(tickWithLoop(null));
    expect(deck.loop).toBeNull();
  });

  it("is null for an out point that does not follow the in point", () => {
    // An engine that has an in point and no out yet is not looping.
    deliver(tickWithLoop({ inSeconds: 4, outSeconds: 4, active: false }));
    expect(deck.loop).toBeNull();
  });

  it("keeps EXIT apart from RELOOP: the range stays, the flag drops", () => {
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 6, active: true }));
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 6, active: false }));
    expect(deck.loop).toEqual({ inSeconds: 2, outSeconds: 6, active: false });
  });

  it("is the same object tick after tick while the loop is unchanged", () => {
    // Thirty ticks a second: a new object each time would re-render every
    // reader of `loop` for a loop that never moved.
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 6, active: true }));
    const first = deck.loop;
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 6, active: true }));
    expect(deck.loop).toBe(first);
  });

  it("is a new object once any of the three changes", () => {
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 6, active: true }));
    const first = deck.loop;
    deliver(tickWithLoop({ inSeconds: 2, outSeconds: 8, active: true }));
    expect(deck.loop).not.toBe(first);
    expect(deck.loop).toEqual({ inSeconds: 2, outSeconds: 8, active: true });
  });
});
