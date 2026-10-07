import { describe, expect, it } from "vitest";

import { clampWidth, fit, subBounds, subTreeBounds, TREE_BOUNDS, type SplitterBounds } from "./splitter";

const wide: SplitterBounds = { ...TREE_BOUNDS, available: 1800 };

describe("clampWidth", () => {
  it("leaves a reasonable width alone", () => {
    expect(clampWidth(305, wide)).toBe(305);
  });

  it("holds the floor", () => {
    expect(clampWidth(10, wide)).toBe(TREE_BOUNDS.min);
    expect(clampWidth(-500, wide)).toBe(TREE_BOUNDS.min);
  });

  it("never lets the panel take more than half the window", () => {
    expect(clampWidth(5000, wide)).toBeLessThanOrEqual(1800 * 0.5);
  });

  it("leaves the other pane its minimum", () => {
    // 1000 wide: half is 500, but the list wants 360, so the ceiling is 640 —
    // half wins here.
    const bounds = { ...TREE_BOUNDS, available: 1000 };
    const got = clampWidth(900, bounds);
    expect(1000 - got).toBeGreaterThanOrEqual(TREE_BOUNDS.minOther);
  });

  it("gives the panel its floor when the window cannot satisfy both", () => {
    // 500 wide: the list alone wants 360, leaving 140, at the 140 floor.
    // The panel takes the floor and the list scrolls.
    const bounds = { ...TREE_BOUNDS, available: 500 };
    expect(clampWidth(300, bounds)).toBe(TREE_BOUNDS.min);
  });

  it("does not return more than the window when it is tiny", () => {
    const bounds = { ...TREE_BOUNDS, available: 100 };
    expect(clampWidth(300, bounds)).toBeLessThanOrEqual(100);
  });

  it("returns the floor rather than NaN before the window is measured", () => {
    expect(clampWidth(305, { ...TREE_BOUNDS, available: 0 })).toBe(TREE_BOUNDS.min);
    expect(clampWidth(Number.NaN, wide)).toBe(TREE_BOUNDS.min);
  });

  it("returns whole pixels", () => {
    const got = clampWidth(305.7, wide);
    expect(Number.isInteger(got)).toBe(true);
  });
});

describe("fit", () => {
  it("pulls a stored width back in when the window shrinks", () => {
    const stored = clampWidth(700, wide);
    expect(stored).toBe(700);
    // The same width in a much smaller window has to come back down.
    const shrunk = fit(stored, { ...TREE_BOUNDS, available: 900 });
    expect(shrunk).toBeLessThan(stored);
    expect(900 - shrunk).toBeGreaterThanOrEqual(TREE_BOUNDS.minOther);
  });

  it("leaves a width that still fits", () => {
    expect(fit(305, wide)).toBe(305);
  });
});

describe("subBounds", () => {
  it("keeps a list beside the sub-browser", () => {
    const bounds = subBounds(1462);
    const got = clampWidth(5000, bounds);
    expect(1462 - got).toBeGreaterThanOrEqual(TREE_BOUNDS.minOther);
    expect(got).toBeLessThanOrEqual(1462 * 0.65);
  });

  it("opens at the measured width in the reference window", () => {
    // 878pt of the 1462 the two lists share at 1800 wide with the tree at
    // its default: fits, unchanged.
    expect(clampWidth(878, subBounds(1462))).toBe(878);
  });

  it("gives the sub-browser its floor in a window too narrow for both", () => {
    expect(clampWidth(878, subBounds(500))).toBe(360);
  });
});

describe("subTreeBounds", () => {
  it("is the tree rule applied to the sub-browser's own width", () => {
    const bounds = subTreeBounds(878);
    expect(clampWidth(298, bounds)).toBe(298);
    expect(clampWidth(2000, bounds)).toBeLessThanOrEqual(878 * 0.5);
  });
});
