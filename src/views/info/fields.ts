/**
 * What the information panel prints, worked out from a track's record.
 *
 * Pure, so the formatting is unit-tested without the panel: the Summary
 * tab's rows, the file-type label, and which Info-tab fields the writer will
 * take.
 */
import type { RowDto, TrackDetails, TrackField } from "@/ipc/types";
import { formatBitrate, formatBytes, formatDuration, formatShortDate } from "@/lib/format";

/** A label and the text beside it. */
export interface Fact {
  label: string;
  value: string;
}

/**
 * `FileType` → the label rekordbox prints, from `german.lang`.
 *
 * The codes were counted against the file extension across the reference
 * library's 38,681 tracks: 1 is every `.mp3`, 4 `.m4a`, 5 `.flac`, 11
 * `.wav`, 12 `.aiff`/`.aif`. The capture confirms 11 prints as "WAV File".
 * One `.m4a` carries 6 — ALAC or AAC, undetermined — and prints nothing
 * rather than a guess.
 */
export function fileTypeLabel(code: number): string {
  switch (code) {
    case 1: return "MP3 File";
    case 4: return "M4A File";
    case 5: return "FLAC File";
    case 11: return "WAV File";
    case 12: return "AIFF File";
    default: return "";
  }
}

/** The file's four facts, printed. */
export interface FileFacts {
  type: string;
  size: string;
  sampleRate: string;
  bitrate: string;
}

/**
 * The file's facts as the Summary tab prints them — "WAV File", "45.1 MB",
 * "44100 Hz", "1411 kbps" in the capture — and as the deck's INFO tab
 * reuses them. A value the library does not hold is blank, except a zero
 * bitrate, which rekordbox prints as "VBR".
 */
export function fileFacts(d: TrackDetails): FileFacts {
  return {
    type: fileTypeLabel(d.fileType),
    size: d.fileSize > 0 ? formatBytes(d.fileSize) : "",
    sampleRate: d.sampleRate > 0 ? `${d.sampleRate} Hz` : "",
    bitrate: formatBitrate(d.bitrate),
  };
}

/**
 * The Summary tab's table, in the captured order.
 *
 * Every row is kept when a value is blank, so the table does not jump as
 * the selection moves. Until the record arrives the row's own duration is
 * shown and the rest is blank rather than the previous track's.
 */
export function summaryFacts(row: RowDto, details: TrackDetails | null): Fact[] {
  const d = details && details.id === row.id ? details : null;
  const file = d ? fileFacts(d) : null;
  return [
    { label: "Time", value: formatDuration(d?.durationSec ?? row.durationSec) },
    { label: "File Type", value: file?.type ?? "" },
    { label: "Size", value: file?.size ?? "" },
    { label: "Date Created", value: d ? formatShortDate(d.dateCreated) : "" },
    { label: "Sample Rate", value: file?.sampleRate ?? "" },
    { label: "Bitrate", value: file?.bitrate ?? "" },
    { label: "DJ Play Count", value: d ? String(d.playCount) : "" },
    { label: "Location", value: d?.path ?? "" },
  ];
}

/**
 * rekordbox's eight track colours, by `ColorID`.
 *
 * Names are `djmdColor.Commnt` in the reference library, ids 1 to 8 in
 * `SortKey` order; `"0"` (38,671 of 38,681 tracks) and NULL are none.
 */
export const COLORS: readonly { id: string; name: string }[] = [
  { id: "1", name: "Pink" },
  { id: "2", name: "Red" },
  { id: "3", name: "Orange" },
  { id: "4", name: "Yellow" },
  { id: "5", name: "Green" },
  { id: "6", name: "Aqua" },
  { id: "7", name: "Blue" },
  { id: "8", name: "Purple" },
];

/** The text an Info-tab field starts out with, from the record. */
export function fieldText(d: TrackDetails, field: TrackField): string {
  switch (field) {
    case "title": return d.title;
    case "artist": return d.artist;
    case "album": return d.album;
    case "year": return String(d.year);
    case "trackNumber": return String(d.trackNumber);
    case "discNumber": return String(d.discNumber);
    case "originalArtist": return d.originalArtist;
    case "composer": return d.composer;
    case "remixer": return d.remixer;
    case "lyricist": return d.lyricist;
    case "playCount": return String(d.playCount);
    case "genre": return d.genre;
    case "label": return d.label;
    case "key": return d.key;
    case "bpm": return (d.bpmX100 / 100).toFixed(2);
  }
}

/**
 * Whether a typed value is one the writer will take, so a refusal is shown
 * before the round trip rather than after it. Mirrors `Writer::set_field`.
 */
export function acceptable(field: TrackField, value: string): boolean {
  switch (field) {
    case "year": return /^\s*\d{1,4}\s*$/.test(value);
    case "trackNumber": return /^\s*\d{1,4}\s*$/.test(value);
    case "discNumber": return /^\s*\d{1,3}\s*$/.test(value);
    case "playCount": return /^\s*\d{1,6}\s*$/.test(value);
    // 20 to 400, as the writer takes it.
    case "bpm": {
      const bpm = Number.parseFloat(value.trim());
      return /^\s*\d+(\.\d+)?\s*$/.test(value) && bpm >= 20 && bpm <= 400;
    }
    default: return true;
  }
}

/**
 * The Release Date box's three segments.
 *
 * The capture's boxes are 47, 109 and 61pt wide — a two-digit day, a month
 * name and a year fit those; a numeric month in the middle would not need
 * 109pt. Assumed from the widths, since the captured date is empty; the box
 * is read-only regardless.
 */
export function dateSegments(iso: string): [string, string, string] {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso);
  if (!m) return ["", "", ""];
  const [, y = "", mo = "", d = ""] = m;
  const month = MONTHS[Number(mo) - 1] ?? "";
  return [String(Number(d)), month, y];
}

const MONTHS = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];
