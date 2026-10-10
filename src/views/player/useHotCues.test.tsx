/**
 * @vitest-environment jsdom
 *
 * What a hot cue pad does when it is pressed, and where the cue lands.
 *
 * The pad has one decision in it — a set slot is called, an empty one is set —
 * and everything else follows from that. The decision is worth a test because
 * getting it the other way round overwrites a cue the DJ set earlier, which is
 * not something an undo would get back: the writer would have soft-deleted the
 * old row. The other half is quantize. With Q on the cue has to land on the
 * grid rather than where the finger was, because every loop and every mix made
 * from that cue inherits the few tens of milliseconds it is off by.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, Cue, CueKind } from "@/ipc/types";
import type { BeatGrid } from "@/lib/player";
import { useHotCues, type HotCueActions, type HotCueDeck } from "./useHotCues";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let pads: HotCueActions;
/** Every cue command the backend was given, in order. */
let sent: string[];
let seeked: number[];
/** What the pads did to the deck, in order: `seek:<s>` and `play`. */
let transport: string[];

const hot = (id: string, letter: string, positionMs: number): Cue => ({
  id, positionMs, outMs: 0, letter, memory: false, colour: null,
});
const memory = (id: string, positionMs: number): Cue => ({
  id, positionMs, outMs: 0, letter: "", memory: true, colour: null,
});

/** Four bars at 120 BPM: a beat every 500 ms. */
const GRID: BeatGrid = {
  times: new Uint32Array([0, 500, 1000, 1500, 2000]),
  numbers: new Uint8Array([1, 2, 3, 4, 1]),
  tempos: new Uint16Array([12_000, 12_000, 12_000, 12_000, 12_000]),
};

function stubBackend(): Backend {
  return {
    edits: {
      addCue: (track: string, kind: CueKind, positionMs: number) => {
        const slot = typeof kind === "string" ? kind : kind.hot;
        sent.push(`add:${track}:${slot}:${positionMs}`);
        return Promise.resolve("new-cue");
      },
      deleteCue: (cue: string) => {
        sent.push(`delete:${cue}`);
        return Promise.resolve();
      },
      moveHotCue: (cue: string, letter: string) => {
        sent.push(`slot:${cue}:${letter}`);
        return Promise.resolve();
      },
    },
  } as unknown as Backend;
}

function Probe({ deck }: { deck: HotCueDeck }) {
  pads = useHotCues(deck);
  return null;
}

/** Mounts the pads against a deck, filling in the parts a test does not care about. */
function mount(deck: Partial<HotCueDeck> = {}) {
  const full: HotCueDeck = {
    trackId: "track-1",
    cues: [],
    positionSeconds: () => 0,
    seek: (seconds: number) => {
      seeked.push(seconds);
      transport.push(`seek:${seconds}`);
    },
    play: () => transport.push("play"),
    quantiseTo: null,
    readOnly: false,
    ...deck,
  };
  act(() => root.render(<Probe deck={full} />));
}

const settle = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  sent = [];
  seeked = [];
  transport = [];
  __setBackend(stubBackend());
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("pressing an empty pad", () => {
  it("sets the cue at the playhead, rounded to the millisecond", async () => {
    mount({ positionSeconds: () => 1.2347 });
    act(() => pads.press("A"));
    await settle();
    expect(sent).toEqual(["add:track-1:A:1235"]);
    // Setting a cue is not a jump: the playhead was already there. Nor does
    // it start a stopped deck; only calling a set cue plays.
    expect(seeked).toEqual([]);
    expect(transport).toEqual([]);
  });

  it("snaps to the nearest beat when quantize is on", async () => {
    // 1.2 s sits 200 ms past the beat at 1000 and 300 ms short of the one at
    // 1500, so it belongs to the earlier one.
    mount({ positionSeconds: () => 1.2, quantiseTo: GRID });
    act(() => pads.press("A"));
    await settle();
    expect(sent).toEqual(["add:track-1:A:1000"]);
  });

  it("snaps forward when the later beat is the nearer one", async () => {
    mount({ positionSeconds: () => 1.4, quantiseTo: GRID });
    act(() => pads.press("B"));
    await settle();
    expect(sent).toEqual(["add:track-1:B:1500"]);
  });

  it("clamps a playhead before the start of the track to zero", async () => {
    // The deck can report a negative position mid-scrub; a cue at -3 s is not
    // a position the backend could store.
    mount({ positionSeconds: () => -3 });
    act(() => pads.press("A"));
    await settle();
    expect(sent).toEqual(["add:track-1:A:0"]);
  });

  it("does nothing while the library is read-only", async () => {
    mount({ positionSeconds: () => 1, readOnly: true });
    expect(pads.canEdit).toBe(false);
    act(() => pads.press("A"));
    await settle();
    expect(sent).toEqual([]);
  });

  it("does nothing with no track loaded", async () => {
    mount({ trackId: null, positionSeconds: () => 1 });
    expect(pads.canEdit).toBe(false);
    act(() => pads.press("A"));
    await settle();
    expect(sent).toEqual([]);
  });
});

describe("pressing a pad that is already set", () => {
  const cues = [hot("cue-a", "A", 30_000), memory("m1", 5_000)];

  it("calls the cue and writes nothing over it", async () => {
    mount({ cues, positionSeconds: () => 1 });
    act(() => pads.press("A"));
    await settle();
    expect(seeked).toEqual([30]);
    expect(sent).toEqual([]);
  });

  it("plays from the cue, so a paused deck starts there", async () => {
    // rekordbox 7 manual, EXPORT mode, "Calling and playing saved hot cue
    // points" (p.102): "Select a hot cue point. Playback starts from the
    // selected hot cue point." Gate Cue, the one exception, is not offered.
    // The jump comes first, so the deck starts at the cue and not before it.
    mount({ cues, positionSeconds: () => 1 });
    act(() => pads.press("A"));
    await settle();
    expect(transport).toEqual(["seek:30", "play"]);
  });

  it("calls it read-only too, since a jump is not a write", async () => {
    mount({ cues, positionSeconds: () => 1, readOnly: true });
    act(() => pads.press("A"));
    await settle();
    expect(seeked).toEqual([30]);
    expect(sent).toEqual([]);
  });

  it("never reads a memory cue as a filled slot", async () => {
    // The memory cue at 5 s has an empty letter; pressing an empty pad must
    // still set, not jump to it.
    mount({ cues: [memory("m1", 5_000)], positionSeconds: () => 2 });
    act(() => pads.press("A"));
    await settle();
    expect(sent).toEqual(["add:track-1:A:2000"]);
    expect(seeked).toEqual([]);
    expect(transport).toEqual([]);
  });
});

describe("calling a set pad with Q on", () => {
  // rekordbox 7.2.19, EXPORT mode: QuantizedCueBehavior::doHotCueLaunch takes
  // moveToCueAndPlayWithWait, the deck plays on to the next quantize step and
  // jumps to the cue there [OBS static, parity/issue-126].
  const cues = [hot("cue-a", "A", 500)];
  let jumps: string[];
  const jumpAt = (at: number, to: number) => jumps.push(`${at}->${to}`);
  beforeEach(() => {
    jumps = [];
  });

  it("waits for the next beat on a playing deck, then jumps to the cue", async () => {
    mount({ cues, positionSeconds: () => 1.2, quantiseTo: GRID, playing: () => true, jumpAt });
    act(() => pads.press("A"));
    await settle();
    expect(jumps).toEqual(["1.5->0.5"]);
    expect(transport).toEqual([]);
  });

  it("jumps at once from pause, and plays", async () => {
    mount({ cues, positionSeconds: () => 1.2, quantiseTo: GRID, playing: () => false, jumpAt });
    act(() => pads.press("A"));
    await settle();
    expect(jumps).toEqual([]);
    expect(transport).toEqual(["seek:0.5", "play"]);
  });

  describe("inside a playing one-beat loop", () => {
    // rekordbox 7.2.19: doHotCueLaunch leaves the loop at the press
    // (CueBehavior::doExitLoop @0x102b1b9bc) and WithWait fires no later
    // than the old out point (min(out, max(step, head)) @0x102b1ab60)
    // [OBS static, parity/issue-126]. Firing at the step alone never came:
    // the deck wrapped back to the in point first.
    const loop = { inSeconds: 1, outSeconds: 1.5 };
    let log: string[];
    const deck = (head: number) => ({
      cues, positionSeconds: () => head, quantiseTo: GRID, playing: () => true,
      activeLoop: () => loop,
      exitLoop: () => log.push("exit"),
      jumpAt: (at: number, to: number, from: number, wrap: number) => log.push(`${at}->${to} from ${from} wrap ${wrap}`),
    });
    beforeEach(() => {
      log = [];
    });

    it("leaves the loop and jumps at its out point", async () => {
      mount(deck(1.2));
      act(() => pads.press("A"));
      await settle();
      expect(log).toEqual(["exit", "1.5->0.5 from 1.2 wrap 0.5"]);
      expect(transport).toEqual([]);
    });

    it("times the wait from where the engine has wrapped to", async () => {
      // A head read between ticks runs on past the out point.
      mount(deck(1.75));
      act(() => pads.press("A"));
      await settle();
      expect(log).toEqual(["exit", "1.5->0.5 from 1.25 wrap 0.5"]);
    });
  });

  it("jumps at once with Q off", async () => {
    mount({ cues, positionSeconds: () => 1.2, quantiseTo: null, playing: () => true, jumpAt });
    act(() => pads.press("A"));
    await settle();
    expect(jumps).toEqual([]);
    expect(transport).toEqual(["seek:0.5", "play"]);
  });
});

describe("calling a hot cue saved as a loop", () => {
  // The issue's cue: pad B, `djmdCue` Kind 2, InMsec 22312, OutMsec 22997.
  // rekordbox 7.2.19 CueBehavior::doHotCueLaunch @0x102b075b0: a cue with an
  // out point moves to its in point, sets the out-to-in warp pair (the loop)
  // and starts the deck [OBS static]. Before this, the pad only seeked.
  const loopB: Cue = { id: "cue-b", positionMs: 22_312, outMs: 22_997, letter: "B", memory: false, colour: null };
  const setLoop = (inSeconds: number, outSeconds: number) => transport.push(`loop:${inSeconds}-${outSeconds}`);

  it("starts the loop at its in point and plays", async () => {
    mount({ cues: [loopB], positionSeconds: () => 1, setLoop });
    act(() => pads.press("B"));
    await settle();
    expect(transport).toEqual(["loop:22.312-22.997", "seek:22.312", "play"]);
  });

  it("does the same on a playing deck with Q off", async () => {
    mount({ cues: [loopB], positionSeconds: () => 1, setLoop, playing: () => true, quantiseTo: null });
    act(() => pads.press("B"));
    await settle();
    expect(transport).toEqual(["loop:22.312-22.997", "seek:22.312", "play"]);
  });

  it("carries the loop through a quantized wait", async () => {
    const jumps: string[] = [];
    mount({
      cues: [{ ...loopB, positionMs: 500, outMs: 1000 }], positionSeconds: () => 1.2, setLoop,
      quantiseTo: GRID, playing: () => true,
      jumpAt: (at, to, _from, _wrap, loop) => jumps.push(`${at}->${to} loop ${loop ? `${loop.inSeconds}-${loop.outSeconds}` : "none"}`),
    });
    act(() => pads.press("B"));
    await settle();
    expect(jumps).toEqual(["1.5->0.5 loop 0.5-1"]);
    expect(transport).toEqual([]);
  });

  it("leaves a plain hot cue a jump", async () => {
    mount({ cues: [hot("cue-a", "A", 30_000)], positionSeconds: () => 1, setLoop });
    act(() => pads.press("A"));
    await settle();
    expect(transport).toEqual(["seek:30", "play"]);
  });

  it("reads OutMsec -1 (stored as 0) as no loop", async () => {
    // rekordbox writes OutMsec -1 on a plain cue; the loader clamps it to 0.
    mount({ cues: [{ ...loopB, outMs: 0 }], positionSeconds: () => 1, setLoop });
    act(() => pads.press("B"));
    await settle();
    expect(transport).toEqual(["seek:22.312", "play"]);
  });
});

describe("at", () => {
  it("gives the cue in a slot and null for an empty one", () => {
    mount({ cues: [hot("cue-a", "A", 30_000)] });
    expect(pads.at("A")?.id).toBe("cue-a");
    expect(pads.at("B")).toBeNull();
  });
});

describe("clearing a pad", () => {
  it("deletes the cue in the slot", async () => {
    mount({ cues: [hot("cue-a", "A", 30_000)] });
    act(() => pads.clear("A"));
    await settle();
    expect(sent).toEqual(["delete:cue-a"]);
    // A clear is not a call: the deck neither moves nor starts.
    expect(transport).toEqual([]);
  });

  it("does nothing for an empty pad", async () => {
    mount({ cues: [hot("cue-a", "A", 30_000)] });
    act(() => pads.clear("C"));
    await settle();
    expect(sent).toEqual([]);
  });

  it("does nothing for a cue the backend cannot address", async () => {
    // An empty id is a row whose id is not a number the writer can take; the
    // list draws it with no ✕, and the key press has to agree.
    mount({ cues: [hot("", "A", 30_000)] });
    act(() => pads.clear("A"));
    await settle();
    expect(sent).toEqual([]);
  });

  it("does nothing while the library is read-only", async () => {
    mount({ cues: [hot("cue-a", "A", 30_000)], readOnly: true });
    act(() => pads.clear("A"));
    await settle();
    expect(sent).toEqual([]);
  });
});

describe("what the deck is told when a write fails", () => {
  it("passes the backend's refusal up", async () => {
    const onError = vi.fn();
    __setBackend({
      edits: { addCue: () => Promise.reject(new Error("the library is open read-only")) },
    } as unknown as Backend);
    mount({ positionSeconds: () => 1, onError });
    act(() => pads.press("A"));
    await settle();
    expect(onError).toHaveBeenLastCalledWith("the library is open read-only");
  });
});

describe("dragging a cue to another slot", () => {
  it("moves the cue by its id, to an empty slot or a set one", async () => {
    mount({ cues: [hot("7", "A", 1000), hot("8", "C", 3000)] });
    act(() => pads.move("A", "B"));
    act(() => pads.move("A", "C"));
    await settle();
    // One write at a time: the second drag is dropped until the first lands.
    expect(sent).toEqual(["slot:7:B"]);
  });

  it("does nothing from an empty slot, onto itself, or when the library is read-only", async () => {
    mount({ cues: [hot("7", "A", 1000)] });
    act(() => { pads.move("B", "C"); pads.move("A", "A"); });
    mount({ cues: [hot("7", "A", 1000)], readOnly: true });
    act(() => pads.move("A", "B"));
    await settle();
    expect(sent).toEqual([]);
  });
});
