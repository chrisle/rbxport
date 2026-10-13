import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AUTOSCROLL_BORDER_PX,
  AUTOSCROLL_MAX_STEP_PX,
  AUTOSCROLL_REPEAT_MS,
  autoscrollStep,
  createDragAutoscroll,
} from "./dragAutoscroll";

// Rows from y=100 to y=500 (400px tall).
const TOP = 100;
const BOTTOM = 500;

describe("autoscrollStep", () => {
  it("uses rekordbox's ListViewer::itemDragMove values (30px zone, 50px cap, 100ms repeat)", () => {
    expect(AUTOSCROLL_BORDER_PX).toBe(30);
    expect(AUTOSCROLL_MAX_STEP_PX).toBe(50);
    expect(AUTOSCROLL_REPEAT_MS).toBe(100);
  });

  it("is idle clear of both zones", () => {
    expect(autoscrollStep(130, TOP, BOTTOM)).toBe(0);
    expect(autoscrollStep(300, TOP, BOTTOM)).toBe(0);
    expect(autoscrollStep(469, TOP, BOTTOM)).toBe(0);
  });

  it("scrolls up by how far the pointer is into the top zone", () => {
    expect(autoscrollStep(129, TOP, BOTTOM)).toBe(-1);
    expect(autoscrollStep(110, TOP, BOTTOM)).toBe(-20);
    expect(autoscrollStep(100, TOP, BOTTOM)).toBe(-30);
  });

  it("scrolls down by how far the pointer is into the bottom zone", () => {
    expect(autoscrollStep(470, TOP, BOTTOM)).toBe(0);
    expect(autoscrollStep(471, TOP, BOTTOM)).toBe(1);
    expect(autoscrollStep(499, TOP, BOTTOM)).toBe(29);
  });

  it("speeds up past the edge, to at most 50px a step", () => {
    expect(autoscrollStep(90, TOP, BOTTOM)).toBe(-40);
    expect(autoscrollStep(80, TOP, BOTTOM)).toBe(-50);
    expect(autoscrollStep(0, TOP, BOTTOM)).toBe(-50);
    expect(autoscrollStep(510, TOP, BOTTOM)).toBe(40);
    expect(autoscrollStep(900, TOP, BOTTOM)).toBe(50);
  });

  it("lets the top zone win when the list is too short for both", () => {
    // 50px of rows: y=25 is inside both zones.
    expect(autoscrollStep(125, 100, 150)).toBe(-5);
    expect(autoscrollStep(140, 100, 150)).toBe(20);
  });
});

describe("createDragAutoscroll", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  const setup = () => {
    const steps: number[] = [];
    const auto = createDragAutoscroll(() => ({ top: TOP, bottom: BOTTOM }), (dy) => { steps.push(dy); return true; });
    return { auto, steps };
  };

  it("steps once per real pointer move and every 100ms while resting", () => {
    const { auto, steps } = setup();
    auto.move(10, 110);
    expect(steps).toEqual([-20]);
    // A browser's own repeat at the same spot does not step again.
    auto.move(10, 110);
    expect(steps).toEqual([-20]);
    vi.advanceTimersByTime(AUTOSCROLL_REPEAT_MS * 3);
    expect(steps).toEqual([-20, -20, -20, -20]);
    auto.move(10, 100);
    expect(steps).toEqual([-20, -20, -20, -20, -30]);
  });

  it("does nothing clear of the zones, and stops when the drag leaves", () => {
    const { auto, steps } = setup();
    auto.move(10, 300);
    vi.advanceTimersByTime(AUTOSCROLL_REPEAT_MS * 2);
    expect(steps).toEqual([]);
    auto.move(10, 495);
    expect(steps).toEqual([25]);
    auto.stop();
    vi.advanceTimersByTime(AUTOSCROLL_REPEAT_MS * 5);
    expect(steps).toEqual([25]);
  });

  it("stops repeating once the list cannot move further", () => {
    const steps: number[] = [];
    let room = 2;
    const auto = createDragAutoscroll(() => ({ top: TOP, bottom: BOTTOM }), (dy) => {
      steps.push(dy);
      return room-- > 0;
    });
    auto.move(10, 100);
    vi.advanceTimersByTime(AUTOSCROLL_REPEAT_MS * 10);
    expect(steps).toEqual([-30, -30, -30]);
    auto.stop();
  });
});
