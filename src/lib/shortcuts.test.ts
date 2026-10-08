import { describe, expect, it } from "vitest";

import {
  actionFor, beatLoopLength, BINDINGS, chordFromEvent, describeChord, dispatch, hotCuePad, isTyping,
  detectPlatform, eqKillBand, matchBinding, memoryCueNumber, menuAccelerator, sameChord, type Platform,
} from "./shortcuts";

const MAC: Platform = { mac: true };
const WIN: Platform = { mac: false };

describe("actionFor", () => {
  it("uses Command on macOS and Control elsewhere", () => {
    expect(actionFor({ key: "f", metaKey: true }, MAC)).toBe("focusSearch");
    expect(actionFor({ key: "f", ctrlKey: true }, WIN)).toBe("focusSearch");
  });

  it("does not fire on the other platform's modifier", () => {
    // Control-A on a Mac moves the caret to the start of the line; taking it
    // over would break that everywhere in the app.
    expect(actionFor({ key: "a", ctrlKey: true }, MAC)).toBeNull();
    expect(actionFor({ key: "f", ctrlKey: true }, MAC)).toBeNull();
    expect(actionFor({ key: "a", metaKey: true }, WIN)).toBeNull();
  });

  it("does not fire when both modifiers are held", () => {
    expect(actionFor({ key: "a", metaKey: true, ctrlKey: true }, MAC)).toBeNull();
    expect(actionFor({ key: "a", metaKey: true, ctrlKey: true }, WIN)).toBeNull();
  });

  it("is not case sensitive about the letter", () => {
    // Shift is not held, but some layouts report an uppercase key anyway.
    expect(actionFor({ key: "F", metaKey: true }, MAC)).toBe("focusSearch");
    expect(actionFor({ key: "A", metaKey: true }, MAC)).toBe("selectAll");
  });

  it("maps the movement keys, and shift extends instead of moving", () => {
    expect(actionFor({ key: "ArrowUp" }, MAC)).toBe("moveUp");
    expect(actionFor({ key: "ArrowDown" }, MAC)).toBe("moveDown");
    expect(actionFor({ key: "ArrowUp", shiftKey: true }, MAC)).toBe("extendUp");
    expect(actionFor({ key: "ArrowDown", shiftKey: true }, MAC)).toBe("extendDown");
    expect(actionFor({ key: "PageUp" }, MAC)).toBe("pageUp");
    expect(actionFor({ key: "PageDown" }, MAC)).toBe("pageDown");
  });

  it("sends Home and End, and the modified arrows, to the ends of the list", () => {
    expect(actionFor({ key: "Home" }, MAC)).toBe("toTop");
    expect(actionFor({ key: "End" }, MAC)).toBe("toBottom");
    expect(actionFor({ key: "ArrowUp", metaKey: true }, MAC)).toBe("toTop");
    expect(actionFor({ key: "ArrowDown", metaKey: true }, MAC)).toBe("toBottom");
    expect(actionFor({ key: "ArrowUp", ctrlKey: true }, WIN)).toBe("toTop");
  });

  it("loads Player 1 on plain Enter, but leaves modified Enter alone", () => {
    expect(actionFor({ key: "Enter" }, MAC)).toBe("loadPlayer1");
    expect(actionFor({ key: "Enter", shiftKey: true }, MAC)).toBe("loadPlayer1");
    expect(actionFor({ key: "Enter", metaKey: true }, MAC)).toBeNull();
    expect(actionFor({ key: "Enter", ctrlKey: true }, WIN)).toBeNull();
  });

  it("returns null for anything it does not claim", () => {
    // A claimed-but-unhandled key would be swallowed, and browser and OS
    // shortcuts would stop working inside the app. The arrows are claimed now
    // — rekordbox's Export map puts beat jump on them — so they are not here.
    for (const chord of [
      { key: "q", metaKey: true },
      { key: "r", metaKey: true },
      { key: "Tab" },
      { key: "z" },
      { key: "F5" },
      { key: "a", metaKey: true, altKey: true },
    ]) {
      expect(actionFor(chord, MAC)).toBeNull();
    }
  });
});

describe("detectPlatform", () => {
  it("identifies Linux separately from macOS and Windows", () => {
    const originalPlatform = navigator.platform;
    const originalUserAgent = navigator.userAgent;
    Object.defineProperty(navigator, "platform", { configurable: true, value: "Linux armv8l" });
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: "Mozilla/5.0 Linux" });
    expect(detectPlatform()).toEqual({ mac: false, linux: true });
    Object.defineProperty(navigator, "platform", { configurable: true, value: originalPlatform });
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: originalUserAgent });
  });
});

describe("dispatch", () => {
  const field = { tagName: "INPUT" };
  const list = { tagName: "DIV" };

  it("lets a field keep its own arrow keys and selection", () => {
    expect(dispatch({ key: "ArrowDown" }, MAC, field)).toBeNull();
    expect(dispatch({ key: "a", metaKey: true }, MAC, field)).toBeNull();
    // Enter in the search box is the box's, not a load of Player 1.
    expect(dispatch({ key: "Enter" }, MAC, field)).toBeNull();
  });

  it("still focuses and clears the search from inside a field", () => {
    expect(dispatch({ key: "f", metaKey: true }, MAC, field)).toBe("focusSearch");
    expect(dispatch({ key: "Escape" }, MAC, field)).toBe("clearSearch");
  });

  it("applies every action outside a field", () => {
    expect(dispatch({ key: "ArrowDown" }, MAC, list)).toBe("moveDown");
    expect(dispatch({ key: "a", metaKey: true }, MAC, null)).toBe("selectAll");
  });
});

describe("menuAccelerator", () => {
  it("routes Windows history keys, leaving macOS to the native menu", () => {
    expect(menuAccelerator({ key: "z", ctrlKey: true }, WIN)).toBe("undo");
    expect(menuAccelerator({ key: "Z", ctrlKey: true, shiftKey: true }, WIN)).toBe("redo");
    expect(menuAccelerator({ key: "y", ctrlKey: true }, WIN)).toBe("redo");
    expect(menuAccelerator({ key: "z", metaKey: true }, MAC)).toBeNull();
    expect(menuAccelerator({ key: "Z", metaKey: true, shiftKey: true }, MAC)).toBeNull();
  });
  it("names the menu item a Windows accelerator stands for", () => {
    expect(menuAccelerator({ key: ",", ctrlKey: true }, WIN)).toBe("settings");
    expect(menuAccelerator({ key: "o", ctrlKey: true }, WIN)).toBe("import");
    expect(menuAccelerator({ key: "i", ctrlKey: true }, WIN)).toBe("info");
    expect(menuAccelerator({ key: "b", ctrlKey: true }, WIN)).toBe("sub");
    expect(menuAccelerator({ key: "7", ctrlKey: true }, WIN)).toBe("layout-one");
    expect(menuAccelerator({ key: "8", ctrlKey: true }, WIN)).toBe("layout-two");
    expect(menuAccelerator({ key: "9", ctrlKey: true }, WIN)).toBe("layout-simple");
    expect(menuAccelerator({ key: "0", ctrlKey: true }, WIN)).toBe("layout-browser");
  });

  it("does nothing on macOS, where the menu handles its own key equivalents", () => {
    expect(menuAccelerator({ key: ",", metaKey: true }, MAC)).toBeNull();
    expect(menuAccelerator({ key: ",", ctrlKey: true }, MAC)).toBeNull();
  });

  it("leaves other chords alone, full screen included", () => {
    expect(menuAccelerator({ key: "f", ctrlKey: true, shiftKey: true }, WIN)).toBeNull();
    expect(menuAccelerator({ key: ",", ctrlKey: true, altKey: true }, WIN)).toBeNull();
    expect(menuAccelerator({ key: "," }, WIN)).toBeNull();
    expect(menuAccelerator({ key: "x", ctrlKey: true }, WIN)).toBeNull();
  });
});

describe("isTyping", () => {
  it("recognises the elements that own their own keys", () => {
    for (const tagName of ["INPUT", "TEXTAREA", "SELECT", "input"]) {
      expect(isTyping({ tagName })).toBe(true);
    }
    expect(isTyping({ tagName: "DIV" })).toBe(false);
    expect(isTyping(null)).toBe(false);
    expect(isTyping(undefined)).toBe(false);
    expect(isTyping({})).toBe(false);
  });

  it("recognises a contenteditable element whatever its tag", () => {
    expect(isTyping({ tagName: "DIV", isContentEditable: true })).toBe(true);
  });
});

describe("rekordbox's own Export key map", () => {
  // Transcribed from `KeyMappings/rekordbox_0000000000030.mappings`, the key
  // map rekordbox ships for the mode this clones. The keys are its, not ours.
  const mac = { mac: true };

  it("gives the deck the keys rekordbox gives it", () => {
    expect(actionFor({ key: " " }, mac)).toBe("playPause");
    expect(actionFor({ key: "c" }, mac)).toBe("cue");
    expect(actionFor({ key: "q" }, mac)).toBe("quantize");
    expect(actionFor({ key: "ArrowLeft" }, mac)).toBe("jumpBack");
    expect(actionFor({ key: "ArrowRight" }, mac)).toBe("jumpForward");
  });

  it("gives the MEMORY cluster M, B, N and X", () => {
    // `M` Memory Cue, `B` Call Previous Memory Cue, `N` Call Next Memory
    // Cue, `X` Delete Memory Cue — the Export preset's own bindings.
    expect(actionFor({ key: "m" }, mac)).toBe("memoryCue");
    expect(actionFor({ key: "b" }, mac)).toBe("previousMemoryCue");
    expect(actionFor({ key: "n" }, mac)).toBe("nextMemoryCue");
    expect(actionFor({ key: "x" }, mac)).toBe("deleteMemoryCue");
    // ⌘X is cut, and ⌘M minimises the window.
    expect(actionFor({ key: "x", metaKey: true }, mac)).not.toBe("deleteMemoryCue");
    expect(actionFor({ key: "m", metaKey: true }, mac)).not.toBe("memoryCue");
    expect(dispatch({ key: "x" }, mac, { tagName: "INPUT" })).toBeNull();
  });

  it("gives the first three pads 1, 2 and 3, and their clears the same with command", () => {
    // `Set Hot Cue A`-`C` on `1`-`3`, `Clear Hot Cue A`-`C` on `command + 1`-`3`.
    // The preset binds nothing past C: 4 is the first beat loop.
    expect(actionFor({ key: "1" }, mac)).toBe("hotCueA");
    expect(actionFor({ key: "2" }, mac)).toBe("hotCueB");
    expect(actionFor({ key: "3" }, mac)).toBe("hotCueC");
    expect(actionFor({ key: "1", metaKey: true }, mac)).toBe("clearHotCueA");
    expect(actionFor({ key: "3", metaKey: true }, mac)).toBe("clearHotCueC");
    expect(actionFor({ key: "3", ctrlKey: true }, { mac: false })).toBe("clearHotCueC");
    expect(actionFor({ key: "4" }, mac)).toBe("beatLoop1");
    expect(actionFor({ key: "4", metaKey: true }, mac)).toBeNull();
    // Typing a digit into the search box is typing.
    expect(dispatch({ key: "1" }, mac, { tagName: "INPUT" })).toBeNull();
    expect(hotCuePad("hotCueB")).toEqual({ letter: "B", clear: false });
    expect(hotCuePad("clearHotCueC")).toEqual({ letter: "C", clear: true });
    expect(hotCuePad("cue")).toBeNull();
  });

  it("lists hot cue pads D to H and every clear as unbound rows the pane can assign", () => {
    const own = BINDINGS.filter((b) => b.pane !== undefined && hotCuePad(b.action ?? "cue") !== null);
    const names = (deck: "a" | "b") => own.filter((b) => b.deck === deck).map((b) => b.action).sort();
    const letters = ["D", "E", "F", "G", "H"];
    const setD2H = letters.map((l) => `hotCue${l}`);
    const clearD2H = letters.map((l) => `clearHotCue${l}`);
    expect(names("a")).toEqual([...clearD2H, ...setD2H].sort());
    // The preset gives Player B no clears at all, so A to C are added there too.
    expect(names("b")).toEqual([...clearD2H, ...setD2H, "clearHotCueA", "clearHotCueB", "clearHotCueC"].sort());
    for (const row of own) expect(row.chord.key).toBe("");
    expect(new Set(BINDINGS.map((b) => b.id)).size).toBe(BINDINGS.length);
    expect(hotCuePad("hotCueH")).toEqual({ letter: "H", clear: false });
    expect(hotCuePad("clearHotCueF")).toEqual({ letter: "F", clear: true });
    // Nothing fires until a key is assigned.
    expect(actionFor({ key: "" }, mac)).toBeNull();
  });

  it("fires a pad D to H or a Player B clear once the person assigns a key", () => {
    const overrides = {
      hotCueH: { key: "8", altKey: true },
      "b.clearHotCueA": { key: "x", shiftKey: true, metaKey: true },
    };
    expect(matchBinding({ key: "8", altKey: true }, mac, overrides)).toMatchObject({ action: "hotCueH", deck: "a" });
    expect(matchBinding({ key: "x", shiftKey: true, metaKey: true }, mac, overrides))
      .toMatchObject({ action: "clearHotCueA", deck: "b" });
  });

  it("gives the loop I, O and R, the beat loops 4 to 9, and / and option + \\ the length", () => {
    expect(actionFor({ key: "i" }, mac)).toBe("loopIn");
    expect(actionFor({ key: "o" }, mac)).toBe("loopOut");
    expect(actionFor({ key: "r" }, mac)).toBe("reloop");
    expect(actionFor({ key: "9" }, mac)).toBe("beatLoop32");
    expect(beatLoopLength("beatLoop16")).toBe(16);
    expect(beatLoopLength("loopIn")).toBeNull();
    expect(actionFor({ key: "/" }, mac)).toBe("loopHalf");
    // Option + \\ on a US Mac reports « as the key; the code says which key it was.
    expect(actionFor({ key: "«", code: "Backslash", altKey: true }, mac)).toBe("loopDouble");
  });

  it("calls the first ten memory cues on A to ;, and gives the tempo the function keys", () => {
    expect(actionFor({ key: "a" }, mac)).toBe("callMemoryCue1");
    expect(actionFor({ key: ";" }, mac)).toBe("callMemoryCue10");
    expect(memoryCueNumber("callMemoryCue7")).toBe(7);
    expect(memoryCueNumber("cue")).toBeNull();
    expect(actionFor({ key: "F1" }, mac)).toBe("sync");
    expect(actionFor({ key: "F2" }, mac)).toBe("masterTempo");
    expect(actionFor({ key: "F3" }, mac)).toBe("tempoReset");
    expect(actionFor({ key: "F9" }, mac)).toBe("metronomeSound");
    // Analysis moved off A to make room, to shift + command + A.
    expect(actionFor({ key: "a", metaKey: true, shiftKey: true }, mac)).toBe("analyseSelection");
  });

  it("is Player B's with shift, and shift + 1 is still 1", () => {
    expect(matchBinding({ key: " ", shiftKey: true }, mac)).toMatchObject({ action: "playPause", deck: "b" });
    expect(matchBinding({ key: " " }, mac)).toMatchObject({ action: "playPause", deck: "a" });
    expect(matchBinding({ key: "!", code: "Digit1", shiftKey: true }, mac)).toMatchObject({ action: "hotCueA", deck: "b" });
    // Shift + cursor up extends the browser's selection; it is not Player B's.
    expect(actionFor({ key: "ArrowUp", shiftKey: true }, mac)).toBe("extendUp");
    expect(matchBinding({ key: "ArrowRight", shiftKey: true }, mac)).toMatchObject({ action: "jumpForward", deck: "b" });
    // The master's keys.
    expect(actionFor({ key: "F12", metaKey: true }, mac)).toBe("volumeUp");
    expect(actionFor({ key: "F10", metaKey: true }, mac)).toBe("mute");
  });

  it("reads a key of the person's own in place of the preset's", () => {
    const overrides = { loopIn: { key: "l", shiftKey: true }, loopOut: { key: "" } };
    expect(actionFor({ key: "i" }, mac, overrides)).toBeNull();
    expect(actionFor({ key: "l", shiftKey: true }, mac, overrides)).toBe("loopIn");
    // A key taken away answers to nothing; Player B's shift + L is untouched.
    expect(actionFor({ key: "o" }, mac, overrides)).toBeNull();
    expect(matchBinding({ key: "l", shiftKey: true }, mac, {})).toMatchObject({ deck: "b" });
    expect(dispatch({ key: "l", shiftKey: true }, mac, { tagName: "INPUT" }, overrides)).toBeNull();
  });

  it("turns an event into a chord to store: the key the physical key stands for", () => {
    expect(chordFromEvent({ key: "!", code: "Digit1", shiftKey: true }, mac)).toEqual({ key: "1", shiftKey: true });
    expect(chordFromEvent({ key: "F", code: "KeyF", metaKey: true }, mac)).toEqual({ key: "f", metaKey: true });
    expect(chordFromEvent({ key: "f", code: "KeyF", ctrlKey: true }, { mac: false })).toEqual({ key: "f", metaKey: true });
    expect(chordFromEvent({ key: "Shift", shiftKey: true }, mac)).toBeNull();
    expect(chordFromEvent({ key: "f", ctrlKey: true }, mac)).toBeNull();
    expect(sameChord({ key: "F", metaKey: true }, { key: "f", metaKey: true, shiftKey: false })).toBe(true);
  });

  it("puts the three cue lists on F10, F11 and F12", () => {
    expect(actionFor({ key: "F10" }, mac)).toBe("showMemory");
    expect(actionFor({ key: "F11" }, mac)).toBe("showHotCues");
    expect(actionFor({ key: "F12" }, mac)).toBe("showInfo");
  });

  it("leaves the deck's letters alone when a modifier is held", () => {
    // ⌘C is copy, and ⌘Q quits. Taking either would be a bug people notice at
    // the worst moment.
    expect(actionFor({ key: "c", metaKey: true }, mac)).not.toBe("cue");
    expect(actionFor({ key: "q", metaKey: true }, mac)).not.toBe("quantize");
  });

  it("does not fire the deck's letters into a search box", () => {
    expect(dispatch({ key: "c" }, mac, { tagName: "INPUT" })).toBeNull();
    expect(dispatch({ key: " " }, mac, { tagName: "INPUT" })).toBeNull();
  });
});

describe("the Keyboard pane's bindings", () => {
  const mac: Platform = { mac: true };
  const windows: Platform = { mac: false };

  it("prints a chord the way rekordbox's badges do", () => {
    expect(describeChord({ key: " " }, mac)).toBe("spacebar");
    expect(describeChord({ key: "q" }, mac)).toBe("Q");
    expect(describeChord({ key: "ArrowDown", metaKey: true }, mac)).toBe("command + cursor down");
    expect(describeChord({ key: "ArrowDown", metaKey: true }, windows)).toBe("ctrl + cursor down");
    expect(describeChord({ key: "f", metaKey: true, shiftKey: true }, mac)).toBe("shift + command + F");
    expect(describeChord({ key: "F10" }, mac)).toBe("F10");
  });

  it("lists only chords the map actually answers to, under the deck and the browser", () => {
    // The menu accelerators are the shell's, and A is the track list's own
    // key for analysis; neither goes through the map.
    const mapped = BINDINGS.filter((b) => b.group !== "Menu" && b.chord.key !== "");
    for (const binding of mapped) {
      const chord = { ...binding.chord, metaKey: binding.chord.metaKey ?? false };
      expect(actionFor(chord, mac), binding.label).not.toBeNull();
    }
  });
});

describe("the GRID panel's keys", () => {
  it("opens the panel and shifts the grid with the modifier held, as the Export preset binds them", () => {
    expect(actionFor({ key: "g", metaKey: true }, MAC)).toBe("adjustGrid");
    expect(actionFor({ key: "g", ctrlKey: true }, WIN)).toBe("adjustGrid");
    expect(actionFor({ key: "ArrowLeft", metaKey: true }, MAC)).toBe("shiftGridLeft");
    expect(actionFor({ key: "ArrowRight", metaKey: true }, MAC)).toBe("shiftGridRight");
    expect(actionFor({ key: "ArrowRight", ctrlKey: true }, WIN)).toBe("shiftGridRight");
  });

  it("leaves the bare arrows to the deck's jumps and the browser's cursor", () => {
    expect(actionFor({ key: "ArrowLeft" }, MAC)).toBe("jumpBack");
    expect(actionFor({ key: "ArrowRight" }, MAC)).toBe("jumpForward");
    // With shift it is Player B's shift, as the preset binds it.
    expect(matchBinding({ key: "ArrowLeft", metaKey: true, shiftKey: true }, MAC)).toMatchObject({
      action: "shiftGridLeft", deck: "b",
    });
    expect(matchBinding({ key: "g", metaKey: true, shiftKey: true }, MAC)).toBeNull();
  });

  it("puts the nearest beat under the playhead on option + command + backslash, by key or by code", () => {
    expect(actionFor({ key: "\\", metaKey: true, altKey: true }, MAC)).toBe("shiftGridToCenter");
    // Option rewrites the character on some layouts; the physical key still says.
    expect(actionFor({ key: "«", code: "Backslash", metaKey: true, altKey: true }, MAC)).toBe("shiftGridToCenter");
    expect(actionFor({ key: "\\", ctrlKey: true, altKey: true }, WIN)).toBe("shiftGridToCenter");
    expect(actionFor({ key: "\\", metaKey: true }, MAC)).toBeNull();
    // Without the command it is the loop's doubling, from the same preset.
    expect(actionFor({ key: "\\", altKey: true }, MAC)).toBe("loopDouble");
  });

  it("lists the four under Player A with rekordbox's command ids", () => {
    const ids = BINDINGS.filter((b) => b.group === "Player A").map((b) => b.command);
    for (const id of ["303e", "3043", "3044", "3045"]) expect(ids).toContain(id);
    expect(describeChord({ key: "\\", metaKey: true, altKey: true }, MAC)).toBe("option + command + \\");
    expect(describeChord({ key: "ArrowLeft", metaKey: true }, WIN)).toBe("ctrl + cursor left");
  });
});

describe("the mixer's EQ kill keys", () => {
  it("are listed for both decks but start unbound, rekordbox's preset having none", () => {
    const kills = BINDINGS.filter((b) => eqKillBand(b.action ?? "cue") !== null);
    expect(kills.map((b) => `${b.deck}:${b.action}`).sort()).toEqual([
      "a:eqKillHigh", "a:eqKillLow", "a:eqKillMid", "b:eqKillHigh", "b:eqKillLow", "b:eqKillMid",
    ]);
    for (const kill of kills) expect(kill.chord.key).toBe("");
    // An unbound row matches no key, not even an empty one.
    expect(actionFor({ key: "" }, MAC)).toBeNull();
  });

  it("answer to the key the person gives them, for the deck it is filed under", () => {
    const overrides = { "b.eqKillMid": { key: "u", shiftKey: true } };
    expect(matchBinding({ key: "U", shiftKey: true }, MAC, overrides)).toMatchObject({ action: "eqKillMid", deck: "b" });
    expect(matchBinding({ key: "u", shiftKey: true }, MAC)).toBeNull();
  });

  it("names the band an action toggles", () => {
    expect(eqKillBand("eqKillLow")).toBe("low");
    expect(eqKillBand("eqKillMid")).toBe("mid");
    expect(eqKillBand("eqKillHigh")).toBe("high");
    expect(eqKillBand("cue")).toBeNull();
  });
});
