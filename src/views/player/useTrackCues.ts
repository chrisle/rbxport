/**
 * A track's cues, kept current.
 *
 * Fetched when the track changes, and fetched again whenever the backend says
 * the track's cues changed — after an edit from this deck, the other deck,
 * or the hot-cue pads — and after any full reload of the library. One small
 * IPC call each time; the markers on both waveforms and the list beside the
 * deck all draw from this one array, so they move together and nothing
 * flashes.
 */
import { useEffect, useRef, useState } from "react";

import type { Cue, RowDto } from "@/ipc/types";
import { getBackend } from "@/ipc/client";

export function useTrackCues(
  track: RowDto | null,
  /**
   * Called with the cues of a newly loaded track, once. The deck opens a
   * track on its first memory cue, and that is decided here rather than on
   * every refetch — a cue added mid-track must not move the cue point.
   */
  onLoaded?: (cues: Cue[]) => void,
): Cue[] {
  const [cues, setCues] = useState<Cue[]>([]);
  // A ref so the fetch effect does not re-run when the callback's identity
  // does; the deck passes a fresh closure each render.
  const loaded = useRef(onLoaded);
  loaded.current = onLoaded;

  useEffect(() => {
    if (!track) {
      setCues([]);
      return;
    }
    const id = track.id;
    const device = track.deviceSource;
    let live = true;
    let first = true;
    let stopCues: (() => void) | undefined;
    let stopLibrary: (() => void) | undefined;

    const fetch = async () => {
      const backend = await getBackend();
      const found = await backend.trackCues(id, device);
      // The track may have changed while this was in flight.
      if (!live) return;
      setCues(found);
      if (first) {
        first = false;
        loaded.current?.(found);
      }
    };
    // Returning from rekordbox is when its cue edits become visible here.
    const onFocus = () => { void fetch(); };
    window.addEventListener("focus", onFocus);

    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      // Subscribed before the first fetch lands, so an edit made in the gap
      // is not missed.
      stopCues = backend.onCuesChanged((changed) => {
        if (changed === id) void fetch();
      });
      stopLibrary = backend.onLibraryChanged(() => {
        void fetch();
      });
      await fetch();
    })();

    return () => {
      live = false;
      window.removeEventListener("focus", onFocus);
      stopCues?.();
      stopLibrary?.();
    };
  }, [track]);

  return cues;
}
