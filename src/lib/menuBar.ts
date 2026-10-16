/**
 * The native Track and Playlist menus: which item runs which command, and
 * which items are greyed right now.
 *
 * Every item runs the shared command (`commands.ts`) its context-menu entry
 * runs, registered by the track list or the tree that holds the selection.
 * Whether it is live follows rekordbox's own rules, `ListViewer::
 * isActiveCommand` @0x1003dc540 for the Track menu and `TreeViewer::
 * isActiveCommand` @0x10155c434 for the Playlist menu [OBS static, rekordbox
 * 7.2.19 macOS; checked against the Windows menu bar, issue #513], put in
 * terms of what the list and the tree here know. Where rbxport cannot do
 * what rekordbox would, the item stays greyed rather than doing nothing.
 */
import type { BrowserFocus, BrowserSide } from "./browserFocus";
import type { Action } from "./shortcuts";
import type { TrackSource, TreeNode } from "@/ipc/types";

/** What the track list on one side holds, as far as the menus care. */
export interface ListMenuState {
  source: TrackSource["kind"];
  /** How many rows are selected. */
  selected: number;
  /**
   * The selected rows that have been fetched. A selection spanning rows never
   * fetched has fewer, and what is not known is taken to be present.
   */
  rows: readonly { missing?: boolean }[];
  /** Some of the selection is files on disk that are not in the collection. */
  loose: boolean;
}

/** What the tree on one side has selected. */
export interface TreeMenuState {
  kind: TreeNode["kind"] | null;
}

/** A native menu item that runs a shared command. */
export interface MenuBarItem {
  id: string;
  action: Action;
  /** The command belongs to the track list or to the tree. */
  owner: "list" | "tree";
  /** Writes the library: refused, with the reason, while it is read-only. */
  writes: boolean;
}

/** Ids as `src-tauri/src/menu.rs` gives them. */
export const MENU_BAR_ITEMS: readonly MenuBarItem[] = [
  { id: "import-to-collection", action: "importToCollection", owner: "list", writes: true },
  { id: "analysis-lock-on", action: "analysisLockOn", owner: "list", writes: true },
  { id: "analysis-lock-off", action: "analysisLockOff", owner: "list", writes: true },
  { id: "reload-tag", action: "reloadTag", owner: "list", writes: true },
  { id: "remove-from-collection", action: "removeFromCollection", owner: "list", writes: true },
  { id: "show-in-finder", action: "showInFinder", owner: "list", writes: false },
  { id: "auto-relocate", action: "autoRelocate", owner: "list", writes: true },
  { id: "relocate", action: "relocate", owner: "list", writes: true },
  { id: "create-playlist", action: "createPlaylist", owner: "tree", writes: true },
  { id: "create-intelligent-playlist", action: "createIntelligentPlaylist", owner: "tree", writes: true },
  { id: "create-folder", action: "createFolder", owner: "tree", writes: true },
  { id: "delete-playlist", action: "deletePlaylist", owner: "tree", writes: true },
  { id: "renumber-track-order", action: "renumberTrackOrder", owner: "list", writes: true },
  { id: "export-playlist-txt", action: "exportPlaylistTxt", owner: "tree", writes: false },
  { id: "export-playlist-m3u8", action: "exportPlaylistM3u8", owner: "tree", writes: false },
];

export function menuBarItem(id: string): MenuBarItem | null {
  return MENU_BAR_ITEMS.find((item) => item.id === id) ?? null;
}

/**
 * The side a menu item's command goes to: the one with the focus. The main
 * browser when nothing has it, which only matters to a command that was
 * never greyed.
 */
export function menuBarSide(focus: BrowserFocus): BrowserSide {
  return focus?.side ?? "main";
}

/** rekordbox's own database lists: Collection, playlists, histories, Related Tracks, Tag List. */
const LIBRARY_LISTS: ReadonlySet<TrackSource["kind"]> = new Set([
  "collection", "playlist", "playlistFolder", "history", "related", "tagList",
]);
/**
 * Where Reload Tag is offered: the library's lists less the histories and
 * the Tag List, plus the Explorer [OBS static: case 0x4003 accepts
 * `isNonList`, `isPlayList`, `isSamplerList`, `isRelatedList`, the cloud
 * lists, iTunes, `isFolderBrowse` and Inflyte].
 */
const RELOAD_LISTS: ReadonlySet<TrackSource["kind"]> = new Set([
  "collection", "playlist", "playlistFolder", "related", "folder",
]);
/** The Playlists tree's own nodes, the Collection with them for the Create items. */
const CREATE_NODES: ReadonlySet<TreeNode["kind"]> = new Set([
  "allTracks", "collection", "folder", "playlist", "smartPlaylist",
]);

export interface MenuBarInput {
  focus: BrowserFocus;
  lists: Partial<Record<BrowserSide, ListMenuState>>;
  trees: Partial<Record<BrowserSide, TreeMenuState>>;
  /** Whether something on screen answers a command for a side (`hasCommand`). */
  has: (action: Action, side: BrowserSide) => boolean;
}

/**
 * Every Track and Playlist menu item's enabled state, keyed by item id,
 * the two submenus included.
 */
export function menuBarEnabled({ focus, lists, trees, has }: MenuBarInput): Record<string, boolean> {
  const out: Record<string, boolean> = {};
  const side = menuBarSide(focus);
  // The Track menu answers the focused list only [OBS static: the focused
  // component is cast to a ListViewer, 0x1003154a0].
  const list = focus?.pane === "list" ? lists[side] : undefined;
  // The Playlist menu answers the tree on the focused side, whether the
  // focus is in it or in its list [OBS: the Playlist menu with the list
  // focused and the Playlists heading selected].
  const tree = focus !== null ? trees[side] : undefined;
  const live = (id: string, rule: boolean) => {
    const item = menuBarItem(id);
    out[id] = rule && (item === null || has(item.action, side));
  };

  const n = list?.selected ?? 0;
  const rows = list?.rows ?? [];
  const onStick = list?.source === "device";
  const anyMissing = rows.some((row) => row.missing === true);
  const anyPresent = rows.length < n || rows.some((row) => row.missing !== true);
  const library = list !== undefined && LIBRARY_LISTS.has(list.source);
  const tracks = list !== undefined && n > 0 && !onStick && !list.loose;

  // Import To Collection, 0x400f: a selection in a browse of files outside
  // the collection. rbxport imports the loose files of the selection.
  live("import-to-collection", list !== undefined && n > 0 && list.loose);
  // Analyze Track, 0x4000: a selected track that is not missing.
  // `analyseSelection` is the main browser's.
  out["analyze-track"] = tracks && side === "main" && anyPresent && has("analyseSelection", side);
  // Analysis Lock On, 0x4012: as Analyze Track. Off, 0x4013: rekordbox wants
  // a locked track among them; the list here does not carry the lock, so
  // Off is live for any track, as the context menu's is.
  live("analysis-lock-on", tracks && anyPresent);
  live("analysis-lock-off", tracks);
  out["analysis-lock"] = out["analysis-lock-on"] === true || out["analysis-lock-off"] === true;
  // Reload Tag, 0x4003: a present file, not in a history or the Tag List.
  live("reload-tag", tracks && list !== undefined && RELOAD_LISTS.has(list.source) && anyPresent);
  // Remove from Collection, 0x400b: any selection in a library list.
  live("remove-from-collection", tracks && library);
  // Show in Finder, 0x400d: exactly one row, whose file is there.
  live("show-in-finder", list !== undefined && !onStick && n === 1 && rows.length === 1 && rows[0]?.missing !== true);
  // Auto Relocate 0x4014 and Relocate 0x400e: any selected row missing.
  live("auto-relocate", tracks && anyMissing);
  live("relocate", tracks && anyMissing);

  const kind = tree?.kind ?? null;
  // Create New Playlist / Intelligent Playlist / Folder, 0x500a-0x500c: the
  // Collection or anything in the Playlists tree.
  const create = kind !== null && CREATE_NODES.has(kind);
  live("create-playlist", create);
  live("create-intelligent-playlist", create);
  live("create-folder", create);
  // Delete Playlist, 0x5004: a playlist, an intelligent playlist or a
  // folder. rekordbox also offers it on the Playlists heading; what it does
  // there is not known, so it is greyed here.
  live("delete-playlist", kind === "folder" || kind === "playlist" || kind === "smartPlaylist");
  // Renumber Track Order, 0x500e: a playlist that is not intelligent, whose
  // list is the one that renumbers.
  live("renumber-track-order", kind === "playlist");
  // Export a playlist to a file, 0x5005/0x5006: a playlist. rekordbox also
  // offers it on a folder and the heading; what it writes then is not known.
  live("export-playlist-txt", kind === "playlist" || kind === "smartPlaylist");
  live("export-playlist-m3u8", kind === "playlist" || kind === "smartPlaylist");
  out["export-playlist-file"] = out["export-playlist-txt"] === true || out["export-playlist-m3u8"] === true;
  return out;
}

/** What the lists and trees on screen last reported. */
const lists: Partial<Record<BrowserSide, ListMenuState>> = {};
const trees: Partial<Record<BrowserSide, TreeMenuState>> = {};
const listeners = new Set<() => void>();

function changed(): void {
  for (const listener of [...listeners]) listener();
}

function sameList(a: ListMenuState | undefined, b: ListMenuState | undefined): boolean {
  if (a === undefined || b === undefined) return a === b;
  return a.source === b.source && a.selected === b.selected && a.loose === b.loose &&
    a.rows.length === b.rows.length && a.rows.every((row, i) => row.missing === b.rows[i]?.missing);
}

/** A track list's report; null when it goes away. */
export function publishListMenuState(side: BrowserSide, state: ListMenuState | null): void {
  const next = state ?? undefined;
  if (sameList(lists[side], next)) return;
  if (next === undefined) delete lists[side];
  else lists[side] = next;
  changed();
}

/** A tree's report; null when it goes away. */
export function publishTreeMenuState(side: BrowserSide, state: TreeMenuState | null): void {
  const next = state ?? undefined;
  if (trees[side]?.kind === next?.kind && (trees[side] === undefined) === (next === undefined)) return;
  if (next === undefined) delete trees[side];
  else trees[side] = next;
  changed();
}

export function menuBarState(): Pick<MenuBarInput, "lists" | "trees"> {
  return { lists: { ...lists }, trees: { ...trees } };
}

/** Calls `listener` whenever a list or a tree reports something new. */
export function onMenuBarState(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
