/**
 * The browser's column catalogue, and the pure rules for arranging it.
 *
 * The catalogue describes the table; the header menu has its own order,
 * transcribed from the supplied capture.
 * Twelve are shown by default; the rest are available.
 *
 * Everything here is a pure function over a layout so the rules are testable
 * without a DOM, and so "which columns, in what order, how wide" lives in one
 * place rather than being spread through the table component.
 */
import type { SortColumn } from "@/ipc/types";

/** Keys that name a real column. Sortable ones match `SortColumn`. */
export type ColumnKey =
  | SortColumn
  | "attr" | "preview" | "artwork" | "comment"
  | "size" | "discNo" | "albumArtist" | "composer" | "lyricist" | "fileType"
  | "year" | "mixName" | "remixer" | "originalArtist" | "sampleRate"
  | "bitrate" | "bitDepth" | "location" | "dateCreated" | "hotCue"
  | "publishTrackInfo" | "message" | "color" | "myTag"
  | "trackNumber" | "cloud" | "fileName";

export interface ColumnSpec {
  key: ColumnKey;
  label: string;
  /** rekordbox's own width where one is known, otherwise a sensible default. */
  width: number;
  align?: "right";
  sortable: boolean;
  /**
   * Always shown, and absent from the header menu.
   *
   * `#` is the only one. Two independent sources agree it is not a menu item:
   * neither the captured header menu nor `german.lang`'s 39 column names has
   * it. It is the row's place in the view, which a playlist is unreadable
   * without, so it is fixed rather than optional.
   */
  fixed?: true;
}

/** How narrow a column may be dragged before it stops being readable. */
export const MIN_COLUMN_WIDTH = 32;
/** Wider than any window this app runs in; a guard against a runaway drag. */
export const MAX_COLUMN_WIDTH = 1200;

/**
 * Every column the header menu offers. The table's default order differs
 * from the menu shown in the capture.
 *
 * Widths for the twelve shown by default are rekordbox's own, from
 * `TableHeader-PlaylistTracks`. The rest have no measured width — they have
 * never been visible to measure — so they take a default and are marked below.
 */
export const CATALOGUE: readonly ColumnSpec[] = [
  // Sortable, as rekordbox's header shows `# ↑` by default: its order is the
  // view's own — a playlist as somebody put it — and descending reverses it.
  { key: "trackNo", label: "#", width: 47, align: "right", sortable: true, fixed: true },
  { key: "attr", label: "Attribute", width: 67, sortable: false },
  { key: "preview", label: "Preview", width: 128, sortable: false },
  { key: "artwork", label: "Artwork", width: 80, sortable: false },
  { key: "title", label: "Track Title", width: 387, sortable: true },
  { key: "key", label: "Key", width: 73, sortable: true },
  { key: "bpm", label: "BPM", width: 80, align: "right", sortable: true },
  { key: "duration", label: "Time", width: 80, align: "right", sortable: true },
  { key: "rating", label: "Rating", width: 101, sortable: true },
  { key: "artist", label: "Artist", width: 301, sortable: true },
  { key: "comment", label: "Comments", width: 210, sortable: true },
  { key: "label", label: "Label", width: 128, sortable: true },
  // Unmeasured from here down: none of these has been visible in a capture.
  { key: "size", label: "Size", width: 90, align: "right", sortable: false },
  { key: "discNo", label: "Disc number", width: 90, align: "right", sortable: false },
  { key: "albumArtist", label: "Album Artist", width: 200, sortable: false },
  { key: "composer", label: "Composer", width: 180, sortable: false },
  { key: "lyricist", label: "Lyricist", width: 180, sortable: false },
  { key: "fileType", label: "File Type", width: 90, sortable: false },
  { key: "year", label: "Year", width: 70, align: "right", sortable: false },
  { key: "mixName", label: "Mix Name", width: 180, sortable: false },
  { key: "remixer", label: "Remixer", width: 180, sortable: false },
  { key: "originalArtist", label: "Original Artist", width: 200, sortable: false },
  { key: "sampleRate", label: "Sample Rate", width: 110, align: "right", sortable: false },
  { key: "bitrate", label: "Bitrate", width: 90, align: "right", sortable: false },
  { key: "bitDepth", label: "Bitdepth", width: 90, align: "right", sortable: false },
  { key: "location", label: "Location", width: 320, sortable: false },
  { key: "dateAdded", label: "Date Added", width: 128, align: "right", sortable: true },
  { key: "releaseDate", label: "Release Date", width: 128, align: "right", sortable: true },
  { key: "dateCreated", label: "Date Created", width: 128, align: "right", sortable: false },
  { key: "hotCue", label: "Hot Cue", width: 110, sortable: false },
  { key: "publishTrackInfo", label: "Publish track information", width: 180, sortable: false },
  { key: "message", label: "Message", width: 180, sortable: false },
  { key: "color", label: "Color", width: 90, sortable: false },
  { key: "djPlayCount", label: "DJ Play Count", width: 120, align: "right", sortable: true },
  { key: "myTag", label: "My Tag", width: 180, sortable: false },
  { key: "album", label: "Album", width: 240, sortable: true },
  { key: "genre", label: "Genre", width: 160, sortable: true },
  { key: "trackNumber", label: "Track number", width: 90, align: "right", sortable: false },
  { key: "cloud", label: "Cloud", width: 80, sortable: false },
  { key: "fileName", label: "File Name", width: 260, sortable: false },
];

/** Ticked in the header menu when nothing has been customised. */
export const DEFAULT_VISIBLE: readonly ColumnKey[] = [
  "preview", "artwork", "title", "key", "bpm", "duration", "rating",
  "artist", "comment", "label", "dateAdded", "releaseDate",
];

/** Columns that are always present, whatever the saved layout says. */
export const FIXED: readonly ColumnKey[] = CATALOGUE.filter((c) => c.fixed).map((c) => c.key);

/** Columns the header menu cannot hide. Track Title may still be moved. */
export const REQUIRED_COLUMNS: readonly ColumnKey[] = [...FIXED, "title"];

/** Every menu column is backed by a browser row or a requested detail field. */
export const AVAILABLE_COLUMNS: readonly ColumnKey[] = CATALOGUE.map((column) => column.key);

/** Fields read from the database only when a table shows one of them. */
export const EXTRA_COLUMNS: readonly ColumnKey[] = [
  "size", "discNo", "albumArtist", "composer", "lyricist", "fileType", "year",
  "mixName", "remixer", "originalArtist", "sampleRate", "bitrate", "bitDepth",
  "location", "dateCreated", "publishTrackInfo", "message", "color",
  "djPlayCount", "myTag", "trackNumber", "cloud",
];

/** The header menu's order in the supplied rekordbox capture. */
const MENU_ORDER: readonly ColumnKey[] = [
  "attr", "preview", "artwork", "title", "releaseDate", "artist", "genre", "comment",
  "size", "discNo", "albumArtist", "trackNumber", "bpm", "rating", "composer",
  "lyricist", "duration", "fileType", "year", "mixName", "remixer", "label",
  "originalArtist", "key", "sampleRate", "bitrate", "bitDepth", "fileName",
  "location", "dateAdded", "dateCreated", "hotCue", "publishTrackInfo", "message",
  "color", "djPlayCount", "myTag", "album", "cloud",
];
export const MENU_COLUMNS: readonly ColumnSpec[] = MENU_ORDER
  .map((key) => CATALOGUE.find((column) => column.key === key))
  .filter((column): column is ColumnSpec => column !== undefined);

/** A column's place in the table: which, in what order, how wide. */
export interface Layout {
  /** Visible columns, left to right. */
  order: ColumnKey[];
  /** Width overrides. A key absent here uses the catalogue width. */
  widths: Partial<Record<ColumnKey, number>>;
}

export function defaultLayout(): Layout {
  return { order: [...DEFAULT_VISIBLE], widths: {} };
}

/**
 * The Explorer's columns: `TableHeader-FolderTracks` in browseSetting.xml,
 * which the 9.10.55 PM capture draws in this order at these widths — Preview
 * 200, Artwork 80, then 128 each with BPM 80, Rating 90 and Time 80. The
 * column ids there are not mapped to names; the capture is what says which
 * width is which.
 */
export const FOLDER_VISIBLE: readonly ColumnKey[] = [
  "preview", "artwork", "title", "artist", "album", "genre", "bpm", "rating", "duration", "key",
  "fileName",
];

const FOLDER_WIDTHS: Partial<Record<ColumnKey, number>> = {
  preview: 200, artwork: 80, title: 128, artist: 128, album: 128, genre: 128, bpm: 80,
  rating: 90, duration: 80, key: 128, fileName: 128,
};

export function folderLayout(): Layout {
  return { order: [...FOLDER_VISIBLE], widths: { ...FOLDER_WIDTHS } };
}

const BY_KEY = new Map(CATALOGUE.map((c) => [c.key, c]));

export function specOf(key: ColumnKey): ColumnSpec | undefined {
  return BY_KEY.get(key);
}

/** The width a column is drawn at. */
export function widthOf(layout: Layout, key: ColumnKey): number {
  return layout.widths[key] ?? specOf(key)?.width ?? 120;
}

/** Restore a required title at its catalogue position in an older layout. */
function withTitle(order: readonly ColumnKey[]): ColumnKey[] {
  if (order.includes("title")) return [...order];
  const titleRank = CATALOGUE.findIndex((column) => column.key === "title");
  const at = order.findIndex((key) => CATALOGUE.findIndex((column) => column.key === key) > titleRank);
  const restored = [...order];
  restored.splice(at === -1 ? restored.length : at, 0, "title");
  return restored;
}

/**
 * The visible columns as full specs, at their current widths.
 *
 * Fixed columns lead, whatever the saved layout holds — including a layout
 * saved before they existed, which is every layout already on disk.
 */
export function resolve(layout: Layout): ColumnSpec[] {
  const chosen = withTitle(layout.order.filter((key) => AVAILABLE_COLUMNS.includes(key) && !FIXED.includes(key)));
  return [...FIXED, ...chosen]
    .map((key) => specOf(key))
    .filter((spec): spec is ColumnSpec => spec !== undefined)
    .map((spec) => ({ ...spec, width: widthOf(layout, spec.key) }));
}

/**
 * Shows or hides a column.
 *
 * A newly-shown column goes back to its place in the catalogue order rather
 * than to the end, so ticking something in the header menu puts it where the
 * menu implies it will be.
 */
export function toggleColumn(layout: Layout, key: ColumnKey): Layout {
  if (!AVAILABLE_COLUMNS.includes(key) || REQUIRED_COLUMNS.includes(key)) return layout;
  if (layout.order.includes(key)) {
    return { ...layout, order: layout.order.filter((k) => k !== key) };
  }
  const wanted = CATALOGUE.findIndex((c) => c.key === key);
  const at = layout.order.findIndex(
    (k) => CATALOGUE.findIndex((c) => c.key === k) > wanted,
  );
  const order = [...layout.order];
  order.splice(at === -1 ? order.length : at, 0, key);
  return { ...layout, order };
}

/**
 * Moves a column to a new position, clamped to the row.
 *
 * `to` counts the *rendered* headers, which lead with the fixed columns, and
 * `order` does not hold those — so the two coordinate spaces differ by however
 * many are fixed. Converting here rather than at the call site keeps that
 * detail with the code that knows about fixed columns at all.
 */
export function moveColumn(layout: Layout, key: ColumnKey, to: number): Layout {
  if (!AVAILABLE_COLUMNS.includes(key) || FIXED.includes(key)) return layout;
  const from = layout.order.indexOf(key);
  if (from === -1) return layout;
  const target = to - FIXED.length;
  const order = [...layout.order];
  order.splice(from, 1);
  order.splice(Math.max(0, Math.min(target, order.length)), 0, key);
  return { ...layout, order };
}

/** Sets a column's width, clamped so it stays readable and finite. */
export function resizeColumn(layout: Layout, key: ColumnKey, width: number): Layout {
  if (!BY_KEY.has(key)) return layout;
  const clamped = Math.round(
    Math.min(Math.max(Number.isFinite(width) ? width : MIN_COLUMN_WIDTH, MIN_COLUMN_WIDTH), MAX_COLUMN_WIDTH),
  );
  return { ...layout, widths: { ...layout.widths, [key]: clamped } };
}

/** Drops a width override, returning the column to its catalogue width. */
export function autoSizeColumn(layout: Layout, key: ColumnKey): Layout {
  if (!(key in layout.widths)) return layout;
  const widths = { ...layout.widths };
  delete widths[key];
  return { ...layout, widths };
}

/** Drops every width override. */
export function autoSizeAll(layout: Layout): Layout {
  return { ...layout, widths: {} };
}

/**
 * Repairs a layout read back from storage.
 *
 * A stored layout can name a column that no longer exists, or repeat one, or
 * be empty — all of which would otherwise render a broken table rather than
 * simply falling back. `fallback` is what it falls back to: the table's own
 * default, which is not the same for every context.
 */
export function sanitise(value: unknown, fallback: () => Layout = defaultLayout): Layout {
  if (typeof value !== "object" || value === null) return fallback();
  const raw = value as Partial<Layout>;
  const seen = new Set<ColumnKey>();
  const order = (Array.isArray(raw.order) ? raw.order : []).filter(
    (key): key is ColumnKey => {
      if (typeof key !== "string" || !AVAILABLE_COLUMNS.includes(key)) return false;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    },
  );
  if (order.length === 0) return fallback();

  const widths: Partial<Record<ColumnKey, number>> = {};
  const rawWidths = typeof raw.widths === "object" && raw.widths !== null ? raw.widths : {};
  for (const [key, width] of Object.entries(rawWidths)) {
    if (!AVAILABLE_COLUMNS.includes(key as ColumnKey)) continue;
    if (typeof width !== "number" || !Number.isFinite(width)) continue;
    widths[key as ColumnKey] = Math.round(
      Math.min(Math.max(width, MIN_COLUMN_WIDTH), MAX_COLUMN_WIDTH),
    );
  }
  return { order: withTitle(order), widths };
}
