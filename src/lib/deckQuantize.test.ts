// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_DECK_QUANTIZE, loadDeckQuantize, saveDeckQuantize } from "./deckQuantize";

describe("deck quantize memory (#277)", () => {
  beforeEach(() => localStorage.clear());

  it("starts on, as rekordbox does with no Player%d_Quantize saved", () => {
    expect(DEFAULT_DECK_QUANTIZE).toBe(true);
    expect(loadDeckQuantize("a")).toBe(true);
    expect(loadDeckQuantize("b")).toBe(true);
  });

  it("keeps Q off once turned off, for that deck only", () => {
    saveDeckQuantize("a", false);
    expect(loadDeckQuantize("a")).toBe(false);
    expect(loadDeckQuantize("b")).toBe(true);
    saveDeckQuantize("b", false);
    saveDeckQuantize("a", true);
    expect(loadDeckQuantize("a")).toBe(true);
    expect(loadDeckQuantize("b")).toBe(false);
  });

  it("falls back to on for a damaged or foreign value", () => {
    localStorage.setItem("rbl.deckQuantize.v1", "{not json");
    expect(loadDeckQuantize("a")).toBe(true);
    localStorage.setItem("rbl.deckQuantize.v1", JSON.stringify({ a: "off", b: 0 }));
    expect(loadDeckQuantize("a")).toBe(true);
    expect(loadDeckQuantize("b")).toBe(true);
    // A damaged store is replaced, not merged into, on the next press.
    localStorage.setItem("rbl.deckQuantize.v1", "[]");
    saveDeckQuantize("b", false);
    expect(loadDeckQuantize("b")).toBe(false);
  });
});
