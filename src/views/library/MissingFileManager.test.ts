import { describe, expect, it } from "vitest";

import { draggedBox, resizedBox } from "./MissingFileManager";

const view = { width: 1600, height: 1000 };
const start = { left: 400, top: 200, width: 608, height: 560 };

describe("resizedBox", () => {
  it("moves only the edges held", () => {
    expect(resizedBox(start, "se", 100, 50, view)).toEqual({ left: 400, top: 200, width: 708, height: 610 });
    // The left and top edges move; the right and bottom stay put.
    expect(resizedBox(start, "nw", -100, -50, view)).toEqual({ left: 300, top: 150, width: 708, height: 610 });
    expect(resizedBox(start, "e", 30, 99, view)).toEqual({ ...start, width: 638 });
    expect(resizedBox(start, "n", 99, 20, view)).toEqual({ ...start, top: 220, height: 540 });
  });

  it("keeps rekordbox's limits, 400 x 300 to 1024 x 768", () => {
    // [OBS rekordbox 7.2.19 static] MissingFileDialog: setResizeLimits(400, 300, 1024, 768).
    expect(resizedBox(start, "se", 2000, 2000, { width: 4000, height: 4000 })).toEqual({ left: 400, top: 200, width: 1024, height: 768 });
    expect(resizedBox(start, "se", -2000, -2000, view)).toEqual({ left: 400, top: 200, width: 400, height: 300 });
    // Shrunk from the left, the right edge stays where it was.
    expect(resizedBox(start, "w", 2000, 0, view)).toEqual({ ...start, left: 608, width: 400 });
  });

  it("stays inside the app's window", () => {
    expect(resizedBox(start, "e", 2000, 0, { width: 1100, height: 1000 })).toEqual({ ...start, width: 700 });
    expect(resizedBox(start, "w", -2000, 0, view)).toEqual({ ...start, left: 0, width: 1008 });
  });
});

describe("draggedBox", () => {
  it("moves the window without changing its size", () => {
    expect(draggedBox(start, 50, 30, view)).toEqual({ ...start, left: 450, top: 230 });
    expect(draggedBox(start, -100, -50, view)).toEqual({ ...start, left: 300, top: 150 });
  });

  it("keeps the title bar reachable instead of letting the window go fully off-screen", () => {
    // Dragged far up: the top edge, where the title bar is, stops at the viewport's top.
    expect(draggedBox(start, 0, -5000, view)).toEqual({ ...start, top: 0 });
    // Dragged far down: at least TITLE_MIN_VISIBLE (40px) of the window stays visible at the bottom.
    expect(draggedBox(start, 0, 5000, view)).toEqual({ ...start, top: 960 });
    // Dragged far left: at least 40px of the title bar's right end stays visible.
    expect(draggedBox(start, -5000, 0, view)).toEqual({ ...start, left: -568 });
    // Dragged far right: at least 40px of the title bar's left end stays visible.
    expect(draggedBox(start, 5000, 0, view)).toEqual({ ...start, left: 1560 });
  });

  it("keeps a consistent position whether resize or drag runs first", () => {
    const resizedThenDragged = draggedBox(resizedBox(start, "se", 100, 50, view), 20, 10, view);
    const draggedThenResized = resizedBox(draggedBox(start, 20, 10, view), "se", 100, 50, view);
    expect(resizedThenDragged).toEqual({ left: 420, top: 210, width: 708, height: 610 });
    expect(draggedThenResized).toEqual(resizedThenDragged);
  });
});
