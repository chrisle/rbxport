/** The shell outlives hidden Players; confirmed ownership is not a selection. */
import { useEffect, useState } from "react";
import { getBackend } from "@/ipc/client";
import type { DeckId, LoadedTrack } from "@/ipc/types";

export const confirmedLoads = new Map<DeckId, LoadedTrack | null>();
export function useLoadedTracks() {
  const state = useState<readonly (LoadedTrack | null)[]>([]);
  const [, setOwners] = state;
  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    void getBackend().then(backend => {
      if (live) stop = backend.onDeckReset?.(() => {
        confirmedLoads.clear();
        setOwners([]);
      });
    });
    return () => { live = false; stop?.(); };
  }, [setOwners]);
  return state;
}
