/**
 * The view a tree selection asks the backend for.
 *
 * Held here because there is more than one browser now — the main one and the
 * sub-browser, each with its own selection — and two copies of this would
 * drift the moment a source kind was added.
 */
import type { KeyDisplay, SortColumn, TreeNode, ViewSpec } from "@/ipc/types";
import { deviceSource } from "./deviceLibrary";
import { explorerPath } from "./explorer";
import { relatedCriterionOf } from "./tree";

export interface SortState {
  column: SortColumn;
  descending: boolean;
}

/**
 * The view's own order: a playlist's membership, the collection's rows.
 *
 * `trackNo` is not a column the backend ranks by — it means "leave the rows in
 * the order this view produced them". That is what makes it the third state of
 * the sort cycle, and what a playlist has to open in: sorting a playlist by
 * anything at all destroys the order somebody put it in.
 */
export const NO_SORT: SortState = { column: "trackNo", descending: false };

/** What a view opens with, which is its own order. */
export const DEFAULT_SORT: SortState = NO_SORT;

/** The spec for a selected node, or the whole collection when none is. */
export function specForNode(
  node: TreeNode | null,
  query: string,
  sort: SortState | null,
  /** How the Key column is shown: classic names or Camelot codes. */
  keyDisplay: KeyDisplay = "classic",
  /** Alphabetical key names, or their order around the Camelot wheel. */
  keySort: "alphabetical" | "musical" = keyDisplay === "alphanumeric" ? "musical" : "alphabetical",
  /**
   * The track Related Tracks relates to: the one on the player. None, and
   * the section's criteria open empty, as rekordbox's do with no track
   * loaded.
   */
  relatedTo: string | null = null,
  /** Bumped after an edit to a stick, so a view of its library opens again. */
  deviceRevision = 0,
): ViewSpec {
  const order = sort ?? DEFAULT_SORT;
  // A stick's own library is read from the stick.
  const device = node ? deviceSource(node, deviceRevision) : null;
  return {
    // A playlist, its parent folder, or a history session narrows the view.
    // History folders still have no members of their own; a device row is
    // its settings panel, and what is under it opens through `deviceSource`.
    // The Explorer's folders open as themselves, and its heading as an empty
    // folder: rekordbox shows an Explorer with nothing in it there.
    // An intelligent playlist is asked for as a playlist: the backend knows
    // which of its playlists are rules and answers with what the rule admits.
    source: device ?? (
      node?.kind === "playlist" || node?.kind === "smartPlaylist"
        ? { kind: "playlist", id: node.id }
        : node?.kind === "folder"
          ? { kind: "playlistFolder", id: node.id }
        : node?.kind === "history"
          ? { kind: "history", id: node.id }
          : node?.kind === "directory"
            ? { kind: "folder", path: explorerPath(node.id) ?? "" }
            : node?.kind === "explorer"
              ? { kind: "folder", path: "" }
              : node?.kind === "relatedCriterion" || node?.kind === "related"
                ? { kind: "related", track: relatedTo ?? "", criterion: relatedCriterionOf(node.id) ?? "bpmKey" }
                : node?.kind === "tagList"
                  ? { kind: "tagList" }
                  : { kind: "collection" }),
    sort: order.column === "key" && keySort === "musical" ? "keyCamelot" : order.column,
    descending: order.descending,
    query,
  };
}

/**
 * Clicking a heading cycles ascending, descending, off.
 *
 * "Off" is the view's own order rather than another column's: a playlist that
 * could not be put back the way it was would make sorting it a one-way door.
 */
export function nextSort(current: SortState, column: SortColumn): SortState {
  if (current.column !== column) return { column, descending: false };
  if (!current.descending) return { column, descending: true };
  return NO_SORT;
}
