import { describe, expect, it } from "vitest";

import { backingSize, exactBackingSize, MAX_RATIO, MAX_WIDTH, STEP } from "./canvasSize";

describe("backingSize", () => {
  it("follows the element rather than a fixed number", () => {
    expect(backingSize(400, 14, 1).width).toBe(400);
    expect(backingSize(1200, 14, 1).width).toBe(1200);
  });

  it("doubles for a retina display", () => {
    expect(backingSize(400, 14, 2)).toEqual({ width: 800, height: 28 });
  });

  it("stops at 2x, because a 14-pixel strip cannot show more", () => {
    expect(backingSize(400, 14, 3)).toEqual(backingSize(400, 14, MAX_RATIO));
  });

  it("quantises the width so a drag does not reallocate on every pixel", () => {
    const a = backingSize(401, 14, 1).width;
    const b = backingSize(400 + STEP - 1, 14, 1).width;
    expect(a).toBe(b);
    expect(a % STEP).toBe(0);
  });

  it("never asks for a buffer a layout bug could make enormous", () => {
    expect(backingSize(100_000, 14, 2).width).toBe(MAX_WIDTH);
  });

  it("survives a zero-sized or absent measurement", () => {
    expect(backingSize(0, 0, 1)).toEqual({ width: STEP, height: 1 });
    expect(backingSize(Number.NaN, 14, Number.NaN).width).toBeGreaterThan(0);
  });
});

describe("exactBackingSize", () => {
  it("is the element's device pixels, not a step wider", () => {
    // backingSize would give 2,368pt here: 28 device pixels squeezed away.
    expect(exactBackingSize(2354, 133, 2)).toEqual({ width: 4708, height: 266 });
    expect(exactBackingSize(1177, 133, 1)).toEqual({ width: 1177, height: 133 });
  });

  it("keeps backingSize's ratio cap, width cap and bad-measurement floor", () => {
    expect(exactBackingSize(400, 14, 3)).toEqual(exactBackingSize(400, 14, MAX_RATIO));
    expect(exactBackingSize(100_000, 14, 2).width).toBe(MAX_WIDTH);
    expect(exactBackingSize(0, 0, 1)).toEqual({ width: 1, height: 1 });
    expect(exactBackingSize(Number.NaN, 14, Number.NaN).width).toBeGreaterThan(0);
  });
});
