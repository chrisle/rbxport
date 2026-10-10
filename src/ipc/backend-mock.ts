/**
 * In-browser stand-in for the Rust backend.
 *
 * Deliberately mirrors the real contract's *shape*: it owns the ordering, hands
 * out view handles, and only ever returns a window of rows. That way the UI is
 * written against the real constraints from day one, and `pnpm dev:mock` runs
 * the actual components with no Tauri, no database and no rekordbox.
 *
 * Its sort and search semantics are checked against the Rust view tests by a
 * parity test once `rbl-index` lands.
 */
import { TRACK_SEARCH_OPTIONS, type TrackSearchField } from "@/lib/search";
import theme from "@/styles/theme";

import type {
  AppErrorDto, Backend, Backup, BackupProgress, BackupSizes, Cue, DeckEvent, Device, DeviceSettings, Edits, ExplorerRoot, ExportReport,
  DatabaseDrive, EditHistoryState, FilterValues, GridState, LibraryProblem, LibrarySummary, Limiter, LinkPeerSeen, LinkStatus, RelatedCriterion, RowDto, SortKey,
  SelectionDetails, SmartRule, StickDefaults, SyncPlaylist, SyncProgress, Tick, TrackDetails, TrackField,
  PreferencesRequest, RelocateSearch, UpdateCheck, UpdateProgress, UpdateReady, ExportProgress,
  DeckId, TrackFilter, TreeNode, ViewHandle, ViewSpec, WaveformKind,
} from "./types";
import { TREE_ROOT } from "./types";
import { applyEditFrom, validateEdit, isDynamicFrom, tempoX100, type EditableBeat } from "@/lib/gridEdit";
import { toCamelot } from "@/lib/camelot";
import { COLOR_NAMES, wholeBpm } from "@/lib/trackFilter";
import { referenceDeviceSettings } from "./mock-device-settings";
import { createMockDeviceLibraries } from "./mock-device-library";

const ARTISTS = [
  "MORTEN", "ARTBAT", "Meduza", "Vintage Culture", "Tujamo", "UMEK", "Kryder", "Joel Corry",
  "Carl Bee", "Pretty Pink", "Dommo", "Eric Prydz", "Sarah de Warren", "Timmy Trumpet",
  "Oliver Heldens", "The Temper Trap", "Wh0", "Benny Benassi", "Hayla", "Anyma",
];
const TITLES = [
  "Take Me Home", "The Abyss", "Love To Give", "Your Eyes", "Love is Gonna Save Us",
  "Sweet Disposition", "Edge Of The World", "Another World", "Something In The Air",
  "Daydream", "Up To My Head", "Walking On A Dream", "Safe With Me", "Renegade Master",
  "Gravity", "Airplane Mode", "Wasted Time", "Brighter Days", "Angels", "Goddess",
];
const MIXES = ["(Extended Mix)", "(Original Mix)", "(Extended Remix)", "(Radio Edit)", "(Club Mix)"];
const KEYS = ["Am", "Bm", "Cm", "Dm", "Em", "Fm", "Gm", "Abm", "Bbm", "Dbm", "Ebm", "F#m", "D", "A", "E"];
const GENRES = ["House", "Tech House", "Melodic House", "Techno", "Trance", "Progressive House", ""];
const LABELS = ["Spinnin'", "Musical Freedom", "Defected", "Armada", "Toolroom", "Drumcode", ""];

/**
 * The two hot-cue sets the reference playlist carries, in the colours
 * rekordbox draws them — the default green on A–D, and the E–H set one DJ
 * tool writes in teal, orange, blue and yellow — plus the coloured A–D set,
 * so every measured colour is on screen somewhere.
 */
const CUE_SETS: readonly (readonly (readonly [string, number, string | null])[])[] = [
  [["A", 0.12, theme.color.cueHot.value], ["B", 0.34, theme.color.cueHot.value], ["C", 0.61, theme.color.cueHot.value], ["D", 0.83, theme.color.cueHot.value]],
  [["E", 0.12, theme.color.cueTeal.value], ["F", 0.29, theme.color.cueOrange.value], ["G", 0.46, theme.color.cueBlue.value], ["H", 0.70, theme.color.cueYellow.value]],
  [["A", 0.12, theme.color.cuePink.value], ["B", 0.26, theme.color.cueAqua.value], ["C", 0.61, theme.color.cueLime.value], ["D", 0.79, theme.color.cuePurple.value]],
];

/** What a hot cue added here draws: index 21, the default the writer stores. */
const DEFAULT_CUE_COLOUR = theme.color.cueHot.value;

/**
 * How long the mock pretends analysis takes.
 *
 * Long enough that a queue can be watched and stopped, short enough that a
 * suite of them is not slow.
 */
const ANALYSIS_MS = 120;

/** Deterministic PRNG so every run, test and screenshot sees identical data. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Each row's `ColorID`, 0 for none and 1 to 8 for the colour comments.
 *
 * Beside the rows rather than on them: the row DTO carries no colour yet, and
 * the filter bar needs one to filter by. Roughly one row in six is coloured,
 * so a ticked colour narrows the list visibly without emptying it.
 */
function makeColors(count: number): Uint8Array {
  const rnd = mulberry32(20260909);
  const out = new Uint8Array(count);
  for (let i = 0; i < count; i++) {
    out[i] = rnd() > 0.85 ? 1 + Math.floor(rnd() * 8) : 0;
  }
  return out;
}

/**
 * The filter bar's semantics, as `rbl-index` has them.
 *
 * Whole BPMs at ±0% match by rounded bucket; a tolerance widens each picked
 * BPM into a band, or with `All` picked, a band around the master player —
 * and with no master player the BPM column matches everything. The other
 * columns are sets, and every ticked column has to agree.
 */
function passesFilter(row: RowDto, color: number, filter: TrackFilter): boolean {
  if (filter.bpm) {
    const { values, tolerancePct, masterBpmX100 } = filter.bpm;
    const within = (centre: number) =>
      Math.abs(row.bpmX100 - centre) <= Math.floor((centre * tolerancePct) / 100);
    if (values.length > 0) {
      const hit =
        tolerancePct === 0
          ? values.includes(wholeBpm(row.bpmX100))
          : values.some((v) => within(v * 100));
      if (!hit) return false;
    } else if (masterBpmX100 !== null && masterBpmX100 > 0 && !within(masterBpmX100)) {
      return false;
    }
  }
  if (filter.keys && !filter.keys.includes(row.key)) return false;
  if (filter.ratings && !filter.ratings.includes(row.rating)) return false;
  if (filter.colors && !filter.colors.includes(COLOR_NAMES[color - 1] ?? "")) return false;
  return true;
}

/** Camelot number then letter, unknown names last: the order the bar lists keys in. */
function keyOrder(key: string): [number, number, string] {
  const code = toCamelot(key);
  if (!code) return [99, 2, key.toLowerCase()];
  return [Number(code.slice(0, -1)), code.endsWith("A") ? 0 : 1, ""];
}

function makeRows(count: number): RowDto[] {
  const rnd = mulberry32(20260907);
  const rows: RowDto[] = Array.from({ length: count });
  for (let i = 0; i < count; i++) {
    const analysed = rnd() > 0.12 ? 1 : 0;
    const artist = ARTISTS[Math.floor(rnd() * ARTISTS.length)] ?? "";
    const key = analysed ? (KEYS[Math.floor(rnd() * KEYS.length)] ?? "") : "";
    const bpmX100 = analysed ? (120 + Math.floor(rnd() * 20)) * 100 : 0;
    const month = 1 + Math.floor(rnd() * 12);
    const day = 1 + Math.floor(rnd() * 28);
    // The draws stay in this order: the e2e suite picks rows by index and
    // relies on which of them the seed makes analysed.
    const title = `${TITLES[Math.floor(rnd() * TITLES.length)]} ${MIXES[Math.floor(rnd() * MIXES.length)]}`;
    const album = rnd() > 0.6 ? "Single" : "";
    const genre = GENRES[Math.floor(rnd() * GENRES.length)] ?? "";
    const label = LABELS[Math.floor(rnd() * LABELS.length)] ?? "";
    const comment = analysed && rnd() > 0.5 ? `${1 + Math.floor(rnd() * 12)}A - ${key.slice(0, 1)} - ${bpmX100 / 100}` : "";
    const durationSec = 180 + Math.floor(rnd() * 240);
    const rating = Math.floor(rnd() * 6);
    const dateAdded = `2026-0${1 + Math.floor(rnd() * 9)}-${String(day).padStart(2, "0")}`;
    const releaseDate = rnd() > 0.3 ? `2026-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}` : "";
    const cueSet = analysed ? (CUE_SETS[Math.floor(rnd() * CUE_SETS.length)] ?? []) : [];
    rows[i] = {
      id: String(100000 + i),
      trackNo: i + 1,
      title,
      artist,
      album,
      genre,
      label,
      comment,
      bpmX100,
      key,
      durationSec,
      rating,
      analysed,
      dateAdded,
      releaseDate,
      memoryCues: analysed ? [Math.round(durationSec * 1000 * 0.02)] : [],
      hotCues: cueSet.map(([letter, at, colour]) => [letter, Math.round(durationSec * 1000 * at), colour]),
      artworkHue: Math.floor(rnd() * 360),
      // The mock has no files to serve, so every row falls back to the tint.
      hasArtwork: false,
      fileName: `${String(i + 1).padStart(2, "0")} ${artist} - track.mp3`,
      extra: {
        size: 8_000_000 + i * 1000,
        discNo: 1,
        albumArtist: artist,
        trackNumber: i % 12 + 1,
        composer: artist,
        lyricist: "",
        fileType: 1,
        year: 2026,
        mixName: MIXES[i % MIXES.length] ?? "",
        remixer: "",
        originalArtist: artist,
        sampleRate: 44100,
        bitrate: 320,
        bitDepth: 16,
        location: `/Users/mock/Music/${artist}/${i + 1}.mp3`,
        dateCreated: dateAdded,
        publishTrackInfo: false,
        message: "",
        color: i % 9,
        djPlayCount: i % 7,
        myTag: i % 3 === 0 ? "Peak Time" : "",
        cloud: false,
      },
    };
  }
  return rows;
}

/**
 * A fake disk for the Explorer.
 *
 * The shape of the capture: the music and home folders, the system volume,
 * one stick. The music folder's Downloads holds twelve files, the first six
 * of them library rows — so a folder view shows both kinds — and Sets holds
 * three loose files. Everything else is folders, and a folder not listed here
 * has nothing under it, which is what the real backend answers for a folder
 * it cannot read.
 */
const EXPLORER_ROOTS: readonly ExplorerRoot[] = [
  { name: "Music", path: "/Users/mock/Music" },
  { name: "mock", path: "/Users/mock" },
  { name: "Macintosh HD", path: "/" },
  { name: "SD", path: "/Volumes/SD" },
];
const EXPLORER_CHILDREN: ReadonlyMap<string, readonly string[]> = new Map([
  ["/Users/mock/Music", ["Downloads", "Rekordbox", "Sets"]],
  ["/Users/mock", ["Desktop", "Documents", "Music"]],
  ["/", ["Applications", "Library", "System", "Users"]],
  ["/Users", ["mock", "Shared"]],
  ["/Volumes/SD", ["Contents", "PIONEER"]],
  ["/Volumes/SD/PIONEER", ["rekordbox", "USBANLZ"]],
]);
/** Files per folder: a library row index, or a loose file's name. */
const EXPLORER_FILES: ReadonlyMap<string, readonly (number | string)[]> = new Map([
  ["/Users/mock/Music/Downloads", [0, 1, 2, 3, 4, 5, "Untitled Bounce 1.wav", "Untitled Bounce 2.wav", "demo_128.aiff", "live edit.mp3", "promo (radio).mp3", "voice memo.m4a"]],
  ["/Users/mock/Music/Sets", ["Set 2026-08-30.mp3", "Set 2026-09-04.mp3", "Warmup.flac"]],
]);

/** A row for a file the library does not hold: its name, and nothing else. */
function looseRow(folder: string, name: string, position: number): RowDto {
  return {
    id: `file:${folder}/${name}`,
    trackNo: position,
    title: name.replace(/\.[^.]+$/, ""),
    artist: "",
    album: "",
    genre: "",
    label: "",
    comment: "",
    bpmX100: 0,
    key: "",
    durationSec: 0,
    rating: 0,
    analysed: 0,
    dateAdded: "",
    releaseDate: "",
    hotCues: [],
    artworkHue: 0,
    hasArtwork: false,
    fileName: name,
  };
}

const FOLDERS = ["CURRENT", "USB", "DOWNLOADS"];
const PLAYLISTS = [
  "Melodic Vox", "Hardstyle", "Drum and Bass", "Eurodance", "Latin", "Main", "Main: Vocal",
  "Melodic Techno", "Techno", "Trance", "Groovy", "Fun House", "House", "Tech House",
  "Special", "HOUSE CLASSIC", "TRIODE",
];

/** How many tracks a mock playlist holds, from its id, so every path agrees. */
function mockPlaylistSize(id: string): number {
  const seed = [...id].reduce((a, c) => a + c.charCodeAt(0), 0);
  return 14 + (seed % 30);
}

type PlaylistFixture = "default" | "empty" | "cueOnly";

function makeTree(playlistFixture: PlaylistFixture): TreeNode[] {
  const nodes: TreeNode[] = [
    { id: "all", name: "All Tracks", kind: "allTracks", depth: 0 },
    { id: "playlists", name: "Playlists", kind: "collection", depth: 0, expanded: true },
  ];
  if (playlistFixture === "cueOnly") {
    nodes.push({ id: "cue", name: "CUE", kind: "playlist", depth: 1, childCount: mockPlaylistSize("cue") });
  } else if (playlistFixture === "default") {
    let n = 0;
    for (const [fi, folder] of FOLDERS.entries()) {
      nodes.push({ id: `folder-${fi}`, name: folder, kind: "folder", depth: 1, expanded: fi < 2 });
      if (fi >= 2) continue;
      const take = fi === 0 ? 3 : PLAYLISTS.length - 3;
      for (let i = 0; i < take; i++) {
        const name = PLAYLISTS[n++ % PLAYLISTS.length] ?? "";
        const id = `pl-${fi}-${i}`;
        // The count the real tree carries, from the same seed `openView` uses.
        nodes.push({ id, name, kind: "playlist", depth: 2, childCount: mockPlaylistSize(id) });
      }
      // One intelligent playlist in the first folder, as a library with rules
      // shows: no count, since a rule's count is only known once it is opened.
      if (fi === 0) {
        nodes.push({ id: "smart-0", name: "Fresh 128s", kind: "smartPlaylist", depth: 2 });
      }
    }
  }
  // Histories, filed as rekordbox files them: a folder per year, one per month
  // inside it under the month's name, and the sessions under that. The year
  // open and the month closed, as the shell sends them.
  nodes.push({ id: "histories", name: "Histories", kind: "histories", depth: 0, expanded: true });
  nodes.push({ id: "hist-2026", name: "2026", kind: "history", depth: 1, expanded: true });
  nodes.push({ id: "hist-202609", name: "September", kind: "history", depth: 2, expanded: false });
  for (const day of ["2026-09-04", "2026-08-30", "2026-08-23"]) {
    nodes.push({ id: `hist-${day}`, name: `LINK HISTORY ${day}`, kind: "history", depth: 3 });
  }
  return nodes;
}

const collator = new Intl.Collator("en", { sensitivity: "base", numeric: true });

/** A detail field as a number, 0 when the row does not carry it. */
function extraNumber(row: RowDto, field: string): number {
  const value = row.extra?.[field];
  return typeof value === "number" ? value : 0;
}

/** A detail field as text, empty when the row does not carry it. */
function extraText(row: RowDto, field: string): string {
  const value = row.extra?.[field];
  return typeof value === "string" ? value : "";
}

function compare(a: RowDto, b: RowDto, col: SortKey): number {
  switch (col) {
    case "keyCamelot": {
      // Round the wheel, unknown keys last, as the index ranks them.
      const rank = (key: string) => {
        const code = toCamelot(key);
        if (code === "") return Number.MAX_SAFE_INTEGER;
        return Number(code.slice(0, -1)) * 2 + (code.endsWith("B") ? 1 : 0);
      };
      return rank(a.key) - rank(b.key) || collator.compare(a.key, b.key);
    }
    case "trackNo": return a.trackNo - b.trackNo;
    case "bpm": return a.bpmX100 - b.bpmX100;
    case "duration": return a.durationSec - b.durationSec;
    case "rating": return a.rating - b.rating;
    case "djPlayCount": case "size": case "year": case "sampleRate": case "bitrate": case "color":
    case "discNo": case "trackNumber": case "fileType": case "bitDepth":
      return extraNumber(a, col) - extraNumber(b, col);
    // Ticked first, as rekordbox's `comparePublic` orders the box.
    case "publishTrackInfo": return Number(b.extra?.publishTrackInfo === true) - Number(a.extra?.publishTrackInfo === true);
    case "fileName": return collator.compare(a.fileName ?? "", b.fileName ?? "");
    case "location": case "composer": case "albumArtist": case "remixer": case "originalArtist":
    case "mixName": case "lyricist": case "message": case "dateCreated":
      return collator.compare(extraText(a, col), extraText(b, col));
    case "title": return collator.compare(a.title, b.title);
    case "artist": return collator.compare(a.artist, b.artist);
    case "album": return collator.compare(a.album, b.album);
    case "genre": return collator.compare(a.genre, b.genre);
    case "label": return collator.compare(a.label, b.label);
    case "comment": return collator.compare(a.comment, b.comment);
    case "key": return collator.compare(a.key, b.key);
    case "dateAdded": return collator.compare(a.dateAdded, b.dateAdded);
    case "releaseDate": return collator.compare(a.releaseDate, b.releaseDate);
  }
}

/** Matches the folding the Rust index uses: lowercase, accents stripped. */
export function fold(s: string): string {
  return s.normalize("NFKD").replace(/[̀-ͯ]/g, "").toLowerCase();
}

export interface MockOptions {
  trackCount?: number;
  /** The playlist-tree fixture used by focused browser tests. */
  playlistFixture?: PlaylistFixture;
  /** Simulated IPC latency in ms; 0 keeps tests fast. */
  latencyMs?: number;
  /**
   * Whether the library reports itself writable. Off by default — the mock
   * stands in for a library rekordbox is holding, which is what the menus
   * and the deck's editing controls are tested against — and `?writable=1`
   * turns it on for the tests that exercise an edit.
   */
  writable?: boolean;
  /**
   * Every how-manyth track's file is gone, from `?missing=N`, starting with
   * the second: the Collection's `[!]` and the Missing File Manager need some.
   * Unset, nothing is missing, as the rest of the suite expects.
   */
  missingEvery?: number;
}

export function createMockBackend(options: MockOptions = {}): Backend {
  const trackCount = options.trackCount ?? readCountFromUrl() ?? 2000;
  const latency = options.latencyMs ?? readLatencyFromUrl() ?? 0;
  const playlistFixture = options.playlistFixture ?? readPlaylistFixtureFromUrl();
  const all = makeRows(trackCount);
  const missingEvery = options.missingEvery ?? readMissingFromUrl();
  if (missingEvery !== null) {
    for (let i = 1; i < all.length; i += missingEvery) {
      const row = all[i];
      if (row) row.missing = true;
    }
  }
  const colors = makeColors(trackCount);
  const rowPositions = new Map(all.map((row, index) => [row.id, index]));
  // What the row DTO does not carry, made up per track and edited in place.
  const details = new Map<string, TrackDetails>();


  /** The rows a source and query leave, before sorting and before the filter. */
  // The mock owns the tree and the playlists' contents the same way Rust
  // does, so the edit flows can be driven end to end in `pnpm dev:mock` and
  // in Playwright without a database.
  let generation = 1;
  /**
   * What a playlist holds once anything has been done to it, as track ids in
   * playing order. A playlist nobody has touched is not here and shows its
   * seeded slice; one made here starts empty, as a new one does.
   */
  const membership = new Map<string, string[]>();
  /** The Tag List's track ids, in its order. */
  const tagList: string[] = [];
  /** Intelligent playlists' rules, by id. */
  const smartRules = new Map<string, SmartRule>();
  let nextId = 1;
  const indexOfId = new Map(all.map((row, i) => [row.id, i] as const));

  /** The deterministic slice a list shows before it is edited. */
  const seededMembers = (id: string): number[] => {
    const seed = [...id].reduce((a, c) => a + c.charCodeAt(0), 0);
    const size = 14 + (seed % 30);
    return Array.from({ length: size }, (_, i) => (seed * 37 + i * 101) % trackCount);
  };
  /** A playlist's tracks as ids, seeding the membership on first edit. */
  const membersOf = (playlist: string): string[] => {
    const held = membership.get(playlist);
    if (held) return held;
    const seeded = seededMembers(playlist).map((i) => all[i]?.id ?? "");
    membership.set(playlist, seeded);
    return seeded;
  };
  /** The count the tree and the export report carry. */
  const playlistSize = (id: string): number =>
    membership.get(id)?.length ?? mockPlaylistSize(id);

  /**
   * Related Tracks, as the index picks them: within five percent of the
   * track's BPM or of half or double it, and in its key or one beside it on
   * the wheel, the same
   * genre added in the last thirty days, or the same artist. The track
   * itself is left out. No track, no rows.
   */
  const relatedTo = (trackId: string, criterion: RelatedCriterion): number[] => {
    const at = indexOfId.get(trackId);
    const track = at === undefined ? undefined : all[at];
    if (!track) return [];
    const rank = (key: string) => {
      const code = toCamelot(key);
      return code === "" ? -1 : (Number(code.slice(0, -1)) - 1) * 2 + (code.endsWith("B") ? 1 : 0);
    };
    const together = (a: number, b: number) =>
      a >= 0 && b >= 0 && (a === b || (a ^ 1) === b || ((a & 1) === (b & 1) && ((a + 2) % 24 === b || (b + 2) % 24 === a)));
    // rekordbox's window: 5% either side, ends rounded ties to even, at the
    // track's tempo, half it and double it.
    const roundEven = (x: number) => {
      const r = Math.round(x);
      return Math.abs(x % 1) === 0.5 && r % 2 !== 0 ? r - 1 : r;
    };
    const within = (centre: number, bpm: number) =>
      bpm >= roundEven(Math.max(centre * (1 - 0.05), 0)) && bpm <= roundEven(centre * (0.05 + 1));
    const bpmMatches = (centre: number, bpm: number) =>
      within(centre, bpm) || within(roundEven(centre * 0.5), bpm) || within(centre * 2, bpm);
    const since = Date.parse("2026-09-18") - 30 * 86_400_000;
    return all.flatMap((row, i) => {
      if (i === at) return [];
      switch (criterion) {
        case "bpmKey": {
          if (track.bpmX100 === 0 && rank(track.key) < 0) return [];
          if (track.bpmX100 !== 0 && !bpmMatches(track.bpmX100, row.bpmX100)) return [];
          if (rank(track.key) >= 0 && !together(rank(track.key), rank(row.key))) return [];
          return [i];
        }
        case "genreRecent":
          return track.genre !== "" && row.genre === track.genre && Date.parse(row.dateAdded) >= since ? [i] : [];
        case "artist":
          return track.artist !== "" && row.artist === track.artist ? [i] : [];
        // The mock keeps no play history, so its suggestions are the BPM
        // and key matches, as the real backend's are without one.
        case "suggestion": {
          if (track.bpmX100 === 0 && rank(track.key) < 0) return [];
          if (track.bpmX100 !== 0 && !bpmMatches(track.bpmX100, row.bpmX100)) return [];
          if (rank(track.key) >= 0 && !together(rank(track.key), rank(row.key))) return [];
          return [i];
        }
        default:
          return [];
      }
    });
  };
  const candidatesFor = (spec: ViewSpec): number[] => {
    let candidates: number[];
    if (spec.source.kind === "playlist") {
      const held = membership.get(spec.source.id);
      candidates = held
        ? held.map((id) => indexOfId.get(id)).filter((i): i is number => i !== undefined)
        : seededMembers(spec.source.id);
    } else if (spec.source.kind === "playlistFolder") {
      const folderId = spec.source.id;
      const at = tree.findIndex((node) => node.id === folderId && node.kind === "folder");
      const depth = tree[at]?.depth ?? 0;
      const seen = new Set<number>();
      candidates = [];
      for (let i = at + 1; at >= 0 && i < tree.length && (tree[i]?.depth ?? 0) > depth; i += 1) {
        const node = tree[i];
        if (node?.kind !== "playlist" && node?.kind !== "smartPlaylist") continue;
        for (const id of membersOf(node.id)) {
          const row = indexOfId.get(id);
          if (row !== undefined && !seen.has(row)) {
            seen.add(row);
            candidates.push(row);
          }
        }
      }
    } else if (spec.source.kind === "history") {
      candidates = membersOf(spec.source.id).map((id) => indexOfId.get(id)).filter((i): i is number => i !== undefined);
    } else if (spec.source.kind === "tagList") {
      candidates = tagList.map((id) => indexOfId.get(id)).filter((i): i is number => i !== undefined);
    } else if (spec.source.kind === "related") {
      candidates = relatedTo(spec.source.track, spec.source.criterion);
    } else {
      candidates = Array.from({ length: trackCount }, (_, i) => i);
    }
    const q = fold(spec.query.trim());
    if (q) candidates = candidates.filter((i) => all[i] && matchesSearch(all[i], q, spec.searchField ?? "all"));
    return candidates;
  };
  const tree = makeTree(playlistFixture);
  type MockEdit = { label: string; undo: () => void; redo: () => void };
  const editUndo: MockEdit[] = [];
  const editRedo: MockEdit[] = [];

  const views = new Map<number, { order: Uint32Array; trackNos?: Uint32Array; gen: number }>();
  // A folder's rows, held whole: a folder is a few files here and a few
  // thousand at most on a disk, which is why the real backend keeps the list
  // and pages it out rather than sending it.
  const folderViews = new Map<number, RowDto[]>();
  let nextViewId = 1;

  const listeners = new Set<(generation: number) => void>();
  const historyListeners = new Set<(history: EditHistoryState) => void>();
  const tagListListeners = new Set<() => void>();

  const historyState = (): EditHistoryState => ({
    generation,
    canUndo: editUndo.length > 0,
    canRedo: editRedo.length > 0,
    undoLabel: editUndo.at(-1)?.label ?? null,
    redoLabel: editRedo.at(-1)?.label ?? null,
  });
  const announceHistory = () => {
    const state = historyState();
    for (const listener of historyListeners) listener(state);
    return state;
  };

  /**
   * Every edit bumps the generation and tells anyone listening, exactly as a
   * real write does — the backend reloads and emits `library:changed`.
   */
  const bump = (clearEditRedo = true): Promise<number> => {
    if (clearEditRedo) editRedo.length = 0;
    generation += 1;
    // The counts the tree shows follow the edit, as the re-read tree does.
    for (const node of tree) {
      if (node.kind === "playlist") node.childCount = playlistSize(node.id);
    }
    for (const listener of listeners) listener(generation);
    if (clearEditRedo) announceHistory();
    return wait(generation);
  };

  /**
   * A Tag List edit, as the real backend answers one: the generation stays,
   * so the lists on screen keep their pages, and only the Tag List is told.
   */
  const tagListChanged = (): Promise<number> => {
    editRedo.length = 0;
    for (const listener of tagListListeners) listener();
    announceHistory();
    return wait(generation);
  };

  const recordEdit = async (edit: MockEdit): Promise<EditHistoryState> => {
    await bump(false);
    editUndo.push(edit);
    if (editUndo.length > 50) editUndo.shift();
    editRedo.length = 0;
    return announceHistory();
  };

  /**
   * One edit over several tracks, recorded as one step of history the way
   * the real backend records a multiple selection's edit.
   */
  const recordTrackEdits = (
    tracks: readonly string[],
    one: (track: string) => { undo: () => void; redo: () => void },
  ): Promise<EditHistoryState> => {
    const steps = tracks.map(one);
    return recordEdit({
      label: "Track Edit",
      undo: () => { for (const step of [...steps].reverse()) step.undo(); },
      redo: () => { for (const step of steps) step.redo(); },
    });
  };

  const setHasArtwork = (track: string, value: boolean) => {
    const row = all.find((r) => r.id === track);
    const before = row?.hasArtwork ?? false;
    const apply = (has: boolean) => {
      if (row) row.hasArtwork = has;
      const detail = details.get(track);
      if (detail) detail.hasArtwork = has;
    };
    apply(value);
    return { undo: () => apply(before), redo: () => apply(value) };
  };

  const findNode = (id: string) => tree.find((n) => n.id === id);
  const treeSnapshot = () => tree.map((node) => ({ ...node }));
  const restoreTree = (snapshot: readonly TreeNode[]) => {
    tree.splice(0, tree.length, ...snapshot.map((node) => ({ ...node })));
  };
  const membershipSnapshot = () => new Map([...membership].map(([id, tracks]) => [id, [...tracks]]));
  const restoreMembership = (snapshot: ReadonlyMap<string, readonly string[]>) => {
    membership.clear();
    for (const [id, tracks] of snapshot) membership.set(id, [...tracks]);
  };

  /**
   * Puts a new node where the re-read tree would show it: last under its
   * parent, or last among the top-level playlists — before the Histories,
   * whose collapsed heading would otherwise hide anything appended after it.
   */
  const insertUnder = (parent: string, node: TreeNode, index?: number) => {
    let at: number;
    const before = index === undefined ? undefined : childrenOf(parent)[index];
    if (before !== undefined) {
      at = tree.findIndex((n) => n.id === before.id);
    } else if (parent === TREE_ROOT) {
      at = tree.findIndex((n) => n.depth === 0 && n.id !== "all" && n.id !== "playlists");
    } else {
      const start = tree.findIndex((n) => n.id === parent);
      const depth = tree[start]?.depth ?? 0;
      at = start + 1;
      while (at < tree.length && (tree[at]?.depth ?? 0) > depth) at += 1;
    }
    tree.splice(at < 0 ? tree.length : at, 0, node);
  };

  /**
   * A parent's children, as the flat tree encodes them.
   *
   * There is no parent field here: depth and array order are the structure, so
   * a child is a node one level down before the run returns to the parent's
   * own level.
   */
  const childrenOf = (parent: string): TreeNode[] => {
    const start = parent === TREE_ROOT ? -1 : tree.findIndex((n) => n.id === parent);
    if (parent !== TREE_ROOT && start < 0) return [];
    const depth = parent === TREE_ROOT ? 0 : (tree[start]?.depth ?? 0);
    const out: TreeNode[] = [];
    for (let i = start + 1; i < tree.length; i += 1) {
      const node = tree[i];
      if (!node) continue;
      if (parent !== TREE_ROOT && node.depth <= depth) break;
      if (node.depth === depth + 1) out.push(node);
    }
    return out;
  };

  /** How many nodes a node spans: itself and everything under it. */
  const subtreeLength = (at: number): number => {
    const depth = tree[at]?.depth ?? 0;
    let end = at + 1;
    while (end < tree.length && (tree[end]?.depth ?? 0) > depth) end += 1;
    return end - at;
  };

  /**
   * Whether the library has "finished loading".
   *
   * The mock answers instantly, which is exactly why the real app could sit on
   * "Loading…" forever without a test noticing: the backend loads on its own
   * thread and the first request can arrive before there is anything to answer
   * with. `?slow` holds the library back until `window.__libraryReady()` is
   * called, so that race can be driven deliberately.
   */
  /**
   * `?nolibrary` is a machine with no rekordbox library at all: nothing loads
   * until `createLibrary`, and `libraryProblem` says so.
   * `?libraryunavailable` is rekordbox set to a library on a drive that is
   * not connected; `useDefaultLibrary` then leaves the default folder, which
   * is empty, to be made. `?drivelibrary` puts a library on a connected
   * drive for Database management to list.
   */
  const defaultMasterDb = "/Users/you/Library/Pioneer/rekordbox/master.db";
  const databaseDrives: DatabaseDrive[] = [
    { name: "Macintosh HD", masterDb: defaultMasterDb, current: true },
    ...(readFlagFromUrl("drivelibrary")
      ? [{ name: "DJ SSD", masterDb: "/Volumes/DJ SSD/PIONEER/Master/master.db", current: false }]
      : []),
  ];
  let problem: LibraryProblem | null = readFlagFromUrl("libraryunavailable")
    ? { kind: "unavailable", masterDb: "/Volumes/DJ SSD/PIONEER/Master/master.db", defaultMasterDb }
    : readFlagFromUrl("nolibrary")
      ? { kind: "missing", masterDb: defaultMasterDb }
      : null;
  let ready =
    problem === null && (typeof location === "undefined" || !new URLSearchParams(location.search).has("slow"));
  const readyListeners = new Set<() => void>();
  const problemListeners = new Set<(problem: LibraryProblem) => void>();
  /** The library is there now: the window loads it like any other start. */
  const libraryFound = () => {
    problem = null;
    ready = true;
    for (const listener of readyListeners) listener();
  };
  if (typeof window !== "undefined") {
    (window as unknown as { __libraryReady: () => void }).__libraryReady = () => {
      ready = true;
      for (const listener of readyListeners) listener();
    };
  }

  /** What the backend says before the library is up. */
  const notReady = () =>
    Promise.reject(new Error("The library has not finished loading yet."));

  // A browser cannot see a real volume, so the mock carries one. It is
  // mutable: exporting to it changes what a later `listDevices` reports, which
  // is what makes the difference between a first export and a sync visible.
  const devices: Device[] = [
    {
      name: "DJ STICK",
      path: "/Volumes/DJ STICK",
      totalBytes: 32 * 1024 ** 3,
      freeBytes: 24 * 1024 ** 3,
      removable: true,
      volumeId: "dev:1",
      export: null,
    },
    // A stick rekordbox wrote, as the device tabs' captures show one: the
    // real TEST stick's sizes and contents, so the panel can be checked
    // against them.
    {
      name: "TEST",
      path: "/Volumes/TEST",
      totalBytes: 1_535_800_000_000,
      freeBytes: 216_800_000_000,
      removable: true,
      volumeId: "dev:2",
      export: { tracks: 77, playlists: 3, ours: false, written: "" },
    },
  ];

  // What each stick's own libraries hold, for the Devices tree.
  const stickLibraries = createMockDeviceLibraries();

  // What each stick's tabs hold. DJ STICK starts empty and gains a library
  // when something is exported to it; TEST carries the rows read off the
  // real stick.
  const deviceSettings = new Map<string, DeviceSettings>([
    ["/Volumes/TEST", referenceDeviceSettings("TEST", true)],
  ]);
  // Handed out by copy, as IPC would: a caller mutating its copy must not
  // reach into the "stick". A few dozen small rows, nothing like a page.
  const copySettings = (s: DeviceSettings): DeviceSettings => ({
    ...s,
    categories: s.categories.map((slot) => ({ ...slot })),
    sorts: s.sorts.map((slot) => ({ ...slot })),
    colors: s.colors.map((c) => ({ ...c })),
  });
  const settingsOf = (path: string): DeviceSettings => {
    const known = deviceSettings.get(path);
    if (known) return known;
    const device = devices.find((d) => d.path === path);
    const fresh = referenceDeviceSettings(device?.name ?? "", device?.export !== null);
    deviceSettings.set(path, fresh);
    return fresh;
  };

  // What the Sync Manager reads off a stick: the playlists our last export
  // was asked for, and the playlist names in its export. TEST is
  // rekordbox's, so it holds playlists but remembers no selection of ours;
  // DJ STICK holds nothing until something is written to it.
  const syncSelections = new Map<string, SyncPlaylist[]>();
  const formattedAs = new Map<string, string>();
  const looseTracks = new Map<string, Set<string>>();
  /** Sticks whose record asks to be synced again when plugged in. */
  const autoSync = new Set<string>();
  const deviceLibraries = new Map<string, string[]>([
    ["/Volumes/TEST", ["Main Set", "Warm Up", "Closing"]],
  ]);
  const syncProgressListeners = new Set<(progress: SyncProgress) => void>();
  const exportListeners = new Set<(progress: ExportProgress) => void>();
  const exportJobs = new Map<string, ExportProgress>();
  const cancelledExports = new Set<string>();
  const tellExport = (path: string, state: ExportProgress["state"], done = 0, total = 100) => {
    const progress: ExportProgress = { path, state, done, total, title: "" };
    exportJobs.set(path, progress);
    for (const listener of exportListeners) listener(progress);
  };
  const mockExport = async (path: string, write: () => ExportReport) => {
    cancelledExports.delete(path);
    tellExport(path, "copying");
    await wait(undefined);
    if (cancelledExports.has(path)) {
      tellExport(path, "cancelled");
      throw new Error("Export stopped.");
    }
    tellExport(path, "copying", 50);
    try {
      const report = write();
      await wait(undefined);
      tellExport(path, "done", 100);
      return report;
    } catch (error) {
      tellExport(path, "failed");
      throw error;
    }
  };

  /**
   * "Writes" playlists to a stick: no filesystem in a browser, so this is
   * the bookkeeping — what a later `listDevices`, `deviceSettings` and
   * `deviceSyncState` report — and the counts an export reports.
   */
  const writeTo = (device: Device, playlistIds: string[], defaults: StickDefaults | undefined, deleteUnlistedMusic = false): ExportReport => {
    // The union of the playlists, each track counted once however many
    // hold it, as the real selection is built.
    const union = new Set<string>();
    for (const id of playlistIds) for (const track of membersOf(id)) union.add(track);
    if (deleteUnlistedMusic) looseTracks.delete(device.path);
    else for (const track of looseTracks.get(device.path) ?? []) union.add(track);
    const tracks = union.size;
    const already = device.export;
    const reused = already?.ours === true ? Math.min(already.tracks, tracks) : 0;
    const removed = already?.ours === true ? Math.max(0, already.tracks - tracks) : 0;
    const tracksAdded = tracks - reused;
    const playlistsAdded = Math.max(0, playlistIds.length - (already?.ours === true ? already.playlists : 0));
    const playlistsRemoved = Math.max(0, (already?.ours === true ? already.playlists : 0) - playlistIds.length);
    device.export = {
      tracks,
      playlists: playlistIds.length,
      ours: true,
      written: "2026-09-08 00:30:00.000 +00:00",
    };
    // The export writes a library, which is what the tabs need. A stick
    // that had none takes the Preferences window's defaults, as the
    // real export does; one that had its own keeps them.
    const settings = settingsOf(device.path);
    const fresh = !settings.hasLibrarySettings && defaults !== undefined;
    deviceSettings.set(device.path, {
      ...settings,
      ...(fresh
        ? {
            hasDevSetting: true,
            waveformColor: defaults.waveformColor,
            waveformPosition: defaults.waveformPosition,
            overviewWaveform: defaults.overviewWaveform,
            keyDisplay: defaults.keyDisplay,
            categories: defaults.categories ?? settings.categories,
            sorts: defaults.sorts ?? settings.sorts,
            subColumn: defaults.subColumn,
          }
        : {}),
      hasDeviceLibrary: true,
      hasOneLibrary: true,
      hasLibrarySettings: true,
      deviceName: settings.deviceName || "RBXPORT",
    });
    const names = playlistIds.map((id) => findNode(id)?.name ?? id);
    syncSelections.set(
      device.path,
      playlistIds.map((id, i) => ({ libraryId: id, name: names[i] ?? id })),
    );
    deviceLibraries.set(device.path, names);
    return {
      tracks,
      playlists: playlistIds.length,
      bytesCopied: (tracks - reused) * 8_000_000,
      analysisFiles: tracks - reused,
      reused,
      removed,
      tracksAdded,
      tracksUpdated: 0,
      playlistsAdded,
      playlistsRemoved,
      skipped: [], failed: [],
      verified: true,
    };
  };

  const edits: Edits = {
    createPlaylist: (name, parent, index) => {
      const depth = parent === TREE_ROOT ? 1 : (findNode(parent)?.depth ?? 0) + 1;
      const id = `made-${nextId++}`;
      membership.set(id, []);
      insertUnder(parent, { id, name, kind: "playlist", depth, childCount: 0 }, index);
      return bump();
    },
    createFolder: (name, parent) => {
      const depth = parent === TREE_ROOT ? 1 : (findNode(parent)?.depth ?? 0) + 1;
      insertUnder(parent, { id: `made-${nextId++}`, name, kind: "folder", depth, expanded: true });
      return bump();
    },
    // The mock keeps the rule and answers it back; it does not evaluate
    // one, so a made intelligent playlist opens empty here.
    createSmartPlaylist: (name, parent, rule) => {
      const depth = parent === TREE_ROOT ? 1 : (findNode(parent)?.depth ?? 0) + 1;
      const id = `made-${nextId++}`;
      smartRules.set(id, rule);
      membership.set(id, []);
      insertUnder(parent, { id, name, kind: "smartPlaylist", depth, childCount: 0 });
      return bump();
    },
    setSmartRule: (playlist, rule) => {
      smartRules.set(playlist, rule);
      return bump();
    },
    renamePlaylist: (id, name) => {
      const node = findNode(id);
      const before = node?.name ?? "";
      if (node) node.name = name;
      return recordEdit({
        label: "Rename Playlist",
        undo: () => { const target = findNode(id); if (target) target.name = before; },
        redo: () => { const target = findNode(id); if (target) target.name = name; },
      });
    },
    movePlaylist: (id, parent, index) => {
      const before = treeSnapshot();
      const from = tree.findIndex((n) => n.id === id);
      if (from < 0) return recordEdit({ label: "Move Playlist", undo: () => restoreTree(before), redo: () => restoreTree(before) });
      // A folder cannot be put inside itself: the subtree would be detached
      // from the tree and never seen again. The backend refuses it, so does this.
      const span = subtreeLength(from);
      const moving = tree.slice(from, from + span);
      if (moving.some((n) => n.id === parent)) {
        return Promise.reject(new Error("that would put a folder inside itself"));
      }

      // Where it is going, decided before the tree is disturbed.
      const siblings = childrenOf(parent).filter((n) => n.id !== id);
      const at = Math.min(index ?? siblings.length, siblings.length);
      const after = siblings[at];

      tree.splice(from, span);
      // Its new level, carried down through everything under it.
      const depth = parent === TREE_ROOT ? 1 : (findNode(parent)?.depth ?? 0) + 1;
      const shift = depth - (moving[0]?.depth ?? depth);
      for (const node of moving) node.depth += shift;

      let to: number;
      if (after) {
        to = tree.findIndex((n) => n.id === after.id);
      } else if (parent === TREE_ROOT) {
        const histories = tree.findIndex(
          (n) => n.depth === 0 && n.id !== "all" && n.id !== "playlists",
        );
        to = histories < 0 ? tree.length : histories;
      } else {
        const start = tree.findIndex((n) => n.id === parent);
        to = start < 0 ? tree.length : start + subtreeLength(start);
      }
      tree.splice(to < 0 ? tree.length : to, 0, ...moving);
      const afterTree = treeSnapshot();
      return recordEdit({
        label: "Move Playlist",
        undo: () => restoreTree(before),
        redo: () => restoreTree(afterTree),
      });
    },
    deletePlaylist: async (id) => {
      const beforeTree = treeSnapshot();
      const beforeMembership = membershipSnapshot();
      const at = tree.findIndex((n) => n.id === id);
      if (at < 0) throw new Error(`no playlist or folder ${id}`);
      const nodes = tree.splice(at, subtreeLength(at));
      for (const node of nodes) {
        membership.delete(node.id);
      }
      const afterTree = treeSnapshot();
      const afterMembership = membershipSnapshot();
      return recordEdit({
        label: "Delete Playlist",
        undo: () => { restoreTree(beforeTree); restoreMembership(beforeMembership); },
        redo: () => { restoreTree(afterTree); restoreMembership(afterMembership); },
      });
    },
    undoEdit: async () => {
      const edit = editUndo.pop();
      if (!edit) throw new Error("There is no library edit to undo.");
      edit.undo();
      editRedo.push(edit);
      await bump(false);
      return announceHistory();
    },
    redoEdit: async () => {
      const edit = editRedo.pop();
      if (!edit) throw new Error("There is no library edit to redo.");
      edit.redo();
      editUndo.push(edit);
      await bump(false);
      return announceHistory();
    },
    addTracksToPlaylist: (playlist, tracks, allowDuplicates = false) => {
      // The real backend refuses while Rekordbox holds the database; the mock
      // never does, so the happy path is what `pnpm dev:mock` exercises.
      const current = membersOf(playlist);
      for (const track of tracks) if (allowDuplicates || !current.includes(track)) current.push(track);
      membership.set(playlist, current);
      return bump();
    },
    playlistDuplicates: (playlist, tracks) => {
      const current = new Set(membersOf(playlist));
      return Promise.resolve(tracks.filter((track) => current.has(track)));
    },
    // The mock's rows have no files to read tags from.
    reloadTags: () => bump(),
    addToTagList: (tracks) => {
      for (const track of tracks) if (!tagList.includes(track)) tagList.push(track);
      return tagListChanged();
    },
    removeFromTagList: (tracks) => {
      for (const track of tracks) {
        const at = tagList.indexOf(track);
        if (at >= 0) tagList.splice(at, 1);
      }
      return tagListChanged();
    },
    clearTagList: () => {
      tagList.length = 0;
      return tagListChanged();
    },
    removeTracksFromPlaylist: (playlist, tracks) => {
      const before = [...membersOf(playlist)];
      const current = membersOf(playlist).filter((t) => !tracks.includes(t));
      membership.set(playlist, current);
      return recordEdit({
        label: "Remove Tracks from Playlist",
        undo: () => membership.set(playlist, [...before]),
        redo: () => membership.set(playlist, [...current]),
      });
    },
    resetPlayCount: (tracks) => {
      const before = tracks.map((id) => {
        const row = all.find((candidate) => candidate.id === id);
        return [id, details.get(id)?.playCount ?? (row ? detailsOf(row).playCount : 0)] as const;
      });
      const apply = (values: readonly (readonly [string, number])[]) => {
        for (const [id, count] of values) {
          const row = all.find((candidate) => candidate.id === id);
          const detail = details.get(id) ?? (row ? detailsOf(row) : undefined);
          if (detail) detail.playCount = count;
        }
      };
      const after = before.map(([id]) => [id, 0] as const);
      apply(after);
      return recordEdit({ label: "Track Edit", undo: () => apply(before), redo: () => apply(after) });
    },
    // The mock keeps no history sessions of its own to add to or take from.
    recordPlay: () => wait(generation),
    removeFromHistory: (history, tracks) => {
      membership.set(history, membersOf(history).filter((t) => !tracks.includes(t)));
      return bump();
    },
    // The mock's rows are addressed by index, so a removal only takes the
    // tracks out of every playlist; the collection keeps its count.
    removeFromCollection: async (tracks) => {
      for (const [playlist, members] of membership) {
        membership.set(playlist, members.filter((t) => !tracks.includes(t)));
      }
      const next = await bump(false);
      editUndo.length = 0;
      editRedo.length = 0;
      announceHistory();
      return next;
    },
    reorderPlaylist: (playlist, tracks) => {
      const current = membersOf(playlist);
      // Mirrors the backend: tracks not named keep their place after the rest,
      // so a partial order cannot silently drop any.
      const named = tracks.filter((t) => current.includes(t));
      membership.set(playlist, [...named, ...current.filter((t) => !named.includes(t))]);
      return bump();
    },
    setTrackRating: (tracks, stars) => {
      const after = Math.max(0, Math.min(5, stars));
      return recordTrackEdits(tracks, (track) => {
        const row = all.find((r) => r.id === track);
        const before = row?.rating ?? 0;
        const apply = (value: number) => {
          if (row) row.rating = value;
          const detail = details.get(track);
          if (detail) detail.rating = value;
        };
        apply(after);
        return { undo: () => apply(before), redo: () => apply(after) };
      });
    },
    setTrackComment: (tracks, comment) =>
      recordTrackEdits(tracks, (track) => {
        const row = all.find((r) => r.id === track);
        const before = row?.comment ?? "";
        const apply = (value: string) => {
          if (row) row.comment = value;
          const detail = details.get(track);
          if (detail) detail.comment = value;
        };
        apply(comment);
        return { undo: () => apply(before), redo: () => apply(comment) };
      }),
    setTrackColor: (tracks, color) =>
      recordTrackEdits(tracks, (track) => {
        const at = all.findIndex((r) => r.id === track);
        const row = all[at];
        const before = details.get(track)?.color ?? String(colors[at] ?? 0);
        const apply = (value: string | null) => {
          const numeric = value === null ? 0 : Number.parseInt(value, 10);
          if (row) row.artworkHue = numeric * 40;
          if (at >= 0) colors[at] = numeric;
          const detail = details.get(track);
          if (detail) detail.color = value ?? "0";
        };
        apply(color);
        return { undo: () => apply(before), redo: () => apply(color) };
      }),
    setMyTags: (track, tags) => {
      const row = all.find((r) => r.id === track);
      const detail = row ? detailsOf(row) : undefined;
      const before = [...(detail?.myTags ?? [])];
      const apply = (value: readonly string[]) => { if (detail) detail.myTags = [...value]; };
      apply(tags);
      return recordEdit({ label: "Track Edit", undo: () => apply(before), redo: () => apply(tags) });
    },
    addPlaylistArtwork: () => bump(),
    addArtwork: (tracks) => recordTrackEdits(tracks, (track) => setHasArtwork(track, true)),
    clearArtwork: (tracks) => recordTrackEdits(tracks, (track) => setHasArtwork(track, false)),
    setTrackField: (tracks, field, value) => {
      // The same refusals the writer makes: a number that is not one, a key
      // the library does not hold, and a title or BPM for several tracks.
      if (tracks.length > 1 && (field === "title" || field === "bpm")) {
        return Promise.reject(new Error(`${field} cannot be edited here.`));
      }
      const numeric: Partial<Record<TrackField, "year" | "trackNumber" | "discNumber" | "playCount">> = {
        year: "year", trackNumber: "trackNumber", discNumber: "discNumber", playCount: "playCount",
      };
      const which = numeric[field];
      const n = /^\s*\d+\s*$/.test(value) ? Number.parseInt(value, 10) : NaN;
      if (which && !Number.isFinite(n)) {
        return Promise.reject(new Error(`${JSON.stringify(value)} is not a whole number`));
      }
      if (field === "key" && value !== "" && !KEYS.includes(value)) {
        return Promise.reject(new Error(`${JSON.stringify(value)} is not a key the library knows`));
      }
      if (field === "bpm") {
        const bpm = Number.parseFloat(value.trim());
        if (!Number.isFinite(bpm) || bpm < 20 || bpm > 400) {
          return Promise.reject(new Error(`${JSON.stringify(value)} is not a BPM between 20 and 400`));
        }
        const row = all.find((r) => r.id === tracks[0]);
        if (row) {
          row.bpmX100 = Math.round(bpm * 100);
          detailsOf(row).bpmX100 = row.bpmX100;
        }
        return bump().then(historyState);
      }
      return recordTrackEdits(tracks, (track) => {
        const row = all.find((r) => r.id === track);
        if (!row) return { undo: () => {}, redo: () => {} };
        const d = detailsOf(row);
        const beforeRow = { ...row };
        const beforeDetails = { ...d, myTags: [...d.myTags] };
        if (which) {
          d[which] = n;
        } else {
          // Narrowed by hand: what is left after the numeric fields is text.
          const text = field as Exclude<TrackField, "year" | "trackNumber" | "discNumber" | "playCount" | "bpm">;
          d[text] = value.trim();
          // The row carries some of the same columns; keep the two in step
          // the way a reload of the index would.
          if (field === "title") row.title = d.title;
          else if (field === "artist") row.artist = d.artist;
          else if (field === "album") row.album = d.album;
          else if (field === "genre") row.genre = d.genre;
          else if (field === "label") row.label = d.label;
          else if (field === "key") row.key = d.key;
        }
        const afterRow = { ...row };
        const afterDetails = { ...d, myTags: [...d.myTags] };
        const apply = (rowValue: RowDto, detailValue: TrackDetails) => {
          Object.assign(row, rowValue);
          Object.assign(d, detailValue, { myTags: [...detailValue.myTags] });
        };
        return { undo: () => apply(beforeRow, beforeDetails), redo: () => apply(afterRow, afterDetails) };
      });
    },
    addCue: (track, kind, positionMs) => {
      if (!all.some((r) => r.id === track)) return refuse(`no track ${track}`);
      if (kind !== "memory" && !/^[A-P]$/.test(kind.hot)) {
        return refuse(`${JSON.stringify(kind.hot)} is not a hot cue slot rekordbox has`);
      }
      const id = `cue-${nextCueId++}`;
      cuesOf(track).push({
        id, positionMs, outMs: 0, letter: kind === "memory" ? "" : kind.hot, memory: kind === "memory",
        colour: kind === "memory" ? null : DEFAULT_CUE_COLOUR,
      });
      return cuesChanged(track, id);
    },
    addLoop: (track, kind, inMs, outMs) => {
      if (!all.some((r) => r.id === track)) return refuse(`no track ${track}`);
      if (outMs <= inMs) return refuse("a loop has to end after it starts");
      const id = `cue-${nextCueId++}`;
      cuesOf(track).push({
        id, positionMs: inMs, outMs, letter: kind === "memory" ? "" : kind.hot, memory: kind === "memory",
        colour: kind === "memory" ? null : DEFAULT_CUE_COLOUR,
      });
      return cuesChanged(track, id);
    },
    setCueColour: (cueId, colour) => {
      for (const [trackId, list] of cueStore) {
        const cue = list.find((item) => item.id === cueId);
        if (!cue) continue;
        const palette = ["#000000", "#305AFF", "#5073FF", "#508CFF", "#50A0FF", "#50B4FF", "#50B0F2", "#50AEE8", "#45ACDB", "#00E0FF", "#19DAF0", "#32D2E6", "#21B4B9", "#20AAA0", "#1FA392", "#19A08C", "#14A584", "#14AA7D", "#10B176", "#30D26E", "#37DE5A", "#3CEB50", "#28E214", "#7DC13D", "#8CC832", "#9BD723", "#A5E116", "#A5DC0A", "#AAD208", "#B4C805", "#B4BE04", "#BAB404", "#C3AF04", "#E1AA00", "#FFA000", "#FF9600", "#FF8C00", "#FF7500", "#E0641B", "#E0461E", "#E0301E", "#E02823", "#E62828", "#FF376F", "#FF2D6F", "#FF127B", "#F51E8C", "#EB2DA0", "#E637B4", "#DE44CF", "#DE448D", "#E630B4", "#E619DC", "#E600FF", "#DC00FF", "#CC00FF", "#B432FF", "#B93CFF", "#C542FF", "#AA5AFF", "#AA72FF", "#8272FF", "#6473FF", "#000000", "#FFFFFF"];
        const memoryPalette = ["#E778F1", "#E33122", "#EBA44A", "#F4E458", "#66DD42", "#56BDF3", "#204FEF", "#8B1EEF"];
        cue.colour = colour === null ? (cue.memory ? null : DEFAULT_CUE_COLOUR) : (cue.memory ? memoryPalette : palette)[colour] ?? null;
        return cuesChanged(trackId, undefined);
      }
      return refuse(`no cue ${cueId}`);
    },
    setCueComment: (cueId, comment) => {
      for (const [trackId, list] of cueStore) {
        const cue = list.find((item) => item.id === cueId);
        if (!cue) continue;
        cue.comment = comment;
        return cuesChanged(trackId, undefined);
      }
      return refuse(`no cue ${cueId}`);
    },
    convertMemoryCuesToHot: (track) => {
      const cues = cuesOf(track);
      const taken = new Set(cues.map((c) => c.letter));
      const free = [..."ABCDEFGHIJKLMNOP"].filter((letter) => !taken.has(letter));
      const memory = cues.filter((c) => c.memory).sort((a, b) => a.positionMs - b.positionMs);
      let made = 0;
      for (const cue of memory) {
        const letter = free.shift();
        if (letter === undefined) break;
        cues.push({
          id: `cue-${nextCueId++}`, positionMs: cue.positionMs, outMs: cue.outMs, letter, memory: false,
          colour: DEFAULT_CUE_COLOUR,
        });
        made += 1;
      }
      return cuesChanged(track, made);
    },
    moveCue: (cue, positionMs) => {
      const found = findCue(cue);
      if (!found) return notFound(`no cue ${cue}`);
      found.cue.positionMs = positionMs;
      return cuesChanged(found.track, undefined);
    },
    moveHotCue: (cue, letter) => {
      const found = findCue(cue);
      if (!found) return notFound(`no cue ${cue}`);
      if (found.cue.memory) return refuse("A memory cue has no hot cue slot.");
      // The cues between the two slots slide one toward the slot this one
      // leaves, as dragging a row in a list does.
      const from = found.cue.letter.charCodeAt(0);
      const to = letter.charCodeAt(0);
      for (const other of cuesOf(found.track)) {
        if (other.memory || other === found.cue) continue;
        const at = other.letter.charCodeAt(0);
        if (from < to && at > from && at <= to) other.letter = String.fromCharCode(at - 1);
        else if (from > to && at >= to && at < from) other.letter = String.fromCharCode(at + 1);
      }
      found.cue.letter = letter;
      return cuesChanged(found.track, undefined);
    },
    deleteCue: (cue) => {
      const found = findCue(cue);
      if (!found) return notFound(`no cue ${cue}`);
      const list = cuesOf(found.track);
      list.splice(list.indexOf(found.cue), 1);
      return cuesChanged(found.track, undefined);
    },
    gridEdit: (track, edit, options) => {
      const held = gridOf(track);
      if (!held) return notFound("That track has no beat grid to edit.");
      if (held.locked) return refuse("The beat grid is locked. Unlock it to edit.");
      const invalid = validateEdit(held.beats, options?.fromMs ?? null, edit);
      if (invalid) return refuse(invalid);
      if (["stretch", "tempo"].includes(edit.kind) && !options?.allowDynamic && isDynamicFrom(held.beats, options?.fromMs ?? null)) return refuse("Confirm replacing this section's tempo changes.");
      const duration = options?.durationMs ?? (all[Number.parseInt(track, 10) - 100000]?.durationSec ?? 0) * 1000;
      const next = applyEditFrom(held.beats, options?.fromMs ?? null, edit, duration);
      if (next.length === 0) return failed("malformed", "That edit would leave the track without a beat.");
      if (sameGrid(next, held.beats)) return wait(gridStateOf(held));
      const label = edit.kind === "nudge"
        ? (edit.ms < 0 ? "Shift Beat Grid Left" : "Shift Beat Grid Right")
        : { double: "Double Tempo", halve: "Halve Tempo", downbeat: "Set Downbeat",
          tempo: "Set Tempo", tap: "Tap Tempo", stretch: "Adjust Tempo", align: "Align Beat Grid" }[edit.kind];
      if (!options?.transaction || held.transaction !== options.transaction) held.undo.push({ beats: held.beats, label });
      held.transaction = edit.kind === "tap" ? options?.transaction : undefined;
      held.redo = [];
      held.beats = next;
      return gridChanged(track, held);
    },
    gridUndo: (track) => {
      const held = gridOf(track);
      if (!held) return notFound("That track has no beat grid to edit.");
      held.transaction = undefined;
      const previous = held.undo.pop();
      if (!previous) return notFound("Nothing to undo.");
      held.redo.push({ beats: held.beats, label: previous.label });
      held.beats = previous.beats;
      return gridChanged(track, held);
    },
    gridRedo: (track) => {
      const held = gridOf(track);
      if (!held) return notFound("That track has no beat grid to edit.");
      held.transaction = undefined;
      const next = held.redo.pop();
      if (!next) return notFound("Nothing to redo.");
      held.undo.push({ beats: held.beats, label: next.label });
      held.beats = next.beats;
      return gridChanged(track, held);
    },
    gridLock: (track, on) => {
      const held = gridOf(track);
      if (!held) return notFound("That track has no beat grid to edit.");
      held.locked = on;
      return gridChanged(track, held);
    },
  };

  /*
   * Grids, per track: a steady grid at the row's own BPM, made on first ask
   * and held from then on so an edit sticks, with the session's undo and
   * redo stacks and the lock beside it — what the real backend keeps in
   * `GridEditor`. The listeners hear which track changed; a tempo change
   * also bumps the generation, because the row's BPM column changed.
   */
  interface HeldGrid {
    beats: EditableBeat[];
    transaction?: string | undefined;
    undo: { beats: EditableBeat[]; label: string }[];
    redo: { beats: EditableBeat[]; label: string }[];
    locked: boolean;
  }
  const gridStore = new Map<string, HeldGrid>();
  const gridListeners = new Set<(trackId: string) => void>();
  const analysisListeners = new Set<(trackId: string) => void>();
  const gridOf = (trackId: string): HeldGrid | null => {
    const held = gridStore.get(trackId);
    if (held) return held;
    const row = all[Number.parseInt(trackId, 10) - 100000];
    if (!row || row.analysed === 0 || row.bpmX100 === 0) return null;
    const beatMs = (60 / (row.bpmX100 / 100)) * 1000;
    const count = Math.min(Math.floor((row.durationSec * 1000) / beatMs) + 1, 65536);
    const beats: EditableBeat[] = [];
    for (let n = 0; n < count; n++) {
      beats.push({ number: (n % 4) + 1, tempoX100: row.bpmX100, timeMs: Math.round(n * beatMs) });
    }
    const made = { beats, undo: [], redo: [], locked: false };
    gridStore.set(trackId, made);
    return made;
  };
  const sameGrid = (a: readonly EditableBeat[], b: readonly EditableBeat[]) =>
    a.length === b.length
    && a.every((beat, i) => beat.timeMs === b[i]?.timeMs && beat.number === b[i]?.number && beat.tempoX100 === b[i]?.tempoX100);
  const gridStateOf = (held: HeldGrid): GridState => ({
    bpmX100: tempoX100(held.beats),
    beats: held.beats.length,
    canUndo: held.undo.length > 0,
    canRedo: held.redo.length > 0,
    undoLabel: held.undo.at(-1)?.label ?? null,
    redoLabel: held.redo.at(-1)?.label ?? null,
    locked: held.locked,
  });
  const gridChanged = async (trackId: string, held: HeldGrid): Promise<GridState> => {
    const row = all[Number.parseInt(trackId, 10) - 100000];
    const bpm = tempoX100(held.beats);
    for (const listener of gridListeners) listener(trackId);
    if (row && row.bpmX100 !== bpm) {
      row.bpmX100 = bpm;
      const d = details.get(trackId);
      if (d) d.bpmX100 = bpm;
      await bump();
    }
    return wait(gridStateOf(held));
  };

  /**
   * The rest of a track's record, invented once per track from its row.
   *
   * Deterministic so a test can name what it expects: the size follows the
   * duration at a constant bitrate, the sample rate is the common one, and
   * the path is where the mock says its files live.
   */
  const detailsOf = (row: RowDto): TrackDetails => {
    let d = details.get(row.id);
    if (d) return d;
    const seed = Number.parseInt(row.id, 10);
    const wav = seed % 7 === 0;
    const bitrate = wav ? 1411 : 320;
    d = {
      id: row.id,
      title: row.title,
      artist: row.artist,
      album: row.album,
      albumArtist: row.album ? row.artist : "",
      originalArtist: "",
      composer: seed % 5 === 0 ? row.artist : "",
      remixer: row.title.includes("Remix") ? "Someone" : "",
      lyricist: "",
      genre: row.genre,
      label: row.label,
      key: row.key,
      comment: row.comment,
      mixName: "",
      message: "",
      // The colour the filter bar matches this row by, so the deck's INFO
      // tab and the bar agree.
      color: String(colors[rowPositions.get(row.id) ?? -1] ?? 0),
      rating: row.rating,
      bpmX100: row.bpmX100,
      durationSec: row.durationSec,
      year: row.releaseDate ? Number.parseInt(row.releaseDate.slice(0, 4), 10) : 0,
      trackNumber: seed % 12,
      discNumber: 0,
      playCount: seed % 9,
      fileType: wav ? 11 : 1,
      fileSize: Math.round((row.durationSec * bitrate * 1000) / 8),
      bitrate,
      sampleRate: 44_100,
      bitDepth: wav ? 16 : 0,
      dateCreated: row.dateAdded.slice(0, 10),
      releaseDate: row.releaseDate,
      path: `/Volumes/MUSIC/${row.artist || "Unknown Artist"}/${row.title}.${wav ? "wav" : "mp3"}`,
      hotCueAutoLoad: true,
      publish: false,
      hasArtwork: false,
      myTags: seed % 4 === 0 ? ["t-peak"] : [],
    };
    details.set(row.id, d);
    return d;
  };

  const matchesSearch = (row: RowDto, query: string, field: TrackSearchField): boolean => {
    const detail = detailsOf(row);
    const value = (key: Exclude<TrackSearchField, "all">): string => {
      if (key === "bpm") return row.bpmX100 ? (row.bpmX100 / 100).toFixed(2) : "";
      if (key === "year") return detail.year ? String(detail.year) : "";
      const raw = key in row ? (row as unknown as Record<string, unknown>)[key] : detail[key];
      return typeof raw === "string" ? raw : "";
    };
    const hay = fold(field === "all" ? TRACK_SEARCH_OPTIONS.filter(option => option.value !== "all")
      .map(option => value(option.value)).join(" ") : value(field));
    return query.split(/\s+/).every(token => hay.includes(token));
  };

  /*
   * Cues, per track, made up on first ask the way `trackCues` always did and
   * held from then on so an edit sticks. The same shape the real backend
   * keeps: the listeners hear which track changed, not a generation.
   */
  const cueStore = new Map<string, Cue[]>();
  let nextCueId = 1;
  const cueListeners = new Set<(trackId: string) => void>();
  const cuesOf = (trackId: string): Cue[] => {
    const held = cueStore.get(trackId);
    if (held) return held;
    const index = Number.parseInt(trackId, 10) - 100000;
    const row = all[index];
    // A memory cue at 2% and the row's own hot cues from 12% on, so the
    // detail's opening window holds exactly the one marker and the badges on
    // the row are the cues the deck shows.
    const made: Cue[] = [];
    if (row && row.analysed !== 0) {
      const total = row.durationSec * 1000;
      made.push(
        { id: `cue-${nextCueId++}`, positionMs: Math.round(total * 0.02), outMs: 0, letter: "", memory: true, colour: null, comment: "136 BPM" },
        ...row.hotCues.map(([letter, positionMs, colour]) => (
          { id: `cue-${nextCueId++}`, positionMs, outMs: 0, letter, memory: false, colour }
        )),
      );
    }
    cueStore.set(trackId, made);
    return made;
  };
  /** The row's badges follow its cues, in slot order as the backend sends them. */
  const syncRow = (trackId: string) => {
    const row = all[Number.parseInt(trackId, 10) - 100000];
    if (!row) return;
    row.memoryCues = cuesOf(trackId).filter(cue => cue.memory).map(cue => cue.positionMs);
    row.hotCues = cuesOf(trackId)
      .filter((cue) => !cue.memory)
      .sort((a, b) => a.letter.localeCompare(b.letter))
      .map((cue) => [cue.letter, cue.positionMs, cue.colour]);
  };
  const findCue = (id: string): { track: string; cue: Cue } | null => {
    for (const [track, list] of cueStore) {
      const cue = list.find((c) => c.id === id);
      if (cue) return { track, cue };
    }
    return null;
  };
  const cuesChanged = <T>(track: string, value: T): Promise<T> => {
    syncRow(track);
    for (const listener of cueListeners) listener(track);
    return wait(value);
  };
  // The shape a refused command arrives in: an `Error` whose `kind` is the
  // `AppErrorDto` kind, so a caller can tell a read-only refusal from a bug.
  const failed = (kind: AppErrorDto["kind"], message: string) =>
    Promise.reject(Object.assign(new Error(message), { kind }));
  const refuse = (message: string) => failed("readOnly", message);
  const notFound = (message: string) => failed("notFound", message);

  /*
   * The mock deck.
   *
   * `startClock` is a chain of timeouts rather than an interval: an interval
   * that outlives the page keeps firing, and the engine's own ticker likewise
   * stops the moment nothing is playing.
   */
  const SAMPLE_RATE = 44_100;
  const TICK_MS = 100;
  const deckA = {
    frames: 0, totalFrames: 0, generation: 0, playing: false, loaded: false, loadId: 0,
    tempo: 1, masterTempo: false, keyShift: 0, startInFrames: 0,
    loopInFrames: 0, loopOutFrames: 0, looping: false, startsAt: 0,
  };
  // Deck B holds its own tempo and key lock even though a browser has no
  // audio to apply them to: a control that snapped back on the next tick would
  // read as a broken one, and the deck it stands for does keep them.
  const deckB = {
    frames: 0, totalFrames: 0, generation: 0, playing: false, loaded: false, loadId: 0,
    tempo: 1, masterTempo: false, keyShift: 0, startInFrames: 0,
    loopInFrames: 0, loopOutFrames: 0, looping: false, startsAt: 0,
  };
  /** The deck that a command names. */
  const deckOf = (deck: DeckId) => (deck === "a" ? deckA : deckB);
  /** The beat of the track on each deck, in seconds, or 0 for none. */
  const deckBeat = { a: 0, b: 0 };
  // Where each deck is, for an end-to-end test that compares the two.
  if (typeof window !== "undefined") {
    (window as unknown as {
      __deckSeconds: () => {
        a: number; b: number; beat: number; beatA: number; looping: boolean; loopingA: boolean; playingB: boolean;
      };
    }).__deckSeconds = () => ({
      a: deckA.frames / SAMPLE_RATE,
      b: deckB.frames / SAMPLE_RATE,
      beat: deckBeat.b,
      beatA: deckBeat.a,
      looping: deckB.looping,
      loopingA: deckA.looping,
      playingB: deckB.playing,
    });
  }
  const deckTickListeners = new Set<(tick: Tick) => void>();
  const deckEventListeners = new Set<(event: DeckEvent) => void>();
  let clock: ReturnType<typeof setTimeout> | null = null;
  let clockAt = 0;

  /*
   * The preview player: its own clock, kept as a start time and an offset
   * rather than a ticking timer, because nothing listens to it — the
   * interface asks where it is.
   */
  const previewed = { track: null as string | null, playing: false, positionMs: 0, durationMs: 0, since: 0 };
  const previewNow = (): number => {
    if (!previewed.playing) return previewed.positionMs;
    const at = previewed.positionMs + (performance.now() - previewed.since);
    if (at < previewed.durationMs) return at;
    // Played to the end: it stops there, as the engine's deck does.
    previewed.playing = false;
    previewed.positionMs = previewed.durationMs;
    return previewed.positionMs;
  };
  /** Stops the preview where it is: `previewStop`, and what a deck's load or Play does. */
  const stopPreviewed = (): void => {
    previewed.positionMs = previewNow();
    previewed.playing = false;
  };

  /** The master level, which a browser can hold even with nothing to apply it to. */
  // −1 dB, the knob at 10: what the engine starts at.
  let master = 0.891_250_9;

  /** The master limiter, likewise. */
  let limiter: Limiter = { enabled: false, inputGainDb: -4, ceilingDb: 0, releaseMs: 250 };

  const preferencesRequestListeners = new Set<(what: PreferencesRequest) => void>();

  /** Who is told how the pretend download is going. */
  const updateProgressListeners = new Set<(progress: UpdateProgress) => void>();
  let updateReady: UpdateReady | null = null;

  const tick = (): Tick => ({
    a: { ...deckA },
    b: { ...deckB },
    sampleRate: SAMPLE_RATE,
    // A browser has no audio callback, so there is nothing to meter. Zero is
    // the truth here rather than a placeholder: nothing is coming out.
    peakLeft: 0,
    peakRight: 0,
    master,
    reduction: 0,
    // The mock stands in for the build that ships, which carries Rubber Band.
    shiftsKey: true,
  });

  const sendTick = () => {
    const now = tick();
    for (const listener of deckTickListeners) listener(now);
  };

  const stopClock = () => {
    if (clock !== null) clearTimeout(clock);
    clock = null;
  };

  const startClock = () => {
    if (clock !== null) return;
    clockAt = performance.now();
    const step = () => {
      clock = null;
      const now = performance.now();
      for (const deck of [deckA, deckB]) {
        if (!deck.playing) continue;
        // A held start (see `deckPlayAfter`) or a move counts from then, not from the last step.
        const from = Math.max(clockAt, deck.startsAt);
        if (now <= from) continue;
        deck.frames = Math.min(deck.frames + Math.round(((now - from) / 1000) * SAMPLE_RATE), deck.totalFrames);
        // Inside a loop the head rounds at the out point, as the deck does.
        if (deck.looping && deck.loopOutFrames > deck.loopInFrames && deck.frames >= deck.loopOutFrames) {
          deck.frames = deck.loopInFrames + ((deck.frames - deck.loopOutFrames) % (deck.loopOutFrames - deck.loopInFrames));
        }
        if (deck.frames >= deck.totalFrames) deck.playing = false;
      }
      clockAt = now;
      sendTick();
      if (deckA.playing || deckB.playing) clock = setTimeout(step, TICK_MS);
    };
    clock = setTimeout(step, TICK_MS);
  };

  const importListeners = new Set<(progress: ExportProgress) => void>();
  const wait = <T>(value: T): Promise<T> =>
    latency > 0 ? new Promise((r) => setTimeout(() => r(value), latency)) : Promise.resolve(value);

  /** LINK in a browser: off, with nothing to run it on. */
  // The mock's tempo-master state, mutable so the master controls do
  // something in the browser.
  const mockMaster = { on: false, bpm: 120 };
  const linkOff = (): LinkStatus => ({
    on: false,
    problem: null,
    interface: null,
    players: [],
    interfaces: [
      { name: "en0", address: "192.168.1.14", adapter: "Wi-Fi", connection: "wireless" },
      { name: "en11", address: "192.168.2.14", adapter: "USB Ethernet", connection: "wired" },
    ],
    master: false,
    masterBpm: mockMaster.bpm,
    state: "off",
    number: null,
  });

  /** A network to look at, from `?link=`; null in a plain browser. */
  const linkMode = readLinkFromUrl();
  const mockPeers: LinkPeerSeen[] = [
    { number: 1, name: "CDJ-3000", kind: "player", address: "192.168.1.152" },
    { number: 2, name: "CDJ-3000", kind: "player", address: "192.168.1.153" },
    { number: 33, name: "DJM-V5", kind: "mixer", address: "192.168.1.155" },
  ];
  const mockLinkOn = (): LinkStatus => ({
    on: true,
    problem: null,
    interface: { name: "en0", address: "192.168.1.14" },
    interfaces: [{ name: "en0", address: "192.168.1.14" }],
    players: [
      {
        number: 1,
        name: "CDJ-3000",
        kind: "player",
        address: "192.168.1.152",
        loaded: { id: "1", title: "GIN AND TONIC (Extended Mix)", artist: "DONT BLINK" },
        playing: true,
        master: true,
        sync: false,
        cued: false,
        linkCue: false,
        mounted: true,
      },
      { number: 2, name: "CDJ-3000", kind: "player", address: "192.168.1.153", loaded: null, playing: false, master: false, sync: false, cued: false, linkCue: false, mounted: true },
      { number: 33, name: "DJM-V5", kind: "mixer", address: "192.168.1.155", loaded: null, playing: false, master: false, sync: false, cued: false, linkCue: false, mounted: false },
    ],
    master: mockMaster.on,
    masterBpm: mockMaster.bpm,
    state: "up",
    number: 17,
  });
  const mockLinkStatus = (): LinkStatus => {
    if (linkMode === "on") return mockLinkOn();
    if (linkMode === "blocked") {
      return { ...linkOff(), problem: "rekordbox is running and holds the link ports. Quit it to turn LINK on." };
    }
    return linkOff();
  };

  const backups = new Map<string, Backup>();
  let backupDirectory = "/mock/backups";
  let backupSizes: BackupSizes | null = null;
  let backupProgress: BackupProgress = { running: false, phase: "", copiedBytes: 0, totalBytes: 0, error: null, path: null };
  const saveBackup = () => {
    const createdAt = Date.now();
    const date = new Date(createdAt);
    const pad = (value: number) => String(value).padStart(2, "0");
    const name = `rbexport-${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}-${pad(date.getHours())}${pad(date.getMinutes())}.zip`;
    const path = `${backupDirectory}/${name}`;
    if (backups.has(path)) throw new Error("A backup for this minute already exists. Try again in the next minute.");
    backups.set(path, { path, name, createdAt, includesAnalysis: true, includesArtwork: true, bytes: new Blob([JSON.stringify(all)]).size });
    return path;
  };

  return {
    rekordboxBrowseSettings: () => Promise.resolve(null),
    librarySummary: () =>
      ready
        ? wait<LibrarySummary>({
            trackCount,
            playlistCount: tree.filter((n) => n.kind === "playlist").length,
            readOnly: !(options.writable ?? readFlagFromUrl("writable")),
            dbVersion: null,
          })
        : notReady(),
    disableReadOnly: () => Promise.reject(new Error("Set RBX_DISABLE_READ_ONLY before launching rbxport to enable this override.")),
    rememberScreenAssets: () => Promise.resolve(),

    // A copy, like the real backend: handing out the internal array lets a
    // caller mutate the backend's own state, and makes a list captured before
    // an edit appear to have changed by itself.
    playlistTree: () => (ready ? wait(tree.map((node) => ({ ...node }))) : notReady()),

    openView: (spec: ViewSpec) => {
      // Every real command goes through `state.library()?`, so none of them
      // answer before the library is up. A mock that served rows while the
      // summary was still failing would not be standing in for anything.
      if (!ready) return notReady();
      if (spec.source.kind === "device") {
        const q = fold(spec.query.trim());
        let rows: RowDto[];
        try {
          rows = stickLibraries.rows(spec.source.path, spec.source.format, spec.source.playlist)
            .filter((row) => q === "" || matchesSearch(row, q, spec.searchField ?? "all"));
        } catch (error) {
          return Promise.reject(error instanceof Error ? error : new Error(String(error)));
        }
        if (spec.sort !== "trackNo") {
          rows.sort((x, y) => {
            const c = compare(x, y, spec.sort);
            return spec.descending ? -c : c;
          });
        } else if (spec.descending) {
          rows.reverse();
        }
        const viewId = nextViewId++;
        folderViews.set(viewId, rows);
        return wait<ViewHandle>({ viewId, len: rows.length, gen: 1 });
      }
      if (spec.source.kind === "folder") {
        const folder = spec.source.path;
        const q = fold(spec.query.trim());
        const rows = (EXPLORER_FILES.get(folder) ?? [])
          .map((entry, at) =>
            typeof entry === "number"
              ? { ...(all[entry] ?? looseRow(folder, "", at + 1)), trackNo: at + 1 }
              : looseRow(folder, entry, at + 1),
          )
          .filter((row) => q === "" || matchesSearch(row, q, spec.searchField ?? "all"));
        if (spec.sort !== "trackNo") {
          rows.sort((x, y) => {
            const c = compare(x, y, spec.sort);
            return spec.descending ? -c : c;
          });
        } else if (spec.descending) {
          rows.reverse();
        }
        const viewId = nextViewId++;
        folderViews.set(viewId, rows);
        return wait<ViewHandle>({ viewId, len: rows.length, gen: 1 });
      }
      let candidates = candidatesFor(spec);
      const filter = spec.filter;
      if (filter) {
        candidates = candidates.filter((i) => {
          const row = all[i];
          return row ? passesFilter(row, colors[i] ?? 0, filter) : false;
        });
      }

      const order = candidates.map((row, position) => ({ row, trackNo: position + 1 }));
      const rows = all;
      // `trackNo` is not a column to rank by: it means "leave them in the
      // order this view produced them", which for a playlist is its
      // membership — the order somebody dragged them into. Ranking by the
      // row's own stored number put the collection's order back instead, so
      // a reordered playlist came out looking untouched.
      const sorted =
        spec.sort === "trackNo"
          ? (spec.descending ? order.reverse() : order)
          : order.sort((x, y) => {
              const rx = rows[x.row];
              const ry = rows[y.row];
              if (!rx || !ry) return 0;
              const c = compare(rx, ry, spec.sort);
              return spec.descending ? -c : c;
            });

      const viewId = nextViewId++;
      views.set(viewId, {
        order: Uint32Array.from(sorted.map(({ row }) => row)),
        ...(spec.source.kind === "playlist" ? { trackNos: Uint32Array.from(sorted.map(({ trackNo }) => trackNo)) } : {}),
        gen: 1,
      });
      return wait<ViewHandle>({ viewId, len: sorted.length, gen: 1 });
    },

    fetchRows: (viewId, offset, len, extraColumns = []) => {
      const project = (row: RowDto): RowDto => {
        const { extra, ...base } = row;
        if (extraColumns.length === 0) return base;
        return {
          ...base,
          extra: Object.fromEntries(extraColumns.flatMap((key) =>
            extra?.[key] === undefined ? [] : [[key, extra[key]]])),
        };
      };
      const folder = folderViews.get(viewId);
      if (folder) return wait(folder.slice(Math.max(0, offset), Math.max(0, offset) + len).map(project));
      const view = views.get(viewId);
      if (!view) return Promise.reject(new Error(`unknown view ${viewId}`));
      const out: RowDto[] = [];
      const end = Math.min(view.order.length, offset + len);
      for (let i = Math.max(0, offset); i < end; i++) {
        const row = all[view.order[i] ?? 0];
        // A playlist keeps its own stored track order when sorted or filtered;
        // every other view numbers the visible rows.
        if (row) out.push(project({ ...row, trackNo: view.trackNos?.[i] ?? i + 1 }));
      }
      return wait(out);
    },

    trackWaveform: (trackId: string, kind: WaveformKind, window) => {
      // Three bytes a column — low, mid, high band energy — which is what the
      // real backend serves from `PWV6` and `PWV7`. Synthesising the old
      // one-byte `PWAV` shape here drew the bands from garbage.
      const index = Number.parseInt(trackId, 10) - 100000;
      const row = all[index];
      if (!row || row.analysed === 0) return wait(new Uint8Array());
      const rnd = mulberry32(index + 1);
      // The overview tags are 1,200 columns for any track; the detail ones
      // are far denser.
      const detail = kind === "bandsDetail" || kind === "monoDetail" || kind === "colourDetail";
      const columns = detail ? 12_000 : 1200;
      // Each palette's tag in its own layout, from the same three bands, so
      // a palette switch shows the same track drawn another way.
      const stride = kind === "mono" || kind === "monoDetail" ? 1
        : kind === "colour" ? 6
          : kind === "colourDetail" ? 2
            : 3;
      const out = new Uint8Array(columns * stride);
      for (let i = 0; i < columns; i++) {
        const at = i / columns;
        // A shape with quiet intros and outros, so the overview reads as a
        // track rather than a block.
        const envelope = Math.min(1, Math.min(at, 1 - at) * 6) * (0.55 + 0.45 * Math.abs(Math.sin(at * Math.PI * 5)));
        const jitter = 0.75 + rnd() * 0.25;
        const low = Math.round(envelope * jitter * 110);
        const mid = Math.round(envelope * jitter * 70);
        // Highs are sparse, which is what puts the bright core in the middle.
        const high = Math.round(envelope * (rnd() > 0.7 ? rnd() * 45 : rnd() * 8));
        const height = Math.max(low, mid, high);
        const base = i * stride;
        if (stride === 3) {
          out[base] = low;
          out[base + 1] = mid;
          out[base + 2] = high;
        } else if (stride === 1) {
          // Five bits of height, three of whiteness from how much is highs.
          out[base] = (Math.round((height / 127) * 31) & 0x1f) | (Math.min(7, Math.round((high / 45) * 7)) << 5);
        } else if (stride === 6) {
          // Height, then red green blue as mid high low.
          out[base] = height;
          out[base + 3] = mid;
          out[base + 4] = high;
          out[base + 5] = low;
        } else {
          const word = (Math.min(7, mid >> 4) << 13) | (Math.min(7, high >> 3) << 10) | (Math.min(7, low >> 4) << 7)
            | ((Math.round((height / 127) * 31) & 0x1f) << 2);
          out[base] = word >> 8;
          out[base + 1] = word & 0xff;
        }
      }
      if (!window) return wait(out);
      const first = Math.min(window.from, columns) * stride;
      const len = Math.min(window.len, columns - window.from) * stride;
      return wait(out.subarray(first, first + Math.max(0, len)));
    },

    viewIdsInRange: (viewId, from, to) => {
      const [lo, hi] = from <= to ? [from, to] : [to, from];
      const folder = folderViews.get(viewId);
      if (folder) return wait(folder.slice(Math.max(0, lo), hi + 1).map((row) => row.id));
      const view = views.get(viewId);
      if (!view) return Promise.reject(new Error(`unknown view ${viewId}`));
      const out: string[] = [];
      for (let i = Math.max(0, lo); i <= Math.min(view.order.length - 1, hi); i++) {
        const row = all[view.order[i] ?? 0];
        if (row) out.push(row.id);
      }
      return wait(out);
    },
    trackPcmWaveform: (_trackId: string, _fromMs: number, _toMs: number, columns: number) => {
      // A deterministic stereo peak envelope for the browser build. The real
      // backend seeks into the source file and returns min/max pairs.
      const count = Math.max(1, Math.min(columns, 7500));
      const out = new Uint8Array(count * 8);
      const view = new DataView(out.buffer);
      for (let i = 0; i < count; i++) {
        const left = Math.sin(i * 0.19) * (0.15 + 0.55 * Math.abs(Math.sin(i * 0.013)));
        const right = Math.sin(i * 0.23 + 0.7) * (0.13 + 0.52 * Math.abs(Math.sin(i * 0.011)));
        const spread = 0.08 + 0.11 * Math.abs(Math.sin(i * 0.071));
        const clamp = (value: number) => Math.max(-1, Math.min(value, 1));
        view.setInt16(i * 8, Math.round(clamp(left - spread) * 32767), true);
        view.setInt16(i * 8 + 2, Math.round(clamp(left + spread) * 32767), true);
        view.setInt16(i * 8 + 4, Math.round(clamp(right - spread) * 32767), true);
        view.setInt16(i * 8 + 6, Math.round(clamp(right + spread) * 32767), true);
      }
      return wait(out);
    },

    edits,

    // The track's grid as the store holds it — a steady grid at the row's
    // own BPM until the GRID panel edits it — so the detail waveform has
    // beats to draw without an analysis file behind it. Encoded as the
    // backend encodes it: seven bytes a beat, milliseconds, the beat's
    // number, then the tempo there x100.
    trackBeats: (trackId) => {
      const held = gridOf(trackId);
      if (!held) return wait(new Uint8Array());
      const bytes = new Uint8Array(held.beats.length * 7);
      const view = new DataView(bytes.buffer);
      held.beats.forEach((beat, n) => {
        view.setUint32(n * 7, beat.timeMs, true);
        view.setUint8(n * 7 + 4, beat.number);
        view.setUint16(n * 7 + 5, beat.tempoX100, true);
      });
      return wait(bytes);
    },
    gridState: (trackId) => {
      const held = gridOf(trackId);
      if (!held) return notFound("That track has no beat grid.");
      return wait(gridStateOf(held));
    },

    // The memory cue and the row's own hot cues, so the player's markers and
    // list agree with the badges on the row that was loaded. A copy, ordered
    // by position as the index orders them, so an edit cannot reach the store.
    trackCues: (trackId) =>
      wait(cuesOf(trackId).map((cue) => ({ ...cue })).sort((a, b) => a.positionMs - b.positionMs)),

    // A plausible structure, so the phrase bar can be driven without an
    // analysis file: a real track's phrases tile it end to end.
    trackPhrases: (trackId) => {
      const index = Number.parseInt(trackId, 10) - 100000;
      const row = all[index];
      if (!row || row.analysed === 0) return wait([]);
      const total = row.durationSec * 1000;
      const shape = [
        "INTRO 2", "CHORUS 1", "DOWN", "UP 1", "UP 3", "CHORUS 1",
        "DOWN", "UP 1", "CHORUS 1", "DOWN", "OUTRO",
      ];
      return wait(
        shape.map((label, i) => ({
          beat: i * 32,
          timeMs: Math.round((total * i) / shape.length),
          kind: i,
          label,
        })),
      );
    },

    // Vocals over the middle two thirds, so the strip has something to draw.
    trackVocals: (trackId) => {
      const index = Number.parseInt(trackId, 10) - 100000;
      const row = all[index];
      if (!row || row.analysed === 0) return wait(new Uint8Array());
      const out = new Uint8Array(1200);
      for (let i = 0; i < out.length; i++) {
        const at = i / out.length;
        out[i] = at > 0.25 && at < 0.85 && Math.floor(at * 40) % 3 !== 0 ? 200 : 0;
      }
      return wait(out);
    },

    // No filesystem in a browser, so nothing is written — but the counts are
    // answered so the device panel's reporting can be driven end to end.
    exportPlaylist: (playlistId, destination, defaults, deleteUnlistedMusic) => {
      const device = devices.find((d) => d.path === destination);
      if (!device) {
        // The counts are still answered for a path no mock stick is at, so
        // a script's export can be driven end to end.
        const tracks = playlistSize(playlistId);
        return wait({
          tracks, playlists: 1, bytesCopied: tracks * 8_000_000, analysisFiles: tracks,
          reused: 0, removed: 0, tracksAdded: tracks, tracksUpdated: 0, playlistsAdded: 1, playlistsRemoved: 0, skipped: [], failed: [], verified: true,
        });
      }
      // Added beside what the stick holds, as rekordbox's Export Playlist
      // adds one (#304); only the Sync Manager's SYNC replaces the selection.
      const held = (syncSelections.get(destination) ?? []).map(p => p.libraryId);
      return mockExport(destination, () => writeTo(device, held.includes(playlistId) ? held : [...held, playlistId], defaults, deleteUnlistedMusic));
    },
    exportTracksToDevice: (tracks, destination, defaults) => {
      const device = devices.find((d) => d.path === destination);
      if (!device) return Promise.reject(new Error("That device is no longer connected."));
      const loose = looseTracks.get(destination) ?? new Set<string>();
      for (const track of tracks) loose.add(track);
      looseTracks.set(destination, loose);
      return mockExport(destination, () => writeTo(device, (syncSelections.get(destination) ?? []).map(p => p.libraryId), defaults));
    },

    // No windows in a browser: the shell draws the manager over itself.
    openReportWindow: () => wait(false),
    reportAttachment: () => wait("System information\nBrowser preview\n\nApplication log\nNo application log available.\n"),
    openReportAttachment: () => Promise.reject(new Error("Opening the text editor requires the desktop app.")),
    openSyncWindow: () => wait(false),
    // Stick after stick, each announced before and after, as the real run
    // is. A destination that is not a mock device is a stick that was
    // pulled: its entry carries the error and the others their reports.
    syncDevices: async (playlists, destinations, defaults, automatic, ejectAfterSync, deleteUnlistedMusic) => {
      if (playlists.length === 0) throw new Error("That playlist has no tracks to export.");
      const reports = [];
      for (const path of destinations) {
        const tell = (state: SyncProgress["state"]) => {
          for (const listener of syncProgressListeners) listener({ path, state });
        };
        tell("writing");
        cancelledExports.delete(path);
        tellExport(path, "copying");
        await wait(undefined);
        tellExport(path, "copying", 50);
        await wait(undefined);
        const device = devices.find((d) => d.path === path);
        if (cancelledExports.has(path)) {
          tellExport(path, "cancelled");
          tell("failed");
          reports.push({ path, error: "Export stopped." });
          continue;
        }
        if (device) {
          const report = writeTo(device, playlists, defaults, deleteUnlistedMusic);
          const ejected = Boolean(ejectAfterSync && report.verified && report.skipped.length === 0 && report.failed.length === 0);
          if (ejected) {
            tell("ejecting");
            devices.splice(devices.indexOf(device), 1);
          }
          reports.push({ path, report, ejected });
          if (automatic) autoSync.add(path);
          else autoSync.delete(path);
          tell("done");
          tellExport(path, "done", 100);
        } else {
          reports.push({ path, error: "That device is no longer connected." });
          tell("failed");
          tellExport(path, "failed");
        }
      }
      return reports;
    },
    // The mock holds no audio, so a playlist is worth a fixed 300 MB of new
    // tracks on a stick that has not been synced.
    estimateSync: (playlists, destinations) => wait(destinations.map((path) => {
      const copyBytes = playlists.length * 300 * 1024 ** 2;
      return {
        path, copyBytes, reuseBytes: 0, freeBytes: 0, tracksNew: playlists.length * 10, tracksChanged: 0,
        tracksKept: 0, tracksRemoved: 0, tracksMissing: 0, approximate: false,
      };
    })),
    validateExportFiles: () => wait([]),
    smartRule: (playlist) => wait(smartRules.get(playlist) ?? { logic: "all", conditions: [] }),
    importUsb: () => Promise.resolve({ tracks: 0, histories: 0, settings: 0, skipped: 0, unchanged: 0 }),
    ejectDevice: async (path) => {
      const index = devices.findIndex(device => device.path === path);
      if (index < 0) throw new Error("That device is no longer connected.");
      devices.splice(index, 1);
      await wait(undefined);
    },
    formatDevice: async (path, layout) => {
      const device = devices.find(d => d.path === path);
      if (!device) throw new Error("That device is no longer connected.");
      formattedAs.set(path, layout === "fat32" ? "FAT32" : "HFS+");
      device.freeBytes = device.totalBytes;
      device.export = null;
      deviceLibraries.delete(path);
      syncSelections.delete(path);
      await wait(undefined);
    },
    deviceSyncState: (path) => {
      if (!devices.some((d) => d.path === path)) {
        return Promise.reject(new Error("That device is no longer connected."));
      }
      return wait({
        selected: (syncSelections.get(path) ?? []).map((p) => ({ ...p })),
        onDevice: [...(deviceLibraries.get(path) ?? [])],
        libraries: ["Device Library", "OneLibrary"].map(name => ({ name, nodes: (deviceLibraries.get(path) ?? []).map((name, i) => ({ id: String(i+1), parentId: "0", name, folder: false })) })),
        automatic: autoSync.has(path),
      });
    },
    onSyncProgress: (listener) => {
      syncProgressListeners.add(listener);
      return () => {
        syncProgressListeners.delete(listener);
      };
    },

    // One device, so the panel has something to show. A browser cannot see a
    // real volume; the app asks the OS.
    listDevices: () => wait(devices.map((device) => ({ ...device, fileSystem: formattedAs.get(device.path) ?? "FAT32" }))),
    onImportProgress: (listener) => { importListeners.add(listener); return () => { importListeners.delete(listener); }; },
    onExportProgress: (listener) => { exportListeners.add(listener); return () => { exportListeners.delete(listener); }; },
    exportProgress: () => wait([...exportJobs.values()]),
    cancelExport: (path) => { cancelledExports.add(path); return Promise.resolve(); },
    // A browser opens the address itself.
    openUrl: (url) => {
      window.open(url, "_blank", "noopener");
      return wait(undefined);
    },
    backupDirectory: () => wait(backupDirectory),
    openBackupDirectory: () => wait(undefined),
    backupSizes: (refresh = false) => {
      if (!backupSizes || refresh || Date.now() - backupSizes.updatedAt >= 7 * 24 * 60 * 60 * 1000) backupSizes = { updatedAt: Date.now(), trackCount: all.length, artwork: 8 * 1024 ** 2, vocals: 2 * 1024 ** 2, database: 48 * 1024 ** 2, waveforms: 240 * 1024 ** 2,
        cues: 8 * 1024 ** 2, beatGrids: 16 * 1024 ** 2, phrases: 4 * 1024 ** 2, other: 2 * 1024 ** 2 };
      return wait({ ...backupSizes });
    },
    listBackups: () => wait([...backups.values()].filter(backup => backup.path.startsWith(`${backupDirectory}/rbexport-`) && backup.name.endsWith(".zip")).map(backup => ({ ...backup })).sort((a, b) => b.createdAt - a.createdAt)),
    backUpLibrary: () => wait(saveBackup()),
    backupProgress: () => wait({ ...backupProgress }),
    cancelBackup: () => {
      if (backupProgress.running) backupProgress = { ...backupProgress, phase: "stopping" };
      return wait(undefined);
    },
    startBackup: () => {
      if (backupProgress.running) return refuse("A backup is already running.");
      backupProgress = { running: true, phase: "preparing", copiedBytes: 0, totalBytes: 100, error: null, path: null, currentItem: "Scanning analysis files" };
      setTimeout(() => { if (backupProgress.phase !== "stopping") backupProgress = { ...backupProgress, phase: "copying", copiedBytes: 50, currentItem: "Analysis files · USBANLZ/001/ANLZ0000.DAT" }; }, 300);
      setTimeout(() => {
        if (backupProgress.phase === "stopping") {
          backupProgress = { ...backupProgress, running: false, phase: "cancelled", currentItem: null };
          return;
        }
        try {
          const path = saveBackup();
          backupProgress = { ...backupProgress, running: false, phase: "complete", copiedBytes: 100, path, currentItem: null };
        } catch (e) {
          backupProgress = { ...backupProgress, running: false, phase: "failed", error: String(e), currentItem: null };
        }
      }, 1000);
      return wait(undefined);
    },
    setBackupDirectory: (directory) => {
      if (backupProgress.running) return refuse("Wait for the current backup to finish.");
      backupDirectory = directory;
      return wait(directory);
    },
    deleteBackup: (path) => {
      if (!backups.delete(path)) return notFound("Backup not found.");
      return wait(undefined);
    },
    // A browser cannot ask; the answer is yes, so the flow can be driven. A
    // test sets `window.__confirmAnswer = false` to answer Cancel instead, and
    // reads what was asked from `window.__confirmed`.
    // `window.__confirmTitles` keeps each question's title, and
    // `window.__confirmAnswers`, when set, answers question by question.
    confirm: (message, labels) => {
      const page = window as unknown as {
        __confirmAnswer?: boolean;
        __confirmAnswers?: boolean[];
        __confirmed?: string[];
        __confirmTitles?: (string | null)[];
        __confirmLabels?: (string | null)[];
      };
      (page.__confirmed ??= []).push(message);
      (page.__confirmTitles ??= []).push(labels?.title ?? null);
      (page.__confirmLabels ??= []).push(labels ? `${labels.yes}/${labels.no}` : null);
      const queued = page.__confirmAnswers?.shift();
      return Promise.resolve(queued ?? page.__confirmAnswer ?? true);
    },
    // A message box: what it said, in `window.__told`, as "title: text".
    tell: (message, title) => {
      const page = window as unknown as { __told?: string[] };
      (page.__told ??= []).push(`${title}: ${message}`);
      return wait(undefined);
    },

    // A deck that keeps time but makes no sound. The audio engine is Rust and
    // is not here, so this counts frames and emits the same ticks the engine
    // does; everything above it — the scrolling waveform, the cue point, the
    // readouts — then behaves in a browser exactly as it does in the app, and
    // can be tested. What a browser cannot do is make a noise.
    deckLoad: (deck, trackId, loadId) => {
      // A deck load stops the preview, as the app's does (#242).
      stopPreviewed();
      const d = deckOf(deck);
      const index = Number.parseInt(trackId, 10) - 100000;
      const row = all[index];
      // A missing file is refused before the deck changes, as the app's is.
      if (row?.missing === true) return notFound("Load error. The file could not be found.");
      d.frames = 0;
      d.totalFrames = row ? row.durationSec * SAMPLE_RATE : 0;
      deckBeat[deck] = row && row.bpmX100 > 0 ? 6000 / row.bpmX100 : 0;
      d.playing = false;
      d.loaded = row !== undefined;
      d.loadId = row === undefined ? 0 : loadId;
      d.generation += 1;
      if (!deckA.playing && !deckB.playing) stopClock();
      for (const listener of deckEventListeners) {
        listener({
          deck,
          loadId,
          totalFrames: d.totalFrames,
          sampleRate: SAMPLE_RATE,
          message: row ? null : "That track's file could not be found.",
        });
      }
      sendTick();
      return wait(undefined);
    },
    deckUnload: (deck) => {
      const d = deckOf(deck);
      d.loaded = false;
      d.playing = false;
      d.frames = 0;
      d.loadId = 0;
      if (!deckA.playing && !deckB.playing) stopClock();
      sendTick();
      return wait(undefined);
    },
    deckPlay: (deck) => {
      // A deck that plays stops the preview, as the app's does (#242).
      stopPreviewed();
      const d = deckOf(deck);
      if (!d.loaded) return wait(undefined);
      d.playing = true;
      // From now, not from the clock's last step: a deck that starts between
      // two steps has not been playing since the earlier one.
      d.startsAt = performance.now();
      startClock();
      return wait(undefined);
    },
    // The wait is a timer here rather than counted in output frames: a
    // browser has no callback to count them in, and the timing is only
    // ever judged by ear against a real device.
    deckPlayAfter: (deck, delayMs) => {
      stopPreviewed();
      const d = deckOf(deck);
      if (!d.loaded) return wait(undefined);
      d.playing = true;
      d.startsAt = performance.now() + Math.max(0, delayMs);
      startClock();
      return wait(undefined);
    },
    deckPause: (deck) => {
      const d = deckOf(deck);
      d.playing = false;
      if (!deckA.playing && !deckB.playing) stopClock();
      sendTick();
      return wait(undefined);
    },
    deckSeek: (deck, positionMs) => {
      const d = deckOf(deck);
      d.frames = Math.max(-5 * SAMPLE_RATE, Math.round((positionMs / 1000) * SAMPLE_RATE));
      d.generation += 1;
      d.startsAt = Math.max(d.startsAt, performance.now());
      sendTick();
      return wait(undefined);
    },
    deckMove: (deck, byMs) => {
      const d = deckOf(deck);
      d.frames = Math.max(-5 * SAMPLE_RATE, d.frames + Math.round((byMs / 1000) * SAMPLE_RATE));
      d.generation += 1;
      sendTick();
      return wait(undefined);
    },
    deckSetLoop: (deck, inMs, outMs) => {
      const d = deckOf(deck);
      const from = Math.max(0, Math.round((inMs / 1000) * SAMPLE_RATE));
      const to = Math.max(0, Math.round((outMs / 1000) * SAMPLE_RATE));
      if (to <= from) return wait(undefined);
      d.loopInFrames = from;
      d.loopOutFrames = to;
      d.looping = true;
      if (d.frames >= to || d.frames < from) {
        d.frames = from;
        d.generation += 1;
        d.startsAt = Math.max(d.startsAt, performance.now());
      }
      sendTick();
      return wait(undefined);
    },
    deckLoopActive: (deck, on) => {
      const d = deckOf(deck);
      if (d.loopOutFrames <= d.loopInFrames) return wait(undefined);
      if (!on && d.looping && d.playing) {
        // The clock only steps ten times a second, but the deck wraps at the
        // out point the moment it gets there. Brought up to now first, so an
        // exit after the out point leaves the head where the deck had
        // wrapped it to, not past the end of the loop.
        const now = performance.now();
        const from = Math.max(clockAt, d.startsAt);
        if (now > from) {
          d.frames += Math.round(((now - from) / 1000) * SAMPLE_RATE);
          if (d.frames >= d.loopOutFrames) {
            d.frames = d.loopInFrames + ((d.frames - d.loopOutFrames) % (d.loopOutFrames - d.loopInFrames));
          }
          d.startsAt = now;
        }
      }
      d.looping = on;
      if (on) {
        d.frames = d.loopInFrames;
        d.generation += 1;
        d.startsAt = Math.max(d.startsAt, performance.now());
      }
      sendTick();
      return wait(undefined);
    },
    deckClearLoop: (deck) => {
      const d = deckOf(deck);
      d.loopInFrames = 0;
      d.loopOutFrames = 0;
      d.looping = false;
      sendTick();
      return wait(undefined);
    },
    // A browser has no audio, so a drag is a seek that follows the pointer:
    // the position moves, nothing is heard, and the visuals are the same.
    deckScrubBegin: () => wait(undefined),
    deckScrubTo: (deck, positionMs) => {
      const d = deckOf(deck);
      d.frames = Math.max(-5 * SAMPLE_RATE, Math.round((positionMs / 1000) * SAMPLE_RATE));
      d.startsAt = Math.max(d.startsAt, performance.now());
      sendTick();
      return wait(undefined);
    },
    setMasterLevel: (level) => {
      master = Math.min(Math.max(level, 0), 10 ** (2 / 20));
      sendTick();
      return wait(undefined);
    },
    // A browser has one output and no way to name it, so the list is empty
    // and the picker says so rather than inventing devices.
    audioDevices: () => wait({ devices: [], default: null, chosen: null }),
    setAudioDevice: () => wait(undefined),
    // Held and given back clamped as the engine would, so the controls in
    // Settings behave in a browser.
    // A browser has nothing to update, so this offers a pretend version two
    // releases on, with a changelog shaped like the real one, and its
    // download runs at a believable pace so the bar can be watched. Once
    // downloaded it stays downloaded, as the shell's does.
    checkForUpdate: () =>
      wait<UpdateCheck>({
        ready: updateReady,
        storeInstall: false,
        currentVersion: "0.4.0",
        version: "0.6.0",
        date: "2026-09-12T18:00:00Z",
        changes: [
          {
            version: "0.6.0",
            date: "2026-09-12",
            body:
              "## [0.6.0] — 2026-09-12\n\n### Added\n- The app checks for a newer version when it " +
              "starts, downloads it with a progress bar, and shows what changed since the version " +
              "running.\n\n### Fixed\n- A track dragged to a player carries a faded copy of its row.",
          },
          {
            version: "0.5.0",
            date: "2026-09-11",
            body:
              "## [0.5.0] — 2026-09-11\n\n### Added\n- The app icon is the rekordbox cube ring with a " +
              "feather in the middle.\n\n### Changed\n- The right-click menus list what rekordbox's do.",
          },
        ],
      }),
    readyUpdate: () => wait(updateReady),
    downloadUpdate: () =>
      new Promise<UpdateReady>((resolve) => {
        if (updateReady) {
          resolve(updateReady);
          return;
        }
        const total = 16_342_693;
        let downloaded = 0;
        const tick = () => {
          downloaded = Math.min(total, downloaded + 900_000 + Math.random() * 400_000);
          for (const listener of updateProgressListeners) listener({ downloaded, total });
          if (downloaded < total) {
            setTimeout(tick, 100);
          } else {
            // The swap on disk takes a moment on a real machine too.
            setTimeout(() => {
              updateReady = { version: "0.6.0", installed: true };
              resolve(updateReady);
            }, 800);
          }
        };
        setTimeout(tick, 300);
      }),
    // A real restart never comes back; a browser cannot restart, so the
    // manager is told what it would be told if the restart had failed —
    // which is the only way it ever hears back.
    restartToUpdate: () => wait(undefined).then(() => Promise.reject(new Error("A browser cannot restart into an update."))),
    onUpdateProgress: (listener) => {
      updateProgressListeners.add(listener);
      return () => {
        updateProgressListeners.delete(listener);
      };
    },
    // A browser is not an install; nothing is counted.
    masterLimiter: () => wait({ ...limiter }),
    setMasterLimiter: (wanted) => {
      limiter = {
        enabled: wanted.enabled,
        inputGainDb: Number.isFinite(wanted.inputGainDb) ? Math.min(Math.max(wanted.inputGainDb, -24), 24) : -4,
        ceilingDb: Number.isFinite(wanted.ceilingDb)
          ? Math.min(Math.max(wanted.ceilingDb, -12), 0)
          : -0.3,
        releaseMs: Number.isFinite(wanted.releaseMs)
          ? Math.min(Math.max(wanted.releaseMs, 10), 1000)
          : 100,
      };
      return wait({ ...limiter });
    },
    // The mixer is the engine's; a browser has no audio to apply it to, so
    // these are accepted and dropped rather than pretended at.
    // The tempo is the engine's, but the mock keeps it so the readout and the
    // MT button move: a browser has no audio to apply it to, and a control
    // that does not respond reads as a broken one.
    deckTempo: (deck, tempo) => {
      const on = deck === "b" ? deckB : deckA;
      on.tempo = Math.min(Math.max(tempo, 0.5), 2);
      sendTick();
      return wait(undefined);
    },
    deckMasterTempo: (deck, on) => {
      (deck === "b" ? deckB : deckA).masterTempo = on;
      sendTick();
      return wait(undefined);
    },
    deckMetronome: () => wait(undefined),
    setMetronomeGrid: () => wait(undefined),
    deckKeyShift: (deck, semitones) => {
      (deck === "b" ? deckB : deckA).keyShift = Math.max(-12, Math.min(12, Math.round(semitones)));
      sendTick();
      return wait(undefined);
    },
    setMetronome: () => wait(undefined),
    setAudioConfig: () => wait(undefined),
    setChannelBand: () => wait(undefined),
    setChannelKill: () => wait(undefined),
    setChannelTrim: () => wait(undefined),
    setCrossfade: () => wait(undefined),
    setEqCurve: () => wait(undefined),
    deckScrubEnd: (deck) => {
      deckOf(deck).generation += 1;
      sendTick();
      return wait(undefined);
    },
    // A browser cannot see its own process. Zeroes would read as an app that
    // costs nothing, so every figure the platform will not give is null.
    appVersion: () => wait("0.4.0"),
    openLog: () => wait(undefined),
    appDiagnostics: () =>
      wait({ audioLoad: 0, audioXruns: 0, cpu: 0, memoryMb: 0, threads: null, openFiles: null, gpu: null }),
    // A browser has no Finder to open. Refusing is the truth; succeeding
    // silently made the menu item look as if it had done something.
    revealTrack: () =>
      wait(undefined).then(() => {
        throw new Error("A browser cannot show a file in the Finder.");
      }),

    deckState: () => wait(tick()),
    previewPlay: (trackId, positionMs) => {
      const row = all[Number.parseInt(trackId, 10) - 100000];
      if (!row) return notFound("That track's file could not be found.");
      // rekordbox outside PERFORMANCE mode pauses the decks for a preview.
      if (deckA.playing || deckB.playing) {
        deckA.playing = false;
        deckB.playing = false;
        stopClock();
        sendTick();
      }
      previewed.track = trackId;
      previewed.durationMs = row.durationSec * 1000;
      previewed.positionMs = Math.min(Math.max(0, positionMs), previewed.durationMs);
      previewed.since = performance.now();
      previewed.playing = previewed.positionMs < previewed.durationMs;
      return wait(undefined);
    },
    previewStop: () => {
      stopPreviewed();
      return wait(undefined);
    },
    previewState: () => {
      const positionMs = previewNow();
      return wait({
        track: previewed.track, playing: previewed.playing, positionMs, durationMs: previewed.durationMs,
      });
    },
    onDeckTick: (listener) => {
      deckTickListeners.add(listener);
      return () => deckTickListeners.delete(listener);
    },
    // A browser has no audio callback, so there is nothing to meter and no
    // beat to send it on.
    onMeters: () => () => undefined,
    onDeckEvent: (listener) => {
      deckEventListeners.add(listener);
      return () => deckEventListeners.delete(listener);
    },
    onDeckReset: () => () => undefined,

    onLibraryReady: (listener) => {
      readyListeners.add(listener);
      return () => readyListeners.delete(listener);
    },
    onLibraryProblem: (listener) => {
      problemListeners.add(listener);
      return () => problemListeners.delete(listener);
    },
    libraryProblem: () => wait<LibraryProblem | null>(problem),
    createLibrary: async () => {
      await wait(undefined);
      libraryFound();
    },
    useDefaultLibrary: async () => {
      await wait(undefined);
      // The default folder is empty here, so it is offered to be made, as
      // the real backend's next look reports.
      problem = { kind: "missing", masterDb: defaultMasterDb };
      for (const listener of problemListeners) listener({ ...problem });
    },
    databaseDrives: () => wait(databaseDrives.map((drive) => ({ ...drive }))),
    switchLibrary: async (masterDb) => {
      await wait(undefined);
      if (!databaseDrives.some((drive) => drive.masterDb === masterDb)) {
        throw new Error(`${masterDb} is not a master.db`);
      }
      // The real app starts again on it; the mock marks it open.
      for (const drive of databaseDrives) drive.current = drive.masterDb === masterDb;
    },

    // A browser has no native menu bar. The mock exposes the listener so a
    // test can fire an item the way the shell would; this is the mock, which
    // exists to be driven, rather than a seam in the app.
    // The fake volumes never come or go.
    onDevicesChanged: () => () => undefined,

    // A browser has no AppleScript to ask anything of the window.
    serveScripts: () => () => undefined,
    mirrorPreferences: () => wait(undefined),

    onMenu: (listener) => {
      const w = window as unknown as { __menu?: (id: string) => void };
      w.__menu = listener;
      return () => {
        delete w.__menu;
      };
    },
    setMenuLabels: () => wait(undefined),

    // No network in a browser, so LINK cannot turn on. Saying why is better
    // than a switch that silently does nothing.
    linkStatus: () => wait(mockLinkStatus()),
    linkPeers: () => wait(linkMode === null || linkMode === "blocked" ? [] : mockPeers),
    onLinkPeers: () => () => undefined,
    startLinkExport: () =>
      wait(
        linkMode === null
          ? {
              ...linkOff(),
              problem: "LINK needs the desktop application; a browser has no access to the network.",
            }
          : mockLinkOn(),
      ),
    stopLinkExport: () => wait(linkOff()),
    loadTrackOnLink: () => wait(undefined),
    setLinkMaster: (on) => {
      mockMaster.on = on;
      return wait(mockLinkStatus());
    },
    nudgeLinkMaster: (deltaBpm) => {
      mockMaster.bpm = Math.min(300, Math.max(40, Math.round((mockMaster.bpm + deltaBpm) * 100) / 100));
      return wait(mockLinkStatus());
    },
    takeLinkMasterTempo: () => {
      // A mock master player runs at 128.00; take it.
      mockMaster.bpm = 128;
      return wait(mockLinkStatus());
    },
    onLinkStatus: () => () => undefined,

    // Analysis is real work in the app; here it just answers, so the queue's
    // sequencing and progress can be driven end to end without audio.
    analyseTrack: (trackId, _mode, settings) => {
      const index = Number.parseInt(trackId, 10) - 100000;
      const row = all[index];
      if (!row) return Promise.reject(new Error("That track is not in the library."));
      if (gridOf(trackId)?.locked) return Promise.reject(new Error("This track's analysis is locked. Unlock it to analyze."));
      if (settings && !settings.bpmGrid && !settings.key) return Promise.reject(new Error("Select BPM / Grid or KEY to analyze."));
      // Every seventh track fails, so the failure path is exercised too.
      if (index % 7 === 6) {
        return Promise.reject(new Error("That file could not be decoded."));
      }
      // Deliberately not instant. Real analysis is a decode and a DSP pass —
      // seconds per track — and a mock that answers immediately makes the
      // queue's progress, cancellation and failure handling unobservable.
      return new Promise((resolve) =>
        setTimeout(
          () => {
            if (settings?.bpmGrid !== false) {
              row.analysed = 1;
              row.bpmX100 ||= 12_800;
            }
            if (settings?.key !== false) row.key ||= "Am";
            // As the shell says it: a deck showing the track redraws.
            for (const listener of analysisListeners) listener(trackId);
            const firstBeatMs = settings?.bpmGrid !== false && settings?.firstBeatCue
              ? gridOf(trackId)?.beats[0]?.timeMs : undefined;
            const cues = cuesOf(trackId);
            if (firstBeatMs !== undefined && !cues.some(cue => cue.memory && Math.abs(cue.positionMs - firstBeatMs) <= 5)) {
              cues.push({ id: `cue-${nextCueId++}`, positionMs: firstBeatMs, outMs: 0, letter: "", memory: true, colour: null });
              void cuesChanged(trackId, null);
            }
            resolve({
            trackId,
            analysed: row.analysed,
            bpmX100: row.bpmX100,
            key: row.key,
            beats: Math.round((row.durationSec * (row.bpmX100 || 12_800)) / 6000),
            peak: 0.9,
            durationSec: row.durationSec,
            elapsedMs: ANALYSIS_MS,
            analysisPath: `/PIONEER/USBANLZ/P${String(index % 1000).padStart(3, "0")}/${index.toString(16).toUpperCase().padStart(8, "0")}/ANLZ0000.DAT`,
            });
          },
          ANALYSIS_MS,
        ),
      );
    },

    // No picker in a browser, so nothing can be chosen to import or written.
    importFiles: () => wait(null),
    importFolder: () => wait(null),
    importPaths: (paths) => wait({ imported: 0, skipped: paths.map((p) => `${p}: the mock library takes no files`), tracks: [], existing: [] }),
    // No file system in a browser: a path is its own one file.
    listImportFiles: (paths) => wait([...paths]),
    importPathsSlice: (paths) => wait({ imported: 0, skipped: paths.map((p) => `${p}: the mock library takes no files`), tracks: [], existing: [] }),
    // Emits progress, then holds until `window.__finishImport()` so a test
    // can watch the status line while an import is still running. A test
    // sets `window.__xmlSameNamed` to the lists the file would replace: the
    // mock then asks first, as the real backend's caller does, and
    // `window.__xmlImportStarted` says whether anything was imported.
    importXml: async (confirmReplace) => {
      const page = window as unknown as { __xmlSameNamed?: string[]; __xmlImportStarted?: boolean };
      const sameNamed = page.__xmlSameNamed ?? [];
      if (sameNamed.length > 0 && !(await confirmReplace(sameNamed))) return null;
      page.__xmlImportStarted = true;
      const emit = (done: number) => importListeners.forEach((listener) =>
        listener({ path: "", state: "copying", done, total: 3, title: "" }));
      emit(0);
      emit(1);
      await new Promise<void>((resolve) => {
        (window as unknown as { __finishImport?: () => void }).__finishImport = resolve;
      });
      return null;
    },
    // No file system in a browser: a path without an extension stands for a
    // folder, which becomes an empty playlist the way a real drop names one.
    // Every folder of one drop goes to the drop's one insert index, as in
    // the real backend (rekordbox's createNewList), so a later folder lands
    // before an earlier one.
    importFolderPlaylist: async (path, parent, replace, given) => {
      const name = path.split(/[\\/]/).filter(Boolean).pop() ?? "";
      const report = { name, playlist: null, conflict: null, folder: false, imported: 0, skipped: [], tracks: [], existing: 0, at: given ?? null };
      if (!name || /\.[a-z0-9]+$/i.test(name)) return wait(report);
      const siblings = childrenOf(parent);
      let at = given ?? siblings.length;
      const clash = siblings.find((n) => n.name === name);
      if (clash && clash.id !== replace) return wait({ ...report, folder: true, conflict: clash.id, at });
      if (clash) {
        if (siblings.indexOf(clash) < at) at -= 1;
        await edits.deletePlaylist(clash.id);
      }
      const before = new Set(tree.map((n) => n.id));
      await edits.createPlaylist(name, parent);
      const made = tree.find((n) => !before.has(n.id))?.id ?? null;
      if (made && childrenOf(parent).findIndex((n) => n.id === made) !== at) await edits.movePlaylist(made, parent, at);
      return wait({ ...report, folder: true, playlist: made, at });
    },
    exportLoopWav: () => wait(null),
    importItunes: () => wait(null),
    itunesDefaultLibrary: () => wait({
      path: "/Users/dj/Music/Music/Library.xml",
      tree: [
        { id: "itunes:0", name: "Chill", kind: "folder", depth: 1 },
        { id: "itunes:1", name: "Airplane x Coding", kind: "playlist", depth: 2 },
        { id: "itunes:2", name: "SHOWS", kind: "folder", depth: 1 },
        { id: "itunes:3", name: "Green Day Essentials", kind: "playlist", depth: 2 },
        { id: "itunes:4", name: "The Police Essentials", kind: "playlist", depth: 1 },
      ],
    }),
    chooseItunesLibrary: () => wait(null),
    importItunesSelected: (_path, ids) => wait({ imported: ids.length, existing: 0, skipped: [], playlists: ids.length, cues: 0, tracks: [] }),
    // The mock's phrases are drawn from a table, not a file: nothing to cut.
    editPhrase: () => wait(false),
    exportPlaylistFile: () => wait(null),
    exportXml: () => wait(null),

    // The tracks `?missing=N` took the files of, in collection order.
    missingTracks: (offset, limit) => {
      const gone = all.filter((row) => row.missing === true);
      return wait({
        total: gone.length,
        tracks: gone.slice(offset, offset + limit).map((row) => ({
          id: row.id,
          title: row.title,
          artist: row.artist,
          album: row.album,
          path: String(row.extra?.location ?? ""),
        })),
      });
    },
    removeMissingTracks: async (tracks) => {
      const gone = all.filter((row) => row.missing === true && (tracks === null || tracks.includes(row.id)));
      const ids = gone.map((row) => row.id);
      for (const [playlist, members] of membership) {
        membership.set(playlist, members.filter((t) => !ids.includes(t)));
      }
      // The mock's collection is fixed, so a deleted track stays listed but
      // stops being missing; the manager's list is what shows the change.
      for (const row of gone) delete row.missing;
      if (ids.length > 0) await bump(false);
      return ids.length;
    },
    // The unanalysed rows whose file is there; `?missing=N` takes files away.
    unanalysedTracks: (from, limit) => {
      const tracks: { id: string; title: string }[] = [];
      for (let index = from; index < all.length; index += 1) {
        const row = all[index];
        if (!row || row.analysed !== 0 || row.missing === true) continue;
        if (tracks.length === limit) return wait({ tracks, next: index });
        tracks.push({ id: row.id, title: row.title });
      }
      return wait({ tracks, next: null });
    },
    // The mock's titles are drawn from a short list, so the same title under
    // the same artist comes up as it does in a real library.
    findDuplicates: (limit) => {
      const groups = new Map<string, RowDto[]>();
      for (const row of all) {
        const key = `${row.title.toLowerCase()}\u0000${row.artist.toLowerCase()}`;
        groups.set(key, [...(groups.get(key) ?? []), row]);
      }
      const found = [...groups.values()].filter((rows) => rows.length > 1);
      return wait({
        groups: found.length,
        extra: found.reduce((n, rows) => n + rows.length - 1, 0),
        shown: found.slice(0, limit).map((rows) => ({
          title: rows[0]?.title ?? "",
          artist: rows[0]?.artist ?? "",
          tracks: rows.map((row) => ({ id: row.id, path: `/Music/${row.title}.mp3`, durationSec: row.durationSec, present: true })),
        })),
      });
    },
    // No picker in a browser: the chooser answers with the track's own file
    // name under a fixed folder, or with `window.__relocatePicks` in turn
    // (null is a cancel). What it was asked is kept in
    // `window.__relocateChooser` as "title | folder".
    chooseRelocateFile: (title, fileName, folder) => {
      const page = window as unknown as { __relocatePicks?: (string | null)[]; __relocateChooser?: string[] };
      (page.__relocateChooser ??= []).push(`${title} | ${folder ?? ""}`);
      const queued = page.__relocatePicks?.shift();
      return wait(queued !== undefined ? queued : `/Users/mock/Music/Moved/${fileName}`);
    },
    // A file another row of the collection already has is refused, as
    // rekordbox refuses it; anything else stops the track being missing.
    relocateTrack: async (trackId, path) => {
      const row = all.find((r) => r.id === trackId);
      const taken = all.some((r) => r.id !== trackId && r.missing !== true && String(r.extra?.location ?? "") === path);
      if (taken) return false;
      if (row?.missing === true) {
        delete row.missing;
        await bump();
      }
      return true;
    },
    relocationTargets: (tracks) => {
      const wanted = tracks.slice(0, 128);
      return wait(wanted
        .map((id) => all.find((row) => row.id === id))
        .filter((row): row is RowDto => row?.missing === true)
        .map((row) => ({
          id: row.id,
          title: row.title,
          artist: row.artist,
          album: row.album,
          path: String(row.extra?.location ?? ""),
        })));
    },
    // The new folder holds every missing file named: each is found.
    // `window.__relocatedBy` keeps "from -> to".
    relocateByLocation: async (tracks, from, to) => {
      const page = window as unknown as { __relocatedBy?: string[] };
      (page.__relocatedBy ??= []).push(`${from} -> ${to}`);
      const found = all.filter((row) => row.missing === true && tracks.includes(row.id));
      for (const row of found) delete row.missing;
      if (found.length > 0) await bump();
      return found.length;
    },
    // The search folders hold every other missing file, in list order, so a
    // run both relocates and leaves some unresolved; with no folder ticked
    // nothing is found. `window.__relocateSearch` keeps what was searched;
    // a test that sets `window.__relocateHeld` keeps the search going until
    // it calls `window.__releaseRelocate()`, as a long walk of the folders
    // would.
    autoRelocate: async (search, tracks) => {
      const page = window as unknown as {
        __relocateSearch?: RelocateSearch[]; __relocateHeld?: boolean; __releaseRelocate?: () => void;
      };
      (page.__relocateSearch ??= []).push(search);
      if (page.__relocateHeld === true) {
        await new Promise<void>((resolve) => { page.__releaseRelocate = resolve; });
      }
      const searched = search.folders.length > 0 || search.music || search.video || search.desktop;
      const gone = all.filter((row) => row.missing === true && (tracks === null || tracks.includes(row.id)));
      const found = searched ? gone.filter((_, at) => at % 2 === 0) : [];
      for (const row of found) delete row.missing;
      if (found.length > 0) await bump();
      return { relocated: found.length, unresolved: gone.length - found.length };
    },
    // No dialogs in a browser: the folder is a fixed one, so the search
    // folders list can be driven end to end.
    pickFolder: () => wait("/Users/mock/Music/Moved"),
    pickImage: () => wait("/Users/mock/Pictures/cover.jpg"),
    // No windows in a browser: the shell draws the Preferences over itself,
    // and its resets are its own to do.
    openPreferences: () => wait(false),
    // In a browser the Preferences overlay and the shell share a page, so a
    // request is handed straight to whoever is listening.
    onPreferencesReset: (listener) => {
      preferencesRequestListeners.add(listener);
      return () => {
        preferencesRequestListeners.delete(listener);
      };
    },
    requestPreferencesReset: (what) => {
      for (const listener of preferencesRequestListeners) listener(what);
      return wait(undefined);
    },
    closeWindow: () => wait(undefined),
    referenceStickSettings: () => {
      const reference = referenceDeviceSettings("", false);
      return wait({ categories: reference.categories, sorts: reference.sorts });
    },

    // The device tabs. Held per stick so a change survives switching tabs
    // and devices, the way a written file would.
    deviceSettings: (path) => wait(copySettings(settingsOf(path))),
    ensureDeviceLibrary: (path, defaults) => {
      const current = settingsOf(path);
      if (current.hasDeviceLibrary) return wait(copySettings(current));
      // The empty database a blank stick gets: the reference rows, the
      // pane's display choices, nothing exported yet.
      const next: DeviceSettings = {
        ...current,
        hasDeviceLibrary: true,
        hasOneLibrary: true,
        hasLibrarySettings: true,
        hasDevSetting: true,
        deviceLibraryBackgroundColorType: current.deviceLibraryBackgroundColorType ?? 0,
        waveformColor: defaults?.waveformColor ?? current.waveformColor,
        waveformPosition: defaults?.waveformPosition ?? current.waveformPosition,
        overviewWaveform: defaults?.overviewWaveform ?? current.overviewWaveform,
        keyDisplay: defaults?.keyDisplay ?? current.keyDisplay,
        categories: defaults?.categories ?? current.categories,
        sorts: defaults?.sorts ?? current.sorts,
        subColumn: defaults?.subColumn ?? current.subColumn,
      };
      deviceSettings.set(path, next);
      const device = devices.find((d) => d.path === path);
      if (device && device.export === null) device.export = { tracks: 0, playlists: 0, ours: true, written: "" };
      return wait(copySettings(next));
    },
    // The mock's sticks carry their settings from the start, so there is
    // nothing to give; what the stick holds comes back.
    writeDeviceDefaults: (path) => wait(copySettings(settingsOf(path))),
    saveDeviceSettings: (path, settings) => {
      const current = settingsOf(path);
      // A stick without a library keeps its reference rows: nothing to
      // write them into, as the real backend also refuses.
      const next: DeviceSettings = current.hasLibrarySettings
        ? { ...copySettings(settings), hasDevSetting: true }
        : {
            ...current,
            hasDevSetting: true,
            waveformColor: settings.waveformColor,
            waveformPosition: settings.waveformPosition,
            overviewWaveform: settings.overviewWaveform,
            keyDisplay: settings.keyDisplay,
          };
      deviceSettings.set(path, next);
      return wait(copySettings(next));
    },

    onLibraryChanged: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    onTagListChanged: (listener) => {
      tagListListeners.add(listener);
      return () => tagListListeners.delete(listener);
    },
    onEditHistory: (listener) => {
      historyListeners.add(listener);
      return () => historyListeners.delete(listener);
    },
    reloadLibrary: () => bump(),
    // The mock's analysis rewrites no files, so nothing redraws.

    filterValues: (spec) => {
      if (!ready) return notReady();
      // Over the source and query alone, never the filter's own result.
      const bpms = new Map<number, number>();
      const keys = new Map<string, number>();
      for (const i of candidatesFor(spec)) {
        const row = all[i];
        if (!row) continue;
        const whole = wholeBpm(row.bpmX100);
        if (whole > 0) bpms.set(whole, (bpms.get(whole) ?? 0) + 1);
        if (row.key !== "") keys.set(row.key, (keys.get(row.key) ?? 0) + 1);
      }
      const values: FilterValues = {
        bpms: [...bpms].map(([value, count]) => ({ value, count })).sort((x, y) => x.value - y.value),
        keys: [...keys]
          .map(([value, count]) => ({ value, count }))
          .sort((x, y) => {
            const [nx, lx, sx] = keyOrder(x.value);
            const [ny, ly, sy] = keyOrder(y.value);
            return nx - ny || lx - ly || sx.localeCompare(sy);
          }),
        // What the reference library holds, read once: one used category and
        // rekordbox's three unused ones, which it names `Empty Category`.
        tags: [
          {
            name: "Lexicon Tags",
            tags: [
              "Components ▶ Synth", "Components ▶ Vocal", "Components ▶ Beat",
              "Components ▶ Sub Bass", "Components ▶ Percussion", "Components ▶ Piano",
              "Components ▶ Upper", "Situation ▶ Main Floor", "Situation ▶ Second Floor",
              "Situation ▶ Lounge",
            ],
          },
          { name: "Empty Category", tags: [] },
          { name: "Empty Category", tags: [] },
          { name: "Empty Category", tags: [] },
        ],
      };
      return wait(values);
    },

    onCuesChanged: (listener) => {
      cueListeners.add(listener);
      return () => cueListeners.delete(listener);
    },
    onGridChanged: (listener) => {
      gridListeners.add(listener);
      return () => gridListeners.delete(listener);
    },
    onAnalysisChanged: (listener) => {
      analysisListeners.add(listener);
      return () => analysisListeners.delete(listener);
    },
    // The fake disk above. Copies, as with the tree: the map is the mock's.
    deviceLibraries: (path) => wait(stickLibraries.libraries(path)),
    devicePlaylistEdit: (path, format, edit) => {
      try {
        return wait(stickLibraries.edit(path, format, edit));
      } catch (error) {
        return Promise.reject(error instanceof Error ? error : new Error(String(error)));
      }
    },
    explorerRoots: () => wait(EXPLORER_ROOTS.map((root) => ({ ...root }))),
    explorerChildren: (path) => {
      const names = [...(EXPLORER_CHILDREN.get(path) ?? [])];
      // The stick's PIONEER folder stands in for one the cap cut: the real
      // backend keeps the first two thousand of a 14,503-folder card.
      return wait({ names, total: path === "/Volumes/SD/PIONEER" ? 14_503 : names.length });
    },
    trackDetails: (trackId) => {
      if (!ready) return notReady();
      // Counted for the deck's test: its INFO tab must not fetch a record
      // for every load when the tab is not showing.
      if (typeof window !== "undefined") {
        const w = window as unknown as { __detailsFetches?: number };
        w.__detailsFetches = (w.__detailsFetches ?? 0) + 1;
      }
      const row = all.find((r) => r.id === trackId);
      if (!row) return Promise.reject(new Error("That track is no longer in the library."));
      // A copy: the panel must not be able to edit the backend's own record.
      return wait({ ...detailsOf(row) });
    },
    selectionDetails: (trackIds) => {
      if (!ready) return notReady();
      const rows = trackIds.flatMap((id) => all.filter((r) => r.id === id));
      const [head] = rows;
      if (!head) return Promise.reject(new Error("That track is no longer in the library."));
      const first = detailsOf(head);
      const others = rows.slice(1).map(detailsOf);
      // Every field but the id and the My Tags, as the real backend compares.
      const compared = (Object.keys(first) as (keyof TrackDetails)[])
        .filter((key) => key !== "id" && key !== "myTags" && key !== "hasArtwork");
      const mixed: SelectionDetails["mixed"] = compared.filter((key) =>
        others.some((other) => other[key] !== first[key]),
      );
      // The mock serves a different picture for each track that has one.
      if (rows.length > 1 && rows.some((r) => r.hasArtwork)) mixed.push("artwork");
      return wait({ first: { ...first, myTags: [...first.myTags] }, count: rows.length, mixed });
    },
    trackLookups: () =>
      wait({
        keys: [...KEYS],
        genres: GENRES.filter((g) => g !== ""),
        myTagCategories: [
          { name: "Situation", tags: [{ id: "t-peak", name: "Peak" }, { id: "t-warm", name: "Warm-up" }] },
          { name: "Components", tags: [{ id: "t-synth", name: "Synth" }, { id: "t-vocal", name: "Vocal" }] },
        ],
      }),
  };
}

/** A bare `?name` flag in the URL. */
function readFlagFromUrl(name: string): boolean {
  if (typeof location === "undefined") return false;
  return new URLSearchParams(location.search).has(name);
}

/** Focused tree fixtures for browser regressions; ordinary mock use stays full. */
function readPlaylistFixtureFromUrl(): PlaylistFixture {
  if (typeof location === "undefined") return "default";
  const fixture = new URLSearchParams(location.search).get("playlistFixture");
  return fixture === "empty" || fixture === "cueOnly" ? fixture : "default";
}

/**
 * `?link=detected|on|blocked` puts the mock on a Pro DJ LINK network, which a
 * browser has no way to be on. `detected` hears two players and a mixer with
 * LINK off, `on` serves them, `blocked` reports the ports held.
 */
function readLinkFromUrl(): "detected" | "on" | "blocked" | null {
  if (typeof location === "undefined") return null;
  const raw = new URLSearchParams(location.search).get("link");
  return raw === "detected" || raw === "on" || raw === "blocked" ? raw : null;
}

/** `?tracks=40000` lets the perf spec load a full-size library into the mock. */
function readMissingFromUrl(): number | null {
  if (typeof location === "undefined") return null;
  const raw = new URLSearchParams(location.search).get("missing");
  const n = raw ? Number.parseInt(raw, 10) : NaN;
  return Number.isFinite(n) && n > 0 ? n : null;
}

function readCountFromUrl(): number | null {
  if (typeof location === "undefined") return null;
  const raw = new URLSearchParams(location.search).get("tracks");
  const n = raw ? Number.parseInt(raw, 10) : NaN;
  return Number.isFinite(n) && n > 0 && n <= 200000 ? n : null;
}

/**
 * Simulated IPC latency, from `?latency=25`.
 *
 * The real backend answers `fetch_rows` in single-digit milliseconds and the
 * mock answers in zero, and the difference is not cosmetic: a race between a
 * moving window and a landing page cannot happen at zero. This is how a scroll
 * is tested against a backend that takes any time at all.
 */
function readLatencyFromUrl(): number | null {
  if (typeof location === "undefined") return null;
  const raw = new URLSearchParams(location.search).get("latency");
  const n = raw ? Number.parseInt(raw, 10) : NaN;
  return Number.isFinite(n) && n >= 0 && n <= 2000 ? n : null;
}
