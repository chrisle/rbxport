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

/** `144.8 GB` / `15.2 MB` — status-bar totals. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "";
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}

/** `821 hours 32 minutes` — how rekordbox reports a selection's total time. */
export function formatTotalTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "0 minutes";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  if (h === 0) return `${m} minute${m === 1 ? "" : "s"}`;
  return `${h} hour${h === 1 ? "" : "s"} ${m} minute${m === 1 ? "" : "s"}`;
}

/** `Selected: 9952 Tracks, 821 hours 32 minutes, 144.8 GB` */
export function formatSelectionSummary(count: number, seconds: number, bytes: number): string {
  if (count <= 0) return "";
  return `Selected: ${count} Track${count === 1 ? "" : "s"}, ${formatTotalTime(seconds)}, ${formatBytes(bytes)}`;
}
