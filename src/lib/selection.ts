/**
 * Track-table selection model.
 *
 * Selection is by row id, not row index, so it survives a re-sort. A range
 * selection can span rows the frontend has not fetched, so the caller resolves
 * ranges through the backend rather than from cached pages.
 */
export interface SelectionState {
  ids: ReadonlySet<string>;
  /** Row index the next shift-click extends from. */
  anchorIndex: number | null;
}

export const emptySelection: SelectionState = { ids: new Set(), anchorIndex: null };

export type ClickModifier = "none" | "toggle" | "range";

export function modifierFor(e: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean }): ClickModifier {
  if (e.shiftKey) return "range";
  return e.metaKey || e.ctrlKey ? "toggle" : "none";
}

/** The parts of a mouse event the press rules read. */
export interface Press {
  button: number;
  shiftKey: boolean;
  metaKey: boolean;
  ctrlKey: boolean;
}

/**
 * Whether a press asks for the context menu rather than a selection.
 *
 * The right button everywhere, and on macOS a primary press with Control
 * held: that is the system's secondary click, and the browser sends it as a
 * left-button `mousedown` with `ctrlKey` before the `contextmenu` [OBS
 * WebKit and Chromium on macOS, issue #135]. Read as a ⌘-style toggle it
 * dropped the row from the selection, and the menu then acted on that one
 * row alone: Analyze Track over three tracks analysed one.
 */
export function contextPress(e: Press, mac = false): boolean {
  return e.button === 2 || (mac && e.button === 0 && e.ctrlKey && !e.metaKey);
}

/**
 * How a press changes the selection: a context press is a plain click on the
 * row (it selects a row outside the selection, alone, as rekordbox does), and
 * anything else follows its modifier keys.
 */
export function pressModifier(e: Press, mac = false): ClickModifier {
  return contextPress(e, mac) ? "none" : modifierFor(e);
}

/**
 * Whether a press on a row applies the click at once.
 *
 * Not on a row already selected, when the press is plain: that is the start
 * of a drag as often as a click, and what gets dragged is the selection the
 * row is in — collapsing it on the press dropped one track on a playlist
 * where five were chosen. The release settles it (`clickSettles`). A context
 * press on a selected row keeps it too, since the menu that follows acts on
 * the selection; rekordbox keeps it in both cases, as every native list does.
 * A modified press (shift, ⌘/ctrl) is never a drag's start and applies now.
 */
export function pressSelects(e: Press, selected: boolean, mac = false): boolean {
  if (!selected) return true;
  if (contextPress(e, mac)) return false;
  return modifierFor(e) !== "none";
}

/**
 * Whether a click — a press and a release with no drag between — applies
 * the click a plain press on a selected row held back. The browser sends no
 * click after a drag, which is what makes the two tell apart. WebKit sends
 * one after a macOS Control-click too; that one was a context press and
 * leaves the selection alone.
 */
export function clickSettles(e: Press, selected: boolean, mac = false): boolean {
  return selected && e.button === 0 && !contextPress(e, mac) && modifierFor(e) === "none";
}

/**
 * Applies a click. `rangeIds` must be supplied by the caller for "range"
 * clicks — it is the ids between the anchor and the clicked index inclusive.
 */
export function applyClick(
  state: SelectionState,
  clicked: { id: string; index: number },
  modifier: ClickModifier,
  rangeIds?: readonly string[],
): SelectionState {
  switch (modifier) {
    case "none":
      return { ids: new Set([clicked.id]), anchorIndex: clicked.index };
    case "toggle": {
      const next = new Set(state.ids);
      if (next.has(clicked.id)) next.delete(clicked.id);
      else next.add(clicked.id);
      return { ids: next, anchorIndex: clicked.index };
    }
    case "range": {
      if (state.anchorIndex === null || !rangeIds) {
        return { ids: new Set([clicked.id]), anchorIndex: clicked.index };
      }
      // Anchor stays put so successive shift-clicks grow from the same origin.
      return { ids: new Set(rangeIds), anchorIndex: state.anchorIndex };
    }
  }
}

/**
 * Selects every row in the view.
 *
 * The ids are resolved by the backend for the whole view, so a selection can
 * cover rows the frontend never fetched. The anchor is kept so a following
 * shift-click still grows from where it was, and falls back to the top for a
 * selection made from nothing.
 */
export function selectAll(state: SelectionState, ids: readonly string[]): SelectionState {
  return { ids: new Set(ids), anchorIndex: state.anchorIndex ?? (ids.length > 0 ? 0 : null) };
}

export function isSelected(state: SelectionState, id: string): boolean {
  return state.ids.has(id);
}

/**
 * The tracks behind a selection, for queueing analysis.
 *
 * Every selected id is returned, whether or not its row is on a fetched page.
 * The row cache is a bounded LRU (about 6,400 rows), so resolving the
 * selection through cached rows silently dropped everything beyond it: select
 * all on 30,000 tracks queued about 6,000. A title is only a label; an id the
 * cache no longer holds is labelled by the id.
 */
export function selectedTracks(
  ids: Iterable<string>,
  titles: ReadonlyMap<string, string>,
): { id: string; title: string }[] {
  const tracks: { id: string; title: string }[] = [];
  for (const id of ids) tracks.push({ id, title: titles.get(id) ?? id });
  return tracks;
}

/**
 * The selected ids in the order the list shows them, given the list's ids
 * top to bottom. An id the list does not show (its track has gone) goes
 * last, in selection order.
 *
 * A selection is a set in the order its rows were picked, so after a
 * ⌘-click it is click order. rekordbox hands its information panel the
 * selected tracks top to bottom: `BrowseBasicView::changedSelectedRows`
 * walks the list's `juce::SparseSet` of selected row indices, which JUCE
 * keeps sorted, from the lowest up (7.2.11, static analysis). The panel
 * shows the first track's colour, so the order decides which one it shows.
 */
export function inListOrder(ids: ReadonlySet<string>, listed: Iterable<string>): string[] {
  const ordered: string[] = [];
  const placed = new Set<string>();
  for (const id of listed) {
    if (ids.has(id) && !placed.has(id)) {
      placed.add(id);
      ordered.push(id);
      if (placed.size === ids.size) return ordered;
    }
  }
  for (const id of ids) if (!placed.has(id)) ordered.push(id);
  return ordered;
}
