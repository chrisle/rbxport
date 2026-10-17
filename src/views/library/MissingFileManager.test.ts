import { describe, expect, it } from "vitest";

import { resizedBox } from "./MissingFileManager";

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
