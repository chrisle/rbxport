/**
 * The two context menus, transcribed from rekordbox rather than designed.
 *
 * Every label is a left-hand key of `german.lang`, and the order, the
 * separators and which entries carry a submarker are the capture's. Items the
 * app cannot do yet are still drawn, greyed, exactly as rekordbox greys the
 * ones a given selection cannot do — a menu that is half the length of the
 * real one is a menu people have to relearn later.
 */

/**
 * What an entry does, or `null` for one that is only drawn.
 *
 * The parameterised ones carry their target after a colon: the playlist a
 * track goes to, the device it is exported to.
 */
export type TrackAction =
  | "analyse"
  | "importToCollection"
  | "analysisLock"
  | "analysisUnlock"
  | `addToPlaylist:${string}`
  | "addToTagList"
  | "reloadTag"
  | `exportTrack:${string}`
  | "resetPlayCount"
  | "convertMemoryCues"
  | "removeFromPlaylist"
  | "removeFromTagList"
  | "removeFromHistory"
  | "removeFromCollection"
  | "showInformation"
  | "showInFinder"
  | "loadPlayer1"
  | "loadPlayer2";

export type TreeAction =
  | "export"
  | "exportM3u8"
  | "exportTxt"
  | "createPlaylist"
  | "createSmartPlaylist"
  | "editSmartPlaylist"
  | "createFolder"
  | "addArtwork"
  | "rename"
  | "delete"
  | "addToShortcut"
  | "sortItems";

/** The deck's ≡ menu: the choices it changes, and the one thing it does. */
export type DeckAction =
  | "waveformBlue"
  | "waveformRgb"
  | "waveform3band"
  | "analyse"
  | "exportLoopWav"
  | `exportTrack:${string}`
  | "beatPosition"
  | "beatToMemoryBars"
  | "beatToMemoryBeats"
  | "waveformClickOn"
  | "waveformClickOff";

export interface MenuEntry<A> {
  /** The label, as `german.lang` spells it. */
  label: string;
  /** What it runs. `null` is drawn and greyed: rekordbox has it, we do not. */
  action: A | null;
  /** Opens a submenu, drawn with rekordbox's arrow. */
  submenu?: boolean;
  /**
   * The submenu's own rows, when there is one behind the arrow.
   *
   * Absent with `submenu` set is rekordbox's arrow over nothing: the entry is
   * drawn and greyed, because a menu missing half its rows is a menu people
   * have to relearn later.
   */
  items?: readonly MenuRow<A>[];
  /** A rule beyond "we have it": no playlist to remove from, and so on. */
  needs?: "playlist" | "history" | "file" | "loose" | "track";
  /**
   * Live over files the library does not hold as well: the files are
   * imported first, then the entry runs on the tracks they became.
   */
  importsLoose?: boolean;
  /** In a submenu of choices, the one in force: drawn with a tick. */
  checked?: boolean;
}

/** A separator between groups. */
export const SEPARATOR = "-" as const;

export type MenuRow<A> = MenuEntry<A> | typeof SEPARATOR;

/**
 * Right-clicking a track, top to bottom as the capture has it.
 *
 * The greyed entries are not oversights. `Get Info from iTunes` reads
 * iTunes's own record of a track, which this does not keep; `Track Type`
 * and `Add New Analysis Data` are greyed in rekordbox 7.2.11 itself over
 * a track of the collection [OBS 2026-09-18]; `Auto Load Hot Cue` writes a
 * column whose "off" spelling has never been seen [UNKNOWN]; `Track
 * information` is the KUVO switch, left alone. `Load`, `Add To Playlist`
 * and `Export Track` are filled in by `trackMenuFor` below, which knows
 * the players, the playlists and the sticks there are.
 *
 * The cloud entries are gone rather than greyed: there is no cloud library
 * behind this app to sync with, so drawing the row promises a feature that is
 * not coming. `Analyze Track` writes the result to the library — the grid,
 * the waveforms, the BPM and the key — so it is a write, greyed while
 * rekordbox holds the file. `Import To Collection` is live over a file the
 * Explorer lists and greyed over a track, which is already in.
 *
 * What is live follows the rows' state, not the view [OBS rekordbox 7,
 * Winrig 2026-10-08, issue #105]. In the Explorer, a file the collection
 * holds gets a track's menu: Import To Collection greyed, Analyze Track,
 * Analysis Lock and Remove from Collection live. A file it does not hold
 * gets the reverse of those four, and Add To Playlist stays live, importing
 * the file on the way in. rekordbox also leaves Add To Tag List, Reload Tag,
 * Export Track, Reset DJ Play Count and Show information live over such a
 * file; what each does to it there has not been observed [UNKNOWN], so
 * they stay greyed here until it has.
 */
export const TRACK_MENU: readonly MenuRow<TrackAction>[] = [
  { label: "Load", action: null, submenu: true },
  SEPARATOR,
  { label: "Import To Collection", action: "importToCollection", needs: "loose" },
  { label: "Analyze Track", action: "analyse" },
  {
    label: "Analysis Lock",
    action: null,
    submenu: true,
    // rekordbox's two rows, On and Off [OBS 7.2.11, 2026-09-18]. The lock
    // is the GRID panel's, this application's own until a recording says
    // where rekordbox keeps it.
    items: [
      { label: "On", action: "analysisLock", needs: "track" },
      { label: "Off", action: "analysisUnlock", needs: "track" },
    ],
  },
  SEPARATOR,
  { label: "Add To Playlist", action: null, submenu: true },
  { label: "Add To Tag List", action: "addToTagList", needs: "track" },
  { label: "Reload Tag", action: "reloadTag", needs: "track" },
  { label: "Get Info from iTunes", action: null },
  { label: "Track Type", action: null, submenu: true },
  SEPARATOR,
  { label: "Export Track", action: null, submenu: true },
  SEPARATOR,
  // Its two rows [OBS 7.2.11, 2026-09-18], greyed: the "off" spelling of
  // `HotCueAutoLoad` has never been seen.
  {
    label: "Auto Load Hot Cue",
    action: null,
    submenu: true,
    items: [
      { label: "Enable Auto Load Hot Cue", action: null },
      { label: "Disable Auto Load Hot Cue", action: null },
    ],
  },
  { label: "Reset DJ Play Count", action: "resetPlayCount" },
  { label: "Add New Analysis Data", action: null },
  { label: "Convert Memory Cues to Hot Cues", action: "convertMemoryCues" },
  SEPARATOR,
  { label: "Remove from Playlist", action: "removeFromPlaylist", needs: "playlist" },
  { label: "Remove from Collection", action: "removeFromCollection" },
  { label: "Remove from History", action: "removeFromHistory", needs: "history" },
  SEPARATOR,
  { label: "Show information", action: "showInformation" },
  { label: "Show in Finder", action: "showInFinder", needs: "file" },
  SEPARATOR,
  // Publish / Do not publish, the KUVO delivery switch [OBS 7.2.11,
  // 2026-09-18]: drawn, and left alone by Chris's word.
  {
    label: "Track information",
    action: null,
    submenu: true,
    items: [
      { label: "Publish", action: null },
      { label: "Do not publish", action: null },
    ],
  },
];

/**
 * Right-clicking the tree, top to bottom as the capture has it
 * (docs/screenshots context-menu-tree@2x, over a playlist).
 *
 * `Delete` and `Export` take the node's own word — rekordbox writes "Delete
 * Folder" over a folder and "Delete Playlist" over a playlist, so this does
 * too; the folder's menu is its own list [OBS 7.2.11, 2026-09-18].
 *
 * `Playlist display setting` stays greyed, as rekordbox 7.2.11 greys it
 * over a playlist and a folder of the collection [OBS 2026-09-18]. `Edit Intelligent Playlist` is not in the capture, which
 * was taken over a plain playlist; rekordbox opens the rule editor from
 * the intelligent playlist's own menu, and so does this [ASSUME the label].
 * `Add To Shortcut` stays `Add To Shortcut` on a playlist that is one
 * already [OBS 7.2.11, 2026-09-18: rekordbox even takes it twice]; a
 * shortcut goes away from its own menu, `shortcutMenu` below.
 *
 * The cloud rows rekordbox draws first — "Cloud Library Sync", "Auto Upload"
 * and "Batch Auto Upload setting" — and "Collaborative playlist" are left
 * out rather than greyed. There is no cloud library behind this app, so the
 * rows would promise a feature that is not coming, which is a different
 * thing from one not built yet.
 */
export function treeMenu(kind: "playlist" | "smartPlaylist" | "folder" | "collection"): readonly MenuRow<TreeAction>[] {
  if (kind === "collection") {
    return [
      { label: "Create New Playlist", action: "createPlaylist" },
      { label: "Create New Folder", action: "createFolder" },
    ];
  }
  const folder = kind === "folder";
  const smart = kind === "smartPlaylist";
  if (folder) {
    // rekordbox's folder menu [OBS 7.2.11, 2026-09-18]: no artwork, no
    // file export, and Sort Items, which puts the folder's children in
    // name order. Rename is not in it either (a double click on the row);
    // it is kept here beside Delete as on a playlist [ASSUME].
    return [
      { label: "Export Folder", action: "export", submenu: true },
      SEPARATOR,
      { label: "Create New Playlist", action: "createPlaylist" },
      { label: "Create New Folder", action: "createFolder" },
      SEPARATOR,
      { label: "Playlist display setting", action: null },
      SEPARATOR,
      { label: "Rename Folder", action: "rename" },
      { label: "Delete Folder", action: "delete" },
      SEPARATOR,
      { label: "Sort Items", action: "sortItems" },
      SEPARATOR,
      { label: "Add To Shortcut", action: "addToShortcut" },
    ];
  }
  // An intelligent playlist is exported, renamed and deleted like any other;
  // what it cannot do is take tracks by hand, which its rows never offer.
  return [
    { label: "Export Playlist", action: "export", submenu: true },
    SEPARATOR,
    { label: "Create New Playlist", action: "createPlaylist" },
    ...(smart ? [{ label: "Edit Intelligent Playlist", action: "editSmartPlaylist" as const }] : []),
    { label: "Create New Folder", action: "createFolder" },
    SEPARATOR,
    { label: "Playlist display setting", action: null },
    SEPARATOR,
    { label: "Add Artwork", action: "addArtwork" },
    SEPARATOR,
    // Rename is not in the capture [ASSUME]: rekordbox renames from a double
    // click on the row. It sits with Delete because the two are the same kind
    // of thing — the node itself rather than what is in it.
    { label: "Rename Playlist", action: "rename" },
    { label: "Delete Playlist", action: "delete" },
    SEPARATOR,
    {
      label: "Export a playlist to a file",
      action: null,
      submenu: true,
      items: [
        { label: "m3u8", action: "exportM3u8" },
        { label: "txt", action: "exportTxt" },
      ],
    },
    SEPARATOR,
    { label: "Add To Shortcut", action: "addToShortcut" },
  ];
}

export type ShortcutAction = "deleteShortcut";

/**
 * A shortcut's own menu, over its button: one row, "Delete Shortcut"
 * [OBS 7.2.11, 2026-09-18, docs/screenshots/shortcuts-column-7.2.11@2x].
 * That is the only way a shortcut goes; the playlist's tree menu does not
 * offer it.
 */
export function shortcutMenu(): readonly MenuRow<ShortcutAction>[] {
  return [{ label: "Delete Shortcut", action: "deleteShortcut" }];
}

/** What is in a menu, for the caller to decide what an entry can do. */
export interface MenuContext {
  /** The view is a playlist, so a track can be taken out of it. */
  inPlaylist: boolean;
  /** The view is a history session, so a play can be taken off it. */
  inHistory?: boolean;
  /** The track's file is known, so the OS can be asked to show it. */
  hasFile: boolean;
  /**
   * The rows are files the Explorer lists, not tracks of the library: they
   * can be imported, and nothing that needs a track can take them.
   */
  loose?: boolean;
  /** The library is open read-only, because rekordbox is running. */
  readOnly: boolean;
}

/** What the deck's menu shows, so it can tick what is in force. */
export interface DeckMenuState {
  waveformColor: "blue" | "rgb" | "3band";
  beatCount: "position" | "toMemoryBars" | "toMemoryBeats";
  waveformClick: boolean;
  /** The deck has a loop set, so there is one to export. */
  hasLoop?: boolean;
  /** The connected sticks Export Track offers. */
  devices?: readonly MenuTarget[];
}

/**
 * The ≡ at the foot of the deck, top to bottom as rekordbox draws it
 * (capture of 7.2.11's player menu). The waveform colour, the beat count and
 * the waveform click are Preferences › View's own choices, reachable from
 * here as well; each submenu ticks what is in force. Export Track lists the
 * connected sticks; Export Loop As WAV is live while the deck has a loop,
 * greyed otherwise as rekordbox greys it with nothing to export. Active
 * Loop Playback stays greyed: it writes `djmdCue.ActiveLoop`, whose "on"
 * value has never been seen [UNKNOWN: 0 on every cue in the reference
 * library]. Analyze Track analyses the loaded track and writes the result,
 * as it does from the track list.
 */
export function deckMenu(state: DeckMenuState): readonly MenuRow<DeckAction>[] {
  const tick = (on: boolean) => ({ checked: on });
  return [
    {
      label: "Change waveform color",
      action: null,
      items: [
        { label: "BLUE", action: "waveformBlue", ...tick(state.waveformColor === "blue") },
        { label: "RGB", action: "waveformRgb", ...tick(state.waveformColor === "rgb") },
        { label: "3Band", action: "waveform3band", ...tick(state.waveformColor === "3band") },
      ],
    },
    { label: "Analyze Track", action: "analyse" },
    SEPARATOR,
    {
      label: "Beat Count Display",
      action: null,
      items: [
        { label: "Current Position (Bars)", action: "beatPosition", ...tick(state.beatCount === "position") },
        {
          label: "Count to the next MEMORY CUE (Bars)",
          action: "beatToMemoryBars",
          ...tick(state.beatCount === "toMemoryBars"),
        },
        {
          label: "Count to the next MEMORY CUE (Beats)",
          action: "beatToMemoryBeats",
          ...tick(state.beatCount === "toMemoryBeats"),
        },
      ],
    },
    SEPARATOR,
    {
      label: "Export Track",
      action: null,
      submenu: true,
      ...(state.devices && state.devices.length > 0
        ? { items: state.devices.map((d) => ({ label: d.name, action: `exportTrack:${d.id}` as const })) }
        : {}),
    },
    SEPARATOR,
    { label: "Export Loop As WAV", action: state.hasLoop ? "exportLoopWav" : null },
    SEPARATOR,
    { label: "Active Loop Playback", action: null, submenu: true },
    SEPARATOR,
    {
      label: "Click on the waveform for PLAY and CUE",
      action: null,
      // The submenu's wording is [ASSUME]: the capture shows the arrow and
      // not what is behind it. Preferences calls the switch "Disable".
      items: [
        { label: "Enable", action: "waveformClickOn", ...tick(state.waveformClick) },
        { label: "Disable", action: "waveformClickOff", ...tick(!state.waveformClick) },
      ],
    },
  ];
}

/** Writes to the shared library, so rekordbox running is a refusal. */
const WRITES: ReadonlySet<string> = new Set([
  "analyse",
  "importToCollection",
  "createSmartPlaylist",
  "editSmartPlaylist",
  "addArtwork",
  "sortItems",
  "resetPlayCount",
  "convertMemoryCues",
  "removeFromPlaylist",
  "removeFromTagList",
  "addToTagList",
  "reloadTag",
  "removeFromHistory",
  "removeFromCollection",
  "createPlaylist",
  "createFolder",
  "rename",
  "delete",
]);

/** Whether an entry can be clicked. Everything else is drawn and greyed. */
export function enabled<A extends string>(
  entry: MenuEntry<A>,
  context: MenuContext,
): boolean {
  // An entry that opens a submenu does nothing itself; what makes it live is
  // having something under it that is.
  if (entry.items) return entriesOf(entry.items).some((row) => enabled(row, context));
  if (entry.action === null) return false;
  if (context.readOnly && (WRITES.has(entry.action) || entry.action.startsWith("addToPlaylist:"))) return false;
  if (context.loose === true && entry.needs !== "loose" && entry.needs !== "file" && entry.importsLoose !== true) {
    return entry.action.startsWith("loadPlayer");
  }
  if (entry.needs === "playlist") return context.inPlaylist;
  if (entry.needs === "history") return context.inHistory === true;
  if (entry.needs === "file") return context.hasFile;
  if (entry.needs === "loose") return context.loose === true;
  return true;
}

/** A playlist the Add To Playlist submenu offers, and a stick Export Track offers. */
export interface MenuTarget {
  id: string;
  name: string;
}

/**
 * The track menu for a layout drawing `players` decks, with the playlists
 * a track can be added to and the sticks it can be exported to.
 *
 * rekordbox lists players 1 to 4 under `Load` whether or not they are on
 * screen; this lists the ones there are, because a player that is not drawn
 * has nowhere to put a track. With none — the Full Browser layout — `Load` is
 * the greyed arrow it is in `TRACK_MENU`. `Add To Playlist` lists the
 * playlists, each under the folders above it as "Folder › Playlist", since
 * a submenu here goes one level deep; intelligent playlists take no track
 * by hand and are left out. `Export Track` lists the connected sticks. A
 * submenu with nothing to list is the greyed arrow.
 *
 * The labels are `german.lang`'s own keys: "Load track to player 1".
 */
export function trackMenuFor(
  players: number,
  playlists: readonly MenuTarget[] = [],
  devices: readonly MenuTarget[] = [],
  // Over the Tag List, "Remove from Playlist" is "Remove from Tag List"
  // [ASSUME: the capture is over a playlist].
  // In the Explorer, rekordbox draws no Convert Memory Cues to Hot Cues row,
  // over an imported file or a loose one [OBS 7, Winrig 2026-10-08].
  options: { tagList?: boolean; explorer?: boolean } = {},
): readonly MenuRow<TrackAction>[] {
  const every: MenuRow<TrackAction>[] = [
    { label: "Load track to player 1", action: "loadPlayer1" },
    { label: "Load track to player 2", action: "loadPlayer2" },
  ];
  const decks = every.slice(0, Math.max(0, players));
  const lists: MenuRow<TrackAction>[] = playlists.map((p) => ({
    label: p.name,
    action: `addToPlaylist:${p.id}` as const,
    importsLoose: true,
  }));
  const sticks: MenuRow<TrackAction>[] = devices.map((d) => ({
    label: d.name,
    action: `exportTrack:${d.id}` as const,
    needs: "track" as const,
  }));
  const rows = options.explorer === true
    ? TRACK_MENU.filter((row) => row === SEPARATOR || row.action !== "convertMemoryCues")
    : TRACK_MENU;
  return rows.map((row) => {
    if (row === SEPARATOR) return row;
    if (row.label === "Load" && decks.length > 0) return { ...row, items: decks };
    if (row.label === "Add To Playlist" && lists.length > 0) return { ...row, items: lists };
    if (row.label === "Export Track" && sticks.length > 0) return { ...row, items: sticks };
    if (row.label === "Remove from Playlist" && options.tagList === true) {
      return { label: "Remove from Tag List", action: "removeFromTagList", needs: "track" };
    }
    return row;
  });
}

/** The entries of a menu, without its separators. */
export function entriesOf<A>(rows: readonly MenuRow<A>[]): MenuEntry<A>[] {
  return rows.filter((row): row is MenuEntry<A> => row !== SEPARATOR);
}
