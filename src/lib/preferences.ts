/**
 * The Preferences window's choices, and how they are kept.
 *
 * Every field here drives something the application actually does; a choice
 * with nothing behind it is not stored, because a switch that changes nothing
 * is worse than no switch. The layout of rekordbox's window — which pane and
 * tab a choice sits under — is the window's business, not this module's.
 *
 * Stored in localStorage beside the session, and checked on the way back in
 * the same way: a hand-edited value, a value from a build that spelt a choice
 * differently, or nothing at all must each come back as a working set.
 */
import { ANALYSIS_SLOTS, SLOTS } from "./queue";
import type { KeyChord } from "./shortcuts";
import { toCamelot, type TrafficLightReach } from "./camelot";
import type {
  KeyDisplay, MenuSlot, OverviewWaveform, WaveformColor, WaveformPosition,
} from "@/ipc/types";

/** BEAT SYNC matches the tempo and the bar; BPM SYNC the tempo alone. */
export type SyncType = "beat" | "bpm";

/** View › Color › HOT CUE color: each cue its own, or every one the CDJ green. */
export type HotCueColor = "colorful" | "cdj";

/**
 * View › Display Type › Beat Count Display: what the number beside the
 * playhead counts — bars into the track, or bars or beats to the next memory
 * cue, the way a CDJ's count-down does.
 */
export type BeatCount = "position" | "toMemoryBars" | "toMemoryBeats";

/** Audio › Sample Rate, in hertz; what the device is asked to run at. */
export type SampleRate = 44_100 | 48_000 | 88_200 | 96_000;
export const SAMPLE_RATES: readonly SampleRate[] = [44_100, 48_000, 88_200, 96_000];

/** Audio › Buffer size, in frames; the slider's stops. */
export const BUFFER_SIZES: readonly number[] = [64, 128, 256, 512, 1024, 2048];

/** Audio › Metronome: which click, and how loud. */
export type MetronomeSound = 1 | 2 | 3;
export type MetronomeVolume = "small" | "middle" | "large";

/** The fraction of a beat the quantized cue snaps to. */
export type QuantizeBeat = "1/1" | "1/2" | "1/4" | "1/8";

export const QUANTIZE_BEATS: readonly QuantizeBeat[] = ["1/1", "1/2", "1/4", "1/8"];

/** How many beats one quantize step is: `1/4` is a quarter of a beat. */
export function quantizeFraction(value: QuantizeBeat): number {
  switch (value) {
    case "1/2": return 0.5;
    case "1/4": return 0.25;
    case "1/8": return 0.125;
    default: return 1;
  }
}

/**
 * The five stops of the FontSize and Line Space sliders, as multiples of the
 * measured token. The middle stop is the measured size; the others are
 * [ASSUME] — rekordbox's own steps have not been measured, only that the
 * slider has a small end and a large one.
 */
export const BROWSE_SCALE_STEPS = 5;
export const BROWSE_SCALES: readonly number[] = [0.8, 0.9, 1, 1.15, 1.3];
export const BROWSE_SCALE_DEFAULT = 2;

export function browseScale(step: number): number {
  return BROWSE_SCALES[step] ?? 1;
}

/**
 * Browse › FontSize, Bold and Line Space as the CSS custom properties the
 * browser's lists draw with, so the track table and the playlist tree share
 * one rule. `rowBase` is the measured row height in px (`--s-row-height`).
 */
export function browseListVars(
  view: { browseFontSize: number; browseLineSpace: number; browseBold: boolean },
  rowBase: number,
): Record<string, string | number> {
  return {
    "--s-row-height": `${Math.round(rowBase * browseScale(view.browseLineSpace))}px`,
    "--f-size-ui": `calc(${browseScale(view.browseFontSize)} * var(--f-size-ui-base))`,
    "--browse-weight": view.browseBold ? 700 : 400,
  };
}

export type VuMeterMode = "normal" | "fabulous";

export const LOCALES = [
  "en", "fr", "de", "es", "it", "nl", "ru", "pt", "sv", "da", "tr", "el", "hu", "cs",
  "zh-CN", "zh-TW", "ko", "ja",
] as const;
export type Locale = (typeof LOCALES)[number];

export interface ViewPreferences {
  /** Language used by the application UI. */
  locale: Locale;
  /** Show BPM-change labels and ramps on player waveforms. */
  showBpmChanges: boolean;
  vuMeter: VuMeterMode;
  /** Media Player › Display Tempo slider. */
  tempoSlider: boolean;
  /** Show Tooltips. */
  tooltips: boolean;
  /** Browse › FontSize, as a slider stop 0 to 4. */
  browseFontSize: number;
  /** Browse › Bold. */
  browseBold: boolean;
  /** Browse › Line Space, as a slider stop 0 to 4. */
  browseLineSpace: number;
  /** Key display format: `Ebm` or `2A`. */
  keyDisplay: KeyDisplay;
  /** How the browser's Key column is ordered. */
  keySort: "alphabetical" | "musical";
  /** Full/Preview Waveform: the deck's overview, single-sided or mirrored. */
  overviewWaveform: OverviewWaveform;
  /** Media Browser › Explorer: the folders on disk in the tree. */
  explorer: boolean;
  /** Browser panel › Display Cue Markers on Preview. */
  previewCueMarkers: boolean;
  /** Browser panel › Display All Tracks in the Playlists. */
  allTracks: boolean;
  /** Browser panel › Display the number of tracks in a playlist on the Tree View. */
  playlistCounts: boolean;
  /**
   * The tree rail's shortcuts: playlists put there by Add To Shortcut, by
   * their ids, in the order they were added. Not a Preferences pane's
   * choice, but kept with them because it is the same kind of thing: how
   * this window is set up, for this person.
   */
  shortcuts: string[];
  /** Phrases › Phrase (Full Waveform): the phrase strip over the overview. */
  phraseFull: boolean;
  /** Phrases › Always show types of phrases: the labels in that strip. */
  phraseLabels: boolean;
  /** Vocal › Vocal (Full Waveform): the vocal strip under the overview. */
  vocalFull: boolean;
  /** Traffic Light: how far around the loaded track's key the browser lights. */
  trafficLight: TrafficLightReach;
  /** Color › Waveform color: the deck's palette — BLUE, RGB or 3Band. */
  waveformColor: WaveformColor;
  /** Color › HOT CUE color. */
  hotCueColor: HotCueColor;
  /** Display Type › Beat Count Display. */
  beatCount: BeatCount;
  /**
   * Display Type › Click on the waveform for PLAY and CUE. On, a click on
   * the enlarged waveform moves the playhead there, sets the cue there when
   * the deck is stopped, and plays; the window's switch is "Disable", so
   * this is stored the way round the deck reads it.
   */
  waveformClick: boolean;
}

export interface AudioPreferences {
  sampleRate: SampleRate;
  /** Frames per device buffer; one of `BUFFER_SIZES`. */
  bufferSize: number;
  metronomeSound: MetronomeSound;
  metronomeVolume: MetronomeVolume;
}

export type AnalysisMode = "rekordbox" | "rbxport";

export interface AnalysisPreferences {
  mode: AnalysisMode;
  concurrentTracks: number;
  /** Auto Analysis: analyse a track when it is added to the library. */
  auto: boolean;
  /** Add a memory cue on the first beat; also the Analysis Setting default. */
  firstBeatCue: boolean;
}

/**
 * DJ System: what a stick gets on its first export. A stick that already
 * carries settings keeps its own, which its device panel edits.
 */
export interface DjSystemPreferences {
  waveformColor: WaveformColor;
  waveformPosition: WaveformPosition;
  overviewWaveform: OverviewWaveform;
  keyDisplay: KeyDisplay;
  /** The browse categories, or null for rekordbox's reference rows. */
  categories: MenuSlot[] | null;
  /** The sort options, or null for the reference rows. */
  sorts: MenuSlot[] | null;
  /** `menuItem` of the sort option shown beside the track name; null for none. */
  subColumn: number | null;
  /**
   * Whether a new USB drive gets its PIONEER folder structure when it is
   * synced for the first time. Before that first sync its device settings
   * remain read-only.
   */
  createDatabaseFolders: boolean;
  /**
   * The OS name of the network interface PRO DJ LINK runs on (`en0`,
   * `Ethernet 2`), or null to take the one the players are reached through.
   */
  linkInterface: string | null;
  /** Start PRO DJ LINK when a player or mixer first appears on the network. */
  autoJoinLink: boolean;
  linkKeySort: "alphabetical" | "musical";
}

export const UPDATE_FREQUENCIES = ["start", "daily", "weekly"] as const;
export type UpdateFrequency = (typeof UPDATE_FREQUENCIES)[number];

export interface AdvancedPreferences {
  /** Auto Relocate Search Folders › Specified user folders. */
  relocateFolders: string[];
  /** Library Protection: refuse every edit, whatever rekordbox is doing. */
  protectLibrary: boolean;
  /** Edit Library › Double-click to edit; off is a click on a selected row. */
  doubleClickToEdit: boolean;
  syncType: SyncType;
  /** Allow BEAT/BPM SYNC with double/half BPM. */
  syncDoubleHalf: boolean;
  quantizeBeat: QuantizeBeat;
  /** Ask the download server for a newer version when the app starts. */
  checkUpdates: boolean;
  /** How often that automatic check runs: every start, once a day, once a week. */
  updateFrequency: UpdateFrequency;
  /** A track played for a minute goes on today's history and its count goes up. */
  recordHistory: boolean;
}

/** Keyboard: the keys changed from the preset, by binding id (`shortcuts.ts`). */
export interface KeyboardPreferences {
  overrides: Record<string, KeyChord>;
}

export interface Preferences {
  rekordbox: { syncBrowseSettings: boolean };
  view: ViewPreferences;
  audio: AudioPreferences;
  analysis: AnalysisPreferences;
  djSystem: DjSystemPreferences;
  advanced: AdvancedPreferences;
  keyboard: KeyboardPreferences;
  usbExport: {
    importSettings: boolean; importHistory: boolean; deleteUnlistedMusic: boolean; maximumCompatibility: boolean; conversionFormat: "wav" | "aiff" | "mp3";
    /** What Sync Manager's Import button has ticked when the window opens. */
    importButtonCues: boolean; importButtonHistory: boolean; importButtonSettings: boolean;
  };
}

export type PreferencePane = keyof Preferences;

/**
 * rekordbox's own defaults, as the captures of a fresh window show them
 * [OBS], except where the application differs on purpose: the browse scale
 * is the measured size, and a stick's rows default to the reference rows.
 */
export const DEFAULT_PREFERENCES: Preferences = {
  rekordbox: { syncBrowseSettings: true },
  view: {
    locale: "en",
    showBpmChanges: true,
    vuMeter: "normal",
    tempoSlider: false,
    tooltips: false,
    browseFontSize: BROWSE_SCALE_DEFAULT,
    browseBold: false,
    browseLineSpace: BROWSE_SCALE_DEFAULT,
    keyDisplay: "classic",
    keySort: "alphabetical",
    overviewWaveform: "half",
    explorer: true,
    previewCueMarkers: true,
    allTracks: true,
    playlistCounts: false,
    shortcuts: [],
    phraseFull: true,
    phraseLabels: true,
    vocalFull: true,
    trafficLight: "related3",
    waveformColor: "3band",
    hotCueColor: "colorful",
    beatCount: "position",
    waveformClick: true,
  },
  audio: {
    // The capture shows 96000 Hz and 512 samples [OBS]; the rate is what
    // that machine's device ran at, and 48000 is what most do.
    sampleRate: 48_000,
    bufferSize: 512,
    metronomeSound: 2,
    metronomeVolume: "large",
  },
  analysis: {
    mode: "rbxport",
    concurrentTracks: SLOTS,
    auto: false,
    firstBeatCue: false,
  },
  djSystem: {
    waveformColor: "3band",
    waveformPosition: "center",
    overviewWaveform: "half",
    keyDisplay: "classic",
    categories: null,
    sorts: null,
    subColumn: null,
    createDatabaseFolders: true,
    linkInterface: null,
    autoJoinLink: false,
    linkKeySort: "musical",
  },
  usbExport: { importSettings: false, importHistory: true, deleteUnlistedMusic: false, maximumCompatibility: false, conversionFormat: "wav", importButtonCues: true, importButtonHistory: true, importButtonSettings: false },
  advanced: {
    relocateFolders: [],
    protectLibrary: true,
    doubleClickToEdit: false,
    syncType: "beat",
    syncDoubleHalf: true,
    quantizeBeat: "1/1",
    checkUpdates: true,
    updateFrequency: "start",
    recordHistory: true,
  },
  keyboard: {
    overrides: {},
  },
};

/** `Ebm` as the library stores it, or `2A` when the window says Alphanumeric. */
export function formatKey(key: string, display: KeyDisplay): string {
  if (display !== "alphanumeric" || key === "") return key;
  // A key the wheel does not know — a blank, "Unknown", a typo — is shown as
  // it is rather than hidden.
  return toCamelot(key) || key;
}

/** The stored key changes that are chords: a key, and booleans for the rest. */
function chords(value: unknown): Record<string, KeyChord> {
  const out: Record<string, KeyChord> = {};
  if (typeof value !== "object" || value === null) return out;
  for (const [id, raw] of Object.entries(value)) {
    if (typeof raw !== "object" || raw === null) continue;
    const chord = raw as Partial<KeyChord>;
    if (typeof chord.key !== "string" || chord.key.length > 32) continue;
    const kept: KeyChord = { key: chord.key };
    if (chord.metaKey === true) kept.metaKey = true;
    if (chord.shiftKey === true) kept.shiftKey = true;
    if (chord.altKey === true) kept.altKey = true;
    out[id] = kept;
  }
  return out;
}

function bool(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function oneOf<T extends string>(value: unknown, choices: readonly T[], fallback: T): T {
  return choices.includes(value as T) ? (value as T) : fallback;
}

function step(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 && value < BROWSE_SCALE_STEPS
    ? value
    : fallback;
}

function strings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && item !== "");
}

/** The stored rows, only if every one is a row; anything else is the reference. */
function slots(value: unknown): MenuSlot[] | null {
  if (!Array.isArray(value) || value.length === 0) return null;
  const ok = value.every(
    (s: unknown) =>
      typeof s === "object" && s !== null &&
      typeof (s as MenuSlot).id === "number" && typeof (s as MenuSlot).menuItem === "number" &&
      typeof (s as MenuSlot).name === "string" && typeof (s as MenuSlot).seq === "number" &&
      typeof (s as MenuSlot).visible === "boolean",
  );
  return ok ? (value as MenuSlot[]) : null;
}

const KEY_DISPLAYS: readonly KeyDisplay[] = ["classic", "alphanumeric"];
const OVERVIEWS: readonly OverviewWaveform[] = ["half", "full"];
const WAVEFORM_COLORS: readonly WaveformColor[] = ["blue", "rgb", "3band"];
const POSITIONS: readonly WaveformPosition[] = ["center", "left"];
const SYNC_TYPES: readonly SyncType[] = ["beat", "bpm"];
const HOT_CUE_COLORS: readonly HotCueColor[] = ["colorful", "cdj"];
const BEAT_COUNTS: readonly BeatCount[] = ["position", "toMemoryBars", "toMemoryBeats"];
const METRONOME_SOUNDS: readonly MetronomeSound[] = [1, 2, 3];
const METRONOME_VOLUMES: readonly MetronomeVolume[] = ["small", "middle", "large"];

function oneOfNumber<T extends number>(value: unknown, choices: readonly T[], fallback: T): T {
  return choices.includes(value as T) ? (value as T) : fallback;
}
const REACHES: readonly TrafficLightReach[] = ["same", "related1", "related2", "related3"];

type Raw<T> = Partial<Record<keyof T, unknown>>;

function part<T>(value: unknown): Raw<T> {
  return typeof value === "object" && value !== null ? value : {};
}

/** Turns whatever was stored into a set the application can run on. */
export function sanitisePreferences(value: unknown): Preferences {
  const raw = part<Preferences>(value);
  const hasStoredSection = Object.values(raw).some(section =>
    typeof section === "object" && section !== null && !Array.isArray(section)
  );
  const view = part<ViewPreferences>(raw.view);
  const audio = part<AudioPreferences>(raw.audio);
  const analysis = part<AnalysisPreferences>(raw.analysis);
  const dj = part<DjSystemPreferences>(raw.djSystem);
  const advanced = part<AdvancedPreferences>(raw.advanced);
  const keyboard = part<KeyboardPreferences>(raw.keyboard);
  const usb = part<Preferences["usbExport"]>(raw.usbExport);
  const rekordbox = part<Preferences["rekordbox"]>(raw.rekordbox);
  const d = DEFAULT_PREFERENCES;
  return {
    rekordbox: { syncBrowseSettings: bool(rekordbox.syncBrowseSettings, true) },
    usbExport: { importSettings: bool(usb.importSettings, false), importHistory: bool(usb.importHistory, true), deleteUnlistedMusic: bool(usb.deleteUnlistedMusic, false), maximumCompatibility: bool(usb.maximumCompatibility, false), conversionFormat: oneOf(usb.conversionFormat, ["wav", "aiff", "mp3"] as const, "wav"), importButtonCues: bool(usb.importButtonCues, true), importButtonHistory: bool(usb.importButtonHistory, true), importButtonSettings: bool(usb.importButtonSettings, false) },
    view: {
      locale: oneOf(view.locale, LOCALES, d.view.locale),
      showBpmChanges: bool(view.showBpmChanges, d.view.showBpmChanges),
      vuMeter: oneOf(view.vuMeter, ["normal", "fabulous"] as const, d.view.vuMeter),
      tempoSlider: bool(view.tempoSlider, d.view.tempoSlider),
      tooltips: bool(view.tooltips, d.view.tooltips),
      browseFontSize: step(view.browseFontSize, d.view.browseFontSize),
      browseBold: bool(view.browseBold, d.view.browseBold),
      browseLineSpace: step(view.browseLineSpace, d.view.browseLineSpace),
      keyDisplay: oneOf(view.keyDisplay, KEY_DISPLAYS, d.view.keyDisplay),
      keySort: oneOf(
        view.keySort,
        ["alphabetical", "musical"] as const,
        view.keyDisplay === "alphanumeric" ? "musical" : d.view.keySort,
      ),
      overviewWaveform: oneOf(view.overviewWaveform, OVERVIEWS, d.view.overviewWaveform),
      explorer: bool(view.explorer, d.view.explorer),
      previewCueMarkers: bool(view.previewCueMarkers, d.view.previewCueMarkers),
      allTracks: bool(view.allTracks, d.view.allTracks),
      playlistCounts: bool(view.playlistCounts, d.view.playlistCounts),
      shortcuts: strings(view.shortcuts),
      phraseFull: bool(view.phraseFull, d.view.phraseFull),
      phraseLabels: bool(view.phraseLabels, d.view.phraseLabels),
      vocalFull: bool(view.vocalFull, d.view.vocalFull),
      trafficLight: oneOf(view.trafficLight, REACHES, d.view.trafficLight),
      waveformColor: oneOf(view.waveformColor, WAVEFORM_COLORS, d.view.waveformColor),
      hotCueColor: oneOf(view.hotCueColor, HOT_CUE_COLORS, d.view.hotCueColor),
      beatCount: oneOf(view.beatCount, BEAT_COUNTS, d.view.beatCount),
      waveformClick: bool(view.waveformClick, d.view.waveformClick),
    },
    audio: {
      sampleRate: oneOfNumber(audio.sampleRate, SAMPLE_RATES, d.audio.sampleRate),
      bufferSize: oneOfNumber(audio.bufferSize, BUFFER_SIZES, d.audio.bufferSize),
      metronomeSound: oneOfNumber(audio.metronomeSound, METRONOME_SOUNDS, d.audio.metronomeSound),
      metronomeVolume: oneOf(audio.metronomeVolume, METRONOME_VOLUMES, d.audio.metronomeVolume),
    },
    analysis: {
      mode: oneOf(analysis.mode, ["rekordbox", "rbxport"], d.analysis.mode),
      concurrentTracks: oneOfNumber(analysis.concurrentTracks, ANALYSIS_SLOTS, SLOTS),
      auto: bool(analysis.auto, d.analysis.auto),
      firstBeatCue: bool(analysis.firstBeatCue, d.analysis.firstBeatCue),
    },
    djSystem: {
      waveformColor: oneOf(dj.waveformColor, WAVEFORM_COLORS, d.djSystem.waveformColor),
      waveformPosition: oneOf(dj.waveformPosition, POSITIONS, d.djSystem.waveformPosition),
      overviewWaveform: oneOf(dj.overviewWaveform, OVERVIEWS, d.djSystem.overviewWaveform),
      keyDisplay: oneOf(dj.keyDisplay, KEY_DISPLAYS, d.djSystem.keyDisplay),
      categories: slots(dj.categories),
      sorts: slots(dj.sorts),
      subColumn: typeof dj.subColumn === "number" && Number.isInteger(dj.subColumn)
        ? dj.subColumn
        : null,
      createDatabaseFolders: bool(dj.createDatabaseFolders, d.djSystem.createDatabaseFolders),
      autoJoinLink: bool(dj.autoJoinLink, d.djSystem.autoJoinLink),
      linkKeySort: oneOf(dj.linkKeySort, ["alphabetical", "musical"] as const, d.djSystem.linkKeySort),
      linkInterface: typeof dj.linkInterface === "string" && dj.linkInterface !== "" ? dj.linkInterface : null,
    },
    advanced: {
      relocateFolders: strings(advanced.relocateFolders),
      recordHistory: bool(advanced.recordHistory, d.advanced.recordHistory),
      // A stored object predates this switch when the key is absent. Preserve
      // that user's writable library; only a truly empty store gets defaults.
      protectLibrary: bool(advanced.protectLibrary, hasStoredSection ? false : d.advanced.protectLibrary),
      doubleClickToEdit: bool(advanced.doubleClickToEdit, d.advanced.doubleClickToEdit),
      syncType: oneOf(advanced.syncType, SYNC_TYPES, d.advanced.syncType),
      syncDoubleHalf: bool(advanced.syncDoubleHalf, d.advanced.syncDoubleHalf),
      quantizeBeat: oneOf(advanced.quantizeBeat, QUANTIZE_BEATS, d.advanced.quantizeBeat),
      checkUpdates: bool(advanced.checkUpdates, d.advanced.checkUpdates),
      updateFrequency: oneOf(advanced.updateFrequency, UPDATE_FREQUENCIES, d.advanced.updateFrequency),
    },
    keyboard: {
      overrides: chords(keyboard.overrides),
    },
  };
}

export const PREFERENCES_KEY = "rbl.preferences";
const KEY = PREFERENCES_KEY;

/** Reads the stored preferences, falling back to the defaults on anything odd. */
export function loadPreferences(): Preferences {
  try {
    const raw = localStorage.getItem(KEY);
    return raw === null ? DEFAULT_PREFERENCES : sanitisePreferences(JSON.parse(raw));
  } catch {
    // A private window, cleared storage, or a browser that refuses it: the
    // defaults are a working window, so there is nothing to report.
    return DEFAULT_PREFERENCES;
  }
}

export function savePreferences(preferences: Preferences): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(preferences));
  } catch {
    // Not being able to remember a preference is not worth interrupting anyone.
  }
}
