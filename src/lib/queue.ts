/**
 * The analysis queue's state, as pure transitions.
 *
 * Analysis is a decode and a DSP pass per track, so a few run at once against
 * the backend and the queue tracks where it has got to. Kept apart from the
 * component so the sequencing is testable without a backend: ordering,
 * cancellation and the counting are exactly the parts that go wrong.
 */

/**
 * How many tracks are analysed at once.
 *
 * Measured on 27 five-minute files on a 14-core machine, decode and analysis
 * together: one at a time 289 ms a track, two 153, three 106, four 89, six 84.
 * The analysis already runs its own front stages on several threads, so three
 * tracks fill a machine with eight cores and the progress still reads as a
 * count; beyond four the workers only fight.
 */
export const SLOTS = 3;
export const ANALYSIS_SLOTS = [1, 2, 3, 4] as const;

import type { AnalysisSettings } from "@/ipc/types";
import type { AnalysisMode } from "./preferences";

export interface QueueItem {
  id: string;
  title: string;
  analysis?: AnalysisSettings & { mode: AnalysisMode };
}

export interface QueueState {
  /** Waiting, in order. */
  pending: QueueItem[];
  /** Being analysed now, up to the requested limit, in the order they started. */
  running: QueueItem[];
  done: number;
  /** One entry per track that failed, with why. */
  failed: { id: string; title: string; reason: string }[];
  /** Set when the user asks to stop; the running track still finishes. */
  cancelling: boolean;
}

export const emptyQueue: QueueState = {
  pending: [],
  running: [],
  done: 0,
  failed: [],
  cancelling: false,
};

/** Total tracks this run covers, finished or not. */
export function total(state: QueueState): number {
  return state.done + state.failed.length + state.pending.length + state.running.length;
}

/**
 * Whether the run is only waiting: tracks are left but none is being
 * analysed, because a library job holds it.
 */
export function isWaiting(state: QueueState, held: boolean): boolean {
  return held && state.running.length === 0 && state.pending.length > 0;
}

/** Whether anything is left to do. */
export function isRunning(state: QueueState): boolean {
  return state.running.length > 0 || state.pending.length > 0;
}

/**
 * Adds tracks, skipping any already queued or running.
 *
 * Queueing the same track twice would analyse it twice and count it twice,
 * which makes the progress meaningless.
 */
export function enqueue(state: QueueState, items: readonly QueueItem[]): QueueState {
  const known = new Set([...state.pending, ...state.running].map((i) => i.id));
  const fresh = items.filter((item) => {
    if (known.has(item.id)) return false;
    known.add(item.id);
    return true;
  });
  if (fresh.length === 0) return state;
  return { ...state, pending: [...state.pending, ...fresh], cancelling: false };
}

/**
 * Fills the free slots from the waiting tracks, in order, or parks if there
 * is nothing waiting, the slots are full, the run is cancelling, or it is
 * `held` behind a library job that moves files (Auto Relocate): the tracks
 * already started finish, and the rest wait for it.
 */
export function start(state: QueueState, slots = SLOTS, held = false): QueueState {
  if (state.cancelling) {
    return state.pending.length > 0 ? { ...state, pending: [] } : state;
  }
  if (held) return state;
  const limit = Number.isInteger(slots) ? Math.max(1, Math.min(4, slots)) : SLOTS;
  const free = limit - state.running.length;
  if (free <= 0 || state.pending.length === 0) return state;
  return {
    ...state,
    running: [...state.running, ...state.pending.slice(0, free)],
    pending: state.pending.slice(free),
  };
}

/** Records a running track as finished. */
export function succeed(state: QueueState, id: string): QueueState {
  if (!state.running.some((i) => i.id === id)) return state;
  return { ...state, running: state.running.filter((i) => i.id !== id), done: state.done + 1 };
}

/** Records a running track as failed, keeping why. */
export function fail(state: QueueState, id: string, reason: string): QueueState {
  const track = state.running.find((i) => i.id === id);
  if (!track) return state;
  return {
    ...state,
    running: state.running.filter((i) => i.id !== id),
    failed: [...state.failed, { ...track, reason }],
  };
}

/**
 * Asks the run to stop.
 *
 * The tracks already being analysed finish: each is a few hundred
 * milliseconds already spent, and abandoning one would leave half a result.
 */
export function cancel(state: QueueState): QueueState {
  return { ...state, cancelling: true, pending: [] };
}

/** Clears a finished run so the next one starts from nothing. */
export function reset(state: QueueState): QueueState {
  return isRunning(state) ? state : emptyQueue;
}
