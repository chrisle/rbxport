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

/** `9/6/26` — US short date, no leading zeros, two-digit year. */
export function formatShortDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso);
  if (!m) return "";
  const [, y, mo, d] = m;
  if (!y || !mo || !d) return "";
  return `${Number(mo)}/${Number(d)}/${y.slice(2)}`;
}

/**
 * A moment in seconds since the Unix epoch as the date columns print a day,
 * `M/D/YY`, on this machine's calendar: the File Created column's time is
 * the file system's, not a date the library stored, so its day is local.
 */
export function formatEpochDate(seconds: number | null | undefined): string {
  if (typeof seconds !== "number" || !Number.isFinite(seconds)) return "";
  const date = new Date(seconds * 1000);
  if (Number.isNaN(date.getTime())) return "";
  return `${date.getMonth() + 1}/${date.getDate()}/${String(date.getFullYear()).slice(-2)}`;
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
