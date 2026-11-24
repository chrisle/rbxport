import { nativeKeyRows } from "./nativeCommands";

/**
 * rekordbox's keyboard map: the Export preset, as its Keyboard pane lists it.
 *
 * Transcribed from `KeyMappings/rekordbox_0000000000030.mappings` under
 * `~/Library/Application Support/Pioneer/rekordbox6` (rekordbox 7.2.11), the
 * file rekordbox reads its "Export (Preset)" from: every command it binds,
 * with the key as the file spells it. Grouped as the pane groups them, by
 * the command id's family — 30xx is Player A, 31xx Player B (the same
 * commands with shift), 70xx Browse, 20xx File, b0xx View, b1xx Track, 50xx
 * Playlist — and the rows in the file's order, which is the capture's
 * (docs/screenshots 9.48.54 PM). The beat loops the capture shows unbound
 * are here unbound [OBS]; Help and Link Export bind nothing in the preset.
 *
 * Where a command sits in the pane when it is not a deck's — the master
 * volume and mute, Quit — is [ASSUME]: General.
 *
 * The hot cue pads past C, and Player B's clears, are rows the preset
 * leaves unbound: `djplay::UiPlayer::getAllCommands` lists them in EXPORT
 * mode, after Set Hot Cue C and Clear Hot Cue C, with no key [OBS static,
 * rekordbox 7.2.19; issue #412]. Only D to H are listed, the pads this app
 * has; I to P wait for the rest of that tree.
 *
 * The fixed-distance beat jumps (`Forward 4 beats`, `Backward 8 bars` and
 * the rest, 3097-30a4 and 3197-31a4) are PERFORMANCE mode's commands, read
 * from a Performance 1 mapping file [OBS, and [ASSUME] for eight of the ids:
 * see `FIXED_JUMPS` in `shortcuts.ts`]. The Export preset binds none of
 * them, so they are here unbound; [UNKNOWN] whether EXPORT mode's own pane
 * lists them at all, and where in its order. They sit after the menu-driven
 * Jump Forward and Jump Reverse, the commands they are the fixed form of.
 *
 * What is *built* is `shortcuts.ts`'s business: a row here says what the
 * key is in rekordbox, and the pane greys the ones this app does not answer.
 */

/** The pane's groups, in its order. */
export const KEYMAP_GROUPS = [
  "Browse", "Player A", "Player B", "General", "File", "View", "Track", "Playlist", "Help", "Link Export",
] as const;
export type KeymapGroup = (typeof KEYMAP_GROUPS)[number];

export interface KeymapRow {
  /** rekordbox's command id, or `u…` for a row it lists unbound. */
  id: string;
  label: string;
  /** The key as the preset spells it on macOS, or null when unbound. */
  key: string | null;
}

type KeyData = readonly [id: string, label: string, key: string | null];

// Player B repeats these rows with shift. Its seven absent commands and
// three unbound clears remain explicit below; the full public vector is tested.
const PLAYER_A_DATA: readonly KeyData[] = [
    ["3006", "Play/Pause", "spacebar"],
    ["301c", "Quantize", "Q"],
    ["301d", "Time Mode", "T"],
    ["3007", "Cue", "C"],
    ["301a", "Next Track", "command + cursor down"],
    ["301b", "Previous Track", "command + cursor up"],
    ["3024", "Memory Cue", "M"],
    ["300a", "Loop In", "I"],
    ["300b", "Loop Out", "O"],
    ["300c", "Exit/Reloop", "R"],
    ["u1/64", "1/64 Beat Loop", null],
    ["u1/32", "1/32 Beat Loop", null],
    ["u1/16", "1/16 Beat Loop", null],
    ["u1/8", "1/8 Beat Loop", null],
    ["u1/4", "1/4 Beat Loop", null],
    ["u1/2", "1/2 Beat Loop", null],
    ["3012", "1 Beat Loop", "4"],
    ["3013", "2 Beat Loop", "5"],
    ["3014", "4 Beat Loop", "6"],
    ["3015", "8 Beat Loop", "7"],
    ["3016", "16 Beat Loop", "8"],
    ["3017", "32 Beat Loop", "9"],
    ["u64", "64 Beat Loop", null],
    ["3018", "Loop /2", "/"],
    ["3019", "Loop x2", "option + \\"],
    ["301e", "Set Hot Cue A", "1"],
    ["301f", "Set Hot Cue B", "2"],
    ["3020", "Set Hot Cue C", "3"],
    ["3054", "Set Hot Cue D", null],
    ["307f", "Set Hot Cue E", null],
    ["3080", "Set Hot Cue F", null],
    ["3081", "Set Hot Cue G", null],
    ["3082", "Set Hot Cue H", null],
    ["3021", "Clear Hot Cue A", "command + 1"],
    ["3022", "Clear Hot Cue B", "command + 2"],
    ["3023", "Clear Hot Cue C", "command + 3"],
    ["3055", "Clear Hot Cue D", null],
    ["308b", "Clear Hot Cue E", null],
    ["308c", "Clear Hot Cue F", null],
    ["308d", "Clear Hot Cue G", null],
    ["308e", "Clear Hot Cue H", null],
    ["3039", "Call Next Memory Cue", "N"],
    ["303a", "Call Previous Memory Cue", "B"],
    ["303b", "Delete Memory Cue", "X"],
    ["3008", "Jump Forward", "cursor right"],
    ["3009", "Jump Reverse", "cursor left"],
    ["3097", "Forward 1/2 beat", null],
    ["3098", "Forward 1 beat", null],
    ["3099", "Forward 2 beats", null],
    ["309a", "Forward 4 beats", null],
    ["309b", "Forward 8 beats", null],
    ["309c", "Forward 16 beats", null],
    ["309d", "Forward 8 bars", null],
    ["309e", "Backward 1/2 beat", null],
    ["309f", "Backward 1 beat", null],
    ["30a0", "Backward 2 beats", null],
    ["30a1", "Backward 4 beats", null],
    ["30a2", "Backward 8 beats", null],
    ["30a3", "Backward 16 beats", null],
    ["30a4", "Backward 8 bars", null],
    ["303c", "Loop/Cue", "command + C"],
    ["303e", "Adjust BPM/BeatGrid", "command + G"],
    ["3025", "Memory Cue 1", "A"],
    ["3026", "Memory Cue 2", "S"],
    ["3027", "Memory Cue 3", "D"],
    ["3028", "Memory Cue 4", "F"],
    ["3029", "Memory Cue 5", "G"],
    ["302a", "Memory Cue 6", "H"],
    ["302b", "Memory Cue 7", "J"],
    ["302c", "Memory Cue 8", "K"],
    ["302d", "Memory Cue 9", "L"],
    ["302e", "Memory Cue 10", ";"],
    ["303f", "Show Memory Cues", "F10"],
    ["3040", "Show Hot Cues", "F11"],
    ["3041", "Show Information", "F12"],
    ["3042", "Change Metronome sound", "F9"],
    ["3043", "Shift Beatgrid right", "command + cursor right"],
    ["3044", "Shift Beatgrid left", "command + cursor left"],
    ["3045", "Shift Beatgrid to the center", "option + command + \\"],
    ["3046", "Expand beat intervals", "shift + >"],
    ["3047", "Shrink beat intervals", "shift + <"],
    ["304b", "SYNC", "F1"],
    ["304d", "Master Tempo", "F2"],
    ["304e", "Tempo Reset", "F3"],
    ["304f", "Pitch Bend +", "F5"],
    ["3050", "Pitch Bend -", "F4"],
    ["3051", "BPM +", "F7"],
    ["3052", "BPM -", "F6"],
];

const KEYMAP_DATA: Readonly<Record<KeymapGroup, readonly KeyData[]>> = {
  "Browse": [
    ["7000", "Search for tracks in Collections", "ctrl + F"],
    ["7003", "Search for tracks in the track list", "command + F"],
    ["7005", "Search playlists/folders", "ctrl + shift + F"],
    ["7001", "Preview : Play/Stop", "command + P"],
    ["7002", "Preview : Skip", "command + ["],
  ],
  "Player A": PLAYER_A_DATA,
  "Player B": PLAYER_A_DATA
    .filter(([id]) => !["301a", "301b", "303c", "303e", "3042", "3046", "3047"].includes(id))
    .map(([id, label, key]): KeyData => [
      id.startsWith("u") ? `ub${id.slice(1)}` : id.replace(/^30/, "31"),
      label,
      key === null || ["3021", "3022", "3023"].includes(id) ? null : `shift + ${key}`,
    ]),
  "General": [
    ["3003", "Volume", "command + F12"],
    ["3004", "Volume Down", "command + F11"],
    ["3005", "Mute", "command + F10"],
    ["1001", "Quit rekordbox", "command + Q"],
  ],
  "File": [
    ["2000", "Import File", "command + O"],
    ["200a", "Preferences", "command + ,"],
  ],
  "View": [
    ["b04e", "Full screen", "shift + command + F"],
    ["b040", "1 Player", "command + 7"],
    ["b043", "2 Players", "command + 8"],
    ["b041", "Simple Player", "command + 9"],
    ["b042", "Full Browser", "command + 0"],
  ],
  "Track": [
    ...nativeKeyRows("Track").map(({ id, label, key }): KeyData => [id, label, key]),

    // Track menu command; rbxport supplies its own default in shortcuts.ts.
    ["400d", "Show in Finder", null],
    ["b103", "Information Window", "command + I"],
    ["b137", "Undo track load", "command + Z"],
    ["b139", "Locate track loaded on deck", "command + L"],
    ["b138", "Display in bold", "command + B"],
  ],
  // `Renumber Track Order` is command 0x500e, filed under "Playlist" and
  // bound by no preset [OBS: `MainComponent::getCommandInfo` @0x10096b078,
  // rekordbox 7.2.19 macOS, sets the info with no `addDefaultKeypress`]; the
  // reporter's Italian capture in #407 shows it there with an empty key.
  // `Sort Items` is 0x500d, just above it and unbound the same way
  // (`getCommandInfo` @0x10096b01c).
  "Playlist": [
    ...nativeKeyRows("Playlist").map(({ id, label, key }): KeyData => [id, label, key]),

    ["500d", "Sort Items", null],
    ["500e", "Renumber Track Order", null],
  ],
  "Help": [
  ],
  "Link Export": [
  ],
};

/** Preserve the pane's public rows while storing repeated field names once. */
export const KEYMAP = Object.fromEntries(KEYMAP_GROUPS.map(group => [
  group, KEYMAP_DATA[group].map(([id, label, key]): KeymapRow => ({ id, label, key })),
])) as unknown as Readonly<Record<KeymapGroup, readonly KeymapRow[]>>;

/**
 * The key as the pane prints it on the running platform: the preset's
 * `command` and `option` are `ctrl` and `alt` on Windows, as rekordbox's own
 * Windows pane has them.
 */
export function keyForPlatform(key: string, mac: boolean): string {
  if (mac) return key;
  return key.replace(/\bcommand\b/g, "ctrl").replace(/\boption\b/g, "alt");
}

