/**
 * One cue write per track at a time, with its failure reported rather than thrown.
 *
 * Shared by the MEMORY cluster and the hot cue pads. Every write goes through
 * the backend's cue commands and nothing is shown ahead of it: the backend
 * re-reads the track and says so, `useTrackCues` refetches, and the pads, the
 * markers and the list follow from the same array. A cue write is one indexed
 * row, so there is no reload to wait through.
 */
import { useCallback, useLayoutEffect, useMemo, useRef } from "react";

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
 * A `write(action)` that runs one action per track at a time.
 *
 * A held key repeats thirty times a second, and each repeat used to be
 * another cue at the same point until the first came back; the guard drops
 * later presses for that track. Only the currently committed owner hears
 * the failure, or `null` once its write lands.
 */
export function useCueWriter(
  onError: ((message: string | null) => void) | undefined,
  owner: string | null,
): (action: (edits: CueEdits) => Promise<unknown>) => void {
  const inFlight = useRef(new Set<string | null>());
  const scope = useMemo(() => ({ owner }), [owner]);
  const current = useRef<typeof scope | null>(null);
  // Only a committed selection owns notifications. An abandoned render must
  // not retire the visible track, and A remains locked across A-B-A.
  useLayoutEffect(() => {
    current.current = scope;
    return () => { if (current.current === scope) current.current = null; };
  }, [scope]);
  return useCallback(
    (action: (edits: CueEdits) => Promise<unknown>) => {
      if (current.current !== scope || inFlight.current.has(owner)) return;
      inFlight.current.add(owner);
      void (async () => {
        try {
          const backend = await getBackend();
          await action(backend.edits);
          if (current.current === scope) onError?.(null);
        } catch (error) {
          if (current.current === scope) onError?.(describe(error));
        } finally {
          inFlight.current.delete(owner);
        }
      })();
    },
    [onError, owner, scope],
  );
}
