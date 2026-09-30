/**
 * Software updates, as the Update Manager window sees them.
 *
 * One state machine: idle → checking → up to date | downloading →
 * installing → ready, with failed reachable from any of the working states.
 * The backend does the checking, downloading and installing; this holds
 * where it has got to and whether the window is showing.
 *
 * An update is taken without asking. A check the app starts on its own
 * downloads what it finds and puts it in place with a compact notification at
 * the bottom of the app: the next launch is the new version, and a download
 * that fails is left for the next launch to try again. A check somebody asked
 * for opens the window at once
 * and shows the same work as it happens, ending with the offer to restart
 * into the new version now rather than later.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { UpdateFrequency } from "@/lib/preferences";
import type { UpdateCheck, UpdateProgress, UpdateReady } from "@/ipc/types";

/** How long after launch the automatic check runs: after the library, not before it. */
const AUTO_CHECK_AFTER_MS = 15_000;

export type UpdaterState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "upToDate"; currentVersion: string }
  /** The update was found; the following effect starts its automatic download. */
  | { phase: "available"; check: UpdateCheck }
  | { phase: "downloading"; check: UpdateCheck; progress: UpdateProgress | null }
  | { phase: "installing"; check: UpdateCheck }
  /** Downloaded and, where the platform allows, already in place. */
  | { phase: "ready"; check: UpdateCheck; ready: UpdateReady }
  | { phase: "failed"; message: string; check: UpdateCheck | null };

export interface Updater {
  state: UpdaterState;
  /** The current work was started by the automatic start-up check. */
  automatic: boolean;
  /** Whether the Update Manager window is showing. */
  open: boolean;
  /** Ask the server. `manual` opens the window whatever the answer. */
  check: (manual: boolean) => void;
  /** Download what the last check found, after a download that failed. */
  retry: () => void;
  /** Restart into the downloaded update now rather than at the next launch. */
  restart: () => void;
  /** Close the window; a download in progress keeps going. */
  dismiss: () => void;
}

/** Why a call that failed did, in the words the window shows. */
function reason(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message !== "") return message;
  }
  if (typeof error === "string" && error !== "") return error;
  return "An error occurred. Please try later.";
}

/** Where the last automatic check's time is kept, so the frequency holds across launches. */
const LAST_CHECK_KEY = "rbxport.updates.lastCheck";

/** The shortest gap between automatic checks for each frequency, in milliseconds. */
const CHECK_GAP_MS: Record<UpdateFrequency, number> = {
  start: 0,
  daily: 24 * 60 * 60 * 1000,
  weekly: 7 * 24 * 60 * 60 * 1000,
};

/** Whether an automatic check is due, by the last one's time. */
export function checkDue(frequency: UpdateFrequency, lastCheckMs: number | null, nowMs: number): boolean {
  if (frequency === "start" || lastCheckMs === null || !Number.isFinite(lastCheckMs)) return true;
  return nowMs - lastCheckMs >= CHECK_GAP_MS[frequency];
}

function lastCheck(): number | null {
  try {
    const stored = localStorage.getItem(LAST_CHECK_KEY);
    return stored === null ? null : Number.parseInt(stored, 10);
  } catch {
    return null;
  }
}

function noteCheck(nowMs: number): void {
  try {
    localStorage.setItem(LAST_CHECK_KEY, String(nowMs));
  } catch {
    // Storage refused: the next launch checks again, which is the safe side.
  }
}

export function useUpdater(autoCheck: boolean, frequency: UpdateFrequency = "start"): Updater {
  const [state, setState] = useState<UpdaterState>({ phase: "idle" });
  const [open, setOpen] = useState(false);
  const [automatic, setAutomatic] = useState(false);
  // The state outside a render, so an action can read it without a side
  // effect inside a state updater.
  const latest = useRef(state);
  latest.current = state;
  // The check that is running, so a slow one does not overwrite a newer one.
  const sequence = useRef(0);
  const downloadStarted = useRef(0);
  const checkedOnStart = useRef(false);

  // The download, and its end: in place, staged, or failed. The backend
  // answers at once when the version is already downloaded this run.
  const download = useCallback((found: UpdateCheck, mine: number) => {
    setState({ phase: "downloading", check: found, progress: null });
    void (async () => {
      try {
        const backend = await getBackend();
        const ready = await backend.downloadUpdate();
        if (mine !== sequence.current) return;
        setState({ phase: "ready", check: found, ready });
      } catch (error) {
        if (mine !== sequence.current) return;
        setState({ phase: "failed", message: reason(error), check: found });
      }
    })();
  }, []);

  const check = useCallback((manual: boolean) => {
    const current = latest.current;
    // Work already under way, or done: a request to look again is a request
    // to see it, not to start over.
    if (current.phase === "downloading" || current.phase === "installing" || current.phase === "ready") {
      if (manual) setOpen(true);
      return;
    }
    const mine = ++sequence.current;
    setAutomatic(!manual);
    setState({ phase: "checking" });
    if (manual) setOpen(true);
    void (async () => {
      try {
        const backend = await getBackend();
        const found = await backend.checkForUpdate();
        if (mine !== sequence.current) return;
        if (found.version === null) {
          setState({ phase: "upToDate", currentVersion: found.currentVersion });
        } else if (found.ready) {
          setState({ phase: "ready", check: found, ready: found.ready });
        } else {
          // Let the bottom notice show that an update was found before its
          // download starts. The effect below also keeps this transition out
          // of the check's async callback.
          setState({ phase: "available", check: found });
        }
      } catch (error) {
        if (mine !== sequence.current) return;
        setState({ phase: "failed", message: reason(error), check: null });
        // A check the app ran on its own that could not reach the server is
        // not worth a window: the next launch tries again.
        if (manual) setOpen(true);
      }
    })();
  }, []);

  // Every update is downloaded automatically. Keeping this as its own render
  // lets the compact notice report "New update available" before progress
  // begins, even when React batches the check's state changes.
  useEffect(() => {
    if (state.phase !== "available") return;
    const mine = sequence.current;
    if (downloadStarted.current === mine) return;
    downloadStarted.current = mine;
    download(state.check, mine);
  }, [state, download]);

  const retry = useCallback(() => {
    const current = latest.current;
    if (current.phase !== "failed" || !current.check) return;
    download(current.check, sequence.current);
  }, [download]);

  const restart = useCallback(() => {
    const current = latest.current;
    if (current.phase !== "ready") return;
    const found = current.check;
    void (async () => {
      try {
        const backend = await getBackend();
        // A success never returns: the process ends. Returning is failure.
        await backend.restartToUpdate();
        setState({ phase: "failed", message: "The update did not restart the app.", check: found });
        setOpen(true);
      } catch (error) {
        setState({ phase: "failed", message: reason(error), check: found });
        setOpen(true);
      }
    })();
  }, []);

  // The download's progress, and the moment it turns into an install.
  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    void getBackend().then((backend) => {
      if (!live) return;
      stop = backend.onUpdateProgress((progress) => {
        setState((current) => {
          if (current.phase !== "downloading") return current;
          const done = progress.total !== null && progress.downloaded >= progress.total;
          return done
            ? { phase: "installing", check: current.check }
            : { ...current, progress };
        });
      });
    });
    return () => {
      live = false;
      stop?.();
    };
  }, []);

  useEffect(() => {
    if (!autoCheck || checkedOnStart.current) return;
    checkedOnStart.current = true;
    if (!checkDue(frequency, lastCheck(), Date.now())) return;
    const timer = setTimeout(() => {
      noteCheck(Date.now());
      check(false);
    }, AUTO_CHECK_AFTER_MS);
    return () => clearTimeout(timer);
  }, [autoCheck, frequency, check]);

  const dismiss = useCallback(() => setOpen(false), []);

  return { state, automatic, open, check, retry, restart, dismiss };
}
