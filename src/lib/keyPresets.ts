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
 * are ctrl and alt on Windows. That holds [OBS] for the 220 entries the
 * reference prints as "ctrl/command" and "alt/option"; for every other row it
 * is [ASSUME] from JUCE's `ModifierKeys::commandModifier`, which is ctrl on
 * Windows. The one key on macOS's control key, Search for tracks in
 * Collections, is alt + F on Windows [OBS: the reference]. [UNKNOWN] the
 * Windows binary as a whole: it was not available to read, so no Windows
 * key here was read from rekordbox itself.
 *
 * The fixed-distance beat jumps (`Forward 1/2 beat` to `Backward 8 bars`,
 * 3097-30a4 and 3197-31a4) are the one exception to "every command either
 * preset gives a key": they are listed with no key in either preset, since
 * [UNKNOWN] what rekordbox's defaults for them are. The emulator run above
 * did not record them; a person's edited Performance 1 file names six of
 * them [OBS: see `FIXED_JUMPS` in `shortcuts.ts`] but with that person's
 * keys, which are not the preset's. The rows are here so the pane and
 * `presetChord` treat them as rekordbox's, not this app's own.
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

// Names and grouping describe the reference, not the key lookup. Keep them
// in the test fixture so menu/Keyboard command additions do not ship unused data.
type KeyData = string | readonly string[] | null;
const PERFORMANCE_KEYS: readonly (readonly [string, KeyData, KeyData?])[] = [
  ["3006", "Z", "W"],
  ["3007", "A", "I"],
  ["3057", "shift + A"],
  ["3008", "command + T"],
  ["3009", "command + R"],
  ["304b", "Q"],
  ["304c", "shift + Q"],
  ["304d", "shift + 5", "F5"],
  ["304f", "E", "Y"],
  ["3050", "W", "T"],
  ["305a", null, "R"],
  ["305b", "shift + E"],
  ["305d", null, "E"],
  ["305e", "shift + W"],
  ["3060", null, "Q"],
  ["3061", "S", "U"],
  ["301a", "option + A", "option + W"],
  ["301b", "option + Q", "option + Q"],
  ["3024", "shift + T"],
  ["303b", "shift + R"],
  ["3039", "T"],
  ["303a", "R"],
  ["301e", "1", "1"],
  ["301f", "2", "2"],
  ["3020", "3", "3"],
  ["3054", "4", "4"],
  ["307f", null, "5"],
  ["3021", "shift + command + 1", "shift + command + 1"],
  ["3022", "shift + command + 2", "shift + command + 2"],
  ["3023", "shift + command + 3", "shift + command + 3"],
  ["3055", "shift + command + 4", "shift + command + 4"],
  ["308b", null, "shift + command + 5"],
  ["3018", "shift + D"],
  ["3019", "shift + G"],
  ["30a5", "shift + F"],
  ["300a", "D", "O"],
  ["300b", "F", "P"],
  ["300c", "G", "option + @"],
  ["300f", null, "option + 1"],
  ["3010", null, "option + 2"],
  ["3011", "command + 1", "option + 3"],
  ["3012", "command + 2", "option + 4"],
  ["3013", "command + 3", "option + 5"],
  ["3014", "command + 4"],
  ["3015", "command + 5"],
  ["30a7", "option + 1"],
  ["3045", "option + 3"],
  ["3043", "option + 4"],
  ["3044", "option + 2"],
  ["3046", "option + E"],
  ["3047", "option + W"],
  ["30ac", null, "option + cursor down"],
  ["30ad", null, "option + cursor up"],
  ["30af", "option + G", "option + spacebar"],
  ["3097", null],
  ["3098", null],
  ["3099", null],
  ["309a", null],
  ["309b", null],
  ["309c", null],
  ["309d", null],
  ["309e", null],
  ["309f", null],
  ["30a0", null],
  ["30a1", null],
  ["30a2", null],
  ["30a3", null],
  ["30a4", null],
  ["3000", ["=", "shift + -"], "shift + -"],
  ["3001", "-", "-"],
  ["3002", "shift + ="],
  ["30b2", null, "F1"],
  ["30b3", null, "F2"],
  ["30b4", null, "F3"],
  ["30b5", null, "F4"],
  ["3106", "N", "S"],
  ["3107", "H", "K"],
  ["3157", "shift + H"],
  ["3108", "command + P"],
  ["3109", "command + O"],
  ["314b", "Y"],
  ["314c", "shift + Y"],
  ["314d", "shift + 0", "F10"],
  ["314f", "I", "H"],
  ["3150", "U", "G"],
  ["315a", null, "F"],
  ["315b", "shift + I"],
  ["315d", null, "D"],
  ["315e", "shift + U"],
  ["3160", null, "A"],
  ["3161", "J", "J"],
  ["311a", "option + H", "option + S"],
  ["311b", "option + Y", "option + A"],
  ["3124", "shift + P"],
  ["313b", "shift + O"],
  ["3139", "P"],
  ["313a", "O"],
  ["311e", "6", "6"],
  ["311f", "7", "7"],
  ["3120", "8", "8"],
  ["3154", "9", "9"],
  ["317f", null, "0"],
  ["3121", "shift + command + 6", "shift + command + 6"],
  ["3122", "shift + command + 7", "shift + command + 7"],
  ["3123", "shift + command + 8", "shift + command + 8"],
  ["3155", "shift + command + 9", "shift + command + 9"],
  ["318b", null, "shift + command + 0"],
  ["3118", "shift + K"],
  ["3119", "shift + :"],
  ["31a5", "shift + L"],
  ["310a", "K", "L"],
  ["310b", "L", ";"],
  ["310c", ";", "option + :"],
  ["310f", null, "option + 6"],
  ["3110", null, "option + 7"],
  ["3111", "command + 6", "option + 8"],
  ["3112", "command + 7", "option + 9"],
  ["3113", "command + 8", "option + 0"],
  ["3114", "command + 9"],
  ["3115", "command + 0"],
  ["31a7", "option + 6"],
  ["3145", "option + 8"],
  ["3143", "option + 9"],
  ["3144", "option + 7"],
  ["3146", "option + I"],
  ["3147", "option + U"],
  ["31af", "option + ;"],
  ["3197", null],
  ["3198", null],
  ["3199", null],
  ["319a", null],
  ["319b", null],
  ["319c", null],
  ["319d", null],
  ["319e", null],
  ["319f", null],
  ["31a0", null],
  ["31a1", null],
  ["31a2", null],
  ["31a3", null],
  ["31a4", null],
  ["31b2", null, "F6"],
  ["31b3", null, "F7"],
  ["31b4", null, "F8"],
  ["31b5", null, "F9"],
  ["7000", "ctrl + F", "ctrl + F"],
  ["7003", "command + F", "command + F"],
  ["7005", "shift + ctrl + F", "shift + ctrl + F"],
  ["b04d", "spacebar", "spacebar"],
  ["b05a", "option + /", "option + /"],
  ["400d", null, "command + R"],
  ["500a", "command + N"],
  ["b129", "shift + cursor left"],
  ["b12a", "shift + cursor right"],
  ["b12d", null, "shift + cursor left"],
  ["b12e", null, "shift + cursor right"],
  ["b12f", "shift + command + cursor left"],
  ["b130", "shift + command + cursor right"],
  ["b133", null, "shift + command + cursor left"],
  ["b134", null, "shift + command + cursor right"],
  ["b137", "command + Z", "command + Z"],
  ["b11b", "shift + command + L"],
  ["b139", "command + L", "command + L"],
  ["b103", "command + I", "command + I"],
  ["b04c", "`", "`"],
  ["b138", "command + B", "command + B"],
  ["f000", "X", "Z"],
  ["f001", "C", "X"],
  ["f002", "V", "C"],
  ["f003", "B", "V"],
  ["f004", null, "B"],
  ["f005", null, "N"],
  ["f008", "M"],
  ["f009", ","],
  ["f00a", "."],
  ["f00b", "/"],
  ["f010", "shift + X", "option + Z"],
  ["f011", "shift + C", "option + X"],
  ["f012", "shift + V", "option + C"],
  ["f013", "shift + B", "option + V"],
  ["f014", null, "option + B"],
  ["f015", null, "option + N"],
  ["f018", "shift + M"],
  ["f019", "shift + ,"],
  ["f01a", "shift + ."],
  ["f01b", "shift + /"],
  ["f03d", "command + Y"],
  ["f03f", "command + J"],
  ["f040", "command + K"],
  ["d0f0", "command + return", "command + N"],
  ["1001", null, "escape"],
 ];

const EMPTY_KEYS: readonly string[] = [];
const keyList = (value: KeyData | undefined): readonly string[] => typeof value === "string" ? [value] : value ?? EMPTY_KEYS;

const BY_ID = new Map(PERFORMANCE_KEYS.map(([id, p1, p2]) => [id, [keyList(p1), keyList(p2)] as const]));

/** The keys a Performance preset gives a command, as the pane spells them. */
export function performanceKeys(preset: KeyPreset, command: string): readonly string[] {
  const row = BY_ID.get(command);
  if (row === undefined) return [];
  if (preset === "performance1") return row[0];
  if (preset === "performance2") return row[1];
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
