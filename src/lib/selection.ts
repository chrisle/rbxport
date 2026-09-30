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
 * Whether a press on a row applies the click at once.
 *
 * Not on a row already selected, when the press is plain: that is the start
 * of a drag as often as a click, and what gets dragged is the selection the
 * row is in — collapsing it on the press dropped one track on a playlist
 * where five were chosen. The release settles it (`clickSettles`). The right
 * button on a selected row keeps it too, since the menu that follows acts on
 * the selection; rekordbox keeps it in both cases, as every native list does.
 * A modified press (shift, ⌘/ctrl) is never a drag's start and applies now.
 */
export function pressSelects(e: Press, selected: boolean): boolean {
  if (!selected) return true;
  if (e.button === 2) return false;
  return modifierFor(e) !== "none";
}

/**
 * Whether a click — a press and a release with no drag between — applies
 * the click a plain press on a selected row held back. The browser sends no
 * click after a drag, which is what makes the two tell apart.
 */
export function clickSettles(e: Press, selected: boolean): boolean {
  return selected && e.button === 0 && modifierFor(e) === "none";
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
