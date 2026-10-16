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

export const KEYMAP: Readonly<Record<KeymapGroup, readonly KeymapRow[]>> = {
  "Browse": [
    { id: "7000", label: "Search for tracks in Collections", key: "ctrl + F" },
    { id: "7003", label: "Search for tracks in the track list", key: "command + F" },
    { id: "7005", label: "Search playlists/folders", key: "ctrl + shift + F" },
    { id: "7001", label: "Preview : Play/Stop", key: "command + P" },
    { id: "7002", label: "Preview : Skip", key: "command + [" },
  ],
  "Player A": [
    { id: "3006", label: "Play/Pause", key: "spacebar" },
    { id: "301c", label: "Quantize", key: "Q" },
    { id: "301d", label: "Time Mode", key: "T" },
    { id: "3007", label: "Cue", key: "C" },
    { id: "301a", label: "Next Track", key: "command + cursor down" },
    { id: "301b", label: "Previous Track", key: "command + cursor up" },
    { id: "3024", label: "Memory Cue", key: "M" },
    { id: "300a", label: "Loop In", key: "I" },
    { id: "300b", label: "Loop Out", key: "O" },
    { id: "300c", label: "Exit/Reloop", key: "R" },
    { id: "u1/64", label: "1/64 Beat Loop", key: null },
    { id: "u1/32", label: "1/32 Beat Loop", key: null },
    { id: "u1/16", label: "1/16 Beat Loop", key: null },
    { id: "u1/8", label: "1/8 Beat Loop", key: null },
    { id: "u1/4", label: "1/4 Beat Loop", key: null },
    { id: "u1/2", label: "1/2 Beat Loop", key: null },
    { id: "3012", label: "1 Beat Loop", key: "4" },
    { id: "3013", label: "2 Beat Loop", key: "5" },
    { id: "3014", label: "4 Beat Loop", key: "6" },
    { id: "3015", label: "8 Beat Loop", key: "7" },
    { id: "3016", label: "16 Beat Loop", key: "8" },
    { id: "3017", label: "32 Beat Loop", key: "9" },
    { id: "u64", label: "64 Beat Loop", key: null },
    { id: "3018", label: "Loop /2", key: "/" },
    { id: "3019", label: "Loop x2", key: "option + \\" },
    { id: "301e", label: "Set Hot Cue A", key: "1" },
    { id: "301f", label: "Set Hot Cue B", key: "2" },
    { id: "3020", label: "Set Hot Cue C", key: "3" },
    { id: "3054", label: "Set Hot Cue D", key: null },
    { id: "307f", label: "Set Hot Cue E", key: null },
    { id: "3080", label: "Set Hot Cue F", key: null },
    { id: "3081", label: "Set Hot Cue G", key: null },
    { id: "3082", label: "Set Hot Cue H", key: null },
    { id: "3021", label: "Clear Hot Cue A", key: "command + 1" },
    { id: "3022", label: "Clear Hot Cue B", key: "command + 2" },
    { id: "3023", label: "Clear Hot Cue C", key: "command + 3" },
    { id: "3055", label: "Clear Hot Cue D", key: null },
    { id: "308b", label: "Clear Hot Cue E", key: null },
    { id: "308c", label: "Clear Hot Cue F", key: null },
    { id: "308d", label: "Clear Hot Cue G", key: null },
    { id: "308e", label: "Clear Hot Cue H", key: null },
    { id: "3039", label: "Call Next Memory Cue", key: "N" },
    { id: "303a", label: "Call Previous Memory Cue", key: "B" },
    { id: "303b", label: "Delete Memory Cue", key: "X" },
    { id: "3008", label: "Jump Forward", key: "cursor right" },
    { id: "3009", label: "Jump Reverse", key: "cursor left" },
    { id: "303c", label: "Loop/Cue", key: "command + C" },
    { id: "303e", label: "Adjust BPM/BeatGrid", key: "command + G" },
    { id: "3025", label: "Memory Cue 1", key: "A" },
    { id: "3026", label: "Memory Cue 2", key: "S" },
    { id: "3027", label: "Memory Cue 3", key: "D" },
    { id: "3028", label: "Memory Cue 4", key: "F" },
    { id: "3029", label: "Memory Cue 5", key: "G" },
    { id: "302a", label: "Memory Cue 6", key: "H" },
    { id: "302b", label: "Memory Cue 7", key: "J" },
    { id: "302c", label: "Memory Cue 8", key: "K" },
    { id: "302d", label: "Memory Cue 9", key: "L" },
    { id: "302e", label: "Memory Cue 10", key: ";" },
    { id: "303f", label: "Show Memory Cues", key: "F10" },
    { id: "3040", label: "Show Hot Cues", key: "F11" },
    { id: "3041", label: "Show Information", key: "F12" },
    { id: "3042", label: "Change Metronome sound", key: "F9" },
    { id: "3043", label: "Shift Beatgrid right", key: "command + cursor right" },
    { id: "3044", label: "Shift Beatgrid left", key: "command + cursor left" },
    { id: "3045", label: "Shift Beatgrid to the center", key: "option + command + \\" },
    { id: "3046", label: "Expand beat intervals", key: "shift + >" },
    { id: "3047", label: "Shrink beat intervals", key: "shift + <" },
    { id: "304b", label: "SYNC", key: "F1" },
    { id: "304d", label: "Master Tempo", key: "F2" },
    { id: "304e", label: "Tempo Reset", key: "F3" },
    { id: "304f", label: "Pitch Bend +", key: "F5" },
    { id: "3050", label: "Pitch Bend -", key: "F4" },
    { id: "3051", label: "BPM +", key: "F7" },
    { id: "3052", label: "BPM -", key: "F6" },
  ],
  "Player B": [
    { id: "3106", label: "Play/Pause", key: "shift + spacebar" },
    { id: "311c", label: "Quantize", key: "shift + Q" },
    { id: "311d", label: "Time Mode", key: "shift + T" },
    { id: "3107", label: "Cue", key: "shift + C" },
    { id: "3124", label: "Memory Cue", key: "shift + M" },
    { id: "310a", label: "Loop In", key: "shift + I" },
    { id: "310b", label: "Loop Out", key: "shift + O" },
    { id: "310c", label: "Exit/Reloop", key: "shift + R" },
    { id: "ub1/64", label: "1/64 Beat Loop", key: null },
    { id: "ub1/32", label: "1/32 Beat Loop", key: null },
    { id: "ub1/16", label: "1/16 Beat Loop", key: null },
    { id: "ub1/8", label: "1/8 Beat Loop", key: null },
    { id: "ub1/4", label: "1/4 Beat Loop", key: null },
    { id: "ub1/2", label: "1/2 Beat Loop", key: null },
    { id: "3112", label: "1 Beat Loop", key: "shift + 4" },
    { id: "3113", label: "2 Beat Loop", key: "shift + 5" },
    { id: "3114", label: "4 Beat Loop", key: "shift + 6" },
    { id: "3115", label: "8 Beat Loop", key: "shift + 7" },
    { id: "3116", label: "16 Beat Loop", key: "shift + 8" },
    { id: "3117", label: "32 Beat Loop", key: "shift + 9" },
    { id: "ub64", label: "64 Beat Loop", key: null },
    { id: "3118", label: "Loop /2", key: "shift + /" },
    { id: "3119", label: "Loop x2", key: "shift + option + \\" },
    { id: "311e", label: "Set Hot Cue A", key: "shift + 1" },
    { id: "311f", label: "Set Hot Cue B", key: "shift + 2" },
    { id: "3120", label: "Set Hot Cue C", key: "shift + 3" },
    { id: "3154", label: "Set Hot Cue D", key: null },
    { id: "317f", label: "Set Hot Cue E", key: null },
    { id: "3180", label: "Set Hot Cue F", key: null },
    { id: "3181", label: "Set Hot Cue G", key: null },
    { id: "3182", label: "Set Hot Cue H", key: null },
    { id: "3121", label: "Clear Hot Cue A", key: null },
    { id: "3122", label: "Clear Hot Cue B", key: null },
    { id: "3123", label: "Clear Hot Cue C", key: null },
    { id: "3155", label: "Clear Hot Cue D", key: null },
    { id: "318b", label: "Clear Hot Cue E", key: null },
    { id: "318c", label: "Clear Hot Cue F", key: null },
    { id: "318d", label: "Clear Hot Cue G", key: null },
    { id: "318e", label: "Clear Hot Cue H", key: null },
    { id: "3139", label: "Call Next Memory Cue", key: "shift + N" },
    { id: "313a", label: "Call Previous Memory Cue", key: "shift + B" },
    { id: "313b", label: "Delete Memory Cue", key: "shift + X" },
    { id: "3108", label: "Jump Forward", key: "shift + cursor right" },
    { id: "3109", label: "Jump Reverse", key: "shift + cursor left" },
    { id: "3125", label: "Memory Cue 1", key: "shift + A" },
    { id: "3126", label: "Memory Cue 2", key: "shift + S" },
    { id: "3127", label: "Memory Cue 3", key: "shift + D" },
    { id: "3128", label: "Memory Cue 4", key: "shift + F" },
    { id: "3129", label: "Memory Cue 5", key: "shift + G" },
    { id: "312a", label: "Memory Cue 6", key: "shift + H" },
    { id: "312b", label: "Memory Cue 7", key: "shift + J" },
    { id: "312c", label: "Memory Cue 8", key: "shift + K" },
    { id: "312d", label: "Memory Cue 9", key: "shift + L" },
    { id: "312e", label: "Memory Cue 10", key: "shift + ;" },
    { id: "313f", label: "Show Memory Cues", key: "shift + F10" },
    { id: "3140", label: "Show Hot Cues", key: "shift + F11" },
    { id: "3141", label: "Show Information", key: "shift + F12" },
    { id: "3143", label: "Shift Beatgrid right", key: "shift + command + cursor right" },
    { id: "3144", label: "Shift Beatgrid left", key: "shift + command + cursor left" },
    { id: "3145", label: "Shift Beatgrid to the center", key: "shift + option + command + \\" },
    { id: "314b", label: "SYNC", key: "shift + F1" },
    { id: "314d", label: "Master Tempo", key: "shift + F2" },
    { id: "314e", label: "Tempo Reset", key: "shift + F3" },
    { id: "314f", label: "Pitch Bend +", key: "shift + F5" },
    { id: "3150", label: "Pitch Bend -", key: "shift + F4" },
    { id: "3151", label: "BPM +", key: "shift + F7" },
    { id: "3152", label: "BPM -", key: "shift + F6" },
  ],
  "General": [
    { id: "3003", label: "Volume", key: "command + F12" },
    { id: "3004", label: "Volume Down", key: "command + F11" },
    { id: "3005", label: "Mute", key: "command + F10" },
    { id: "1001", label: "Quit rekordbox", key: "command + Q" },
  ],
  "File": [
    { id: "2000", label: "Import File", key: "command + O" },
    { id: "200a", label: "Preferences", key: "command + ," },
  ],
  "View": [
    { id: "b04e", label: "Full screen", key: "shift + command + F" },
    { id: "b040", label: "1 Player", key: "command + 7" },
    { id: "b043", label: "2 Players", key: "command + 8" },
    { id: "b041", label: "Simple Player", key: "command + 9" },
    { id: "b042", label: "Full Browser", key: "command + 0" },
  ],
  "Track": [
    { id: "b103", label: "Information Window", key: "command + I" },
    { id: "b137", label: "Undo track load", key: "command + Z" },
    { id: "b139", label: "Locate track loaded on deck", key: "command + L" },
    { id: "b138", label: "Display in bold", key: "command + B" },
  ],
  // `Renumber Track Order` is command 0x500e, filed under "Playlist" and
  // bound by no preset [OBS: `MainComponent::getCommandInfo` @0x10096b078,
  // rekordbox 7.2.19 macOS, sets the info with no `addDefaultKeypress`]; the
  // reporter's Italian capture in #407 shows it there with an empty key.
  // `Sort Items` is 0x500d, just above it and unbound the same way
  // (`getCommandInfo` @0x10096b01c).
  "Playlist": [
    { id: "500d", label: "Sort Items", key: null },
    { id: "500e", label: "Renumber Track Order", key: null },
  ],
  "Help": [
  ],
  "Link Export": [
  ],
};

/**
 * The key as the pane prints it on the running platform: the preset's
 * `command` and `option` are `ctrl` and `alt` on Windows, as rekordbox's own
 * Windows pane has them.
 */
export function keyForPlatform(key: string, mac: boolean): string {
  if (mac) return key;
  return key.replace(/\bcommand\b/g, "ctrl").replace(/\boption\b/g, "alt");
}

