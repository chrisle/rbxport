/**
 * The keyboard map, as a pure function.
 *
 * Kept apart from the components so the whole map is testable without a DOM,
 * and so the platform difference lives in one place: macOS uses Command where
 * Windows uses Control, and getting that wrong makes every shortcut either
 * dead or triggered by accident.
 */

/** What a key press asks for. */
export type Action =
  | "focusSearch"
  | "clearSearch"
  // The selected tracks go to the analyser.
  | "analyseSelection"
  | "selectAll"
  | "clearSelection"
  | "moveUp"
  | "moveDown"
  | "extendUp"
  | "extendDown"
  | "pageUp"
  | "pageDown"
  | "toTop"
  | "toBottom"
  // Enter loads the highlighted track onto Player 1 (deck A), wherever the
  // focus is in the browser — the keyboard counterpart of double-clicking it.
  | "loadPlayer1"
  // The deck. Every key below is rekordbox's own, transcribed from the Export
  // preset in `KeyMappings/rekordbox_0000000000030.mappings` — the key map the
  // mode we clone ships with, not a guess at what feels natural. The same
  // actions with shift are Player B's.
  | "playPause"
  | "cue"
  | "quantize"
  | "jumpBack"
  | "jumpForward"
  | "showMemory"
  | "showHotCues"
  | "showInfo"
  // The MEMORY cluster: M stores the cue point as a memory cue, B and N call
  // the one before and after the playhead, X deletes the one it is on, and
  // A to ; call the first ten by number.
  | "memoryCue"
  | "previousMemoryCue"
  | "nextMemoryCue"
  | "deleteMemoryCue"
  | "callMemoryCue1"
  | "callMemoryCue2"
  | "callMemoryCue3"
  | "callMemoryCue4"
  | "callMemoryCue5"
  | "callMemoryCue6"
  | "callMemoryCue7"
  | "callMemoryCue8"
  | "callMemoryCue9"
  | "callMemoryCue10"
  // The loop: I and O mark it by hand, R leaves it or goes round again, 4 to
  // 9 are the beat loops, / and option + \ halve and double the length.
  | "loopIn"
  | "loopOut"
  | "reloop"
  | "beatLoop1"
  | "beatLoop2"
  | "beatLoop4"
  | "beatLoop8"
  | "beatLoop16"
  | "beatLoop32"
  | "loopHalf"
  | "loopDouble"
  // The hot cue pads: the Export preset binds `1`, `2` and `3` to `Set Hot
  // Cue A` to `C` and `command + 1`-`3` to `Clear Hot Cue A` to `C`, and
  // nothing to D onwards.
  | "hotCueA"
  | "hotCueB"
  | "hotCueC"
  | "clearHotCueA"
  | "clearHotCueB"
  | "clearHotCueC"
  // The tempo: F1 SYNC, F2 MASTER TEMPO, F3 resets the slider, F6 and F7
  // step it, F9 changes the metronome's sound.
  | "sync"
  | "masterTempo"
  | "tempoReset"
  | "bpmUp"
  | "bpmDown"
  | "metronomeSound"
  // The master: command + F12 and F11 turn it up and down, command + F10
  // mutes it.
  | "volumeUp"
  | "volumeDown"
  | "mute"
  // The GRID panel: `Adjust BPM/BeatGrid` (`command + G`) opens it, the
  // modified arrows are `Shift Beatgrid left/right`, and `Shift Beatgrid to
  // the center` (`option + command + \`) puts the nearest beat under the
  // playhead, which is the centre of the detail waveform.
  | "adjustGrid"
  | "shiftGridLeft"
  | "shiftGridRight"
  | "shiftGridToCenter";

/**
 * The pad a hot cue action names, and whether it clears rather than sets.
 * `null` for any other action.
 */
export function hotCuePad(action: Action): { letter: string; clear: boolean } | null {
  const set = /^hotCue([A-C])$/.exec(action);
  if (set) return { letter: set[1] ?? "", clear: false };
  const clear = /^clearHotCue([A-C])$/.exec(action);
  if (clear) return { letter: clear[1] ?? "", clear: true };
  return null;
}

/** The memory cue a call action names, one-based, or `null`. */
export function memoryCueNumber(action: Action): number | null {
  const call = /^callMemoryCue(\d+)$/.exec(action);
  return call ? Number(call[1]) : null;
}

/** The beats a beat-loop action asks for, or `null`. */
export function beatLoopLength(action: Action): number | null {
  const loop = /^beatLoop(\d+)$/.exec(action);
  return loop ? Number(loop[1]) : null;
}

/** The parts of a keyboard event the map reads. */
export interface KeyChord {
  /** `KeyboardEvent.key`; empty for a binding with its key taken away. */
  key: string;
  /** `KeyboardEvent.code`, when the event has one: how shift + 1 is still 1. */
  code?: string;
  /** Command on macOS. */
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
}

export interface Platform {
  /** True on macOS, where Command is the modifier rather than Control. */
  mac: boolean;
  /** True on Linux, where the window manager owns the title bar. */
  linux?: boolean;
}

/** Whether the platform's primary modifier is held, and only it. */
function primary(chord: KeyChord, platform: Platform): boolean {
  return platform.mac ? chord.metaKey === true : chord.ctrlKey === true;
}

/**
 * The native menu item a chord is the accelerator of, on a platform where
 * the webview eats it — or `null`.
 *
 * On macOS the menu's key equivalents are handled by the application before
 * the webview sees a keystroke, so nothing is needed and nothing is
 * returned: a fallback there would fire the item twice. On Windows the
 * accelerators are bound too, but a keystroke that lands in the focused
 * webview never reaches them (Ctrl+, and Ctrl+8 did nothing on 0.5.1 while
 * typing into the search field proved the keys arrived), so the shell's ids
 * are produced here and handled exactly as a menu click is. Full screen is
 * left out: the shell does that one itself, on the native event.
 */
export function menuAccelerator(chord: KeyChord, platform: Platform): string | null {
  if (platform.mac || chord.ctrlKey !== true || chord.metaKey === true || chord.altKey === true) {
    return null;
  }
  if (chord.key.toLowerCase() === "z") return chord.shiftKey === true ? "redo" : "undo";
  if (chord.shiftKey === true) return null;
  switch (chord.key.toLowerCase()) {
    case "y":
      return "redo";
    case ",":
      return "settings";
    case "o":
      return "import";
    case "i":
      return "info";
    case "b":
      return "sub";
    case "7":
      return "layout-one";
    case "8":
      return "layout-two";
    case "9":
      return "layout-simple";
    case "0":
      return "layout-browser";
    default:
      return null;
  }
}

/**
 * The key a physical key stands for whatever shift or option made of it:
 * shift + 1 reports `!` and option + \ reports `«` on a US Mac, and both
 * are still the 1 and the \ the preset names. Letters and digits come back
 * as themselves; the punctuation the preset uses is named; anything else is
 * `null` and the event's own `key` is what there is.
 */
export function keyFromCode(code: string | undefined): string | null {
  if (!code) return null;
  const digit = /^Digit(\d)$/.exec(code);
  if (digit) return digit[1] ?? null;
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return (letter[1] ?? "").toLowerCase();
  switch (code) {
    case "Slash": return "/";
    case "Backslash": return "\\";
    case "Semicolon": return ";";
    case "Comma": return ",";
    case "Period": return ".";
    case "BracketLeft": return "[";
    case "BracketRight": return "]";
    case "Minus": return "-";
    case "Equal": return "=";
    case "Quote": return "'";
    case "Backquote": return "`";
    default: return null;
  }
}

/** Whether an event's key is a binding's, letter case and shift aside. */
function keyMatches(bound: string, chord: KeyChord): boolean {
  if (bound === "") return false;
  if (bound.length === 1) {
    if (chord.key.toLowerCase() === bound.toLowerCase()) return true;
    return keyFromCode(chord.code) === bound.toLowerCase();
  }
  return chord.key === bound;
}

/**
 * Whether an event is a binding's chord. `metaKey` in a binding means the
 * platform's primary modifier; shift and option have to agree exactly, so
 * shift + cursor up is not cursor up.
 */
function chordMatches(bound: KeyChord, chord: KeyChord, platform: Platform): boolean {
  if (!keyMatches(bound.key, chord)) return false;
  if ((bound.metaKey === true) !== primary(chord, platform)) return false;
  if ((bound.shiftKey === true) !== (chord.shiftKey === true)) return false;
  if ((bound.altKey === true) !== (chord.altKey === true)) return false;
  return true;
}

/** A key taken away from a binding, or changed: the Keyboard pane's edits. */
export type KeyOverrides = Readonly<Record<string, KeyChord>>;

/**
 * The binding a chord asks for, with the person's own keys applied, or
 * `null`. The first binding in the table that matches wins, which is how
 * shift + cursor up extends the selection rather than jumping Player B.
 */
export function matchBinding(chord: KeyChord, platform: Platform, overrides: KeyOverrides = {}): Binding | null {
  // The other platform's modifier must not also trigger it, or Control-A on a
  // Mac would select all *and* move the caret to the start of the line.
  const wrongMod = platform.mac ? chord.ctrlKey === true : chord.metaKey === true;
  if (wrongMod) return null;
  for (const binding of BINDINGS) {
    if (binding.action === undefined) continue;
    const bound = overrides[binding.id] ?? binding.chord;
    if (chordMatches(bound, chord, platform)) return binding;
  }
  return null;
}

/**
 * The action a chord asks for, or `null`.
 *
 * `null` means "not ours" — the caller must let the event through rather than
 * swallow it, or browser and OS shortcuts stop working inside the app.
 */
export function actionFor(chord: KeyChord, platform: Platform, overrides: KeyOverrides = {}): Action | null {
  return matchBinding(chord, platform, overrides)?.action ?? null;
}

/**
 * Just enough of an element to decide whether it owns the keyboard.
 *
 * Structural rather than `HTMLElement`, so this module needs no DOM and stays
 * testable in the same plain-node environment as the rest of `src/lib`.
 */
export interface FocusTarget {
  tagName?: string;
  isContentEditable?: boolean;
}

/**
 * Whether a key press should be ignored because the user is typing into a
 * field.
 *
 * Arrow keys and Escape inside the search box belong to the box, not to the
 * track list — except the shortcut that focuses the box, which must still work
 * from anywhere.
 */
export function isTyping(target: FocusTarget | null | undefined): boolean {
  if (!target) return false;
  if (target.isContentEditable === true) return true;
  switch (target.tagName?.toUpperCase()) {
    case "INPUT":
    case "TEXTAREA":
    case "SELECT":
      return true;
    default:
      return false;
  }
}

/** Actions that still apply while a field has focus. */
const WHILE_TYPING: ReadonlySet<Action> = new Set<Action>([
  "focusSearch",
  "clearSearch",
]);

/** The action to run for an event, accounting for where the focus is. */
export function dispatch(
  chord: KeyChord,
  platform: Platform,
  target: FocusTarget | null | undefined,
  overrides: KeyOverrides = {},
): Action | null {
  return dispatchBinding(chord, platform, target, overrides)?.action ?? null;
}

/** As `dispatch`, but the whole binding: which deck it is for, and its id. */
export function dispatchBinding(
  chord: KeyChord,
  platform: Platform,
  target: FocusTarget | null | undefined,
  overrides: KeyOverrides = {},
): Binding | null {
  const binding = matchBinding(chord, platform, overrides);
  if (binding?.action === undefined) return null;
  if (isTyping(target) && !WHILE_TYPING.has(binding.action)) return null;
  return binding;
}

/**
 * The bindings: the map itself, one row each.
 *
 * `actionFor` walks this table, so what the Keyboard pane lists is what the
 * keys do, and a key changed there changes both. The rows are rekordbox's
 * own from the Export preset, in its wording and grouping (Browse, Player
 * A, Player B), plus this app's own few and the menu accelerators the shell
 * binds. A binding's `id` is what a changed key is filed under.
 */
export interface Binding {
  /** Stable, for the person's own key to be stored against. */
  id: string;
  group: "Browse" | "Player A" | "Player B" | "General" | "Menu";
  /** rekordbox's description of the command. */
  label: string;
  chord: KeyChord;
  /**
   * What the key does. A menu accelerator has none: the native menu answers
   * it, and the row is listed so the map is complete.
   */
  action?: Action;
  /** The deck a Player row drives; a browse or menu row has none. */
  deck?: "a" | "b";
  /**
   * rekordbox's command id for it in `keymap.ts`, when it is one of
   * rekordbox's: the Keyboard pane draws that row live. A binding without
   * one is this app's own, listed under `pane`.
   */
  command?: string;
  /** Where the Keyboard pane files a binding that is this app's own. */
  pane?: "Browse" | "View" | "Track" | "File" | "General";
  /** A second chord for the same thing, not listed in the pane. */
  alias?: true;
}

/** Player A's rows: Player B's are the same with shift, `31xx` for `30xx`. */
const PLAYER_A: readonly Omit<Binding, "id" | "group" | "deck">[] = [
  { label: "Play/Pause", chord: { key: " " }, action: "playPause", command: "3006" },
  { label: "Quantize", chord: { key: "q" }, action: "quantize", command: "301c" },
  { label: "Cue", chord: { key: "c" }, action: "cue", command: "3007" },
  { label: "Memory Cue", chord: { key: "m" }, action: "memoryCue", command: "3024" },
  { label: "Loop In", chord: { key: "i" }, action: "loopIn", command: "300a" },
  { label: "Loop Out", chord: { key: "o" }, action: "loopOut", command: "300b" },
  { label: "Exit/Reloop", chord: { key: "r" }, action: "reloop", command: "300c" },
  { label: "1 Beat Loop", chord: { key: "4" }, action: "beatLoop1", command: "3012" },
  { label: "2 Beat Loop", chord: { key: "5" }, action: "beatLoop2", command: "3013" },
  { label: "4 Beat Loop", chord: { key: "6" }, action: "beatLoop4", command: "3014" },
  { label: "8 Beat Loop", chord: { key: "7" }, action: "beatLoop8", command: "3015" },
  { label: "16 Beat Loop", chord: { key: "8" }, action: "beatLoop16", command: "3016" },
  { label: "32 Beat Loop", chord: { key: "9" }, action: "beatLoop32", command: "3017" },
  { label: "Loop /2", chord: { key: "/" }, action: "loopHalf", command: "3018" },
  { label: "Loop x2", chord: { key: "\\", altKey: true }, action: "loopDouble", command: "3019" },
  { label: "Set Hot Cue A", chord: { key: "1" }, action: "hotCueA", command: "301e" },
  { label: "Set Hot Cue B", chord: { key: "2" }, action: "hotCueB", command: "301f" },
  { label: "Set Hot Cue C", chord: { key: "3" }, action: "hotCueC", command: "3020" },
  { label: "Clear Hot Cue A", chord: { key: "1", metaKey: true }, action: "clearHotCueA", command: "3021" },
  { label: "Clear Hot Cue B", chord: { key: "2", metaKey: true }, action: "clearHotCueB", command: "3022" },
  { label: "Clear Hot Cue C", chord: { key: "3", metaKey: true }, action: "clearHotCueC", command: "3023" },
  { label: "Call Next Memory Cue", chord: { key: "n" }, action: "nextMemoryCue", command: "3039" },
  { label: "Call Previous Memory Cue", chord: { key: "b" }, action: "previousMemoryCue", command: "303a" },
  { label: "Delete Memory Cue", chord: { key: "x" }, action: "deleteMemoryCue", command: "303b" },
  { label: "Jump Forward", chord: { key: "ArrowRight" }, action: "jumpForward", command: "3008" },
  { label: "Jump Reverse", chord: { key: "ArrowLeft" }, action: "jumpBack", command: "3009" },
  { label: "Memory Cue 1", chord: { key: "a" }, action: "callMemoryCue1", command: "3025" },
  { label: "Memory Cue 2", chord: { key: "s" }, action: "callMemoryCue2", command: "3026" },
  { label: "Memory Cue 3", chord: { key: "d" }, action: "callMemoryCue3", command: "3027" },
  { label: "Memory Cue 4", chord: { key: "f" }, action: "callMemoryCue4", command: "3028" },
  { label: "Memory Cue 5", chord: { key: "g" }, action: "callMemoryCue5", command: "3029" },
  { label: "Memory Cue 6", chord: { key: "h" }, action: "callMemoryCue6", command: "302a" },
  { label: "Memory Cue 7", chord: { key: "j" }, action: "callMemoryCue7", command: "302b" },
  { label: "Memory Cue 8", chord: { key: "k" }, action: "callMemoryCue8", command: "302c" },
  { label: "Memory Cue 9", chord: { key: "l" }, action: "callMemoryCue9", command: "302d" },
  { label: "Memory Cue 10", chord: { key: ";" }, action: "callMemoryCue10", command: "302e" },
  { label: "Show Memory Cues", chord: { key: "F10" }, action: "showMemory", command: "303f" },
  { label: "Show Hot Cues", chord: { key: "F11" }, action: "showHotCues", command: "3040" },
  { label: "Show Information", chord: { key: "F12" }, action: "showInfo", command: "3041" },
  { label: "Change Metronome sound", chord: { key: "F9" }, action: "metronomeSound", command: "3042" },
  { label: "SYNC", chord: { key: "F1" }, action: "sync", command: "304b" },
  { label: "Master Tempo", chord: { key: "F2" }, action: "masterTempo", command: "304d" },
  { label: "Tempo Reset", chord: { key: "F3" }, action: "tempoReset", command: "304e" },
  { label: "BPM +", chord: { key: "F7" }, action: "bpmUp", command: "3051" },
  { label: "BPM -", chord: { key: "F6" }, action: "bpmDown", command: "3052" },
  { label: "Adjust BPM/BeatGrid", chord: { key: "g", metaKey: true }, action: "adjustGrid", command: "303e" },
  { label: "Shift Beatgrid left", chord: { key: "ArrowLeft", metaKey: true }, action: "shiftGridLeft", command: "3044" },
  { label: "Shift Beatgrid right", chord: { key: "ArrowRight", metaKey: true }, action: "shiftGridRight", command: "3043" },
  { label: "Shift Beatgrid to the center", chord: { key: "\\", metaKey: true, altKey: true }, action: "shiftGridToCenter", command: "3045" },
];

/** Player B's row for one of Player A's: shift, and the `31xx` command. */
function playerB(row: Omit<Binding, "id" | "group" | "deck">): Binding {
  // The metronome's sound is the engine's, not a deck's: Player B has no row.
  const shifted: Binding = {
    ...row,
    id: `b.${row.action ?? row.label}`,
    group: "Player B",
    deck: "b",
    chord: { ...row.chord, shiftKey: true },
  };
  if (row.command !== undefined) shifted.command = row.command.replace(/^30/, "31");
  return shifted;
}

export const BINDINGS: readonly Binding[] = [
  { id: "focusSearch", group: "Browse", label: "Search", chord: { key: "f", metaKey: true }, action: "focusSearch", command: "7003" },
  { id: "clearSearch", group: "Browse", label: "Clear Search", chord: { key: "Escape" }, action: "clearSearch", pane: "Browse" },
  { id: "selectAll", group: "Browse", label: "Select All", chord: { key: "a", metaKey: true }, action: "selectAll", pane: "Browse" },
  { id: "moveUp", group: "Browse", label: "Cursor Up", chord: { key: "ArrowUp" }, action: "moveUp", pane: "Browse" },
  { id: "moveDown", group: "Browse", label: "Cursor Down", chord: { key: "ArrowDown" }, action: "moveDown", pane: "Browse" },
  { id: "extendUp", group: "Browse", label: "Extend Selection Up", chord: { key: "ArrowUp", shiftKey: true }, action: "extendUp", pane: "Browse" },
  { id: "extendDown", group: "Browse", label: "Extend Selection Down", chord: { key: "ArrowDown", shiftKey: true }, action: "extendDown", pane: "Browse" },
  { id: "pageUp", group: "Browse", label: "Page Up", chord: { key: "PageUp" }, action: "pageUp", pane: "Browse" },
  { id: "pageDown", group: "Browse", label: "Page Down", chord: { key: "PageDown" }, action: "pageDown", pane: "Browse" },
  { id: "toTop", group: "Browse", label: "Cursor to Top", chord: { key: "Home" }, action: "toTop", pane: "Browse" },
  { id: "toBottom", group: "Browse", label: "Cursor to Bottom", chord: { key: "End" }, action: "toBottom", pane: "Browse" },
  { id: "toTop.arrow", group: "Browse", label: "Cursor to Top", chord: { key: "ArrowUp", metaKey: true }, action: "toTop", alias: true },
  { id: "toBottom.arrow", group: "Browse", label: "Cursor to Bottom", chord: { key: "ArrowDown", metaKey: true }, action: "toBottom", alias: true },
  { id: "analyseSelection", group: "Browse", label: "Analyze Track", chord: { key: "a", metaKey: true, shiftKey: true }, action: "analyseSelection", pane: "Browse" },
  { id: "loadPlayer1", group: "Browse", label: "Load on Player 1", chord: { key: "Enter" }, action: "loadPlayer1", pane: "Browse" },
  { id: "loadPlayer1.shift", group: "Browse", label: "Load on Player 1", chord: { key: "Enter", shiftKey: true }, action: "loadPlayer1", alias: true },
  ...PLAYER_A.map((row): Binding => ({ ...row, id: row.action ?? row.label, group: "Player A", deck: "a" })),
  // The preset gives Player B no clears for its pads, no `Adjust
  // BPM/BeatGrid`, and the metronome's sound is the engine's: none of those
  // rows exists there.
  ...PLAYER_A.filter((row) =>
    row.action !== "metronomeSound" && row.action !== "adjustGrid" && hotCuePad(row.action ?? "cue")?.clear !== true)
    .map(playerB),
  { id: "volumeUp", group: "General", label: "Volume", chord: { key: "F12", metaKey: true }, action: "volumeUp", command: "3003" },
  { id: "volumeDown", group: "General", label: "Volume Down", chord: { key: "F11", metaKey: true }, action: "volumeDown", command: "3004" },
  { id: "mute", group: "General", label: "Mute", chord: { key: "F10", metaKey: true }, action: "mute", command: "3005" },
  { id: "menu.import", group: "Menu", label: "Import File", chord: { key: "o", metaKey: true }, command: "2000" },
  { id: "menu.settings", group: "Menu", label: "Preferences", chord: { key: ",", metaKey: true }, command: "200a" },
  { id: "menu.info", group: "Menu", label: "Information Window", chord: { key: "i", metaKey: true }, command: "b103" },
  { id: "menu.sub", group: "Menu", label: "Sub Browser", chord: { key: "b", metaKey: true }, pane: "View" },
  { id: "menu.layout-one", group: "Menu", label: "1 Player", chord: { key: "7", metaKey: true }, command: "b040" },
  { id: "menu.layout-two", group: "Menu", label: "2 Players", chord: { key: "8", metaKey: true }, command: "b043" },
  { id: "menu.layout-simple", group: "Menu", label: "Simple Player", chord: { key: "9", metaKey: true }, command: "b041" },
  { id: "menu.layout-browser", group: "Menu", label: "Full Browser", chord: { key: "0", metaKey: true }, command: "b042" },
  { id: "menu.fullscreen", group: "Menu", label: "Full Screen", chord: { key: "f", metaKey: true, shiftKey: true }, command: "b04e" },
];

/** Whether two chords are the same keys. */
export function sameChord(a: KeyChord, b: KeyChord): boolean {
  return a.key.toLowerCase() === b.key.toLowerCase()
    && (a.metaKey === true) === (b.metaKey === true)
    && (a.shiftKey === true) === (b.shiftKey === true)
    && (a.altKey === true) === (b.altKey === true);
}

/**
 * The chord an event is, for the Keyboard pane to store: the key the
 * physical key stands for, and the platform's own modifier as `metaKey`.
 * A modifier on its own, or the other platform's, is not a chord: `null`.
 */
export function chordFromEvent(event: KeyChord, platform: Platform): KeyChord | null {
  if (["Shift", "Meta", "Control", "Alt", "Dead", "Unidentified", ""].includes(event.key)) return null;
  if (platform.mac ? event.ctrlKey === true : event.metaKey === true) return null;
  const chord: KeyChord = { key: keyFromCode(event.code) ?? event.key };
  if (primary(event, platform)) chord.metaKey = true;
  if (event.shiftKey === true) chord.shiftKey = true;
  if (event.altKey === true) chord.altKey = true;
  return chord;
}

/** The names rekordbox prints in a key badge for keys that are not letters. */
const KEY_NAMES: Record<string, string> = {
  " ": "spacebar",
  Enter: "enter",
  ArrowLeft: "cursor left",
  ArrowRight: "cursor right",
  ArrowUp: "cursor up",
  ArrowDown: "cursor down",
  Home: "home",
  End: "end",
  Escape: "esc",
  PageUp: "page up",
  PageDown: "page down",
  Backspace: "backspace",
  Delete: "delete",
  Tab: "tab",
};

/**
 * A chord as rekordbox's Keyboard pane prints it: `spacebar`,
 * `command + cursor down`, `shift + command + F`. `metaKey` in a binding
 * means the platform's primary modifier, so it reads `ctrl` on Windows.
 */
export function describeChord(chord: KeyChord, platform: Platform): string {
  const parts: string[] = [];
  if (chord.shiftKey) parts.push("shift");
  if (chord.altKey) parts.push(platform.mac ? "option" : "alt");
  if (chord.metaKey || chord.ctrlKey) parts.push(platform.mac ? "command" : "ctrl");
  const name = KEY_NAMES[chord.key] ?? (chord.key.length === 1 ? chord.key.toUpperCase() : chord.key);
  parts.push(name);
  return parts.join(" + ");
}

/** The running platform, read once. */
export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return { mac: false };
  // `platform` is deprecated but is the only reliable signal in WKWebView;
  // userAgent carries "Macintosh" there too, so either answers.
  const hint = `${navigator.platform ?? ""} ${navigator.userAgent}`;
  return { mac: /Mac|iPhone|iPad/.test(hint), linux: /Linux/.test(hint) };
}
