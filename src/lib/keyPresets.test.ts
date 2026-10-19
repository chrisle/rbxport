import { describe, expect, it } from "vitest";

import {
  chordFromText, heldByShell, KEY_PRESET_NAMES, KEY_PRESETS, keyMap, PERFORMANCE_KEYMAP, performanceKeys,
  presetChord, type KeyPreset,
} from "./keyPresets";
import { sanitisePreferences } from "./preferences";
import { BINDINGS, describeChord, dispatchBinding, matchBinding, type KeyChord } from "./shortcuts";
import { assignChord, rowsFor } from "@/views/settings/KeyboardPane";

const MAC = { mac: true };
const WIN = { mac: false };

/** A preset key as the event a Mac or a Windows keyboard sends for it. */
function press(text: string, mac: boolean): KeyChord {
  const chord = chordFromText(text);
  if (chord === null) throw new Error(`cannot press ${text}`);
  const { metaKey, ...rest } = chord;
  return metaKey === true ? { ...rest, ...(mac ? { metaKey: true } : { ctrlKey: true }) } : rest;
}

const PERFORMANCE: readonly ("performance1" | "performance2")[] = ["performance1", "performance2"];

describe("rekordbox's Performance presets (#412)", () => {
  it("offers rekordbox's four presets, in its list's order and wording", () => {
    expect(KEY_PRESETS.map((preset) => KEY_PRESET_NAMES[preset])).toEqual([
      "Performance 1 (Preset)", "Performance 2 (Preset)", "Export (Preset)", "None (Preset)",
    ]);
  });

  it("holds every command either preset gives a key, as rekordbox 7.2.19 adds them", () => {
    expect(PERFORMANCE_KEYMAP).toHaveLength(163);
    expect(new Set(PERFORMANCE_KEYMAP.map((row) => row.id)).size).toBe(163);
    const groups = new Map<string, number>();
    for (const row of PERFORMANCE_KEYMAP) groups.set(row.group, (groups.get(row.group) ?? 0) + 1);
    expect(Object.fromEntries(groups)).toEqual({
      "Deck 1": 58, "All Decks": 3, "Deck 2": 56, "General": 3, "View": 2, "Track": 1, "Playlist": 1,
      "Browse": 14, "Sampler": 23, "Recordings": 1, "File": 1,
    });
    for (const row of PERFORMANCE_KEYMAP) expect(row.p1.length + row.p2.length, row.name).toBeGreaterThan(0);
  });

  it("pins the keys observed in rekordbox for the deck, browser and the rest", () => {
    // [OBS static, rekordbox 7.2.19; the same as AlphaTheta's reference].
    const keys = (id: string) => [performanceKeys("performance1", id), performanceKeys("performance2", id)];
    expect(keys("3006")).toEqual([["Z"], ["W"]]); // Deck 1 Play/Pause
    expect(keys("3106")).toEqual([["N"], ["S"]]); // Deck 2 Play/Pause
    expect(keys("3007")).toEqual([["A"], ["I"]]); // Cue
    expect(keys("304b")).toEqual([["Q"], []]); // BEAT SYNC
    expect(keys("304d")).toEqual([["shift + 5"], ["F5"]]); // Master Tempo
    // The reference prints shift + O here; the binary adds shift + 0.
    expect(keys("314d")).toEqual([["shift + 0"], ["F10"]]);
    expect(keys("3021")).toEqual([["shift + command + 1"], ["shift + command + 1"]]); // Clear Hot Cue A
    expect(keys("3011")).toEqual([["command + 1"], ["option + 3"]]); // 1/2 Beat Loop
    expect(keys("3115")).toEqual([["command + 0"], []]); // Deck 2 8 Beat Loop
    expect(keys("300c")).toEqual([["G"], ["option + @"]]); // Exit/Reloop
    expect(keys("3000")).toEqual([["=", "shift + -"], ["shift + -"]]); // Zoom In: two keys in Performance 1
    expect(keys("b139")).toEqual([["command + L"], ["command + L"]]); // Locate track loaded on deck
    expect(keys("7000")).toEqual([["ctrl + F"], ["ctrl + F"]]); // macOS's control key
    expect(keys("1001")).toEqual([[], ["escape"]]); // Exit
    expect(keys("d0f0")).toEqual([["command + return"], ["command + N"]]); // Start/Pause REC
    // Decks 3 and 4 get no key, and None and Export are not Performance presets.
    expect(PERFORMANCE_KEYMAP.some((row) => /^3[24]/.test(row.id))).toBe(false);
    expect(performanceKeys("none", "3006")).toEqual([]);
    expect(performanceKeys("export", "3006")).toEqual([]);
  });

  it("reads every key it lists as a chord, but macOS's control key", () => {
    for (const row of PERFORMANCE_KEYMAP) {
      for (const text of [...row.p1, ...row.p2]) {
        const chord = chordFromText(text);
        if (text.includes("ctrl")) expect(chord, text).toBeNull();
        // This app's badges say "enter" and "esc" where rekordbox's say
        // "return" and "escape".
        else expect(describeChord(chord ?? { key: "?" }, MAC), text).toBe(text.replace("return", "enter").replace("escape", "esc"));
      }
    }
    expect(chordFromText("shift + command + cursor left")).toEqual({ key: "ArrowLeft", shiftKey: true, metaKey: true });
    expect(chordFromText("option + spacebar")).toEqual({ key: " ", altKey: true });
  });
});

describe("a Performance preset's keys on this app's decks (#412)", () => {
  /** The binding a preset gives a command here, or undefined when this app lacks it. */
  const built = (command: string) => BINDINGS.find((b) => b.command === command && b.action !== undefined);

  it("audits every entry: answered on rekordbox's key, held by the menu, or not built here", () => {
    const answered: Record<string, string[]> = { performance1: [], performance2: [] };
    const shell: Record<string, string[]> = { performance1: [], performance2: [] };
    for (const preset of PERFORMANCE) {
      for (const row of PERFORMANCE_KEYMAP) {
        const first = performanceKeys(preset, row.id)[0];
        const binding = built(row.id);
        if (first === undefined || binding === undefined) continue;
        const chord = chordFromText(first);
        if (chord !== null && heldByShell(chord)) {
          shell[preset]?.push(row.id);
          continue;
        }
        answered[preset]?.push(row.id);
        for (const platform of [MAC, WIN]) {
          const hit = matchBinding(press(first, platform.mac), platform, keyMap({ preset, overrides: {} }));
          expect(hit?.id, `${preset} ${row.name} ${first}`).toBe(binding.id);
        }
      }
    }
    expect(answered.performance1?.sort()).toEqual([
      "3006", "3007", "3008", "3009", "300a", "300b", "300c", "3012", "3013", "3014", "3015", "3018", "3019",
      "301e", "301f", "3020", "3021", "3022", "3023", "3024", "3039", "303a", "303b", "3043", "3044", "3045",
      "304b", "304d", "3054", "3055", "3106", "3107", "3108", "310a", "310b", "310c", "3118", "3119", "311e",
      "311f", "3120", "3121", "3122", "3123", "3124", "3139", "313a", "313b", "3143", "3144", "3145", "314b",
      "314d", "3154", "3155", "7003", "b139",
    ]);
    expect(answered.performance2?.sort()).toEqual([
      "3006", "3007", "300a", "300b", "300c", "3012", "3013", "301e", "301f", "3020", "3021", "3022", "3023",
      "304d", "3054", "3055", "307f", "308b", "3106", "3107", "310a", "310b", "310c", "3112", "3113", "311e",
      "311f", "3120", "3121", "3122", "3123", "314d", "3154", "3155", "317f", "318b", "7003", "b139",
    ]);
    // Deck 2's Jump Reverse and its 1 to 8 beat loops are command + O, 7, 8,
    // 9 and 0 in Performance 1, which this app's File and View menus hold.
    expect(shell.performance1?.sort()).toEqual(["3109", "3112", "3113", "3114", "3115"]);
    expect(shell.performance2).toEqual([]);
  });

  it("takes away the Export keys the Performance preset does not give", () => {
    for (const preset of PERFORMANCE) {
      const keys = keyMap({ preset, overrides: {} });
      for (const binding of BINDINGS) {
        if (binding.command === undefined || binding.action === undefined) continue;
        if (performanceKeys(preset, binding.command).length > 0) continue;
        expect(keys[binding.id] ?? binding.chord, `${preset} ${binding.label}`).toEqual({ key: "" });
      }
    }
    // Export's spacebar and F9 do nothing in Performance 1, and Z plays.
    const p1 = keyMap({ preset: "performance1", overrides: {} });
    expect(matchBinding({ key: " " }, MAC, p1)).toBeNull();
    expect(matchBinding({ key: "F9" }, MAC, p1)).toBeNull();
    expect(matchBinding({ key: "z" }, MAC, p1)).toMatchObject({ action: "playPause", deck: "a" });
    expect(matchBinding({ key: "n" }, WIN, p1)).toMatchObject({ action: "playPause", deck: "b" });
    expect(matchBinding({ key: "1", code: "Digit1", shiftKey: true, ctrlKey: true }, WIN, p1))
      .toMatchObject({ action: "clearHotCueA", deck: "a" });
    expect(matchBinding({ key: "4" }, MAC, p1)).toMatchObject({ action: "hotCueD", deck: "a" });
    expect(matchBinding({ key: "9" }, MAC, p1)).toMatchObject({ action: "hotCueD", deck: "b" });
  });

  it("gives Performance 2 its own deck keys, on alt for the beat loops on Windows", () => {
    const p2 = keyMap({ preset: "performance2", overrides: {} });
    expect(matchBinding({ key: "w" }, MAC, p2)).toMatchObject({ action: "playPause", deck: "a" });
    expect(matchBinding({ key: "s" }, MAC, p2)).toMatchObject({ action: "playPause", deck: "b" });
    expect(matchBinding({ key: "4", code: "Digit4", altKey: true }, WIN, p2)).toMatchObject({ action: "beatLoop1", deck: "a" });
    expect(matchBinding({ key: "0" }, MAC, p2)).toMatchObject({ action: "hotCueE", deck: "b" });
    expect(matchBinding({ key: "F10" }, MAC, p2)).toMatchObject({ action: "masterTempo", deck: "b" });
  });

  it("keeps this app's own keys and the menu's in every preset, and None takes rekordbox's away", () => {
    for (const preset of KEY_PRESETS) {
      const keys = keyMap({ preset, overrides: {} });
      expect(matchBinding({ key: "a", metaKey: true }, MAC, keys)?.action, preset).toBe("selectAll");
      expect(matchBinding({ key: "ArrowDown" }, MAC, keys)?.action, preset).toBe("moveDown");
      expect(matchBinding({ key: "Enter" }, MAC, keys)?.action, preset).toBe("loadPlayer1");
      expect(matchBinding({ key: "Enter", shiftKey: true }, MAC, keys)?.action, preset).toBe("loadPlayer2");
      for (const binding of BINDINGS) {
        if (binding.command === undefined || binding.action === undefined) {
          expect(presetChord(binding, preset).chord, `${preset} ${binding.id}`).toEqual(binding.chord);
        }
      }
    }
    const none = keyMap({ preset: "none", overrides: {} });
    expect(matchBinding({ key: " " }, MAC, none)).toBeNull();
    expect(matchBinding({ key: "f", metaKey: true }, MAC, none)).toBeNull();
    expect(keyMap({ preset: "export", overrides: {} })).toEqual({});
  });

  it("never gives one key two rows, in any preset, on either platform", () => {
    for (const preset of KEY_PRESETS) {
      const keys = keyMap({ preset, overrides: {} });
      for (const platform of [MAC, WIN]) {
        const seen = new Map<string, string>();
        for (const binding of BINDINGS) {
          if (binding.action === undefined) continue;
          const chord = keys[binding.id] ?? binding.chord;
          if (chord.key === "") continue;
          const name = describeChord(chord, platform);
          expect(seen.get(name), `${preset}: ${name} is ${seen.get(name)} and ${binding.id}`).toBeUndefined();
          seen.set(name, binding.id);
          expect(heldByShell(chord), `${preset}: ${binding.id} on the menu's ${name}`).toBe(false);
        }
      }
    }
  });

  it("puts the person's own keys on top, and takes their key from the row the preset gave it", () => {
    // Cue on Z, then Performance 1, whose Z is Play/Pause: Z stays Cue.
    const keys = keyMap({ preset: "performance1", overrides: { cue: { key: "z" } } });
    expect(matchBinding({ key: "z" }, MAC, keys)).toMatchObject({ action: "cue", deck: "a" });
    expect(keys.playPause).toEqual({ key: "" });
    // The pane's own edit: Play/Pause given back Z forgets the change.
    expect(assignChord({ playPause: { key: "p" } }, "playPause", { key: "z" }, "performance1")).toEqual({});
    // A key another row has under the preset moves to the row that took it,
    // but only the edited row is saved: the clash is the preset's, not theirs.
    expect(assignChord({}, "loopIn", { key: "a" }, "performance1")).toEqual({ loopIn: { key: "a" } });
    const p1 = keyMap({ preset: "performance1", overrides: { loopIn: { key: "a" } } });
    expect(dispatchBinding({ key: "a" }, MAC, null, p1)).toMatchObject({ action: "loopIn" });
    expect(p1.cue).toEqual({ key: "" });
    // A key the person saved on another row is theirs, so that row loses it.
    expect(assignChord({ cue: { key: "x" } }, "loopIn", { key: "x" }, "performance1"))
      .toEqual({ cue: { key: "" }, loopIn: { key: "x" } });
  });

  it("gives a row that lost its key to a clash its key back in a preset without the clash", () => {
    // Performance 1: Loop In takes Cue's A. Export: Cue is back on C.
    const overrides = assignChord({}, "loopIn", { key: "a" }, "performance1");
    const exportKeys = keyMap({ preset: "export", overrides });
    expect(exportKeys.cue ?? BINDINGS.find((b) => b.id === "cue")?.chord).toEqual({ key: "c" });
    expect(dispatchBinding({ key: "c" }, MAC, null, exportKeys)).toMatchObject({ action: "cue" });
    expect(dispatchBinding({ key: "a" }, MAC, null, exportKeys)).toMatchObject({ action: "loopIn" });
    // Export: Play/Pause takes Cue's C. Performance 1: Cue is back on A.
    const back = assignChord({}, "playPause", { key: "c" }, "export");
    expect(back).toEqual({ playPause: { key: "c" } });
    const p1Keys = keyMap({ preset: "performance1", overrides: back });
    expect(p1Keys.cue).toEqual({ key: "a" });
    expect(dispatchBinding({ key: "a" }, MAC, null, p1Keys)).toMatchObject({ action: "cue", deck: "a" });
  });
});

describe("the Keyboard pane under a preset (#412)", () => {
  it("shows each row's key in the chosen preset, greyed rows included", () => {
    const rows = (group: Parameters<typeof rowsFor>[0], preset: KeyPreset, mac = true) => rowsFor(group, mac, {}, preset);
    expect(rows("Player A", "performance1").find((r) => r.label === "Play/Pause")).toMatchObject({ key: "Z", built: true, changed: false });
    expect(rows("Player A", "performance2", false).find((r) => r.label === "1 Beat Loop")).toMatchObject({ key: "alt + 4" });
    // Time Mode is not built here; Performance 1 gives it no key, Export T.
    expect(rows("Player A", "performance1").find((r) => r.label === "Time Mode")).toMatchObject({ key: null, built: false });
    expect(rows("Player A", "export").find((r) => r.label === "Time Mode")).toMatchObject({ key: "T", built: false });
    // Next Track is not built here either, but shows Performance 1's key.
    expect(rows("Player A", "performance1").find((r) => r.label === "Next Track")).toMatchObject({ key: "option + A", built: false });
    // Set Hot Cue D is rekordbox's row, in its place after C.
    const labels = rows("Player A", "performance1").map((r) => r.label);
    expect(labels.indexOf("Set Hot Cue D")).toBe(labels.indexOf("Set Hot Cue C") + 1);
    expect(rows("Player A", "performance1").find((r) => r.label === "Set Hot Cue D")).toMatchObject({ key: "4", bindingId: "hotCueD" });
    // Player B's Jump Reverse is command + O, the menu's Import here.
    expect(rows("Player B", "performance1").find((r) => r.label === "Jump Reverse"))
      .toMatchObject({ key: null, shell: true, bindingId: "b.jumpBack" });
    // The menu's own rows keep the shell's keys.
    expect(rows("View", "performance1").find((r) => r.label === "1 Player")).toMatchObject({ key: "command + 7", built: true });
    expect(rows("Browse", "none").find((r) => r.label === "Select All")).toMatchObject({ key: "command + A" });
  });
});

describe("the stored preset", () => {
  it("is Export unless one of rekordbox's presets was stored", () => {
    expect(sanitisePreferences({}).keyboard).toEqual({ preset: "export", overrides: {} });
    expect(sanitisePreferences({ keyboard: { preset: "performance2" } }).keyboard.preset).toBe("performance2");
    expect(sanitisePreferences({ keyboard: { preset: "Performance 2 (Preset)" } }).keyboard.preset).toBe("export");
    expect(sanitisePreferences({ keyboard: { overrides: { cue: { key: "k" } } } }).keyboard)
      .toEqual({ preset: "export", overrides: { cue: { key: "k" } } });
  });
});
