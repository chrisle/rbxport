/**
 * The browser's preview player, as the rows see it.
 *
 * A click on a row's waveform plays that track from where it was clicked,
 * without loading it onto a deck — rekordbox's `PreviewComponent::clickWave`;
 * see `src-tauri/src/preview.rs` for what rekordbox does and how that was
 * established. The audio is the backend's; this holds which track is
 * previewing and where, for the playhead the row draws over its waveform.
 *
 * The preview has no tick of its own. While it plays it is asked where it is
 * a few times a second, and the playhead is extrapolated between answers.
 * One store for every row: only the row holding the track draws anything.
 */
import { useSyncExternalStore } from "react";

import { getBackend } from "@/ipc/client";
import { reasonFrom } from "@/store/usePlayback";

export interface PreviewSnapshot {
  /** The track previewing, or null for none. */
  trackId: string | null;
  playing: boolean;
  /** Where it was at `at`. */
  positionMs: number;
  durationMs: number;
  /** `performance.now()` when `positionMs` was true. */
  at: number;
}

/** How often a playing preview is asked where it is. */
export const PREVIEW_POLL_MS = 100;

const IDLE: PreviewSnapshot = { trackId: null, playing: false, positionMs: 0, durationMs: 0, at: 0 };

let current: PreviewSnapshot = IDLE;
/** Bumped by every start and stop, so an answer to an older one is ignored. */
let generation = 0;
let poll: ReturnType<typeof setTimeout> | null = null;
const listeners = new Set<() => void>();
const errorListeners = new Set<(message: string) => void>();

function publish(next: PreviewSnapshot): void {
  current = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function stopPolling(): void {
  if (poll !== null) clearTimeout(poll);
  poll = null;
}

function schedulePoll(asked: number): void {
  stopPolling();
  poll = setTimeout(() => {
    poll = null;
    void getBackend()
      .then((backend) => backend.previewState())
      .then((state) => {
        if (asked !== generation) return;
        if (state.track !== current.trackId) return;
        publish({
          trackId: state.track,
          playing: state.playing,
          positionMs: state.positionMs,
          durationMs: state.durationMs > 0 ? state.durationMs : current.durationMs,
          at: performance.now(),
        });
        if (state.playing) schedulePoll(asked);
      })
      .catch(() => {
        // A missed answer is only a playhead that waits for the next one.
        if (asked === generation && current.playing) schedulePoll(asked);
      });
  }, PREVIEW_POLL_MS);
}

/** Where the preview is now, extrapolated from the last answer. */
export function previewPositionMs(snapshot: PreviewSnapshot, now: number = performance.now()): number {
  if (!snapshot.playing) return snapshot.positionMs;
  const at = snapshot.positionMs + Math.max(0, now - snapshot.at);
  return snapshot.durationMs > 0 ? Math.min(at, snapshot.durationMs) : at;
}

/**
 * Previews `trackId` from `positionMs`. The playhead moves there at once; a
 * refusal (a file that is not there) takes it away again and is reported to
 * whoever listens through `onPreviewError`.
 */
export function startPreview(trackId: string, positionMs: number, durationMs: number): Promise<void> {
  generation += 1;
  const asked = generation;
  stopPolling();
  publish({ trackId, playing: true, positionMs: Math.max(0, positionMs), durationMs, at: performance.now() });
  return getBackend()
    .then((backend) => backend.previewPlay(trackId, Math.max(0, positionMs)))
    .then(() => {
      if (asked !== generation) return;
      // Started now, after the file opened: the playhead starts with it.
      publish({ ...current, at: performance.now() });
      schedulePoll(asked);
    })
    .catch((error: unknown) => {
      if (asked !== generation) return;
      publish(IDLE);
      const message = reasonFrom(error);
      for (const listener of errorListeners) listener(message);
    });
}

/** Stops the preview where it is. */
export function stopPreview(): Promise<void> {
  generation += 1;
  stopPolling();
  if (current.trackId !== null) publish(IDLE);
  return getBackend()
    .then((backend) => backend.previewStop())
    .catch(() => undefined);
}

/** Told why a preview was refused. Returns its own unsubscribe. */
export function onPreviewError(listener: (message: string) => void): () => void {
  errorListeners.add(listener);
  return () => errorListeners.delete(listener);
}

/** The preview, re-rendering on a start, a stop and each answer. */
export function usePreview(): PreviewSnapshot {
  return useSyncExternalStore(subscribe, () => current, () => IDLE);
}

/** For tests: back to nothing previewing. */
export function __resetPreview(): void {
  generation += 1;
  stopPolling();
  current = IDLE;
  listeners.clear();
  errorListeners.clear();
}
