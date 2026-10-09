/**
 * What the information panel prints, worked out from a track's record.
 *
 * Pure, so the formatting is unit-tested without the panel: the Summary
 * tab's rows, the file-type label, and which Info-tab fields the writer will
 * take.
 */
import type { RowDto, SelectionDetails, TrackDetails, TrackField } from "@/ipc/types";
import { formatBitrate, formatBpm, formatBytes, formatDuration, formatShortDate } from "@/lib/format";

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

/**
 * What the Info tab shows and which tracks its edits go to: one track's
 * record, or a multiple selection read the way rekordbox reads one.
 */
export interface InfoView {
  /** Every track an edit goes to. */
  ids: readonly string[];
  /** Several tracks: the Track Title box is greyed and My Tag is not offered. */
  multiple: boolean;
  text: (field: TrackField) => string;
  rating: number;
  comment: string;
  color: string;
  albumArtist: string;
  bpm: string;
  releaseDate: string;
  mixName: string;
  message: string;
  hotCueAutoLoad: boolean;
  publish: boolean;
  /** The My Tags on the track; `null` when the toggles are not offered. */
  myTags: readonly string[] | null;
}

/** One track: its record, or the row's own fields until the record arrives. */
export function singleView(row: RowDto, details: TrackDetails | null): InfoView {
  const record = details ?? fromRow(row);
  return {
    ids: [row.id],
    multiple: false,
    text: (field) => fieldText(record, field),
    rating: record.rating,
    comment: record.comment,
    color: details?.color ?? "0",
    albumArtist: details?.albumArtist ?? "",
    bpm: formatBpm(record.bpmX100),
    releaseDate: record.releaseDate,
    mixName: details?.mixName ?? "",
    message: details?.message ?? "",
    hotCueAutoLoad: details?.hotCueAutoLoad ?? false,
    publish: details?.publish ?? false,
    myTags: details ? details.myTags : null,
  };
}

/** The `TrackDetails` field each Info-tab field is read from. */
const RECORD_FIELD: Record<TrackField, keyof TrackDetails> = {
  title: "title", artist: "artist", album: "album", year: "year", trackNumber: "trackNumber",
  discNumber: "discNumber", originalArtist: "originalArtist", composer: "composer",
  remixer: "remixer", lyricist: "lyricist", playCount: "playCount", genre: "genre",
  label: "label", key: "key", bpm: "bpmX100",
};

/**
 * Several tracks, as rekordbox's Information Window shows them.
 *
 * Each field is the tracks' shared value. One they do not share is blank —
 * text and the Year, Track number, Disc number and DJ Play Count boxes
 * alike — except the BPM, which reads 0.00, and the rating, which shows no
 * stars [OBS: rekordbox 7 on Windows 11, three and two tracks selected;
 * static: 7.2.11 `TrackInfoConcreteMediator::getTrackProp` returns an empty
 * string for an unshared field, `getBrowseInfoIntValue` 0 for an unshared BPM
 * or rating]. The two boxes are ticked only when every track has them
 * ticked (`getBrowseInfoBoolValue`). The colour is the first track's, shared
 * or not: `getBrowseInfoIntValue` reads it without comparing [static only;
 * not observed, the captured tracks had no colour].
 *
 * Until the record arrives every field is blank.
 */
export function selectionView(ids: readonly string[], selection: SelectionDetails | null): InfoView {
  const mixed = new Set<string>(selection?.mixed ?? []);
  const first = selection?.first;
  const shared = <K extends keyof TrackDetails>(key: K): TrackDetails[K] | undefined =>
    first && !mixed.has(key) ? first[key] : undefined;
  const sharedText = (key: keyof TrackDetails) => {
    const value = shared(key);
    return value === undefined ? "" : String(value);
  };
  const bpmX100 = shared("bpmX100");
  return {
    ids,
    multiple: true,
    text: (field) => (field === "bpm" ? formatBpm(bpmX100 ?? 0) : sharedText(RECORD_FIELD[field])),
    rating: shared("rating") ?? 0,
    comment: sharedText("comment"),
    color: first?.color || "0",
    albumArtist: sharedText("albumArtist"),
    bpm: first ? (bpmX100 === undefined ? (0).toFixed(2) : formatBpm(bpmX100)) : "",
    releaseDate: sharedText("releaseDate"),
    mixName: sharedText("mixName"),
    message: sharedText("message"),
    hotCueAutoLoad: shared("hotCueAutoLoad") ?? false,
    publish: shared("publish") ?? false,
    myTags: null,
  };
}

/** The record's shape from a row alone, for the moment before it arrives. */
export function fromRow(row: RowDto): TrackDetails {
  return {
    id: row.id,
    title: row.title,
    artist: row.artist,
    album: row.album,
    albumArtist: "",
    originalArtist: "",
    composer: "",
    remixer: "",
    lyricist: "",
    genre: row.genre,
    label: row.label,
    key: row.key,
    comment: row.comment,
    mixName: "",
    message: "",
    color: "0",
    rating: row.rating,
    bpmX100: row.bpmX100,
    durationSec: row.durationSec,
    year: 0,
    trackNumber: 0,
    discNumber: 0,
    playCount: 0,
    fileType: 0,
    fileSize: 0,
    bitrate: 0,
    sampleRate: 0,
    bitDepth: 0,
    dateCreated: "",
    releaseDate: row.releaseDate,
    path: "",
    hotCueAutoLoad: false,
    publish: false,
    hasArtwork: row.hasArtwork,
    myTags: [],
  };
}
