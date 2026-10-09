/**
 * @vitest-environment jsdom
 *
 * The MEMORY cluster: storing a cue, calling one, and deleting the one under
 * the playhead.
 *
 * The part with no coverage anywhere until now is the memory *loop*. A memory
 * cue with an out point past its in point is a loop, and calling it is not a
 * seek — the deck goes round it, the way a CDJ's CUE/LOOP CALL does. The
 * library is full of these (rekordbox writes them from the loop In/Out), so a
 * call that only seeked would silently drop the loop half of the cue and the
 * DJ would find the deck running straight through a saved loop.
 *
 * Calling also moves the cue point, which is what makes CUE come back to the
 * cue that was called rather than to wherever the track was before.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, Cue, CueKind } from "@/ipc/types";
import { useMemoryCues, type MemoryCueActions, type MemoryCueDeck } from "./useMemoryCues";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let memory: MemoryCueActions;
let sent: string[];
let seeked: number[];
let looped: [number, number][];
let cuePointSet: number[];

const cue = (id: string, positionMs: number): Cue => ({
  id, positionMs, outMs: 0, letter: "", memory: true, colour: null,
});
/** A memory cue with an out point: rekordbox's saved loop. */
const loopCue = (id: string, positionMs: number, outMs: number): Cue => ({
  id, positionMs, outMs, letter: "", memory: true, colour: null,
});
const hot = (id: string, letter: string, positionMs: number): Cue => ({
  id, positionMs, outMs: 0, letter, memory: false, colour: null,
});

/** Three plain memory cues and one saved loop, with a hot cue to be ignored. */
const CUES: Cue[] = [
  cue("m1", 1_000),
  cue("m2", 30_000),
  loopCue("loop", 60_000, 64_000),
  cue("m4", 90_000),
  hot("h", "A", 45_000),
];

function stubBackend(): Backend {
  return {
    edits: {
      addCue: (track: string, kind: CueKind, positionMs: number) => {
        const slot = typeof kind === "string" ? kind : kind.hot;
        sent.push(`add:${track}:${slot}:${positionMs}`);
        return Promise.resolve("new-cue");
      },
      deleteCue: (id: string) => {
        sent.push(`delete:${id}`);
        return Promise.resolve();
      },
    },
  } as unknown as Backend;
}

function Probe({ deck }: { deck: MemoryCueDeck }) {
  memory = useMemoryCues(deck);
  return null;
}

function mount(deck: Partial<MemoryCueDeck> = {}) {
  const full: MemoryCueDeck = {
    trackId: "track-1",
    cues: CUES,
    positionSeconds: () => 0,
    seek: (seconds: number) => seeked.push(seconds),
    setLoop: (inSeconds: number, outSeconds: number) => looped.push([inSeconds, outSeconds]),
    cuePoint: 0,
    setCuePoint: (seconds: number) => cuePointSet.push(seconds),
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
  looped = [];
  cuePointSet = [];
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

describe("calling a memory loop", () => {
  it("sets the deck looping round it rather than seeking to its start", () => {
    // Called from before it: N lands on the loop at 60 s.
    mount({ positionSeconds: () => 40 });
    act(() => memory.callNext());
    expect(looped).toEqual([[60, 64]]);
    expect(seeked).toEqual([]);
  });

  it("does the same when called backwards", () => {
    mount({ positionSeconds: () => 70 });
    act(() => memory.callPrevious());
    expect(looped).toEqual([[60, 64]]);
    expect(seeked).toEqual([]);
  });

  it("does the same when called by number", () => {
    // Third memory cue from the start of the track; the hot cue is not counted.
    mount();
    act(() => memory.callNumber(3));
    expect(looped).toEqual([[60, 64]]);
    expect(seeked).toEqual([]);
  });

  it("makes the loop's in point the cue point, so CUE comes back to it", () => {
    mount();
    act(() => memory.callNumber(3));
    expect(cuePointSet).toEqual([60]);
  });

  it("falls back to a seek on a deck with no loop control", () => {
    // Deck B on a build without the loop plumbing: better to land on the in
    // point than to ignore the cue.
    mount({ setLoop: undefined });
    act(() => memory.callNumber(3));
    expect(looped).toEqual([]);
    expect(seeked).toEqual([60]);
    expect(cuePointSet).toEqual([60]);
  });

  it("treats an out point at or before the in point as a plain cue", () => {
    // A zero out point is what a plain memory cue carries, and a malformed row
    // with out <= in is not a loop the deck could play.
    mount({ cues: [loopCue("odd", 10_000, 10_000), loopCue("back", 20_000, 5_000)] });
    act(() => memory.callNumber(1));
    act(() => memory.callNumber(2));
    expect(looped).toEqual([]);
    expect(seeked).toEqual([10, 20]);
  });
});

describe("calling a plain memory cue", () => {
  it("seeks to it and makes it the cue point", () => {
    mount({ positionSeconds: () => 0 });
    act(() => memory.callNext());
    expect(seeked).toEqual([1]);
    expect(cuePointSet).toEqual([1]);
    expect(looped).toEqual([]);
  });

  it("walks backwards past the cue it is standing on", () => {
    mount({ positionSeconds: () => 30 });
    act(() => memory.callPrevious());
    expect(seeked).toEqual([1]);
  });

  it("is one-based, and out of range does nothing", () => {
    mount();
    act(() => memory.callNumber(1));
    expect(seeked).toEqual([1]);
    act(() => memory.callNumber(4));
    expect(seeked).toEqual([1, 90]);

    act(() => memory.callNumber(0));
    act(() => memory.callNumber(5));
    act(() => memory.callNumber(99));
    expect(seeked).toEqual([1, 90]);
  });

  it("does nothing past the last cue or before the first", () => {
    mount({ positionSeconds: () => 120 });
    act(() => memory.callNext());
    expect(seeked).toEqual([]);
    expect(cuePointSet).toEqual([]);

    mount({ positionSeconds: () => 0 });
    act(() => memory.callPrevious());
    expect(seeked).toEqual([]);
  });

  it("still calls while the library is read-only, because a call is not a write", () => {
    mount({ positionSeconds: () => 40, readOnly: true });
    expect(memory.canEdit).toBe(false);
    act(() => memory.callPrevious());
    act(() => memory.callNext());
    act(() => memory.callNumber(1));
    expect(seeked).toEqual([30, 1]);
    expect(looped).toEqual([[60, 64]]);
  });

  it("does nothing with no track loaded", () => {
    mount({ trackId: null, positionSeconds: () => 40 });
    act(() => memory.callPrevious());
    act(() => memory.callNext());
    act(() => memory.callNumber(1));
    expect(seeked).toEqual([]);
    expect(looped).toEqual([]);
    expect(cuePointSet).toEqual([]);
  });
});

describe("storing a memory cue", () => {
  it("stores at the cue point, not at the playhead", async () => {
    // CUE has set the point; the track has played on since. rekordbox's M
    // stores the point, which is the whole reason the two are separate.
    mount({ cuePoint: 45, positionSeconds: () => 78.5 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual(["add:track-1:memory:45000"]);
  });

  it("stores the cue point whether or not the deck is playing", async () => {
    // [OBS] rekordbox 7.2.19 eventMemoryCue reads the current cue, never the
    // play position: the head being elsewhere changes nothing.
    mount({ cues: [], cuePoint: 12.5, positionSeconds: () => 200 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual(["add:track-1:memory:12500"]);
  });

  it("refuses an eleventh, counting memory loops, as rekordbox's MEMORY does", async () => {
    const nine = Array.from({ length: 9 }, (_, i) => cue(`m${i}`, (i + 1) * 10_000));
    mount({ cues: [...nine, loopCue("l", 100_000, 104_000)], cuePoint: 150 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual([]);

    // Nine plus a hot cue is still room for a tenth.
    mount({ cues: [...nine, hot("h", "A", 100_000)], cuePoint: 150 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual(["add:track-1:memory:150000"]);
  });

  it("refuses a second cue on a point that already has one", async () => {
    mount({ cuePoint: 30 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual([]);
  });

  it("refuses one within the tolerance of an existing cue", async () => {
    // 10 ms off the cue at 30 s: a second row there is nothing the list or the
    // waveform could draw apart.
    mount({ cuePoint: 30.01 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual([]);
  });

  it("clamps a cue point before the start of the track to zero", async () => {
    mount({ cues: [], cuePoint: -2 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual(["add:track-1:memory:0"]);
  });

  it("does nothing read-only or with no track loaded", async () => {
    mount({ cuePoint: 45, readOnly: true });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual([]);

    mount({ trackId: null, cuePoint: 45 });
    act(() => memory.store());
    await settle();
    expect(sent).toEqual([]);
  });
});

describe("deleting", () => {
  it("deletes the cue the playhead is standing on", async () => {
    mount({ positionSeconds: () => 30 });
    act(() => memory.deleteAtHead());
    await settle();
    expect(sent).toEqual(["delete:m2"]);
  });

  it("does nothing away from a cue", async () => {
    mount({ positionSeconds: () => 45 });
    act(() => memory.deleteAtHead());
    await settle();
    expect(sent).toEqual([]);
  });

  it("never deletes a hot cue standing at the playhead", async () => {
    // The hot cue sits at 45 s; X is the memory cluster's key and must not
    // reach it.
    mount({ positionSeconds: () => 45 });
    act(() => memory.deleteAtHead());
    await settle();
    expect(sent).toEqual([]);
  });

  it("removes a row from the list", async () => {
    mount();
    act(() => memory.remove(cue("m4", 90_000)));
    await settle();
    expect(sent).toEqual(["delete:m4"]);
  });

  it("does nothing for a cue the backend cannot address", async () => {
    mount();
    act(() => memory.remove(cue("", 90_000)));
    await settle();
    expect(sent).toEqual([]);
  });

  it("does nothing read-only", async () => {
    mount({ positionSeconds: () => 30, readOnly: true });
    act(() => memory.deleteAtHead());
    act(() => memory.remove(cue("m4", 90_000)));
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
    mount({ cues: [], cuePoint: 45, onError });
    act(() => memory.store());
    await settle();
    expect(onError).toHaveBeenLastCalledWith("the library is open read-only");
  });
});
