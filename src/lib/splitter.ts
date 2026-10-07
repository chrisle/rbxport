/**
 * Where a draggable splitter may sit.
 *
 * Separate from the component so the clamping is testable without a DOM, and
 * so the rule that the panel can never eat the window lives in one place.
 */

export interface SplitterBounds {
  /** Total width available to both panes. */
  available: number;
  /** Narrowest the panel may be dragged. */
  min: number;
  /** Widest, as a fraction of `available`, so a small window cannot be filled. */
  maxFraction: number;
  /** The pane on the other side never drops below this. */
  minOther: number;
}

export const TREE_BOUNDS: SplitterBounds = {
  available: 0,
  min: 140,
  maxFraction: 0.5,
  minOther: 360,
};

/**
 * Clamps a requested width to what the window can give.
 *
 * When the window is too narrow to satisfy both minimums the panel loses —
 * the track list is the point of the window, and a tree wider than the list it
 * is filtering helps nobody.
 */
export function clampWidth(requested: number, bounds: SplitterBounds): number {
  const { available, min, maxFraction, minOther } = bounds;
  if (!Number.isFinite(requested) || available <= 0) return min;

  const ceiling = Math.min(available * maxFraction, available - minOther);
  // A window narrower than both minimums together: give the panel the floor
  // and let the other pane scroll.
  if (ceiling <= min) return Math.min(min, Math.max(0, available));
  return Math.round(Math.min(Math.max(requested, min), ceiling));
}

/** Re-clamps a stored width after the window changes size. */
export function fit(current: number, bounds: SplitterBounds): number {
  return clampWidth(current, bounds);
}

/**
 * The sub-browser's outer splitter, between the main track list and the
 * sub-browser's own rail.
 *
 * `available` is what the two lists share: the row, less the main tree and
 * whatever other panels are open. The floor is a rail, a tree at its own
 * floor and a list with room for a title: below that the panel is two
 * scrollbars. The other side keeps the same floor the tree splitter keeps
 * for the main list, so opening the sub-browser in a narrow window shrinks
 * it rather than the list it sits beside.
 */
export function subBounds(available: number): SplitterBounds {
  return {
    available,
    min: 360,
    maxFraction: 0.65,
    minOther: TREE_BOUNDS.minOther,
  };
}

/** The splitter inside the sub-browser, between its tree and its list. */
export function subTreeBounds(available: number): SplitterBounds {
  return { ...TREE_BOUNDS, available };
}
