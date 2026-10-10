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
import { isLooseId } from "./explorer";

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

/**
 * Whether loading `row` onto a deck should analyse it first, as rekordbox
 * does with a Collection track it has never analysed.
 *
 * [OBS static, rekordbox 7.2.19 macOS arm64] `UiPlayer::handleMessageDragAndDrop`
 * @0x101abc1a8 calls `DatabaseIF::analyzeCollectionLoad`; that
 * (`DatabaseMediator::analyzeCollectionLoad` @0x101499d44) skips a track that
 * is analysed (and not by an older version) and otherwise asks through
 * `AnalysisSettingWindow::show(true, false)` and analyses it. Unlike Auto
 * Analysis on import (`addAnalysis` @0x1014970d8 checks
 * `SettingIF::getCurrentAutoAnalyze` first), it does not look at the Auto
 * Analysis preference. Its Preferences note says the same: "If tracks are not
 * analyzed, BPM/Grid analysis starts when importing, loading, or exporting
 * the tracks".
 *
 * Not for a file outside the library (Explorer, a stick's own tracks), a
 * locked track (`Analysed` has bit 0x80 set, so it is not 0), a library that
 * cannot be written, or a track already waiting in the analysis queue.
 */
export function analysesOnLoad(
  row: { id: string; analysed: number; missing?: boolean },
  { readOnly, queued }: { readOnly: boolean; queued: (id: string) => boolean },
): boolean {
  if (readOnly || row.missing === true || row.analysed !== 0) return false;
  if (isLooseId(row.id)) return false;
  return !queued(row.id);
}

const ASK_ON_LOAD = "rbxport.askAnalysisSettingWindow";

/**
 * Whether loading an unanalysed track asks with the Analysis Setting window
 * first. rekordbox's `askAnalysisSettingWindow` setting: asked until "Don't
 * ask me again" is ticked and confirmed with OK
 * (`AnalysisSettingWindow::show` @0x100923abc, `buttonClicked` @0x1009237e8).
 */
export function asksAnalysisOnLoad(): boolean {
  try {
    return localStorage.getItem(ASK_ON_LOAD) !== "false";
  } catch {
    return true;
  }
}

export function stopAskingAnalysisOnLoad(): void {
  try {
    localStorage.setItem(ASK_ON_LOAD, "false");
  } catch {
    // Asking again next time is the safe way to lose this.
  }
}
