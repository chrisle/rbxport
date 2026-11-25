/**
 * @vitest-environment jsdom
 *
 * The one-write-at-a-time guard, and what the interface is told when a write
 * fails.
 *
 * The guard is not a nicety: a held key repeats about thirty times a second,
 * and every repeat used to reach the backend as another cue at the same point
 * before the first had come back. So the test that matters is the one that
 * fires several presses inside a single frame and expects the backend to have
 * heard one. The error wording matters for the same reason the guard does —
 * the deck shows whatever comes out of `describe`, so a failure with no usable
 * message has to end up as a sentence rather than `[object Object]`.
 */
import { act, startTransition, StrictMode, Suspense } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { useCueWriter, type CueEdits } from "./useCueWriter";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let write: (action: (edits: CueEdits) => Promise<unknown>) => void;
let onError: ReturnType<typeof vi.fn>;
/** The `edits` the stub backend hands out, so an action can be seen to get it. */
let edits: CueEdits;
let owner: string | null;

/** A promise the test resolves by hand, to hold a write in flight. */
function deferred<T>() {
  let settle!: (value: T) => void;
  let fail!: (reason: unknown) => void;
  const promise = new Promise<T>((resolve, reject) => {
    settle = resolve;
    fail = reject;
  });
  return { promise, settle, fail };
}

function Probe() {
  write = useCueWriter(onError, owner);
  return null;
}

/** Lets the writer's async work run to wherever it next blocks. */
const settle = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  onError = vi.fn();
  owner = "track-A";
  edits = { addCue: () => Promise.resolve("cue-1") } as unknown as CueEdits;
  __setBackend({ edits } as unknown as Backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<Probe />));
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("the in-flight guard", () => {
  it("drops every press after the first while a write is still out", async () => {
    // A held key, as the deck sees it: one frame, many repeats.
    const gate = deferred<string>();
    let calls = 0;
    const action = () => {
      calls += 1;
      return gate.promise;
    };

    act(() => {
      write(action);
      write(action);
      write(action);
      write(action);
    });
    await settle();

    expect(calls).toBe(1);

    gate.settle("cue-1");
    await settle();
    expect(calls).toBe(1);
  });

  it("takes the next press once the write has landed", async () => {
    const first = deferred<string>();
    let calls = 0;
    act(() => write(() => {
      calls += 1;
      return first.promise;
    }));
    await settle();
    expect(calls).toBe(1);

    // Dropped: the first is still out.
    act(() => write(() => {
      calls += 1;
      return Promise.resolve("x");
    }));
    await settle();
    expect(calls).toBe(1);

    first.settle("cue-1");
    await settle();

    act(() => write(() => {
      calls += 1;
      return Promise.resolve("cue-2");
    }));
    await settle();
    expect(calls).toBe(2);
  });

  it("releases the guard after a failed write, so the deck is not wedged", async () => {
    // A refused write must not leave the pads dead for the rest of the session.
    act(() => write(() => Promise.reject(new Error("no"))));
    await settle();
    expect(onError).toHaveBeenLastCalledWith("no");

    let ran = false;
    act(() => write(() => {
      ran = true;
      return Promise.resolve("cue-2");
    }));
    await settle();
    expect(ran).toBe(true);
  });

  it("hands the action the backend's own cue commands", async () => {
    let given: CueEdits | null = null;
    act(() => write((e) => {
      given = e;
      return Promise.resolve("cue-1");
    }));
    await settle();
    expect(given).toBe(edits);
  });
});

/*
 * Rejecting with something that is not an `Error` is the point of these: a
 * Tauri command that fails rejects with a bare object, and a backend that is
 * not there at all can reject with anything. `describe` exists to turn those
 * into a sentence, so the tests have to hand it the shapes it was written for.
 */
/* eslint-disable @typescript-eslint/prefer-promise-reject-errors */
describe("what the deck is told", () => {
  it("clears the message once a write lands", async () => {
    act(() => write(() => Promise.resolve("cue-1")));
    await settle();
    expect(onError).toHaveBeenCalledWith(null);
  });

  it("reports an Error's own message", async () => {
    act(() => write(() => Promise.reject(new Error("Cue slot A is taken"))));
    await settle();
    expect(onError).toHaveBeenLastCalledWith("Cue slot A is taken");
  });

  it("reports the message off a bare object, which is what Tauri throws", async () => {
    act(() => write(() => Promise.reject({ message: "the library is open read-only" })));
    await settle();
    expect(onError).toHaveBeenLastCalledWith("the library is open read-only");
  });

  it("falls back to a sentence for anything with no message to show", async () => {
    const fallback = "That cue could not be saved.";

    act(() => write(() => Promise.reject("a bare string")));
    await settle();
    expect(onError).toHaveBeenLastCalledWith(fallback);

    // An Error whose message is blank reads as nothing at all on the deck.
    act(() => write(() => Promise.reject(new Error("   "))));
    await settle();
    expect(onError).toHaveBeenLastCalledWith(fallback);

    act(() => write(() => Promise.reject({ message: 42 })));
    await settle();
    expect(onError).toHaveBeenLastCalledWith(fallback);

    act(() => write(() => Promise.reject(null)));
    await settle();
    expect(onError).toHaveBeenLastCalledWith(fallback);
  });
});
/* eslint-enable @typescript-eslint/prefer-promise-reject-errors */


describe("track ownership", () => {
  const select = (id: string | null) => {
    owner = id;
    act(() => root.render(<Probe />));
  };

  it.each([false, true])("retires old track notifications on switch (failure=%s)", async (failure) => {
    const old = deferred<string>();
    act(() => write(() => old.promise));
    await settle();
    select("track-B");
    if (failure) old.fail(new Error("old A refused"));
    else old.settle("old A saved");
    await settle();
    expect(onError).not.toHaveBeenCalled();
  });

  it("accepts a new track but retains the pending track lock across A-B-A", async () => {
    const a = deferred<string>();
    const b = deferred<string>();
    const calls: string[] = [];
    act(() => write(() => { calls.push("A"); return a.promise; }));
    await settle();
    select("track-B");
    act(() => write(() => { calls.push("B"); return b.promise; }));
    await settle();
    expect(calls).toEqual(["A", "B"]);
    select("track-A");
    act(() => write(() => { calls.push("duplicate A"); return Promise.resolve("duplicate"); }));
    await settle();
    expect(calls).toEqual(["A", "B"]);
    b.settle("B saved");
    a.settle("old A saved");
    await settle();
    expect(onError).not.toHaveBeenCalled();
    act(() => write(() => { calls.push("new A"); return Promise.resolve("new A saved"); }));
    await settle();
    expect(calls).toEqual(["A", "B", "new A"]);
    expect(onError).toHaveBeenLastCalledWith(null);
  });

  it("does not let an old completion unlock a new owner's pending write", async () => {
    const a = deferred<string>();
    const b = deferred<string>();
    const next = vi.fn(() => b.promise);
    act(() => write(() => a.promise));
    await settle();
    select("track-B");
    act(() => write(next));
    await settle();
    a.settle("A saved");
    await settle();
    act(() => write(next));
    await settle();
    expect(next).toHaveBeenCalledTimes(1);
    expect(onError).not.toHaveBeenCalled();
    b.settle("B saved");
    await settle();
    expect(onError).toHaveBeenLastCalledWith(null);
  });

  it("keeps the current error when an old track later succeeds", async () => {
    const a = deferred<string>();
    act(() => write(() => a.promise));
    await settle();
    select("track-B");
    act(() => write(() => Promise.reject(new Error("current B refused"))));
    await settle();
    expect(onError).toHaveBeenLastCalledWith("current B refused");
    a.settle("old A saved");
    await settle();
    expect(onError).toHaveBeenCalledExactlyOnceWith("current B refused");
  });

  it.each([false, true])("retires notifications on unmount without canceling accepted writes (failure=%s)", async (failure) => {
    const old = deferred<string>();
    const action = vi.fn(() => old.promise);
    act(() => write(action));
    await settle();
    act(() => root.render(null));
    if (failure) old.fail(new Error("disposed refused"));
    else old.settle("disposed saved");
    await settle();
    expect(action).toHaveBeenCalledOnce();
    expect(onError).not.toHaveBeenCalled();
  });

  it("keeps an accepted action's target when selection changes before dispatch", async () => {
    const action = vi.fn(() => Promise.resolve("accepted A"));
    act(() => write(action));
    select("track-B");
    await settle();
    expect(action).toHaveBeenCalledExactlyOnceWith(edits);
    expect(onError).not.toHaveBeenCalled();
  });

  it("refuses a retained callback from a retired owner", async () => {
    const stale = write;
    select("track-B");
    const action = vi.fn(() => Promise.resolve("obsolete"));
    act(() => stale(action));
    await settle();
    expect(action).not.toHaveBeenCalled();
  });
});


it("does not revive an old write callback after A-B-A", async () => {
  const stale = write;
  owner = "track-B";
  act(() => root.render(<Probe />));
  owner = "track-A";
  act(() => root.render(<Probe />));
  const action = vi.fn(() => Promise.resolve("obsolete"));
  act(() => stale(action));
  await settle();
  expect(action).not.toHaveBeenCalled();
});

it("accepts current writes after StrictMode replays subscription cleanup", async () => {
  act(() => root.render(<StrictMode><Probe /></StrictMode>));
  const action = vi.fn(() => Promise.resolve("current"));
  act(() => write(action));
  await settle();
  expect(action).toHaveBeenCalledExactlyOnceWith(edits);
  expect(onError).toHaveBeenCalledExactlyOnceWith(null);
});


it("keeps the committed owner while a transition renders another owner but suspends", async () => {
  const blocked = deferred<void>();
  let attemptedB = false;
  function ConcurrentProbe({ id, suspend }: { id: string; suspend: boolean }) {
    const currentWrite = useCueWriter(onError, id);
    if (suspend) {
      attemptedB = true;
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- React Suspense throws a pending promise.
      throw blocked.promise;
    }
    write = currentWrite;
    return <span>{id}</span>;
  }
  act(() => root.render(<Suspense fallback={<span>Waiting</span>}><ConcurrentProbe id="track-A" suspend={false} /></Suspense>));
  const committedA = write;
  const pending = deferred<string>();
  act(() => committedA(() => pending.promise));
  await settle();
  await act(async () => {
    startTransition(() => root.render(<Suspense fallback={<span>Waiting</span>}><ConcurrentProbe id="track-B" suspend /></Suspense>));
    await Promise.resolve();
  });
  expect(attemptedB).toBe(true);
  expect(host.textContent).toBe("track-A");
  pending.fail(new Error("committed A refused"));
  await settle();
  expect(onError).toHaveBeenLastCalledWith("committed A refused");
  const next = vi.fn(() => Promise.resolve("committed A saved"));
  act(() => committedA(next));
  await settle();
  expect(next).toHaveBeenCalledExactlyOnceWith(edits);
  expect(onError).toHaveBeenLastCalledWith(null);
});
