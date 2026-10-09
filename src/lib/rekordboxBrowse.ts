/** Import the parts of rekordbox's browseSetting.xml that our rows support. */
import { AVAILABLE_COLUMNS, sanitise, type ColumnKey, type Layout } from "./columns";
import { loadPreferences } from "./preferences";
import { getBackend } from "@/ipc/client";
import { loadSession, saveSession } from "./session";

export type BrowseContext = "collection" | "playlist" | "history" | "subBrowser" | "folder";

// Numeric column IDs, from the measured rekordbox headers and from the
// comparator rekordbox 7.2.11 binds to each ID in
// `browse::ListViewSorter::setCompFunc` [OBS: static analysis of the macOS
// binary; every ID the headers had already verified names the same field
// there]. Unknown IDs must stay unknown: the XML has numeric IDs only, so
// guessing would map a visible rekordbox field onto the wrong RBXport column.
// 65 is left out on purpose: rekordbox paints and sorts it by Hot Cue Auto
// Load, which RBXport's Hot Cue column does not show.
const IDS: Readonly<Record<string, ColumnKey>> = {
  "1": "dateAdded", "20": "trackNo", "21": "title", "22": "artist", "23": "album",
  "24": "genre", "25": "comment", "26": "year", "27": "rating", "28": "djPlayCount",
  "29": "bpm", "30": "trackNumber", "31": "remixer", "32": "composer", "33": "label",
  "34": "key", "35": "color", "36": "fileType", "37": "bitrate", "38": "location",
  "39": "dateCreated", "41": "fileName", "42": "size", "43": "sampleRate",
  "44": "duration", "46": "albumArtist", "47": "discNo", "48": "mixName",
  "49": "originalArtist", "52": "bitDepth", "53": "releaseDate", "60": "artwork",
  "66": "publishTrackInfo", "67": "message", "68": "preview", "72": "lyricist",
};

const SOURCES: Readonly<Record<BrowseContext, readonly string[]>> = {
  collection: ["TableHeader-CollectionTracks"],
  playlist: ["TableHeader-PlaylistTracks"],
  history: ["TableHeader-HistoryTracks"],
  subBrowser: ["TableHeader-PlaylistTracks-sub", "TableHeader-CollectionTracks-sub"],
  folder: ["TableHeader-FolderTracks"],
};

export function parseRekordboxBrowse(xml: string): Partial<Record<BrowseContext, Layout>> {
  const doc = new DOMParser().parseFromString(xml, "application/xml");
  if (doc.querySelector("parsererror")) return {};
  const values = Array.from(doc.getElementsByTagName("VALUE"));
  const layouts: Partial<Record<BrowseContext, Layout>> = {};
  for (const context of Object.keys(SOURCES) as BrowseContext[]) {
    const value = SOURCES[context]
      .map((name) => values.find((entry) => entry.getAttribute("name") === name))
      .find((entry) => entry?.getElementsByTagName("COLUMN").length);
    if (!value) continue;
    const order: ColumnKey[] = [];
    const widths: Layout["widths"] = {};
    for (const column of Array.from(value.getElementsByTagName("COLUMN"))) {
      const key = IDS[column.getAttribute("id") ?? ""];
      if (!key || !AVAILABLE_COLUMNS.includes(key)) continue;
      const width = Number(column.getAttribute("width"));
      if (Number.isFinite(width) && width > 0) widths[key] = width;
      if (key !== "trackNo" && column.getAttribute("visible") === "1" && !order.includes(key)) order.push(key);
    }
    if (order.includes("title")) layouts[context] = sanitise({ order, widths });
  }
  return layouts;
}

/** Main tree and second browser widths from BROWSELAYOUT, when usable. */
export function parseRekordboxBrowseWidths(xml: string): { treeWidth?: number; subWidth?: number } {
  const doc = new DOMParser().parseFromString(xml, "application/xml");
  if (doc.querySelector("parsererror")) return {};
  const browse = Array.from(doc.getElementsByTagName("BROWSE"));
  const width = (component: string): number | undefined => {
    const raw = browse.find((item) => item.getAttribute("comp") === component)?.getAttribute("w");
    const value = Number(raw);
    return raw && Number.isFinite(value) && value >= 100 && value <= 4000 ? Math.round(value) : undefined;
  };
  const treeWidth = width("TreeView");
  const subWidth = width("SubBrowse");
  return {
    ...(treeWidth === undefined ? {} : { treeWidth }),
    ...(subWidth === undefined ? {} : { subWidth }),
  };
}

/**
 * What the last startup import took from rekordbox, per context and for the
 * pane widths, as the JSON it compared.
 *
 * rekordbox's file only changes when somebody changes rekordbox. Importing it
 * on every start put rekordbox's columns back over whatever was chosen in
 * RBXport since, so a column added, removed, moved or widened here was gone at
 * the next launch (#207). Comparing against the last import means a layout is
 * taken from rekordbox when rekordbox's own changed, and RBXport's edits stand
 * otherwise.
 */
const IMPORTED_KEY = "rbl.browse-import.v1";

type Imported = Partial<Record<BrowseContext | "panes", string>>;

function loadImported(): Imported {
  try {
    const raw = localStorage.getItem(IMPORTED_KEY);
    const value: unknown = raw === null ? null : JSON.parse(raw);
    if (typeof value !== "object" || value === null) return {};
    const imported: Imported = {};
    for (const [key, entry] of Object.entries(value)) {
      if (typeof entry === "string") imported[key as keyof Imported] = entry;
    }
    return imported;
  } catch {
    return {};
  }
}

/**
 * Applies rekordbox's browse settings, leaving alone every part whose
 * rekordbox value is the one already imported last time.
 */
export function applyRekordboxBrowse(xml: string): void {
  const imported = loadImported();
  const next: Imported = { ...imported };
  for (const [context, layout] of Object.entries(parseRekordboxBrowse(xml)) as [BrowseContext, Layout][]) {
    const value = JSON.stringify(layout);
    if (imported[context] === value) continue;
    localStorage.setItem(`rbl.columns.v2.${context}`, value);
    next[context] = value;
  }
  const widths = parseRekordboxBrowseWidths(xml);
  if (widths.treeWidth !== undefined || widths.subWidth !== undefined) {
    const value = JSON.stringify(widths);
    if (imported.panes !== value) {
      saveSession({ ...loadSession(), ...widths });
      next.panes = value;
    }
  }
  localStorage.setItem(IMPORTED_KEY, JSON.stringify(next));
}

/** Run before the main window mounts, so useColumns reads the imported layout. */
export async function syncRekordboxBrowseAtStartup(): Promise<void> {
  if (!loadPreferences().rekordbox.syncBrowseSettings) return;
  try {
    const xml = await (await getBackend()).rekordboxBrowseSettings();
    if (!xml) return;
    applyRekordboxBrowse(xml);
  } catch {
    // A missing or unreadable rekordbox install leaves local layouts intact.
  }
}
