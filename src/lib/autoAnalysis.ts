/**
 * rekordbox's launch prompt for Auto Analysis.
 *
 * [OBS] rekordbox 7.2.14 on chris-win11, Auto Analysis on and unanalysed
 * tracks in Collection: at launch it opens Analysis Setting with "Auto
 * Analysis is starting." and OK/Cancel. Cancel analyses nothing.
 * [ASSUME] OK analyses those tracks with the settings shown, and with Auto
 * Analysis off there is no prompt; the wording says as much, but neither was
 * run on the rig because OK writes to its library.
 */
import type { Backend, UnanalysedTracks } from "@/ipc/types";

/** Tracks asked for at once; the backend's page cap. */
export const UNANALYSED_PAGE = 128;

type Track = UnanalysedTracks["tracks"][number];

/**
 * The first page of tracks to offer at launch, or null when there is nothing
 * to ask about: Auto Analysis is off, the library cannot be written, or every
 * track is analysed.
 */
export async function autoAnalysisOffer(
  backend: Pick<Backend, "unanalysedTracks">,
  { auto, readOnly }: { auto: boolean; readOnly: boolean },
): Promise<UnanalysedTracks | null> {
  if (!auto || readOnly) return null;
  const page = await backend.unanalysedTracks(0, UNANALYSED_PAGE);
  return page.tracks.length > 0 ? page : null;
}

/**
 * Hands every page from row `next` on to `take`, in order, so a library with
 * thousands of unanalysed tracks queues them a page at a time.
 */
export async function takeRemainingPages(
  backend: Pick<Backend, "unanalysedTracks">,
  next: number | null,
  take: (tracks: readonly Track[]) => void,
): Promise<void> {
  let from = next;
  while (from !== null) {
    const page = await backend.unanalysedTracks(from, UNANALYSED_PAGE);
    if (page.tracks.length > 0) take(page.tracks);
    // A page that does not move on would loop for ever.
    from = page.next !== null && page.next > from ? page.next : null;
  }
}
