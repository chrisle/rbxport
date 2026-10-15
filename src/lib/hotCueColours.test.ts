import { describe, expect, it } from "vitest";

import { HOT_CUE_DEFAULT_INDICES, hotCueColour, hotCueDefaultIndex } from "./hotCueColours";

describe("HOT CUE color defaults (#442)", () => {
  it("pins rekordbox 7.2.19's four 17-byte tables, A to P then the loop", () => {
    // Bytes read at the hotCueColorIndex* symbols; entry 16 is 0x24 in all four.
    const bytes = (mode: keyof typeof HOT_CUE_DEFAULT_INDICES) =>
      [...HOT_CUE_DEFAULT_INDICES[mode], hotCueDefaultIndex("A", true, mode)]
        .map((value) => value.toString(16).padStart(2, "0")).join(" ");
    expect(bytes("cdj")).toBe("16 16 16 16 16 16 16 16 1a 1a 1a 1a 1a 1a 1a 1a 24");
    expect(bytes("cold1")).toBe("04 12 3a 08 0c 3c 0e 01 04 12 3a 08 0c 3c 0e 01 24");
    expect(bytes("cold2")).toBe("0e 0b 08 05 03 01 3d 3b 0e 0b 08 05 03 01 3d 3b 24");
    expect(bytes("colorful")).toBe("2b 08 17 3c 13 26 01 20 2b 08 17 3c 13 26 01 20 24");
  });

  it("draws an uncoloured COLORFUL cue in its slot's colour, not green", () => {
    // #440's reporter: "red for A, light blue for B, green for C".
    expect(["A", "B", "C", "D"].map((letter) => hotCueColour(null, letter, false, "colorful")))
      .toEqual(["#FF376F", "#45ACDB", "#7DC13D", "#AA72FF"]);
    expect(hotCueColour(null, "I", false, "colorful")).toBe("#FF376F");
  });

  it("draws an uncoloured hot loop orange in every mode", () => {
    for (const mode of ["cdj", "cold1", "cold2", "colorful"] as const) {
      expect(hotCueColour(null, "B", true, mode)).toBe("#FF8C00");
    }
  });

  it("keeps a cue's own colour whatever the mode", () => {
    for (const mode of ["cdj", "cold1", "cold2", "colorful"] as const) {
      expect(hotCueColour("#E62828", "A", false, mode)).toBe("#E62828");
    }
  });
});
