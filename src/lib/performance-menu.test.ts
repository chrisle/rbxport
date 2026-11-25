import { describe, expect, it } from "vitest";
import { keyMap, presetChord } from "./keyPresets";
import { BINDINGS, matchBinding, menuAccelerator } from "./shortcuts";
import { rowsFor } from "@/views/settings/KeyboardPane";

const cases = [
  ["o", "jumpBack", "import", "2000", "File"],
  ["7", "beatLoop1", "layout-one", "b040", "View"],
  ["8", "beatLoop2", "layout-two", "b043", "View"],
  ["9", "beatLoop4", "layout-simple", "b041", "View"],
  ["0", "beatLoop8", "layout-browser", "b042", "View"],
] as const;

describe("Performance 1 owns its five Deck B keys before the menus (#412)", () => {
  for (const [key, action, menu, command, group] of cases) {
    it(`${key} reaches ${action} on both platforms and restores the Export menu`, () => {
      for (const mac of [true, false]) {
        const platform = { mac };
        const event = { key, ...(mac ? { metaKey: true } : { ctrlKey: true }) };
        expect(menuAccelerator(event, platform, true)).toBeNull();
        expect(matchBinding(event, platform, keyMap({ preset: "performance1", overrides: {} })))
          .toMatchObject({ action, deck: "b" });
        const nativeMenu = BINDINGS.find(row => row.command === command)!;
        expect(presetChord(nativeMenu, "performance1").chord.key).toBe("");
        expect(rowsFor(group, mac, {}, "performance1").find(row => row.id === command)?.key).toBeNull();
        for (const preset of ["export", "performance2", "none"] as const) {
          expect(presetChord(nativeMenu, preset).chord).toEqual(nativeMenu.chord);
          expect(menuAccelerator(event, platform, false)).toBe(mac ? null : menu);
        }
      }
    });
  }
  it("retains text editing, preferences and other unaffected menu shortcuts", () => {
    for (const key of ["z", "i", "b", ","]) {
      expect(menuAccelerator({ key, ctrlKey: true }, { mac: false }, true))
        .toBe(menuAccelerator({ key, ctrlKey: true }, { mac: false }, false));
    }
  });
});
