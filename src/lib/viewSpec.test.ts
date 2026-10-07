import { describe, expect, it } from "vitest";

import { DEFAULT_SORT, NO_SORT, nextSort, specForNode } from "./viewSpec";
import type { TreeNode } from "@/ipc/types";

const playlist: TreeNode = { id: "pl-1", name: "Warm Up", kind: "playlist", depth: 1 };
const folder: TreeNode = { id: "f-1", name: "SHOWS", kind: "folder", depth: 1 };
const device: TreeNode = { id: "device:/Volumes/X", name: "X", kind: "device", depth: 0 };

describe("specForNode", () => {
  it("narrows to a playlist", () => {
    expect(specForNode(playlist, "", null).source).toEqual({ kind: "playlist", id: "pl-1" });
  });

  it("opens a playlist folder over its descendants", () => {
    expect(specForNode(folder, "", null).source).toEqual({ kind: "playlistFolder", id: "f-1" });
  });

  it("shows the collection for nodes without a track source", () => {
    for (const node of [device, null]) {
      expect(specForNode(node, "", null).source).toEqual({ kind: "collection" });
    }
  });

  it("carries the query and the sort through", () => {
    const spec = specForNode(playlist, "artbat", { column: "bpm", descending: true });
    expect(spec.query).toBe("artbat");
    expect(spec.sort).toBe("bpm");
    expect(spec.descending).toBe(true);
  });

  it("carries DJ Play Count to the backend sort key", () => {
    expect(specForNode(playlist, "", { column: "djPlayCount", descending: true }).sort)
      .toBe("djPlayCount");
  });

  it("falls back to the default order when nothing is chosen", () => {
    const spec = specForNode(playlist, "", null);
    expect(spec.sort).toBe(DEFAULT_SORT.column);
    expect(spec.descending).toBe(DEFAULT_SORT.descending);
  });
});

describe("nextSort", () => {
  it("cycles ascending, descending, off", () => {
    const first = nextSort(DEFAULT_SORT, "bpm");
    expect(first).toEqual({ column: "bpm", descending: false });
    const second = nextSort(first, "bpm");
    expect(second).toEqual({ column: "bpm", descending: true });
    // Off is the view's own order, not another column's: a playlist that could
    // not be put back the way it was would make sorting it a one-way door.
    expect(nextSort(second, "bpm")).toEqual(NO_SORT);
    expect(NO_SORT.column).toBe("trackNo");
  });

  it("opens a view in its own order", () => {
    expect(DEFAULT_SORT).toEqual(NO_SORT);
    expect(specForNode(playlist, "", null).sort).toBe("trackNo");
  });

  it("starts a different column ascending rather than continuing the cycle", () => {
    expect(nextSort({ column: "bpm", descending: true }, "artist")).toEqual({
      column: "artist",
      descending: false,
    });
  });
});

describe("the Explorer's views", () => {
  it("opens a folder as itself", () => {
    const node: TreeNode = { id: "dir:0:/Users/x/Music", name: "Music", kind: "directory", depth: 1, lazy: true };
    expect(specForNode(node, "", null).source).toEqual({ kind: "folder", path: "/Users/x/Music" });
  });

  it("opens the heading as an empty folder, which is what rekordbox shows there", () => {
    const node: TreeNode = { id: "explorer", name: "Explorer", kind: "explorer", depth: 0, expanded: true };
    expect(specForNode(node, "", null).source).toEqual({ kind: "folder", path: "" });
  });
});

describe("the Key column's sort order", () => {
  const node = { id: "all", name: "All Tracks", kind: "allTracks" as const, depth: 0 };
  it("sorts alphabetically independently of display format", () => {
    expect(specForNode(node, "", { column: "key", descending: false }, "classic", "alphabetical").sort).toBe("key");
    expect(specForNode(node, "", { column: "key", descending: false }, "alphanumeric", "alphabetical").sort).toBe("key");
  });
  it("sorts musically independently of display format", () => {
    expect(specForNode(node, "", { column: "key", descending: true }, "classic", "musical").sort).toBe("keyCamelot");
    expect(specForNode(node, "", { column: "key", descending: true }, "alphanumeric", "musical").sort).toBe("keyCamelot");
  });
  it("only the key column changes with the display", () => {
    expect(specForNode(node, "", { column: "bpm", descending: false }, "alphanumeric", "musical").sort).toBe("bpm");
  });
});
