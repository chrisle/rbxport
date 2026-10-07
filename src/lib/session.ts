/**
 * What the interface looked like when it was last closed.
 *
 * The window's own size and position are the shell's job — Tauri's
 * window-state plugin does that. This is everything inside it: which playlist
 * was open, how the panes were split, what it was sorted by, which panels were
 * showing.
 *
 * The search box is deliberately absent. Reopening to a filtered library you
 * did not ask for reads as a broken library, and the one keystroke to clear it
 * is not worth that. So is the deck's track: reopening loads it before the
 * library is up, which fails, and the window comes back showing an error over
 * a track nobody asked to hear.
 *
 * Everything stored is checked on the way back in. A playlist can be deleted
 * between runs, a stored width can come from a wider screen, and a hand-edited
 * value can be anything at all; none of those may produce a broken window.
 */
import { asLayout, type PlayerLayout } from "./layout";
import { DETAIL_BARS, ZOOM_STEPS } from "./player";
import type { RowDto, SortColumn, TreeNode } from "@/ipc/types";
import { DEFAULT_SORT, type SortState } from "./viewSpec";

/**
 * How many rows of the last view are kept.
 *
 * Enough to fill any window at the measured row pitch, and no more: this is
 * the first screen, not a second copy of the library.
 */
export const SEEDED_ROWS = 200;

/** A hard ceiling on the stored tree, so a pathological library cannot fill
 * localStorage. The reference collection has 683 playlists. */
export const SEEDED_NODES = 4000;

export interface Session {
  /** The library tree's width in pixels, before clamping to the window. */
  treeWidth: number;
  /** The tree node that was selected, if it still exists at load. */
  selectedNodeId: string | null;
  /** Folder open/closed choices that override the library's defaults. */
  treeExpansion: TreeExpansion;
  sort: SortState;
  infoOpen: boolean;
  subOpen: boolean;
  /** Whether the track filter bar was showing. Its picks are not kept: a
   * library that opens already narrowed reads as a broken one. */
  filterOpen: boolean;
  /**
   * The last screen, kept so the window can draw itself before the backend has
   * finished reading the library.
   *
   * This is a picture of what was there, not a source of truth: everything in
   * it is replaced the moment the real data arrives. A track edited elsewhere
   * shows its old value for the fraction of a second that takes, which is a
   * far better trade than an empty window for a second and a half.
   */
  tree: TreeNode[];
  rows: RowDto[];
  /** Row count of the view the rows came from, so the scrollbar is right. */
  count: number;
  /** How much of the window the deck took: 1 player, 2, simple, or none. */
  layout: PlayerLayout;
  /** The sub-browser's width, and its own tree's, both before clamping. */
  subWidth: number;
  subTreeWidth: number;
  /** Which deck the browser's Traffic Light reads: the MASTER menu above the list. */
  trafficLight: TrafficLightSource;
  /** Bars visible across each deck's detail waveform. */
  waveformZoom: { a: number; b: number };
}

/**
 * Explicit folder choices in the main library tree.
 *
 * Both sides are needed: most playlist folders arrive open while Explorer
 * folders arrive closed, so absence alone cannot mean the same thing for
 * every node. Explorer ids also let the UI reopen lazy ancestors after a
 * restart and read their children back one level at a time.
 */
export interface TreeExpansion {
  collapsed: string[];
  expanded: string[];
}

/** MASTER DECK, PLAYER A or PLAYER B, as the menu offers them. */
export type TrafficLightSource = "master" | "a" | "b";

const TRAFFIC_LIGHT_SOURCES: readonly TrafficLightSource[] = ["master", "a", "b"];

export const DEFAULT_TREE_WIDTH = 305;
/** Measured from the 2026-09-09 capture: --s-sub-browse-w and --s-sub-tree-w. */
export const DEFAULT_SUB_WIDTH = 878;
export const DEFAULT_SUB_TREE_WIDTH = 298;

export const DEFAULT_SESSION: Session = {
  treeWidth: DEFAULT_TREE_WIDTH,
  selectedNodeId: null,
  treeExpansion: { collapsed: [], expanded: [] },
  sort: DEFAULT_SORT,
  infoOpen: false,
  subOpen: false,
  filterOpen: false,
  tree: [],
  rows: [],
  count: 0,
  layout: "one",
  subWidth: DEFAULT_SUB_WIDTH,
  subTreeWidth: DEFAULT_SUB_TREE_WIDTH,
  trafficLight: "master",
  waveformZoom: { a: DETAIL_BARS, b: DETAIL_BARS },
};

const SORT_COLUMNS: readonly string[] = [
  "trackNo", "title", "artist", "album", "genre", "label", "comment",
  "bpm", "key", "duration", "rating", "djPlayCount", "dateAdded", "releaseDate",
];

function sortOrDefault(value: unknown): SortState {
  if (typeof value !== "object" || value === null) return DEFAULT_SORT;
  const raw = value as { column?: unknown; descending?: unknown };
  if (typeof raw.column !== "string" || !SORT_COLUMNS.includes(raw.column)) {
    return DEFAULT_SORT;
  }
  return { column: raw.column as SortColumn, descending: raw.descending === true };
}

/** A stored pane width, or the default when it is not a usable number. */
function widthOrDefault(value: unknown, fallback: number): number {
  // Clamped again against the window at use; this only rejects nonsense.
  return typeof value === "number" && Number.isFinite(value) && value > 0
    ? Math.round(value)
    : fallback;
}

/** A zoom is one of the discrete steps the waveform controls can select. */
function zoomOrDefault(value: unknown): number {
  return typeof value === "number" && ZOOM_STEPS.includes(value as (typeof ZOOM_STEPS)[number])
    ? value
    : DETAIL_BARS;
}

/** Keeps only entries that are objects carrying a string id. */
function records<T extends { id: string }>(value: unknown, limit: number): T[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter(
      (item): item is T =>
        typeof item === "object" && item !== null && typeof (item as T).id === "string",
    )
    .slice(0, limit);
}

/** A bounded, de-duplicated list of node ids from untrusted storage. */
function nodeIds(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return [...new Set(value.filter((id): id is string => typeof id === "string"))]
    .slice(0, SEEDED_NODES);
}

function treeExpansionOrDefault(value: unknown): TreeExpansion {
  if (typeof value !== "object" || value === null) return { collapsed: [], expanded: [] };
  const raw = value as { collapsed?: unknown; expanded?: unknown };
  const collapsed = nodeIds(raw.collapsed);
  const closed = new Set(collapsed);
  // A corrupt value naming a node on both sides resolves safely to closed.
  const expanded = nodeIds(raw.expanded).filter((id) => !closed.has(id));
  return { collapsed, expanded };
}

/** Turns whatever was stored into a session that will render. */
export function sanitiseSession(value: unknown): Session {
  if (typeof value !== "object" || value === null) return DEFAULT_SESSION;
  const raw = value as Partial<Record<keyof Session, unknown>>;
  return {
    treeWidth: widthOrDefault(raw.treeWidth, DEFAULT_TREE_WIDTH),
    selectedNodeId: typeof raw.selectedNodeId === "string" ? raw.selectedNodeId : null,
    treeExpansion: treeExpansionOrDefault(raw.treeExpansion),
    sort: sortOrDefault(raw.sort),
    infoOpen: raw.infoOpen === true,
    subOpen: raw.subOpen === true,
    filterOpen: raw.filterOpen === true,
    tree: records<TreeNode>(raw.tree, SEEDED_NODES),
    rows: records<RowDto>(raw.rows, SEEDED_ROWS),
    count:
      typeof raw.count === "number" && Number.isFinite(raw.count) && raw.count >= 0
        ? Math.round(raw.count)
        : 0,
    layout: asLayout(raw.layout),
    subWidth: widthOrDefault(raw.subWidth, DEFAULT_SUB_WIDTH),
    subTreeWidth: widthOrDefault(raw.subTreeWidth, DEFAULT_SUB_TREE_WIDTH),
    trafficLight: TRAFFIC_LIGHT_SOURCES.includes(raw.trafficLight as TrafficLightSource)
      ? (raw.trafficLight as TrafficLightSource)
      : "master",
    waveformZoom:
      typeof raw.waveformZoom === "object" && raw.waveformZoom !== null
        ? {
            a: zoomOrDefault((raw.waveformZoom as { a?: unknown }).a),
            b: zoomOrDefault((raw.waveformZoom as { b?: unknown }).b),
          }
        : { a: DETAIL_BARS, b: DETAIL_BARS },
  };
}

const KEY = "rbl.session";

/** Reads the stored session, falling back to the defaults on anything odd. */
export function loadSession(): Session {
  try {
    const raw = localStorage.getItem(KEY);
    return raw === null ? DEFAULT_SESSION : sanitiseSession(JSON.parse(raw));
  } catch {
    // A private window, cleared storage, or a browser that refuses it: the
    // defaults are a working interface, so there is nothing to report.
    return DEFAULT_SESSION;
  }
}

export function saveSession(session: Session): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(session));
  } catch {
    // Not being able to remember the layout is not worth interrupting anyone.
  }
}
