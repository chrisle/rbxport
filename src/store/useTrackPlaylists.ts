/** One bounded selection read; old owners never display a newer owner's paths. */
import { useEffect, useState } from "react";
import { errorMessage } from "@/lib/errorMessage";
import { getBackend } from "@/ipc/client";
import type { RelatedPlaylist } from "@/ipc/types";

export function reportPlaylistError(error: unknown): void {
  void getBackend().then(backend => backend.logStatusMessage(true, errorMessage(error))).catch(() => {});
}

export function useTrackPlaylists(track: string | null, owner: string): RelatedPlaylist[] | null {
  const key = JSON.stringify([owner, track]);
  const [answer, setAnswer] = useState<{ key: string; paths: RelatedPlaylist[] | null }>();
  useEffect(() => {
    if (track === null) return;
    let live = true;
    void getBackend().then(backend => backend.trackPlaylists(track)).then(paths => {
      if (live) setAnswer({ key, paths });
    }).catch(error => {
      if (!live) return;
      setAnswer({ key, paths: null });
      reportPlaylistError(error);
    });
    return () => { live = false; };
  }, [key, track]);
  return track !== null && answer?.key === key ? answer.paths : null;
}
