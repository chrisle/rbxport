/**
 * Drives the analysis queue against the backend.
 *
 * A few tracks at a time (`SLOTS` in the queue): each is a decode and a DSP
 * pass, and three in flight finish a batch nearly three times sooner than one
 * while the progress still reads as a count.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import { usePreferences } from "./usePreferences";
import type { AnalysisResult } from "@/ipc/types";
import type { AnalysisPreferences } from "@/lib/preferences";
import {
  cancel as cancelQueue,
  emptyQueue,
  enqueue,
  fail,
  isRunning,
  reset,
  start,
  succeed,
  total,
  type QueueItem,
  type QueueState,
} from "@/lib/queue";

export interface Analysis {
  state: QueueState;
  running: boolean;
  total: number;
  add: (items: readonly QueueItem[], settings?: QueueItem["analysis"]) => void;
  cancel: () => void;
  clear: () => void;
}

export function useAnalysis(
  onAnalysed?: (trackId: string, result: AnalysisResult) => void,
  /** Called once when a run ends, whether it finished, failed or was stopped. */
  onDrained?: () => void,
  preferences?: AnalysisPreferences,
): Analysis {
  const storedPreferences = usePreferences().analysis;
  const { mode, concurrentTracks, firstBeatCue } = preferences ?? storedPreferences;
  const [state, setState] = useState<QueueState>(emptyQueue);
  // The tracks whose request is in flight, so the effect below never sends
  // one twice.
  const inFlight = useRef(new Set<string>());
  // Whether a run has been going, so the drain fires once at its end.
  const ran = useRef(false);
  const running = isRunning(state);
  useEffect(() => {
    if (running) {
      ran.current = true;
    } else if (ran.current) {
      ran.current = false;
      onDrained?.();
    }
  }, [running, onDrained]);

  useEffect(() => {
    // Fill the free slots first, if the run has not been cancelled; the
    // effect runs again on the new state and sends the requests.
    const next = start(state, concurrentTracks);
    if (next !== state) {
      setState(next);
      return;
    }
    for (const track of state.running) {
      if (inFlight.current.has(track.id)) continue;
      inFlight.current.add(track.id);
      void (async () => {
        try {
          const backend = await getBackend();
          const result = await backend.analyseTrack(track.id, track.analysis?.mode ?? mode, track.analysis);
          setState((s) => succeed(s, track.id));
          onAnalysed?.(track.id, result);
        } catch (e) {
          setState((s) => fail(s, track.id, e instanceof Error ? e.message : String(e)));
        } finally {
          inFlight.current.delete(track.id);
        }
      })();
    }
  }, [state, onAnalysed, mode, concurrentTracks]);

  const add = useCallback((items: readonly QueueItem[], settings?: QueueItem["analysis"]) => {
    // Capture settings at enqueue time, including automatic imports. Later
    // preference changes must not alter tracks still waiting in this batch.
    const chosen = settings ?? { mode, bpmGrid: true, key: true, highPrecision: true, minBpm: 70, maxBpm: 180, firstBeatCue };
    setState((s) => enqueue(reset(s), items.map(item => ({ ...item, analysis: { ...chosen } }))));
  }, [mode, firstBeatCue]);
  const cancel = useCallback(() => setState(cancelQueue), []);
  const clear = useCallback(() => setState(reset), []);

  // Memoised as a whole: see the note in `useColumns`.
  return useMemo(
    () => ({ state, running, total: total(state), add, cancel, clear }),
    [state, running, add, cancel, clear],
  );
}
