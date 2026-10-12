import { describe, expect, it } from "vitest";
import { autoscrollStep } from "./dragAutoscroll";

describe("autoscrollStep", () => {
  it("is idle in the middle", () => expect(autoscrollStep(300, 100, 500)).toBe(0));
  it("scrolls up near the top, faster the closer to the edge", () => {
    expect(autoscrollStep(130, 100, 500)).toBeLessThan(0);
    expect(autoscrollStep(100, 100, 500)).toBeLessThan(autoscrollStep(130, 100, 500));
  });
  it("scrolls down near the bottom", () => expect(autoscrollStep(495, 100, 500)).toBeGreaterThan(0));
  it("keeps scrolling when the pointer is past the edge, capped", () => {
    expect(autoscrollStep(0, 100, 500)).toBe(-24);
    expect(autoscrollStep(900, 100, 500)).toBe(24);
  });
});
