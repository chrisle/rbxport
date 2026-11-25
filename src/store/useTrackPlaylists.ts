/** One bounded selection read; old owners never display a newer owner's paths. */
import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "@/lib/errorMessage";
import { getBackend } from "@/ipc/client";
import type { RelatedPlaylist } from "@/ipc/types";

export function reportPlaylistError(error: unknown): void {
  void getBackend().then(backend => backend.logStatusMessage(true, errorMessage(error))).catch(() => {});
}

export function useTrackPlaylists(track: string | null, owner: string): RelatedPlaylist[] | null {
  const request = useMemo(() => ({ owner, track }), [owner, track]);
  const [answer, setAnswer] = useState<[request: typeof request, paths: RelatedPlaylist[] | null]>();
  useEffect(() => {
    if (request.track === null) return;
    let live = true;
    void getBackend().then(backend => backend.trackPlaylists(request.track!)).then(paths => {
      if (live) setAnswer([request, paths]);
    }).catch(error => {
      if (!live) return;
      setAnswer([request, null]);
      reportPlaylistError(error);
    });
    return () => { live = false; };
  }, [request]);
  return answer?.[0] === request ? answer[1] : null;
}
