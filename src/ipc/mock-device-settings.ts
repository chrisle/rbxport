/**
 * The device settings the mock backend hands out: the rows read off the
 * real TEST stick on 2026-09-09 (`cargo run -p rbl-onelibrary --example
 * stick_settings`), which are also the rows the captures were taken of.
 *
 * Kept apart from `backend-mock.ts` so the table of numbers does not sit in
 * the middle of the backend.
 */
import type { DeviceSettings, MenuSlot } from "./types";

/** `(id, menuItem, name, seq, visible)` — `category` on the TEST stick [OBS]. */
const CATEGORIES: [number, number, string, number, number][] = [
  [1, 1, "GENRE", 0, 0],
  [2, 2, "ARTIST", 1, 1],
  [3, 3, "ALBUM", 2, 1],
  [4, 4, "TRACK", 3, 1],
  [5, 17, "PLAYLIST", 5, 1],
  [6, 5, "BPM", 0, 0],
  [7, 6, "RATING", 0, 0],
  [8, 7, "YEAR", 0, 0],
  [9, 8, "REMIXER", 0, 0],
  [10, 9, "LABEL", 0, 0],
  [11, 10, "ORIGINAL ARTIST", 0, 0],
  [12, 11, "KEY", 4, 1],
  [15, 13, "COLOR", 0, 0],
  [17, 24, "FOLDER", 9, 1],
  [18, 20, "SEARCH", 7, 1],
  [19, 14, "TIME", 0, 0],
  [20, 15, "BITRATE", 0, 0],
  [21, 16, "FILE NAME", 0, 0],
  [22, 19, "HISTORY", 6, 1],
  [23, 18, "HOT CUE BANK", 0, 0],
  [26, 27, "MATCHING", 8, 1],
  [27, 22, "DATE ADDED", 10, 1],
];

/** `sort` on the same stick [OBS]. */
const SORTS: [number, number, string, number, number][] = [
  [0, 25, "DEFAULT", 1, 1],
  [1, 26, "ALPHABET", 2, 1],
  [2, 2, "ARTIST", 3, 1],
  [3, 3, "ALBUM", 4, 1],
  [4, 5, "BPM", 5, 1],
  [5, 6, "RATING", 6, 1],
  [6, 1, "GENRE", 0, 0],
  [7, 21, "COMMENTS", 0, 0],
  [8, 14, "TIME", 0, 0],
  [9, 8, "REMIXER", 0, 0],
  [10, 9, "LABEL", 0, 0],
  [11, 10, "ORIGINAL ARTIST", 0, 0],
  [12, 11, "KEY", 7, 1],
  [13, 15, "BITRATE", 0, 0],
  [15, 13, "COLOR", 0, 0],
  [16, 23, "DJ PLAY COUNT", 0, 0],
  [17, 22, "DATE ADDED", 0, 0],
];

const COLORS = ["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"];

function slots(rows: [number, number, string, number, number][]): MenuSlot[] {
  return rows.map(([id, menuItem, name, seq, visible]) => ({
    id, menuItem, name, seq, visible: visible === 1,
  }));
}

/**
 * A stick's settings as rekordbox leaves them. `withLibrary` false is a
 * stick with no export on it yet: the same rows, but nothing to write them
 * into, so the tabs draw them disabled.
 */
export function referenceDeviceSettings(deviceName: string, withLibrary: boolean): DeviceSettings {
  return {
    hasDeviceLibrary: withLibrary,
    hasOneLibrary: withLibrary,
    hasDevSetting: withLibrary,
    // The TEST stick's DEVSETTING.DAT [OBS].
    waveformColor: withLibrary ? "3band" : "blue",
    waveformPosition: "center",
    overviewWaveform: "half",
    keyDisplay: "classic",
    hasLibrarySettings: withLibrary,
    deviceName: withLibrary ? deviceName : "",
    backgroundColorType: 0,
    deviceLibraryBackgroundColorType: withLibrary ? 0 : null,
    categories: slots(CATEGORIES),
    sorts: slots(SORTS),
    subColumn: null,
    colors: COLORS.map((name, i) => ({ id: i + 1, name })),
  };
}
