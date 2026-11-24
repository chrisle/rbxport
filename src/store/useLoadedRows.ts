/** Loaded-file markers use the current viewport and confirmed deck owners,
 * never transport ticks or the whole collection. Superseded answers stay hidden. */
import { useEffect, useMemo, useState } from "react";
import type { LoadedTrack } from "@/ipc/types";
import { getBackend } from "@/ipc/client";

const NONE: ReadonlySet<string> = new Set();
export function useLoadedRows(rows: readonly string[], tracks: readonly (LoadedTrack | null)[], owner: string): ReadonlySet<string> {
  const key = JSON.stringify([rows, tracks.filter(Boolean), owner]);
  const identity = useMemo(() => ({ key }), [key]);
  const [answer, setAnswer] = useState<{ identity: typeof identity; ids: ReadonlySet<string> }>();
  useEffect(() => {
    const [ids, loaded] = JSON.parse(identity.key) as [string[], LoadedTrack[]];
    if (!ids.length || !loaded.length) return;
    let live = true;
    void getBackend().then(backend => Promise.all(Array.from({ length: Math.ceil(ids.length / 128) },
      (_, at) => backend.loadedRows(ids.slice(at * 128, (at + 1) * 128), loaded))))
      .then(parts => { if (live) setAnswer({ identity, ids: new Set(parts.flat()) }); }).catch(() => {});
    return () => { live = false; };
    // Serialized identity keeps row-window allocations from issuing a request.
  }, [identity]);
  return answer?.identity === identity ? answer.ids : NONE;
}
