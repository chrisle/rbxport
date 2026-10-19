/**
 * The browser's column layout, remembered between sessions.
 *
 * rekordbox persists per-context widths in `browseSetting.xml`; this is the
 * same idea in the one place a browser-only build can keep it. The rules
 * themselves are pure functions in `@/lib/columns` — this only holds the
 * state and writes it back.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  autoSizeAll,
  autoSizeColumn,
  defaultLayout,
  folderLayout,
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

/** What each context opens with. Only the Explorer's differs from the rest. */
function defaultFor(context: ColumnContext): () => Layout {
  return context === "folder" ? folderLayout : defaultLayout;
}

const STORAGE_PREFIX = "rbl.columns.v2";

function keyFor(context: ColumnContext): string {
  return `${STORAGE_PREFIX}.${context}`;
}

function load(context: ColumnContext): Layout {
  try {
    const raw = localStorage.getItem(keyFor(context));
    // sanitise handles null, so a first run needs no special case.
    return sanitise(raw === null ? null : JSON.parse(raw), defaultFor(context));
  } catch {
    // Private browsing, a disabled store, or a half-written value: the table
    // must still render, so fall back rather than throw on the way up.
    return defaultFor(context)();
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

export function useColumns(context: ColumnContext): Columns {
  const [layout, setLayout] = useState<Layout>(() => load(context));

  // Switching context loads that table's own layout. Reading in an effect
  // rather than during render keeps the two stores from crossing over when
  // the context changes and a save is still pending.
  const loaded = useRef(context);
  useEffect(() => {
    if (loaded.current === context) return;
    loaded.current = context;
    setLayout(load(context));
  }, [context]);

  useEffect(() => {
    // Guard against writing the previous context's layout under the new key
    // in the render between the context changing and the load above.
    if (loaded.current !== context) return;
    try {
      localStorage.setItem(keyFor(context), JSON.stringify(layout));
    } catch {
      // Not being able to remember the layout is not a reason to break the
      // session that is running.
    }
  }, [layout, context]);

  const columns = useMemo(() => resolve(layout), [layout]);

  const toggle = useCallback((key: ColumnKey) => setLayout((l) => toggleColumn(l, key)), []);
  const move = useCallback((key: ColumnKey, to: number) => setLayout((l) => moveColumn(l, key, to)), []);
  const resize = useCallback(
    (key: ColumnKey, width: number) => setLayout((l) => resizeColumn(l, key, width)),
    [],
  );
  const autoSize = useCallback((key: ColumnKey) => setLayout((l) => autoSizeColumn(l, key)), []);
  const autoSizeEvery = useCallback(() => setLayout(autoSizeAll), []);
  const reset = useCallback(() => setLayout(defaultFor(context)()), [context]);

  // Memoised as a whole, not just its callbacks. A fresh object on every
  // render is a changed dependency for everything that takes the hook's value
  // rather than its parts, and one such effect calling back into the app was
  // re-rendering the whole window several hundred times a second.
  return useMemo(
    () => ({ layout, columns, toggle, move, resize, autoSize, autoSizeEvery, reset }),
    [layout, columns, toggle, move, resize, autoSize, autoSizeEvery, reset],
  );
}
