/**
 * The eight track colours by `ColorID`, 1 to 8: rekordbox's stock names and
 * the lookups every view naming a colour shares. What the library calls
 * each is `useColorNames`'s.
 */

/**
 * rekordbox's stock names, in `ColorID` order (1 to 8): what a colour the
 * library has not renamed is called, and the key each colour's dot is
 * painted by (`data-color`), whatever the library calls it.
 */
export const STOCK_COLOR_NAMES: readonly string[] =
  ["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"];

/** Colour `id`'s name (1 to 8, as a number or `ColorID` text); empty for 0 and anything else. */
export function colorNameOf(colorNames: readonly string[], id: number | string): string {
  const n = typeof id === "number" ? id : Number(id);
  return Number.isInteger(n) && n >= 1 ? colorNames[n - 1] ?? "" : "";
}

/** The stock name colour `id`'s dot is painted by; empty for no colour. */
export function colorDot(id: number | string): string {
  return colorNameOf(STOCK_COLOR_NAMES, id);
}
