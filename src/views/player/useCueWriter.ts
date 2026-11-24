/**
 * One cue write at a time, with its failure reported rather than thrown.
 *
 * Shared by the MEMORY cluster and the hot cue pads. Every write goes through
 * the backend's cue commands and nothing is shown ahead of it: the backend
 * re-reads the track and says so, `useTrackCues` refetches, and the pads, the
 * markers and the list follow from the same array. A cue write is one indexed
 * row, so there is no reload to wait through.
 */
import { useCallback, useRef } from "react";

import { getBackend } from "@/ipc/client";

/** What the interface says when the library cannot be written to. */
export const READ_ONLY_REASON = "The library is read-only. Check Library Protection in Preferences, and quit rekordbox to edit.";

/** Cue commands address collection rows, never device or loose file IDs. */
export function cueWriteReason(trackId: string | null, readOnly: boolean): string | undefined {
  if (trackId?.startsWith("file:")) return "That track is not in the library.";
  return trackId === null || readOnly ? READ_ONLY_REASON : undefined;
}

/** The cue commands, as `write` hands them to an action. */
export type CueEdits = Awaited<ReturnType<typeof getBackend>>["edits"];

function describe(error: unknown): string {
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  if (typeof error === "object" && error !== null) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") return message;
  }
  return "That cue could not be saved.";
}

/**
 * A `write(action)` that runs one action at a time.
 *
 * A held key repeats thirty times a second, and each repeat used to be
 * another cue at the same point until the first came back; the guard drops
 * everything after the first press. `onError` hears the failure, or `null`
 * once a write lands.
 */
export function useCueWriter(
  onError?: (message: string | null) => void,
): (action: (edits: CueEdits) => Promise<unknown>) => void {
  const inFlight = useRef(false);
  return useCallback(
    (action: (edits: CueEdits) => Promise<unknown>) => {
      if (inFlight.current) return;
      inFlight.current = true;
      void (async () => {
        try {
          const backend = await getBackend();
          await action(backend.edits);
          onError?.(null);
        } catch (error) {
          onError?.(describe(error));
        } finally {
          inFlight.current = false;
        }
      })();
    },
    [onError],
  );
}
