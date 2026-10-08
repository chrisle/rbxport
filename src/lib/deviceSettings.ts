/**
 * The Category and Sort tabs' list logic, kept apart from the components
 * because every rule here is a decision about the stick's rows, not about
 * drawing them.
 *
 * A row is `visible` with a 1-based `seq` among the visible rows, or hidden
 * with `seq` 0 — the shape of `category` and `sort` in `exportLibrary.db`.
 * The Active list is the visible rows in `seq` order; the Inactive list is
 * the rest, alphabetical, which is how rekordbox lists them [OBS: the
 * captures 9.08.32 and 9.08.35 PM].
 */
import type { MenuSlot } from "@/ipc/types";

/** `menuItem` ids, as `exportLibrary.db`'s `menuItem` table numbers them. */
export const MENU_ITEM = {
  GENRE: 1,
  ARTIST: 2,
  ALBUM: 3,
  TRACK: 4,
  BPM: 5,
  RATING: 6,
  YEAR: 7,
  REMIXER: 8,
  LABEL: 9,
  ORIGINAL_ARTIST: 10,
  KEY: 11,
  CUE: 12,
  COLOR: 13,
  TIME: 14,
  BITRATE: 15,
  FILE_NAME: 16,
  PLAYLIST: 17,
  HOT_CUE_BANK: 18,
  HISTORY: 19,
  SEARCH: 20,
  COMMENTS: 21,
  DATE_ADDED: 22,
  DJ_PLAY_COUNT: 23,
  FOLDER: 24,
  DEFAULT: 25,
  ALPHABET: 26,
  MATCHING: 27,
} as const;

/**
 * Items rekordbox greys in the Active list: the browse categories a player
 * always has, and the two sorts every list starts with [OBS: greyed in the
 * captures]. Greyed means they cannot leave the Active list; they can still
 * be reordered with Up / Down.
 */
const FIXED_CATEGORIES: ReadonlySet<number> = new Set([
  MENU_ITEM.TRACK,
  MENU_ITEM.PLAYLIST,
  MENU_ITEM.HISTORY,
  MENU_ITEM.SEARCH,
  MENU_ITEM.FOLDER,
]);
const FIXED_SORTS: ReadonlySet<number> = new Set([MENU_ITEM.DEFAULT, MENU_ITEM.ALPHABET]);

export type ListKind = "category" | "sort";

export function isFixed(kind: ListKind, menuItem: number): boolean {
  return (kind === "category" ? FIXED_CATEGORIES : FIXED_SORTS).has(menuItem);
}

/**
 * The name as rekordbox shows it in the panel. Only one differs from the
 * stick's own: ALPHABET is listed as "ALPHABET/TRACK NAME" [OBS: 9.08.35 PM].
 */
export function displayName(slot: MenuSlot): string {
  return slot.menuItem === MENU_ITEM.ALPHABET ? "ALPHABET/TRACK NAME" : slot.name;
}

/** The Active list: visible rows in their order. */
export function activeSlots(slots: readonly MenuSlot[]): MenuSlot[] {
  return slots.filter((s) => s.visible).sort((a, b) => a.seq - b.seq);
}

/** The Inactive list: hidden rows, alphabetical. */
export function inactiveSlots(slots: readonly MenuSlot[]): MenuSlot[] {
  return slots.filter((s) => !s.visible).sort((a, b) => a.name.localeCompare(b.name));
}

/** Renumbers the visible rows 1..n in the order given, hidden rows to 0. */
function renumber(slots: readonly MenuSlot[], order: readonly number[]): MenuSlot[] {
  const position = new Map(order.map((id, i) => [id, i + 1]));
  return slots.map((s) => {
    const seq = position.get(s.id);
    return seq === undefined ? { ...s, visible: false, seq: 0 } : { ...s, visible: true, seq };
  });
}

/** Moves a hidden row to the end of the Active list. */
export function activate(slots: readonly MenuSlot[], id: number): MenuSlot[] {
  const target = slots.find((s) => s.id === id);
  if (!target || target.visible) return [...slots];
  const order = activeSlots(slots).map((s) => s.id);
  order.push(id);
  return renumber(slots, order);
}

/** Takes a visible row out of the Active list; the rest close up. */
export function deactivate(kind: ListKind, slots: readonly MenuSlot[], id: number): MenuSlot[] {
  const target = slots.find((s) => s.id === id);
  if (!target || !target.visible || isFixed(kind, target.menuItem)) return [...slots];
  const order = activeSlots(slots)
    .map((s) => s.id)
    .filter((other) => other !== id);
  return renumber(slots, order);
}

/** Swaps a visible row with its neighbour above (`-1`) or below (`+1`). */
export function shift(slots: readonly MenuSlot[], id: number, by: -1 | 1): MenuSlot[] {
  const order = activeSlots(slots).map((s) => s.id);
  const at = order.indexOf(id);
  const to = at + by;
  if (at < 0 || to < 0 || to >= order.length) return [...slots];
  [order[at], order[to]] = [order[to] as number, order[at] as number];
  return renumber(slots, order);
}
