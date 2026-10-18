/**
 * The tracks the track list colours as played (#540).
 *
 * rekordbox 7.2.19 draws a played track's text in its theme colour
 * `0x30000e1` (`ListViewer::findTextColour` @0x1003efb70, read by
 * `ListViewer::paintTextColumn` @0x1003f1f48) [OBS static]. Which tracks
 * are played is the backend's: the deck's recorded plays, and while LINK is
 * on the tracks a player added to its history (`src-tauri/src/state.rs`).
 *
 * One store for every list: the set is fetched once per change, not once
 * per table.
 */
import { useSyncExternalStore } from "react";

import { getBackend } from "@/ipc/client";

const NONE: ReadonlySet<string> = new Set();

let current: ReadonlySet<string> = NONE;
const listeners = new Set<() => void>();
let stops: (() => void)[] | null = null;
/** Bumped by every fetch, so an answer to an older one is ignored. */
let asked = 0;

function refresh(): void {
  const ask = ++asked;
  void getBackend()
    .then((backend) => backend.playedTracks())
    .then((ids) => {
      if (ask !== asked) return;
      current = ids.length === 0 ? NONE : new Set(ids);
      for (const listener of listeners) listener();
    })
    .catch(() => undefined);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (stops === null) {
    stops = [];
    const held = stops;
    void getBackend().then((backend) => {
      // Unsubscribed before the backend came: nothing to listen for.
      if (stops !== held) return;
      held.push(backend.onPlayedChanged(refresh), backend.onLinkStatus(refresh));
      refresh();
    });
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && stops !== null) {
      for (const stop of stops) stop();
      stops = null;
      current = NONE;
    }
  };
}

const snapshot = () => current;

export function usePlayedTracks(): ReadonlySet<string> {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}
