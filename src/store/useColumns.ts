/**
 * The browser's column layout, remembered between sessions.
 *
 * rekordbox persists per-context widths in `browseSetting.xml`; this is the
 * same idea in the one place a browser-only build can keep it. The rules
 * themselves are pure functions in `@/lib/columns` — this only holds the
 * state and writes it back.
 */
import { useCallback, useEffect, useMemo, useState } from "react";

import {
  autoSizeAll,
  autoSizeColumn,
  defaultLayout,
  folderLayout,
  fullArtworkLayout,
  moveColumn,
  resizeColumn,
  resolve,
  sanitise,
  toggleColumn,
  type ColumnKey,
  type ColumnSpec,
  type Layout,
} from "@/lib/columns";
import type { TreeNode } from "@/ipc/types";
import type { ArtworkMode } from "@/lib/session";

/**
 * Which table a layout belongs to.
 *
 * rekordbox keys these by context in `browseSetting.xml` — `TableHeader-<
 * Context>`, thirty of them — and by the *kind* of table rather than by each
 * individual playlist, so browsing a second playlist does not start from
 * scratch.
 */
/// The sub-browser keeps its own set: it is usually left narrow, and sharing
/// the main table's widths would make it unusable.
export type ColumnContext = "collection" | "playlist" | "history" | "subBrowser" | "folder";

/**
 * The layout a tree node's table uses.
 *
 * rekordbox names the layout from the list's type and never from the list
 * itself: `browse::BrowseHeaderManager::getXmlKeyName` (rekordbox 7.2, macOS)
 * picks `TableHeader-PlaylistTracks` for every list of the playlist type that
 * is not an intelligent one, a playlist folder included. So a folder of
 * playlists shows, and changes, the same columns as the playlists in it
 * (#596), rather than the collection's.
 */
export function columnContextFor(kind: TreeNode["kind"] | undefined): ColumnContext {
  switch (kind) {
    case "playlist":
    case "smartPlaylist":
    case "folder":
    case "devicePlaylist":
    case "deviceFolder":
      return "playlist";
    case "history":
      return "history";
    case "directory":
    case "explorer":
      return "folder";
    default:
      return "collection";
  }
}

/**
 * What each context opens with. Only the Explorer's differs from the rest.
 * Showing the whole artwork, each table opens with the whole-artwork form of
 * its own default; the Explorer's keeps the columns Summary repeats, as
 * rekordbox's folder table does.
 */
function defaultFor(context: ColumnContext, mode: ArtworkMode = "partial"): () => Layout {
  const base = context === "folder" ? folderLayout : defaultLayout;
  return mode === "full" ? () => fullArtworkLayout(base(), context !== "folder") : base;
}

const STORAGE_PREFIX = "rbl.columns.v2";

/**
 * Where a table's layout is kept. rekordbox keeps the whole artwork's apart,
 * under the same name with `-FullArtwork` on the end, so each mode has its
 * own columns and widths and switching back brings the other's back.
 */
export function columnStorageKey(context: ColumnContext, mode: ArtworkMode = "partial"): string {
  return `${STORAGE_PREFIX}.${context}${mode === "full" ? ".full" : ""}`;
}

function load(context: ColumnContext, mode: ArtworkMode): Layout {
  try {
    const raw = localStorage.getItem(columnStorageKey(context, mode));
    // sanitise handles null, so a first run needs no special case.
    return sanitise(raw === null ? null : JSON.parse(raw), defaultFor(context, mode), mode);
  } catch {
    // Private browsing, a disabled store, or a half-written value: the table
    // must still render, so fall back rather than throw on the way up.
    return defaultFor(context, mode)();
  }
}

export interface Columns {
  layout: Layout;
  /** Visible columns, in order, at their current widths. */
  columns: ColumnSpec[];
  toggle: (key: ColumnKey) => void;
  move: (key: ColumnKey, to: number) => void;
  resize: (key: ColumnKey, width: number) => void;
  autoSize: (key: ColumnKey) => void;
  autoSizeEvery: () => void;
  reset: () => void;
}

export function useColumns(context: ColumnContext, mode: ArtworkMode = "partial"): Columns {
  const storageKey = columnStorageKey(context, mode);
  // The layout travels with the key it belongs to, so a save can never write
  // one table's or one mode's columns under another's name.
  const [held, setHeld] = useState(() => ({ key: storageKey, layout: load(context, mode) }));
  // Switching context, or artwork mode, loads that table's own layout in the
  // same render, so the new mode never draws the old mode's columns.
  let current = held;
  if (held.key !== storageKey) {
    current = { key: storageKey, layout: load(context, mode) };
    setHeld(current);
  }
  const layout = current.layout;
  const setLayout = useCallback(
    (next: Layout | ((layout: Layout) => Layout)) =>
      setHeld((state) => ({ ...state, layout: typeof next === "function" ? next(state.layout) : next })),
    [],
  );

  useEffect(() => {
    if (held.key !== storageKey) return;
    try {
      localStorage.setItem(held.key, JSON.stringify(held.layout));
    } catch {
      // Not being able to remember the layout is not a reason to break the
      // session that is running.
    }
  }, [held, storageKey]);

  const columns = useMemo(() => resolve(layout, mode), [layout, mode]);

  const toggle = useCallback((key: ColumnKey) => setLayout((l) => toggleColumn(l, key, mode)), [setLayout, mode]);
  const move = useCallback((key: ColumnKey, to: number) => setLayout((l) => moveColumn(l, key, to)), [setLayout]);
  const resize = useCallback(
    (key: ColumnKey, width: number) => setLayout((l) => resizeColumn(l, key, width)),
    [setLayout],
  );
  const autoSize = useCallback((key: ColumnKey) => setLayout((l) => autoSizeColumn(l, key)), [setLayout]);
  const autoSizeEvery = useCallback(() => setLayout(autoSizeAll), [setLayout]);
  const reset = useCallback(() => setLayout(defaultFor(context, mode)()), [setLayout, context, mode]);

  // Memoised as a whole, not just its callbacks. A fresh object on every
  // render is a changed dependency for everything that takes the hook's value
  // rather than its parts, and one such effect calling back into the app was
  // re-rendering the whole window several hundred times a second.
  return useMemo(
    () => ({ layout, columns, toggle, move, resize, autoSize, autoSizeEvery, reset }),
    [layout, columns, toggle, move, resize, autoSize, autoSizeEvery, reset],
  );
}
