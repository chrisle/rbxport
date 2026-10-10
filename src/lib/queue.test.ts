import { describe, expect, it } from "vitest";

import {
  cancel, emptyQueue, enqueue, fail, isRunning, isWaiting, reset, SLOTS, start, succeed, total,
  type QueueItem,
} from "./queue";

const running = (q: { running: QueueItem[] }) => q.running.map((i) => i.id);

const items = (...ids: string[]): QueueItem[] =>
  ids.map((id) => ({ id, title: `Track ${id}` }));

describe("enqueue", () => {
  it("adds in order", () => {
    const q = enqueue(emptyQueue, items("a", "b", "c"));
    expect(q.pending.map((i) => i.id)).toEqual(["a", "b", "c"]);
  });

  it("skips a track already queued", () => {
    // Queueing twice analyses twice and counts twice, which makes the
    // progress meaningless.
    let q = enqueue(emptyQueue, items("a", "b"));
    q = enqueue(q, items("b", "c"));
    expect(q.pending.map((i) => i.id)).toEqual(["a", "b", "c"]);
  });

  it("skips duplicates within one batch", () => {
    const q = enqueue(emptyQueue, items("a", "a", "b"));
    expect(q.pending.map((i) => i.id)).toEqual(["a", "b"]);
  });

  it("skips the track already running", () => {
    let q = start(enqueue(emptyQueue, items("a")));
    q = enqueue(q, items("a", "b"));
    expect(q.pending.map((i) => i.id)).toEqual(["b"]);
  });

  it("returns the same state when nothing is new", () => {
    const q = enqueue(emptyQueue, items("a"));
    expect(enqueue(q, items("a"))).toBe(q);
  });

  it("clears a previous cancellation, since this is a new request", () => {
    const q = enqueue(cancel(enqueue(emptyQueue, items("a"))), items("b"));
    expect(q.cancelling).toBe(false);
  });
});

describe("start", () => {
  it("takes the first waiting tracks, up to the slots", () => {
    const ids = ["a", "b", "c", "d", "e", "f"];
    const q = start(enqueue(emptyQueue, items(...ids)));
    expect(running(q)).toEqual(ids.slice(0, SLOTS));
    expect(q.pending.map((i) => i.id)).toEqual(ids.slice(SLOTS));
  });

  it("does nothing while the slots are full", () => {
    const q = start(enqueue(emptyQueue, items("a", "b", "c", "d")));
    expect(start(q)).toBe(q);
  });

  it("fills a slot as soon as one frees, in order", () => {
    let q = start(enqueue(emptyQueue, items("a", "b", "c", "d")));
    q = start(succeed(q, "b"));
    expect(running(q)).toEqual(["a", "c", "d"]);
    expect(q.pending).toEqual([]);
  });

  it("does nothing on an empty queue", () => {
    expect(start(emptyQueue)).toBe(emptyQueue);
  });

  it("starts nothing while held behind a relocate, then fills the slots once it ends (#286)", () => {
    const q = start(enqueue(emptyQueue, items("a", "b")), 2, true);
    expect(running(q)).toEqual([]);
    expect(isWaiting(q, true)).toBe(true);
    expect(isRunning(q)).toBe(true);
    const freed = start(q, 2, false);
    expect(running(freed)).toEqual(["a", "b"]);
    expect(isWaiting(freed, false)).toBe(false);
  });

  it("lets the tracks already started finish while held, and is not waiting meanwhile", () => {
    const going = start(enqueue(emptyQueue, items("a", "b")), 1);
    const held = start(going, 2, true);
    expect(held).toBe(going);
    expect(isWaiting(held, true)).toBe(false);
    expect(isWaiting(succeed(held, "a"), true)).toBe(true);
  });

  it("drops what is waiting when the run is cancelling", () => {
    const q = start(cancel(enqueue(emptyQueue, items("a", "b"))));
    expect(q.running).toEqual([]);
    expect(q.pending).toEqual([]);
  });
});

describe("succeed and fail", () => {
  it("counts a finished track", () => {
    const q = succeed(start(enqueue(emptyQueue, items("a"))), "a");
    expect(q.done).toBe(1);
    expect(q.running).toEqual([]);
  });

  it("finishes one track and keeps the others running", () => {
    const q = succeed(start(enqueue(emptyQueue, items("a", "b", "c"))), "b");
    expect(running(q)).toEqual(["a", "c"]);
    expect(q.done).toBe(1);
  });

  it("keeps why a track failed", () => {
    const q = fail(start(enqueue(emptyQueue, items("a"))), "a", "could not decode");
    expect(q.failed).toEqual([{ id: "a", title: "Track a", reason: "could not decode" }]);
    expect(q.done).toBe(0);
  });

  it("does nothing for a track that is not running", () => {
    expect(succeed(emptyQueue, "a")).toBe(emptyQueue);
    expect(fail(emptyQueue, "a", "x")).toBe(emptyQueue);
    const q = start(enqueue(emptyQueue, items("a")));
    expect(succeed(q, "b")).toBe(q);
  });

  it("keeps going after a failure", () => {
    let q = start(enqueue(emptyQueue, items("a", "b")));
    q = fail(q, "a", "bad file");
    q = succeed(start(q), "b");
    expect(q.done).toBe(1);
    expect(q.failed).toHaveLength(1);
    expect(isRunning(q)).toBe(false);
  });
});

describe("cancel", () => {
  it("lets the running tracks finish but drops the rest", () => {
    // A running track is work already spent; abandoning it would leave half
    // a result.
    let q = start(enqueue(emptyQueue, items("a", "b", "c", "d", "e")));
    q = cancel(q);
    expect(running(q)).toEqual(["a", "b", "c"]);
    expect(q.pending).toEqual([]);
    expect(start(q)).toBe(q);
    q = succeed(succeed(succeed(q, "a"), "b"), "c");
    expect(isRunning(q)).toBe(false);
  });
});

describe("total and isRunning", () => {
  it("counts everything the run covers", () => {
    let q = enqueue(emptyQueue, items("a", "b", "c"));
    expect(total(q)).toBe(3);
    q = succeed(start(q), "a");
    expect(total(q)).toBe(3);
    q = fail(start(q), "b", "x");
    expect(total(q)).toBe(3);
  });

  it("is running while anything is waiting or going", () => {
    expect(isRunning(emptyQueue)).toBe(false);
    const q = enqueue(emptyQueue, items("a"));
    expect(isRunning(q)).toBe(true);
    expect(isRunning(succeed(start(q), "a"))).toBe(false);
  });
});

describe("reset", () => {
  it("clears a finished run", () => {
    const q = succeed(start(enqueue(emptyQueue, items("a"))), "a");
    expect(reset(q)).toEqual(emptyQueue);
  });

  it("refuses to clear a run still going", () => {
    const q = start(enqueue(emptyQueue, items("a", "b")));
    expect(reset(q)).toBe(q);
  });
});

it("changing the concurrency limit drains existing work before filling again", () => {
  const q = start(enqueue(emptyQueue, items("a", "b", "c", "d")), 3);
  expect(start(q, 1)).toBe(q);
  const draining = succeed(succeed(q, "a"), "b");
  expect(start(draining, 1)).toBe(draining);
  expect(running(start(succeed(draining, "c"), 1))).toEqual(["d"]);
  expect(running(start(q, 4))).toEqual(["a", "b", "c", "d"]);
});
