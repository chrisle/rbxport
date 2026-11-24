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
const PERFORMANCE_KEYS: readonly (readonly [number, KeyData, KeyData?])[] = [
  [0x3006, "Z", "W"],
  [0x3007, "A", "I"],
  [0x3057, "shift + A"],
  [0x3008, "command + T"],
  [0x3009, "command + R"],
  [0x304b, "Q"],
  [0x304c, "shift + Q"],
  [0x304d, "shift + 5", "F5"],
  [0x304f, "E", "Y"],
  [0x3050, "W", "T"],
  [0x305a, null, "R"],
  [0x305b, "shift + E"],
  [0x305d, null, "E"],
  [0x305e, "shift + W"],
  [0x3060, null, "Q"],
  [0x3061, "S", "U"],
  [0x301a, "option + A", "option + W"],
  [0x301b, "option + Q", "option + Q"],
  [0x3024, "shift + T"],
  [0x303b, "shift + R"],
  [0x3039, "T"],
  [0x303a, "R"],
  [0x301e, "1", "1"],
  [0x301f, "2", "2"],
  [0x3020, "3", "3"],
  [0x3054, "4", "4"],
  [0x307f, null, "5"],
  [0x3021, "shift + command + 1", "shift + command + 1"],
  [0x3022, "shift + command + 2", "shift + command + 2"],
  [0x3023, "shift + command + 3", "shift + command + 3"],
  [0x3055, "shift + command + 4", "shift + command + 4"],
  [0x308b, null, "shift + command + 5"],
  [0x3018, "shift + D"],
  [0x3019, "shift + G"],
  [0x30a5, "shift + F"],
  [0x300a, "D", "O"],
  [0x300b, "F", "P"],
  [0x300c, "G", "option + @"],
  [0x300f, null, "option + 1"],
  [0x3010, null, "option + 2"],
  [0x3011, "command + 1", "option + 3"],
  [0x3012, "command + 2", "option + 4"],
  [0x3013, "command + 3", "option + 5"],
  [0x3014, "command + 4"],
  [0x3015, "command + 5"],
  [0x30a7, "option + 1"],
  [0x3045, "option + 3"],
  [0x3043, "option + 4"],
  [0x3044, "option + 2"],
  [0x3046, "option + E"],
  [0x3047, "option + W"],
  [0x30ac, null, "option + cursor down"],
  [0x30ad, null, "option + cursor up"],
  [0x30af, "option + G", "option + spacebar"],
  [0x3097, null],
  [0x3098, null],
  [0x3099, null],
  [0x309a, null],
  [0x309b, null],
  [0x309c, null],
  [0x309d, null],
  [0x309e, null],
  [0x309f, null],
  [0x30a0, null],
  [0x30a1, null],
  [0x30a2, null],
  [0x30a3, null],
  [0x30a4, null],
  [0x3000, ["=", "shift + -"], "shift + -"],
  [0x3001, "-", "-"],
  [0x3002, "shift + ="],
  [0x30b2, null, "F1"],
  [0x30b3, null, "F2"],
  [0x30b4, null, "F3"],
  [0x30b5, null, "F4"],
  [0x3106, "N", "S"],
  [0x3107, "H", "K"],
  [0x3157, "shift + H"],
  [0x3108, "command + P"],
  [0x3109, "command + O"],
  [0x314b, "Y"],
  [0x314c, "shift + Y"],
  [0x314d, "shift + 0", "F10"],
  [0x314f, "I", "H"],
  [0x3150, "U", "G"],
  [0x315a, null, "F"],
  [0x315b, "shift + I"],
  [0x315d, null, "D"],
  [0x315e, "shift + U"],
  [0x3160, null, "A"],
  [0x3161, "J", "J"],
  [0x311a, "option + H", "option + S"],
  [0x311b, "option + Y", "option + A"],
  [0x3124, "shift + P"],
  [0x313b, "shift + O"],
  [0x3139, "P"],
  [0x313a, "O"],
  [0x311e, "6", "6"],
  [0x311f, "7", "7"],
  [0x3120, "8", "8"],
  [0x3154, "9", "9"],
  [0x317f, null, "0"],
  [0x3121, "shift + command + 6", "shift + command + 6"],
  [0x3122, "shift + command + 7", "shift + command + 7"],
  [0x3123, "shift + command + 8", "shift + command + 8"],
  [0x3155, "shift + command + 9", "shift + command + 9"],
  [0x318b, null, "shift + command + 0"],
  [0x3118, "shift + K"],
  [0x3119, "shift + :"],
  [0x31a5, "shift + L"],
  [0x310a, "K", "L"],
  [0x310b, "L", ";"],
  [0x310c, ";", "option + :"],
  [0x310f, null, "option + 6"],
  [0x3110, null, "option + 7"],
  [0x3111, "command + 6", "option + 8"],
  [0x3112, "command + 7", "option + 9"],
  [0x3113, "command + 8", "option + 0"],
  [0x3114, "command + 9"],
  [0x3115, "command + 0"],
  [0x31a7, "option + 6"],
  [0x3145, "option + 8"],
  [0x3143, "option + 9"],
  [0x3144, "option + 7"],
  [0x3146, "option + I"],
  [0x3147, "option + U"],
  [0x31af, "option + ;"],
  [0x3197, null],
  [0x3198, null],
  [0x3199, null],
  [0x319a, null],
  [0x319b, null],
  [0x319c, null],
  [0x319d, null],
  [0x319e, null],
  [0x319f, null],
  [0x31a0, null],
  [0x31a1, null],
  [0x31a2, null],
  [0x31a3, null],
  [0x31a4, null],
  [0x31b2, null, "F6"],
  [0x31b3, null, "F7"],
  [0x31b4, null, "F8"],
  [0x31b5, null, "F9"],
  [0x7000, "ctrl + F", "ctrl + F"],
  [0x7003, "command + F", "command + F"],
  [0x7005, "shift + ctrl + F", "shift + ctrl + F"],
  [0xb04d, "spacebar", "spacebar"],
  [0xb05a, "option + /", "option + /"],
  [0x400d, null, "command + R"],
  [0x500a, "command + N"],
  [0xb129, "shift + cursor left"],
  [0xb12a, "shift + cursor right"],
  [0xb12d, null, "shift + cursor left"],
  [0xb12e, null, "shift + cursor right"],
  [0xb12f, "shift + command + cursor left"],
  [0xb130, "shift + command + cursor right"],
  [0xb133, null, "shift + command + cursor left"],
  [0xb134, null, "shift + command + cursor right"],
  [0xb137, "command + Z", "command + Z"],
  [0xb11b, "shift + command + L"],
  [0xb139, "command + L", "command + L"],
  [0xb103, "command + I", "command + I"],
  [0xb04c, "`", "`"],
  [0xb138, "command + B", "command + B"],
  [0xf000, "X", "Z"],
  [0xf001, "C", "X"],
  [0xf002, "V", "C"],
  [0xf003, "B", "V"],
  [0xf004, null, "B"],
  [0xf005, null, "N"],
  [0xf008, "M"],
  [0xf009, ","],
  [0xf00a, "."],
  [0xf00b, "/"],
  [0xf010, "shift + X", "option + Z"],
  [0xf011, "shift + C", "option + X"],
  [0xf012, "shift + V", "option + C"],
  [0xf013, "shift + B", "option + V"],
  [0xf014, null, "option + B"],
  [0xf015, null, "option + N"],
  [0xf018, "shift + M"],
  [0xf019, "shift + ,"],
  [0xf01a, "shift + ."],
  [0xf01b, "shift + /"],
  [0xf03d, "command + Y"],
  [0xf03f, "command + J"],
  [0xf040, "command + K"],
  [0xd0f0, "command + return", "command + N"],
  [0x1001, null, "escape"],
 ];

const EMPTY_KEYS: readonly string[] = [];
const keyList = (value: KeyData | undefined): readonly string[] => typeof value === "string" ? [value] : value ?? EMPTY_KEYS;

const BY_ID = new Map(PERFORMANCE_KEYS.map(([id, p1, p2]) => [id.toString(16).padStart(4, "0"), [keyList(p1), keyList(p2)] as const]));

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
