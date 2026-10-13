import { describe, expect, it } from "vitest";

import { isCollectionTrack, locateStep, needsCentering, restoredScrollTop } from "./locateLoaded";

/**
 * Pinned to rekordbox 7.2.19 (macOS arm64), `BrowseBasicView::
 * browseLoadedTrack()` at 0x100318c34 and `browseCenterAt` at 0x10031ff5c,
 * and `RowStateList::getState` at 0x102136950 (#338).
 */
describe("locateStep", () => {
  it("starts at Deck 1", () => {
    expect(locateStep(0, ["11", "22"])).toEqual({ deck: 0, trackId: "11", cursor: 1 });
  });

  it("takes the loaded decks in turn", () => {
    const decks = ["11", "22"];
    const found: (string | null)[] = [];
    let cursor = 0;
    for (let i = 0; i < 5; i++) {
      const step = locateStep(cursor, decks);
      found.push(step.trackId);
      cursor = step.cursor;
    }
    expect(found).toEqual(["11", "22", "11", "22", "11"]);
  });

  it("skips an empty deck, and keeps to the one that is loaded", () => {
    expect(locateStep(0, [null, "22"])).toEqual({ deck: 1, trackId: "22", cursor: 2 });
    // The cursor runs over four decks: 2 and 3 are empty, so round to Deck 2.
    expect(locateStep(2, [null, "22"])).toEqual({ deck: 1, trackId: "22", cursor: 2 });
  });

  it("finds nothing, and leaves the cursor, with no deck loaded", () => {
    expect(locateStep(1, [null, null])).toEqual({ deck: null, trackId: null, cursor: 1 });
    expect(locateStep(3, [])).toEqual({ deck: null, trackId: null, cursor: 3 });
  });

  it("gives a deck playing a file the collection lacks its turn but locates nothing", () => {
    expect(locateStep(0, ["file:/Music/a.mp3", "22"])).toEqual({ deck: 0, trackId: null, cursor: 1 });
    expect(locateStep(1, ["file:/Music/a.mp3", "22"])).toEqual({ deck: 1, trackId: "22", cursor: 2 });
  });
});

describe("isCollectionTrack", () => {
  it("is a decimal track id", () => {
    expect(isCollectionTrack("123")).toBe(true);
    expect(isCollectionTrack("file:/a.mp3")).toBe(false);
    expect(isCollectionTrack("itunes:4")).toBe(false);
  });
});

describe("needsCentering", () => {
  // Rows of 24px under a 24px sticky heading, in a 600px view.
  const at = (index: number) => 24 + index * 24;
  it("leaves a row wholly in view alone", () => {
    expect(needsCentering(at(0), 24, 0, 600, 24)).toBe(false);
    expect(needsCentering(at(23), 24, 0, 600, 24)).toBe(false);
  });

  it("centres a row cut off at either edge or out of view", () => {
    expect(needsCentering(at(24), 24, 0, 600, 24)).toBe(true);
    expect(needsCentering(at(300), 24, 0, 600, 24)).toBe(true);
    // Under the heading after scrolling.
    expect(needsCentering(at(0), 24, 10, 600, 24)).toBe(true);
  });
});

describe("restoredScrollTop", () => {
  const saved = { top: 3000, sort: "title:asc", height: 600 };
  it("puts a list back where it was left", () => {
    expect(restoredScrollTop(saved, "title:asc", 600)).toBe(3000);
  });

  it("opens at the top when the sort or the view's height differs, or nothing was saved", () => {
    expect(restoredScrollTop(saved, "bpm:asc", 600)).toBe(0);
    expect(restoredScrollTop(saved, "title:desc", 600)).toBe(0);
    expect(restoredScrollTop(saved, "title:asc", 500)).toBe(0);
    expect(restoredScrollTop(undefined, "title:asc", 600)).toBe(0);
  });
});
