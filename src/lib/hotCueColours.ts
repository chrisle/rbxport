/**
 * The colour a hot cue is drawn in, following View › Color › HOT CUE color.
 *
 * A hot cue the user coloured keeps its colour in every mode. One without a
 * colour — `ColorTableIndex` 0, NULL or outside 1-64, which the backend sends
 * as `null` — takes a default from the selected table by slot, and a hot loop
 * without one takes index 36 whatever the mode.
 *
 * [OBS] static, rekordbox 7.2.19 (arm64): `WidgetWaveView::createHotcueImage`
 * @`0x1021090f0` reads the cue's own index and falls back to the table only
 * when it is 0; `BrowseHelper::getCueImageForPreview` @`0x101fc97b4` does the
 * same for a track-list row through `ListViewer::getHotCueColorIndex`
 * @`0x1003eb184`, which indexes the table by slot (A = 0) and reads entry 16
 * for a loop. `ViewHotCueColor`'s choices are CDJ, COLD1, COLD2 and COLORFUL
 * (setting values 1-4, default 4 at `0x1037bbbec`); the `ListViewer`
 * constructor @`0x1003d9338` picks `hotCueColorIndexCold1`, `…Cold2` or
 * `…Colorfull` for 2-4 and `hotCueColorIndexCDJ` otherwise. The tables are
 * 17 bytes each, read at `0x10375dc28`:
 *
 * - CDJ       `16 16 16 16 16 16 16 16 1a 1a 1a 1a 1a 1a 1a 1a 24`
 * - COLD1     `04 12 3a 08 0c 3c 0e 01` twice, then `24`
 * - COLD2     `0e 0b 08 05 03 01 3d 3b` twice, then `24`
 * - COLORFUL  `2b 08 17 3c 13 26 01 20` twice, then `24`
 */
import type { HotCueColor } from "./preferences";

/**
 * The indices the tables use, as rekordbox draws them on screen: the
 * matching entries of `rbl_anlz::DRAWN_CUE_COLOURS` (`djplay::getPadColor`).
 */
const DRAWN: Readonly<Record<number, string>> = {
  1: "#305AFF", 3: "#508CFF", 4: "#50A0FF", 5: "#50B4FF", 8: "#45ACDB",
  11: "#32D2E6", 12: "#21B4B9", 14: "#1FA392", 18: "#10B176", 19: "#30D26E",
  22: "#28E214", 23: "#7DC13D", 26: "#A5E116", 32: "#C3AF04", 36: "#FF8C00",
  38: "#E0641B", 43: "#FF376F", 58: "#C542FF", 59: "#AA5AFF", 60: "#AA72FF",
  61: "#8272FF",
};

const twice = (eight: readonly number[]): readonly number[] => [...eight, ...eight];

/** `ColorTableIndex` defaults for slots A to P, by HOT CUE color. */
export const HOT_CUE_DEFAULT_INDICES: Readonly<Record<HotCueColor, readonly number[]>> = {
  cdj: [22, 22, 22, 22, 22, 22, 22, 22, 26, 26, 26, 26, 26, 26, 26, 26],
  cold1: twice([4, 18, 58, 8, 12, 60, 14, 1]),
  cold2: twice([14, 11, 8, 5, 3, 1, 61, 59]),
  colorful: twice([43, 8, 23, 60, 19, 38, 1, 32]),
};

/** Entry 16 of every table: a hot loop's default. */
export const HOT_LOOP_DEFAULT_INDEX = 36;

/** The `ColorTableIndex` an uncoloured hot cue is drawn with. */
export function hotCueDefaultIndex(letter: string, loop: boolean, mode: HotCueColor): number {
  if (loop) return HOT_LOOP_DEFAULT_INDEX;
  const slot = letter.charCodeAt(0) - 65;
  const table = HOT_CUE_DEFAULT_INDICES[mode];
  return table[slot] ?? table[0] ?? HOT_LOOP_DEFAULT_INDEX;
}

/** `#RRGGBB` for a hot cue: its own colour, or the mode's default for its slot. */
export function hotCueColour(
  colour: string | null | undefined,
  letter: string,
  loop: boolean,
  mode: HotCueColor,
): string {
  if (colour) return colour;
  return DRAWN[hotCueDefaultIndex(letter, loop, mode)] ?? DRAWN[HOT_LOOP_DEFAULT_INDEX] ?? "#FF8C00";
}
