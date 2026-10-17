import { describe, expect, it } from "vitest";

import { MEMORY_CUE_COLOURS, memoryCueListColour, memoryCueMarkerColour, rowMemoryCueColours } from "./memoryCueColours";

describe("memory cue colours", () => {
  it("lists rekordbox's eight in djmdCue.Color order", () => {
    // skins/p2254_mempink.png ... p2261_mempurple.png, which
    // BrowseHelper::getCueImageForPreview indexes by Color 0-7.
    expect(MEMORY_CUE_COLOURS.map(([name]) => name)).toEqual(
      ["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"]);
  });

  it("draws a waveform marker in the cue's own colour", () => {
    for (const [, colour] of MEMORY_CUE_COLOURS.filter(([name]) => name !== "Red")) {
      expect(memoryCueMarkerColour(colour)).toBe(colour);
    }
    expect(memoryCueMarkerColour("#204fef")).toBe("#204FEF");
  });

  it("draws red, and no colour, in the waveform sprite's red", () => {
    // WidgetWaveView::drawHotCueGL and WidgetLoudView::drawTextures take the
    // sprite's red row for any Color above 7, 255 being none.
    expect(memoryCueMarkerColour("#E33122")).toBe("#EA3323");
    expect(memoryCueMarkerColour(null)).toBe("#EA3323");
    expect(memoryCueMarkerColour(undefined)).toBe("#EA3323");
  });

  it("reads djmdCue.Color as rekordbox wrote it on the rig's test library", () => {
    // Memory cues coloured Blue, Green and Pink and one left alone: Color 6,
    // 4, 0 and -1 (rekordbox 7.2.14, chris-win11, issue #533).
    expect([6, 4, 0, -1, 255, undefined].map(memoryCueListColour))
      .toEqual(["#204FEF", "#66DD42", "#E778F1", null, null, null]);
  });

  it("builds a row's colours as the backend does, and none when none has one", () => {
    expect(rowMemoryCueColours(["#204FEF", null, "#e778f1"])).toEqual([6, 255, 0]);
    expect(rowMemoryCueColours([null, null])).toBeUndefined();
    expect(rowMemoryCueColours([])).toBeUndefined();
  });
});
