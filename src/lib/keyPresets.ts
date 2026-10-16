/**
 * rekordbox's two Performance presets, Performance 1 (Preset) and
 * Performance 2 (Preset): every command either of them gives a key.
 *
 * [OBS static] rekordbox 7.2.19 (macOS arm64). The rows are what
 * `djplay::UiPlayer`, `MainComponent`, `SamplerControlPanel`,
 * `RecControlPanel` and `RekordboxApplication`'s `getCommandInfo` add with
 * `addDefaultKeypress` while `ApplicationMode` is PERFORMANCE, read by
 * running those functions (and `getAllCommands`, for the ids and their
 * order) under an emulator once per preset. `isPresetPerformance` is
 * Performance 1 (timestamp 0000000000000) and `isPresetSerato` is
 * Performance 2 (0000000000010) [`KeyMappingFileManager` @0x101bcc0fc,
 * @0x101bcc198, and the `Config` table its static initializer builds].
 * Decks 3 and 4 get no key in either preset; `None (Preset)` gives none at
 * all. The evidence and the emulator are in the PR for #412.
 *
 * [OBS] AlphaTheta's Default Keyboard Shortcut Reference (rekordbox 7.0.5)
 * lists 220 of these keys and agrees on all but three: it prints Deck 2's
 * Master Tempo in Performance 1 as `shift + O` where the binary has
 * `shift + 0`, and Pause slot 10 and 11 as `<` and `>`, which are shift +
 * comma and period. It gives Performance 1's Playback, Previous and Next
 * Sequence a second key each (`command + Q`, `S`, `D`) that the 7.2.19
 * binary does not add [SamplerControlPanel::getCommandInfo @0x100b1682c],
 * and predates `Hide Title, Artist, and Artwork on Decks`. The reference says these presets are PERFORMANCE mode's alone:
 * EXPORT mode always runs `Export (Preset)`, which cannot be edited
 * [`KeyMappingFileManager::getCurrentKeyMappingFile` @0x101bcaae0].
 *
 * Keys are spelled as the pane prints them on macOS; `command` and `option`
 * are ctrl and alt on Windows [OBS: the reference's "ctrl/command" and
 * "alt/option"]. The one key on macOS's control key, Search for tracks in
 * Collections, is alt + F on Windows [OBS: the reference]; [UNKNOWN] the
 * Windows build was not available to read.
 */
import { BINDINGS, sameChord, type Binding, type KeyChord, type KeyOverrides } from "./shortcuts";

/** The presets rekordbox's Keyboard pane offers, in `PresetKeyMapping::getPresetList`'s order. */
export const KEY_PRESETS = ["performance1", "performance2", "export", "none"] as const;
export type KeyPreset = (typeof KEY_PRESETS)[number];

/** Each preset's name as rekordbox's dropdown shows it. */
export const KEY_PRESET_NAMES: Readonly<Record<KeyPreset, string>> = {
  performance1: "Performance 1 (Preset)",
  performance2: "Performance 2 (Preset)",
  export: "Export (Preset)",
  none: "None (Preset)",
};

export interface PerformanceKeyRow {
  /** rekordbox's command id; a deck's are `30xx` for Deck 1 and `31xx` for Deck 2. */
  id: string;
  /** The group rekordbox files it under in PERFORMANCE mode. */
  group: string;
  /** rekordbox's name for the command, in English. */
  name: string;
  /** Its keys in Performance 1 and Performance 2, in the order rekordbox adds them. */
  p1: readonly string[];
  p2: readonly string[];
}

export const PERFORMANCE_KEYMAP: readonly PerformanceKeyRow[] = [
  { id: "3006", group: "Deck 1", name: "Play/Pause", p1: ["Z"], p2: ["W"] },
  { id: "3007", group: "Deck 1", name: "Cue", p1: ["A"], p2: ["I"] },
  { id: "3057", group: "Deck 1", name: "Return to the beginning of the track", p1: ["shift + A"], p2: [] },
  { id: "3008", group: "Deck 1", name: "Jump Forward", p1: ["command + T"], p2: [] },
  { id: "3009", group: "Deck 1", name: "Jump Reverse", p1: ["command + R"], p2: [] },
  { id: "304b", group: "Deck 1", name: "BEAT SYNC", p1: ["Q"], p2: [] },
  { id: "304c", group: "Deck 1", name: "Beat Sync Master", p1: ["shift + Q"], p2: [] },
  { id: "304d", group: "Deck 1", name: "Master Tempo", p1: ["shift + 5"], p2: ["F5"] },
  { id: "304f", group: "Deck 1", name: "Pitch Bend +", p1: ["E"], p2: ["Y"] },
  { id: "3050", group: "Deck 1", name: "Pitch Bend -", p1: ["W"], p2: ["T"] },
  { id: "305a", group: "Deck 1", name: "Increase Tempo (small)", p1: [], p2: ["R"] },
  { id: "305b", group: "Deck 1", name: "Increase Tempo (medium)", p1: ["shift + E"], p2: [] },
  { id: "305d", group: "Deck 1", name: "Decrease Tempo (small)", p1: [], p2: ["E"] },
  { id: "305e", group: "Deck 1", name: "Decrease Tempo (medium)", p1: ["shift + W"], p2: [] },
  { id: "3060", group: "Deck 1", name: "Reverse", p1: [], p2: ["Q"] },
  { id: "3061", group: "Deck 1", name: "Slip Reverse", p1: ["S"], p2: ["U"] },
  { id: "301a", group: "Deck 1", name: "Next Track", p1: ["option + A"], p2: ["option + W"] },
  { id: "301b", group: "Deck 1", name: "Previous Track", p1: ["option + Q"], p2: ["option + Q"] },
  { id: "3024", group: "Deck 1", name: "Set Memory Cue", p1: ["shift + T"], p2: [] },
  { id: "303b", group: "Deck 1", name: "Delete Memory Cue", p1: ["shift + R"], p2: [] },
  { id: "3039", group: "Deck 1", name: "Call Next Memory Cue", p1: ["T"], p2: [] },
  { id: "303a", group: "Deck 1", name: "Call Previous Memory Cue", p1: ["R"], p2: [] },
  { id: "301e", group: "Deck 1", name: "Set Hot Cue A", p1: ["1"], p2: ["1"] },
  { id: "301f", group: "Deck 1", name: "Set Hot Cue B", p1: ["2"], p2: ["2"] },
  { id: "3020", group: "Deck 1", name: "Set Hot Cue C", p1: ["3"], p2: ["3"] },
  { id: "3054", group: "Deck 1", name: "Set Hot Cue D", p1: ["4"], p2: ["4"] },
  { id: "307f", group: "Deck 1", name: "Set Hot Cue E", p1: [], p2: ["5"] },
  { id: "3021", group: "Deck 1", name: "Clear Hot Cue A", p1: ["shift + command + 1"], p2: ["shift + command + 1"] },
  { id: "3022", group: "Deck 1", name: "Clear Hot Cue B", p1: ["shift + command + 2"], p2: ["shift + command + 2"] },
  { id: "3023", group: "Deck 1", name: "Clear Hot Cue C", p1: ["shift + command + 3"], p2: ["shift + command + 3"] },
  { id: "3055", group: "Deck 1", name: "Clear Hot Cue D", p1: ["shift + command + 4"], p2: ["shift + command + 4"] },
  { id: "308b", group: "Deck 1", name: "Clear Hot Cue E", p1: [], p2: ["shift + command + 5"] },
  { id: "3018", group: "Deck 1", name: "Halve the loop", p1: ["shift + D"], p2: [] },
  { id: "3019", group: "Deck 1", name: "Double the loop", p1: ["shift + G"], p2: [] },
  { id: "30a5", group: "Deck 1", name: "Auto Beat Loop ON/OFF", p1: ["shift + F"], p2: [] },
  { id: "300a", group: "Deck 1", name: "Loop In", p1: ["D"], p2: ["O"] },
  { id: "300b", group: "Deck 1", name: "Loop Out", p1: ["F"], p2: ["P"] },
  { id: "300c", group: "Deck 1", name: "Exit/Reloop", p1: ["G"], p2: ["option + @"] },
  { id: "300f", group: "Deck 1", name: "1/8 Beat Loop", p1: [], p2: ["option + 1"] },
  { id: "3010", group: "Deck 1", name: "1/4 Beat Loop", p1: [], p2: ["option + 2"] },
  { id: "3011", group: "Deck 1", name: "1/2 Beat Loop", p1: ["command + 1"], p2: ["option + 3"] },
  { id: "3012", group: "Deck 1", name: "1 Beat Loop", p1: ["command + 2"], p2: ["option + 4"] },
  { id: "3013", group: "Deck 1", name: "2 Beat Loop", p1: ["command + 3"], p2: ["option + 5"] },
  { id: "3014", group: "Deck 1", name: "4 Beat Loop", p1: ["command + 4"], p2: [] },
  { id: "3015", group: "Deck 1", name: "8 Beat Loop", p1: ["command + 5"], p2: [] },
  { id: "30a7", group: "Deck 1", name: "Metronome Volume", p1: ["option + 1"], p2: [] },
  { id: "3045", group: "Deck 1", name: "Shift beatgrid to the center", p1: ["option + 3"], p2: [] },
  { id: "3043", group: "Deck 1", name: "Fine Shift Beatgrid right", p1: ["option + 4"], p2: [] },
  { id: "3044", group: "Deck 1", name: "Fine Shift Beatgrid left", p1: ["option + 2"], p2: [] },
  { id: "3046", group: "Deck 1", name: "Expand beat intervals", p1: ["option + E"], p2: [] },
  { id: "3047", group: "Deck 1", name: "Shrink beat intervals", p1: ["option + W"], p2: [] },
  { id: "30ac", group: "Deck 1", name: "Halve the BPM value", p1: [], p2: ["option + cursor down"] },
  { id: "30ad", group: "Deck 1", name: "Double the BPM value", p1: [], p2: ["option + cursor up"] },
  { id: "30af", group: "Deck 1", name: "Show GRID Panel", p1: ["option + G"], p2: ["option + spacebar"] },
  { id: "3000", group: "All Decks", name: "Zoom In", p1: ["=", "shift + -"], p2: ["shift + -"] },
  { id: "3001", group: "All Decks", name: "Zoom Out", p1: ["-"], p2: ["-"] },
  { id: "3002", group: "All Decks", name: "Reset Zoom", p1: ["shift + ="], p2: [] },
  { id: "30b2", group: "Deck 1", name: "Switch to [ABS]", p1: [], p2: ["F1"] },
  { id: "30b3", group: "Deck 1", name: "Switch to [REL]", p1: [], p2: ["F2"] },
  { id: "30b4", group: "Deck 1", name: "Switch to [INT]", p1: [], p2: ["F3"] },
  { id: "30b5", group: "Deck 1", name: "Switch to [THR]", p1: [], p2: ["F4"] },
  { id: "3106", group: "Deck 2", name: "Play/Pause", p1: ["N"], p2: ["S"] },
  { id: "3107", group: "Deck 2", name: "Cue", p1: ["H"], p2: ["K"] },
  { id: "3157", group: "Deck 2", name: "Return to the beginning of the track", p1: ["shift + H"], p2: [] },
  { id: "3108", group: "Deck 2", name: "Jump Forward", p1: ["command + P"], p2: [] },
  { id: "3109", group: "Deck 2", name: "Jump Reverse", p1: ["command + O"], p2: [] },
  { id: "314b", group: "Deck 2", name: "BEAT SYNC", p1: ["Y"], p2: [] },
  { id: "314c", group: "Deck 2", name: "Beat Sync Master", p1: ["shift + Y"], p2: [] },
  { id: "314d", group: "Deck 2", name: "Master Tempo", p1: ["shift + 0"], p2: ["F10"] },
  { id: "314f", group: "Deck 2", name: "Pitch Bend +", p1: ["I"], p2: ["H"] },
  { id: "3150", group: "Deck 2", name: "Pitch Bend -", p1: ["U"], p2: ["G"] },
  { id: "315a", group: "Deck 2", name: "Increase Tempo (small)", p1: [], p2: ["F"] },
  { id: "315b", group: "Deck 2", name: "Increase Tempo (medium)", p1: ["shift + I"], p2: [] },
  { id: "315d", group: "Deck 2", name: "Decrease Tempo (small)", p1: [], p2: ["D"] },
  { id: "315e", group: "Deck 2", name: "Decrease Tempo (medium)", p1: ["shift + U"], p2: [] },
  { id: "3160", group: "Deck 2", name: "Reverse", p1: [], p2: ["A"] },
  { id: "3161", group: "Deck 2", name: "Slip Reverse", p1: ["J"], p2: ["J"] },
  { id: "311a", group: "Deck 2", name: "Next Track", p1: ["option + H"], p2: ["option + S"] },
  { id: "311b", group: "Deck 2", name: "Previous Track", p1: ["option + Y"], p2: ["option + A"] },
  { id: "3124", group: "Deck 2", name: "Set Memory Cue", p1: ["shift + P"], p2: [] },
  { id: "313b", group: "Deck 2", name: "Delete Memory Cue", p1: ["shift + O"], p2: [] },
  { id: "3139", group: "Deck 2", name: "Call Next Memory Cue", p1: ["P"], p2: [] },
  { id: "313a", group: "Deck 2", name: "Call Previous Memory Cue", p1: ["O"], p2: [] },
  { id: "311e", group: "Deck 2", name: "Set Hot Cue A", p1: ["6"], p2: ["6"] },
  { id: "311f", group: "Deck 2", name: "Set Hot Cue B", p1: ["7"], p2: ["7"] },
  { id: "3120", group: "Deck 2", name: "Set Hot Cue C", p1: ["8"], p2: ["8"] },
  { id: "3154", group: "Deck 2", name: "Set Hot Cue D", p1: ["9"], p2: ["9"] },
  { id: "317f", group: "Deck 2", name: "Set Hot Cue E", p1: [], p2: ["0"] },
  { id: "3121", group: "Deck 2", name: "Clear Hot Cue A", p1: ["shift + command + 6"], p2: ["shift + command + 6"] },
  { id: "3122", group: "Deck 2", name: "Clear Hot Cue B", p1: ["shift + command + 7"], p2: ["shift + command + 7"] },
  { id: "3123", group: "Deck 2", name: "Clear Hot Cue C", p1: ["shift + command + 8"], p2: ["shift + command + 8"] },
  { id: "3155", group: "Deck 2", name: "Clear Hot Cue D", p1: ["shift + command + 9"], p2: ["shift + command + 9"] },
  { id: "318b", group: "Deck 2", name: "Clear Hot Cue E", p1: [], p2: ["shift + command + 0"] },
  { id: "3118", group: "Deck 2", name: "Halve the loop", p1: ["shift + K"], p2: [] },
  { id: "3119", group: "Deck 2", name: "Double the loop", p1: ["shift + :"], p2: [] },
  { id: "31a5", group: "Deck 2", name: "Auto Beat Loop ON/OFF", p1: ["shift + L"], p2: [] },
  { id: "310a", group: "Deck 2", name: "Loop In", p1: ["K"], p2: ["L"] },
  { id: "310b", group: "Deck 2", name: "Loop Out", p1: ["L"], p2: [";"] },
  { id: "310c", group: "Deck 2", name: "Exit/Reloop", p1: [";"], p2: ["option + :"] },
  { id: "310f", group: "Deck 2", name: "1/8 Beat Loop", p1: [], p2: ["option + 6"] },
  { id: "3110", group: "Deck 2", name: "1/4 Beat Loop", p1: [], p2: ["option + 7"] },
  { id: "3111", group: "Deck 2", name: "1/2 Beat Loop", p1: ["command + 6"], p2: ["option + 8"] },
  { id: "3112", group: "Deck 2", name: "1 Beat Loop", p1: ["command + 7"], p2: ["option + 9"] },
  { id: "3113", group: "Deck 2", name: "2 Beat Loop", p1: ["command + 8"], p2: ["option + 0"] },
  { id: "3114", group: "Deck 2", name: "4 Beat Loop", p1: ["command + 9"], p2: [] },
  { id: "3115", group: "Deck 2", name: "8 Beat Loop", p1: ["command + 0"], p2: [] },
  { id: "31a7", group: "Deck 2", name: "Metronome Volume", p1: ["option + 6"], p2: [] },
  { id: "3145", group: "Deck 2", name: "Shift beatgrid to the center", p1: ["option + 8"], p2: [] },
  { id: "3143", group: "Deck 2", name: "Fine Shift Beatgrid right", p1: ["option + 9"], p2: [] },
  { id: "3144", group: "Deck 2", name: "Fine Shift Beatgrid left", p1: ["option + 7"], p2: [] },
  { id: "3146", group: "Deck 2", name: "Expand beat intervals", p1: ["option + I"], p2: [] },
  { id: "3147", group: "Deck 2", name: "Shrink beat intervals", p1: ["option + U"], p2: [] },
  { id: "31af", group: "Deck 2", name: "Show GRID Panel", p1: ["option + ;"], p2: [] },
  { id: "31b2", group: "Deck 2", name: "Switch to [ABS]", p1: [], p2: ["F6"] },
  { id: "31b3", group: "Deck 2", name: "Switch to [REL]", p1: [], p2: ["F7"] },
  { id: "31b4", group: "Deck 2", name: "Switch to [INT]", p1: [], p2: ["F8"] },
  { id: "31b5", group: "Deck 2", name: "Switch to [THR]", p1: [], p2: ["F9"] },
  { id: "7000", group: "General", name: "Search for tracks in Collections", p1: ["ctrl + F"], p2: ["ctrl + F"] },
  { id: "7003", group: "General", name: "Search for tracks in the track list", p1: ["command + F"], p2: ["command + F"] },
  { id: "7005", group: "General", name: "Search playlists/folders", p1: ["shift + ctrl + F"], p2: ["shift + ctrl + F"] },
  { id: "b04d", group: "View", name: "Toggle the layout (decks and browse screen)", p1: ["spacebar"], p2: ["spacebar"] },
  { id: "b05a", group: "View", name: "Hide Title, Artist, and Artwork on Decks", p1: ["option + /"], p2: ["option + /"] },
  { id: "400d", group: "Track", name: "Show in Finder", p1: [], p2: ["command + R"] },
  { id: "500a", group: "Playlist", name: "Create New Playlist", p1: ["command + N"], p2: [] },
  { id: "b129", group: "Browse", name: "Load track to Deck 1", p1: ["shift + cursor left"], p2: [] },
  { id: "b12a", group: "Browse", name: "Load track to Deck 2", p1: ["shift + cursor right"], p2: [] },
  { id: "b12d", group: "Browse", name: "Load track to Left Deck", p1: [], p2: ["shift + cursor left"] },
  { id: "b12e", group: "Browse", name: "Load track to Right Deck", p1: [], p2: ["shift + cursor right"] },
  { id: "b12f", group: "Browse", name: "Instant Doubles Deck 1", p1: ["shift + command + cursor left"], p2: [] },
  { id: "b130", group: "Browse", name: "Instant Doubles Deck 2", p1: ["shift + command + cursor right"], p2: [] },
  { id: "b133", group: "Browse", name: "Instant Doubles Left Deck", p1: [], p2: ["shift + command + cursor left"] },
  { id: "b134", group: "Browse", name: "Instant Doubles Right Deck", p1: [], p2: ["shift + command + cursor right"] },
  { id: "b137", group: "Browse", name: "Undo track load", p1: ["command + Z"], p2: ["command + Z"] },
  { id: "b11b", group: "Browse", name: "Switch to Tag List", p1: ["shift + command + L"], p2: [] },
  { id: "b139", group: "Browse", name: "Locate track loaded on deck", p1: ["command + L"], p2: ["command + L"] },
  { id: "b103", group: "Browse", name: "Information Window", p1: ["command + I"], p2: ["command + I"] },
  { id: "b04c", group: "Browse", name: "Show Split Screen", p1: ["`"], p2: ["`"] },
  { id: "b138", group: "Browse", name: "Display in bold", p1: ["command + B"], p2: ["command + B"] },
  { id: "f000", group: "Sampler", name: "Playback slot 1", p1: ["X"], p2: ["Z"] },
  { id: "f001", group: "Sampler", name: "Playback slot 2", p1: ["C"], p2: ["X"] },
  { id: "f002", group: "Sampler", name: "Playback slot 3", p1: ["V"], p2: ["C"] },
  { id: "f003", group: "Sampler", name: "Playback slot 4", p1: ["B"], p2: ["V"] },
  { id: "f004", group: "Sampler", name: "Playback slot 5", p1: [], p2: ["B"] },
  { id: "f005", group: "Sampler", name: "Playback slot 6", p1: [], p2: ["N"] },
  { id: "f008", group: "Sampler", name: "Playback slot 9", p1: ["M"], p2: [] },
  { id: "f009", group: "Sampler", name: "Playback slot 10", p1: [","], p2: [] },
  { id: "f00a", group: "Sampler", name: "Playback slot 11", p1: ["."], p2: [] },
  { id: "f00b", group: "Sampler", name: "Playback slot 12", p1: ["/"], p2: [] },
  { id: "f010", group: "Sampler", name: "Pause slot 1", p1: ["shift + X"], p2: ["option + Z"] },
  { id: "f011", group: "Sampler", name: "Pause slot 2", p1: ["shift + C"], p2: ["option + X"] },
  { id: "f012", group: "Sampler", name: "Pause slot 3", p1: ["shift + V"], p2: ["option + C"] },
  { id: "f013", group: "Sampler", name: "Pause slot 4", p1: ["shift + B"], p2: ["option + V"] },
  { id: "f014", group: "Sampler", name: "Pause slot 5", p1: [], p2: ["option + B"] },
  { id: "f015", group: "Sampler", name: "Pause slot 6", p1: [], p2: ["option + N"] },
  { id: "f018", group: "Sampler", name: "Pause slot 9", p1: ["shift + M"], p2: [] },
  { id: "f019", group: "Sampler", name: "Pause slot 10", p1: ["shift + ,"], p2: [] },
  { id: "f01a", group: "Sampler", name: "Pause slot 11", p1: ["shift + ."], p2: [] },
  { id: "f01b", group: "Sampler", name: "Pause slot 12", p1: ["shift + /"], p2: [] },
  { id: "f03d", group: "Sampler", name: "Playback Sequence", p1: ["command + Y"], p2: [] },
  { id: "f03f", group: "Sampler", name: "Previous Sequence", p1: ["command + J"], p2: [] },
  { id: "f040", group: "Sampler", name: "Next Sequence", p1: ["command + K"], p2: [] },
  { id: "d0f0", group: "Recordings", name: "Start/Pause REC", p1: ["command + return"], p2: ["command + N"] },
  { id: "1001", group: "File", name: "Exit", p1: [], p2: ["escape"] },
];

const BY_ID: ReadonlyMap<string, PerformanceKeyRow> = new Map(PERFORMANCE_KEYMAP.map((row) => [row.id, row]));

/** The keys a Performance preset gives a command, as the pane spells them. */
export function performanceKeys(preset: KeyPreset, command: string): readonly string[] {
  const row = BY_ID.get(command);
  if (row === undefined) return [];
  if (preset === "performance1") return row.p1;
  if (preset === "performance2") return row.p2;
  return [];
}

const SPELLED_KEYS: Readonly<Record<string, string>> = {
  spacebar: " ",
  return: "Enter",
  escape: "Escape",
  "cursor left": "ArrowLeft",
  "cursor right": "ArrowRight",
  "cursor up": "ArrowUp",
  "cursor down": "ArrowDown",
};

/**
 * A key as the pane spells it (`shift + command + 1`) as a chord, or `null`
 * for one this app cannot bind: macOS's control key, which `metaKey` does not
 * stand for.
 */
export function chordFromText(text: string): KeyChord | null {
  const parts = text.split(" + ");
  const name = parts.pop() ?? "";
  if (name === "") return null;
  const chord: KeyChord = { key: SPELLED_KEYS[name] ?? (name.length === 1 ? name.toLowerCase() : name) };
  for (const modifier of parts) {
    if (modifier === "shift") chord.shiftKey = true;
    else if (modifier === "command") chord.metaKey = true;
    else if (modifier === "option") chord.altKey = true;
    else return null;
  }
  return chord;
}

/**
 * Keys the shell's menu answers before the page sees them, whatever the
 * preset: its accelerators (the `Menu` rows), Import Folder, Undo and Redo,
 * and the items macOS supplies (Quit, Close Window, Hide, Cut, Copy, Paste).
 * rekordbox's PERFORMANCE mode has no menu on these keys, so a preset row on
 * one of them could never fire here.
 */
const SHELL_CHORDS: readonly KeyChord[] = [
  ...BINDINGS.filter((binding) => binding.group === "Menu").map((binding) => binding.chord),
  ...["z", "q", "w", "h", "x", "c", "v"].map((key): KeyChord => ({ key, metaKey: true })),
  { key: "z", metaKey: true, shiftKey: true },
  { key: "o", metaKey: true, shiftKey: true },
];

/** Whether the shell's menu takes a chord before the page can. */
export function heldByShell(chord: KeyChord): boolean {
  return chord.key !== "" && SHELL_CHORDS.some((held) => sameChord(held, chord));
}

/**
 * What a binding's key is in a preset, before the person's own: the Export
 * preset's is the binding's own; a Performance preset's is the first key it
 * gives the same rekordbox command, or none, and None gives none. This
 * app's own rows and the menu's keep their keys in every preset. `shell` is
 * true when the preset's key is one the menu holds here, so the row is left
 * without it.
 */
export function presetChord(binding: Binding, preset: KeyPreset): { chord: KeyChord; shell: boolean } {
  if (preset === "export" || binding.command === undefined || binding.action === undefined) {
    return { chord: binding.chord, shell: false };
  }
  const first = performanceKeys(preset, binding.command)[0];
  const chord = first === undefined ? null : chordFromText(first);
  if (chord === null) return { chord: { key: "" }, shell: false };
  if (heldByShell(chord)) return { chord: { key: "" }, shell: true };
  return { chord, shell: false };
}

/** The Keyboard preferences the map is made from. */
export interface KeyboardChoice {
  preset: KeyPreset;
  overrides: KeyOverrides;
}

const maps = new WeakMap<KeyboardChoice, KeyOverrides>();

/**
 * Every key the preset and the person change, by binding id, for the key
 * handlers to read: the preset's keys, then the person's own on top. A key
 * the person gave one row is taken from any row the preset gave it to, so a
 * key never does two things after the preset changes.
 */
export function keyMap(keyboard: KeyboardChoice): KeyOverrides {
  const cached = maps.get(keyboard);
  if (cached !== undefined) return cached;
  const own = keyboard.overrides;
  const out: Record<string, KeyChord> = {};
  for (const binding of BINDINGS) {
    if (binding.action === undefined) continue;
    const { chord } = presetChord(binding, keyboard.preset);
    if (chord.key !== binding.chord.key || !sameChord(chord, binding.chord)) out[binding.id] = chord;
  }
  for (const chord of Object.values(own)) {
    if (chord.key === "") continue;
    for (const binding of BINDINGS) {
      if (binding.action === undefined || binding.id in own) continue;
      const held = out[binding.id] ?? binding.chord;
      if (held.key !== "" && sameChord(held, chord)) out[binding.id] = { key: "" };
    }
  }
  const map = { ...out, ...own };
  maps.set(keyboard, map);
  return map;
}
