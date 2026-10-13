/**
 * Track › Locate track loaded on deck (command + L, rekordbox's b139), and
 * the per-list scroll position the browser returns to.
 *
 * Both follow rekordbox 7.2.19's own code (macOS arm64, read by disassembly):
 *
 * - `BrowseBasicView::performCommand` sends 0xb139 to
 *   `BrowseBasicView::browseLoadedTrack()`, which keeps one function-static
 *   player cursor (`browseLoadedTrack()::myPlayerNo`, zero at start, so Deck
 *   1). Each press looks at the cursor's deck and then the next ones, Deck 1
 *   to 4 and round again, takes the first that holds a track, and moves the
 *   cursor to the deck after it. With two decks loaded the presses alternate
 *   between them. A deck holding something that is not a collection track
 *   still takes its turn but locates nothing.
 * - `BrowseBasicView::browseCenterAt(id)` then selects the track's first row
 *   in the open list, scrolling it to the middle only when it is not wholly
 *   on screen. When the open list does not have it, it selects Collection in
 *   the tree (`CustomTreeViewItem::setSelected(true, true, …)` on the item
 *   `TreeViewer::addCollectionRoot` made) and looks there.
 * - `ListViewer::restoreSelectedRowsAndPosition` puts a list back where it
 *   was left (`RowStateList`, one state per list): the saved view position,
 *   but only while the list is sorted as it was and the viewport is as tall as
 *   it was. A list without a saved state opens at the top.
 */

import { isLooseId } from "./explorer";

/** rekordbox's decks: the cursor runs over four whether or not all are shown. */
export const LOCATE_DECKS = 4;

export interface LocateStep {
  /** The deck whose track to locate, 0 for Deck 1; null when none is loaded. */
  deck: number | null;
  /** The collection track to locate, or null when there is nothing to find. */
  trackId: string | null;
  /** Where the next press starts. */
  cursor: number;
}

/**
 * Whether a deck's track is one the collection holds. A file played from the
 * Explorer or a stick (`file:`, `itunes:`) is not, and rekordbox locates
 * nothing for it.
 */
export function isCollectionTrack(id: string): boolean {
  return !isLooseId(id) && /^\d+$/.test(id);
}

/**
 * One press of Locate track loaded on deck. `decks[i]` is the track id Deck
 * `i + 1` holds, or null; decks past the end of the array are empty.
 */
export function locateStep(cursor: number, decks: readonly (string | null | undefined)[]): LocateStep {
  const start = ((cursor % LOCATE_DECKS) + LOCATE_DECKS) % LOCATE_DECKS;
  for (let k = 0; k < LOCATE_DECKS; k++) {
    const deck = (start + k) % LOCATE_DECKS;
    const id = decks[deck];
    if (!id) continue;
    return { deck, trackId: isCollectionTrack(id) ? id : null, cursor: (deck + 1) % LOCATE_DECKS };
  }
  return { deck: null, trackId: null, cursor: start };
}

/**
 * How far the list must scroll to show a row, as rekordbox's `browseCenterAt`
 * does: not at all when the row is wholly in view, otherwise so that it sits
 * in the middle. `top` is the row's offset in the scroller, `headerH` the
 * sticky heading that covers the top of the view.
 */
export function needsCentering(top: number, rowH: number, scrollTop: number, viewH: number, headerH: number): boolean {
  return top < scrollTop + headerH || top + rowH > scrollTop + viewH;
}

/** A list's position as it was left, and what it depended on. */
export interface SavedScroll {
  top: number;
  /** The sort it was in, as a comparable string. */
  sort: string;
  /** The viewport's height then. */
  height: number;
}

/**
 * Where a list opens again: its saved position while the sort and the
 * viewport's height are as they were, and otherwise the top.
 */
export function restoredScrollTop(saved: SavedScroll | undefined, sort: string, height: number): number {
  if (!saved) return 0;
  return saved.sort === sort && saved.height === height ? saved.top : 0;
}
