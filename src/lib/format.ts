/** Display formatting that matches rekordbox's own column rendering. */

/** `04:26` — rekordbox pads to two digits and never shows hours. */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "";
  const total = Math.round(seconds);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** rekordbox stores BPM x100 and renders two decimals: 12800 -> "128.00". */
export function formatBpm(bpmX100: number): string {
  if (!Number.isFinite(bpmX100) || bpmX100 <= 0) return "";
  return (bpmX100 / 100).toFixed(2);
}

/**
 * `320 kbps`, or `VBR` for a track the library holds at zero.
 *
 * rekordbox stores `BitRate` 0 for every FLAC, for VBR MP3s and for some M4A
 * files it imports, and its Bitrate column and Summary tab print `VBR` for
 * all of them [OBS: rekordbox 7.2.14 on Windows 11]. It leaves `VBR`
 * untranslated: no rekordbox 7 `.lang` file has an entry for it.
 */
export function formatBitrate(kbps: number): string {
  if (!Number.isFinite(kbps) || kbps < 0) return "";
  return kbps === 0 ? "VBR" : `${kbps} kbps`;
}

/**
 * View › Date format: the order the browser and Info panel print a day in.
 * `system` follows the operating system's locale, as rekordbox does.
 */
export type DateFormat = "system" | "dmy" | "mdy" | "ymd";
export const DATE_FORMATS: readonly DateFormat[] = ["system", "dmy", "mdy", "ymd"];

type DateField = "day" | "month" | "year";

/** A day's printed shape: its three fields, padded or not, and the text between. */
type DatePart = { field: DateField; pad: boolean } | { text: string };
type DateLayout = readonly DatePart[];

const FIXED_LAYOUTS: Record<Exclude<DateFormat, "system">, DateLayout> = {
  dmy: [{ field: "day", pad: true }, { text: "/" }, { field: "month", pad: true }, { text: "/" }, { field: "year", pad: false }],
  mdy: [{ field: "month", pad: true }, { text: "/" }, { field: "day", pad: true }, { text: "/" }, { field: "year", pad: false }],
  ymd: [{ field: "year", pad: false }, { text: "/" }, { field: "month", pad: true }, { text: "/" }, { field: "day", pad: true }],
};

const systemLayouts = new Map<string, DateLayout>();

/**
 * The locale's numeric short date — its order, separators and padding — read
 * once from `Intl` and kept, so a table of dates formats none. Only the
 * shape is taken: the digits are put in by hand, so a stored day never goes
 * through a `Date` and never shifts by the time zone. A locale `Intl` cannot
 * describe prints year first.
 */
function systemLayout(locale: string | undefined): DateLayout {
  const key = locale ?? "";
  const known = systemLayouts.get(key);
  if (known) return known;
  let layout: DateLayout = FIXED_LAYOUTS.ymd;
  try {
    // 3 February 2001: a single-digit day and month show whether the locale pads.
    const parts = new Intl.DateTimeFormat(locale, { year: "numeric", month: "numeric", day: "numeric" })
      .formatToParts(new Date(2001, 1, 3));
    const shape = parts.flatMap((part): DatePart[] =>
      part.type === "day" || part.type === "month" || part.type === "year"
        ? [{ field: part.type, pad: part.type !== "year" && part.value.length === 2 }]
        : part.type === "literal" ? [{ text: part.value }] : []);
    if (shape.filter((part) => "field" in part).length === 3) layout = shape;
  } catch { /* an unknown locale keeps year first */ }
  systemLayouts.set(key, layout);
  return layout;
}

function dateLayout(format: DateFormat, locale: string | undefined): DateLayout {
  return format === "system" ? systemLayout(locale) : FIXED_LAYOUTS[format];
}

/** The order of a format's day, month and year, for the Info panel's date boxes. */
export function dateOrder(format: DateFormat, locale?: string): DateField[] {
  return dateLayout(format, locale).flatMap((part) => ("field" in part ? [part.field] : []));
}

function printDate(year: number, month: number, day: number, format: DateFormat, locale: string | undefined): string {
  const values: Record<DateField, number> = { year, month, day };
  return dateLayout(format, locale).map((part) => {
    if (!("field" in part)) return part.text;
    const text = String(values[part.field]);
    return part.pad ? text.padStart(2, "0") : text;
  }).join("");
}

/**
 * A stored day (`YYYY-MM-DD`, with or without a time after it) in the chosen
 * format, four-digit year. `locale` is the system's unless a test names one.
 */
export function formatShortDate(iso: string | null | undefined, format: DateFormat, locale?: string): string {
  if (!iso) return "";
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso);
  if (!m) return "";
  const [, y, mo, d] = m;
  if (!y || !mo || !d) return "";
  return printDate(Number(y), Number(mo), Number(d), format, locale);
}

/**
 * A moment in seconds since the Unix epoch as the date columns print a day,
 * on this machine's calendar: the File Created column's time is the file
 * system's, not a date the library stored, so its day is local.
 */
export function formatEpochDate(seconds: number | null | undefined, format: DateFormat, locale?: string): string {
  if (typeof seconds !== "number" || !Number.isFinite(seconds)) return "";
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return "";
  return printDate(date.getFullYear(), date.getMonth() + 1, date.getDate(), format, locale);
}

/** `144.8 GB` / `15.2 MB` — status-bar totals. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "";
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}

type Translate = (text: string, values?: Readonly<Record<string, string | number>>) => string;

/**
 * `5 hours 2 minutes` / `18 minutes`, as rekordbox's status bar says a
 * list's time [OBS: rekordbox 7.2.19 `common::selectedTracksInfo` at
 * `0x1004cd05c`]: whole hours and minutes, seconds dropped, and the words
 * always plural — English has no singular entry, so one minute is
 * `1 minutes` there too.
 */
export function formatStatusTime(seconds: number, t: Translate): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  if (hours === 0) return `${minutes} ${t("minutes")}`;
  return `${hours} ${t("hours")} ${minutes} ${t("minutes")}`;
}

/**
 * `759.7 MB` / `161 MB` / `1.9 GB`, as rekordbox's status bar sizes a list
 * [OBS: rekordbox 7.2.19 `descriptionOfSizeInBytes` at `0x10160bed8`]: binary
 * units, switched at 1024, and at most one decimal in the language's own
 * number format, so a whole number shows none.
 */
export function formatStatusSize(bytes: number, t: Translate, locale = "en"): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "";
  const number = (value: number) => new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value);
  if (bytes === 1) return `1 ${t("byte")}`;
  if (bytes < 1024) return `${number(bytes)} ${t("bytes")}`;
  if (bytes < 1024 ** 2) return `${number(bytes / 1024)} ${t("KB")}`;
  if (bytes < 1024 ** 3) return `${number(bytes / 1024 ** 2)} ${t("MB")}`;
  return `${number(bytes / 1024 ** 3)} ${t("GB")}`;
}

/**
 * `61 Tracks, 5 hours 2 minutes, 759.7 MB`: an open playlist's tally on the
 * right of rekordbox's status bar. An empty playlist is `0 Tracks`, with no
 * time or size [OBS: `common::StatusBarComponent::setPlaylistTracks` at
 * `0x1004cda74`].
 */
export function formatListTotals(totals: { tracks: number; seconds: number; bytes: number }, t: Translate, locale = "en"): string {
  if (totals.tracks <= 0) return `0 ${t("Tracks")}`;
  return `${totals.tracks} ${t("Tracks")}, ${formatStatusTime(totals.seconds, t)}, ${formatStatusSize(totals.bytes, t, locale)}`;
}

/**
 * What the right of the status bar says about the open list. rekordbox has
 * one label there, written by whichever changed last [OBS: rekordbox 7.2.19]:
 *
 * - picking the list in the tree writes its tally, or clears the label for
 *   the collection and histories (`changedSelectedFolItem`), so `picked` is
 *   false and `totals` is what `formatListTotals` says, or nothing;
 * - picking tracks in it writes the selection
 *   (`ListViewer::selectedRowsChanged` -> `updateStatusBarWithSelectedTrackNum`
 *   at `0x10040c9b8`), which counts fewer than two rows as none
 *   (`cmp w8, #0x2; b.lt` at `0x10040cab8`), and `setSelectedTracks` writes an
 *   empty label for none (`cbz w1` at `0x1004ccaa0`). So one track selected,
 *   or the selection let go, leaves the bar blank until the list is picked in
 *   the tree again; two or more read `Selected: N Tracks`.
 *
 * rekordbox goes on with the selection's time and size
 * (`common::selectedTracksInfo`); rbx does not sum the selection yet.
 */
export function statusBarListText(
  state: { picked: boolean; selected: number; totals: { tracks: number; seconds: number; bytes: number } | undefined },
  t: Translate,
  locale = "en",
): string {
  if (state.picked) return state.selected >= 2 ? `${t("Selected")}: ${state.selected} ${t("Tracks")}` : "";
  return state.totals ? formatListTotals(state.totals, t, locale) : "";
}
