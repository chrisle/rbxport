import { describe, expect, it } from "vitest";

import type { MenuSlot } from "@/ipc/types";
import {
  activate, activeSlots, deactivate, displayName, inactiveSlots, isFixed, MENU_ITEM, shift,
} from "./deviceSettings";

/** The TEST stick's category rows, as read off it [OBS]. */
function categories(): MenuSlot[] {
  const rows: [number, number, string, number, number][] = [
    [1, 1, "GENRE", 0, 0],
    [2, 2, "ARTIST", 1, 1],
    [3, 3, "ALBUM", 2, 1],
    [4, 4, "TRACK", 3, 1],
    [5, 17, "PLAYLIST", 5, 1],
    [6, 5, "BPM", 0, 0],
    [12, 11, "KEY", 4, 1],
    [17, 24, "FOLDER", 9, 1],
    [22, 19, "HISTORY", 6, 1],
  ];
  return rows.map(([id, menuItem, name, seq, visible]) => ({
    id, menuItem, name, seq, visible: visible === 1,
  }));
}

const names = (slots: MenuSlot[]) => slots.map((s) => s.name);

describe("the Category and Sort lists", () => {
  it("lists the visible rows in order and the hidden ones alphabetically", () => {
    const slots = categories();
    expect(names(activeSlots(slots))).toEqual([
      "ARTIST", "ALBUM", "TRACK", "KEY", "PLAYLIST", "HISTORY", "FOLDER",
    ]);
    expect(names(inactiveSlots(slots))).toEqual(["BPM", "GENRE"]);
  });

  it("activating appends to the end and renumbers", () => {
    const next = activate(categories(), 1);
    const active = activeSlots(next);
    expect(names(active).at(-1)).toBe("GENRE");
    expect(active.map((s) => s.seq)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it("deactivating closes the gap it leaves", () => {
    const next = deactivate("category", categories(), 3);
    expect(names(activeSlots(next))).toEqual([
      "ARTIST", "TRACK", "KEY", "PLAYLIST", "HISTORY", "FOLDER",
    ]);
    expect(activeSlots(next).map((s) => s.seq)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(next.find((s) => s.id === 3)).toMatchObject({ visible: false, seq: 0 });
  });

  it("a fixed item stays in the Active list whatever is asked", () => {
    expect(isFixed("category", MENU_ITEM.TRACK)).toBe(true);
    expect(isFixed("sort", MENU_ITEM.TRACK)).toBe(false);
    expect(isFixed("sort", MENU_ITEM.DEFAULT)).toBe(true);
    const before = categories();
    expect(deactivate("category", before, 4)).toEqual(before);
  });

  it("a fixed item still moves Up and Down within the Active list", () => {
    const slots = categories();
    expect(names(activeSlots(shift(slots, 4, -1)))).toEqual([
      "ARTIST", "TRACK", "ALBUM", "KEY", "PLAYLIST", "HISTORY", "FOLDER",
    ]);
    expect(names(activeSlots(shift(slots, 17, -1)))).toEqual([
      "ARTIST", "ALBUM", "TRACK", "KEY", "PLAYLIST", "FOLDER", "HISTORY",
    ]);
  });

  it("Up and Down swap with the neighbour and stop at the ends", () => {
    const slots = categories();
    expect(names(activeSlots(shift(slots, 12, -1)))).toEqual([
      "ARTIST", "ALBUM", "KEY", "TRACK", "PLAYLIST", "HISTORY", "FOLDER",
    ]);
    expect(names(activeSlots(shift(slots, 12, 1)))).toEqual([
      "ARTIST", "ALBUM", "TRACK", "PLAYLIST", "KEY", "HISTORY", "FOLDER",
    ]);
    expect(shift(slots, 2, -1)).toEqual(slots);
    expect(shift(slots, 17, 1)).toEqual(slots);
    // A hidden row has no place to move from.
    expect(shift(slots, 1, 1)).toEqual(slots);
  });

  it("shows ALPHABET the way rekordbox spells it", () => {
    const alphabet: MenuSlot = { id: 1, menuItem: MENU_ITEM.ALPHABET, name: "ALPHABET", seq: 2, visible: true };
    expect(displayName(alphabet)).toBe("ALPHABET/TRACK NAME");
    expect(displayName({ ...alphabet, menuItem: MENU_ITEM.KEY, name: "KEY" })).toBe("KEY");
  });
});
