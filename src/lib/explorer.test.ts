import { describe, expect, it } from "vitest";

import {
  EXPLORER_ROOT_ID, explorerId, explorerNodes, explorerPath, hasLooseId, isLooseId, joinPath, moreNote,
} from "./explorer";

const ROOTS = [
  { name: "Music", path: "/Users/x/Music" },
  { name: "x", path: "/Users/x" },
  { name: "Macintosh HD", path: "/" },
  { name: "SD", path: "/Volumes/SD" },
];

describe("explorer ids", () => {
  it("round-trips a path and refuses anything else", () => {
    expect(explorerPath(explorerId(3, "/Volumes/SD"))).toBe("/Volumes/SD");
    expect(explorerPath(explorerId(0, "C:\\Users\\x"))).toBe("C:\\Users\\x");
    expect(explorerPath("pl-1")).toBeNull();
    expect(explorerPath(EXPLORER_ROOT_ID)).toBeNull();
  });

  it("tells a loose file's row from a track's", () => {
    expect(isLooseId("file:/Users/x/Music/a.mp3")).toBe(true);
    expect(isLooseId("100001")).toBe(false);
    // The menu follows the rows: an imported file is a track in the Explorer too.
    expect(hasLooseId(["100001", "100002"])).toBe(false);
    expect(hasLooseId(["100001", "file:/m/a.flac"])).toBe(true);
    expect(hasLooseId([])).toBe(false);
  });
});

describe("joinPath", () => {
  it("uses the separator the parent uses", () => {
    expect(joinPath("/Users/x", "Music")).toBe("/Users/x/Music");
    expect(joinPath("C:\\Users\\x", "Music")).toBe("C:\\Users\\x\\Music");
  });

  it("does not double a root's own separator", () => {
    expect(joinPath("/", "Users")).toBe("/Users");
    expect(joinPath("C:\\", "Users")).toBe("C:\\Users");
  });
});

describe("explorerNodes", () => {
  it("is nothing until there are roots", () => {
    expect(explorerNodes([], new Map())).toEqual([]);
  });

  it("is the heading, open, with every root closed under it", () => {
    const nodes = explorerNodes(ROOTS, new Map());
    expect(nodes.map((n) => [n.name, n.depth])).toEqual([
      ["Explorer", 0], ["Music", 1], ["x", 1], ["Macintosh HD", 1], ["SD", 1],
    ]);
    expect(nodes[0]).toMatchObject({ id: EXPLORER_ROOT_ID, kind: "explorer", expanded: true });
    for (const root of nodes.slice(1)) {
      expect(root).toMatchObject({ kind: "directory", expanded: false, lazy: true });
    }
  });

  it("places an opened folder's children under it, in tree order", () => {
    const children = new Map([
      ["/", { names: ["Applications", "Users"], total: 2 }],
      ["/Users", { names: ["Shared", "x"], total: 2 }],
    ]);
    const nodes = explorerNodes(ROOTS, children);
    expect(nodes.map((n) => `${"  ".repeat(n.depth)}${n.name}`)).toEqual([
      "Explorer",
      "  Music",
      "  x",
      "  Macintosh HD",
      "    Applications",
      "    Users",
      "      Shared",
      "      x",
      "  SD",
    ]);
    expect(nodes.find((n) => n.name === "Shared")?.id).toBe(explorerId(2, "/Users/Shared"));
  });

  it("draws a folder that answered with nothing as a closed branch still", () => {
    const nodes = explorerNodes(ROOTS, new Map([["/Volumes/SD", { names: [], total: 0 }]]));
    const sd = nodes.find((n) => n.name === "SD");
    expect(sd?.lazy).toBe(true);
    expect(nodes.length).toBe(5);
  });

  it("shows the home folder both as a root and under Users, as the capture does", () => {
    const nodes = explorerNodes(
      ROOTS,
      new Map([
        ["/", { names: ["Users"], total: 1 }],
        ["/Users", { names: ["x"], total: 1 }],
        ["/Users/x", { names: ["Music"], total: 1 }],
      ]),
    );
    const homes = nodes.filter((n) => explorerPath(n.id) === "/Users/x");
    expect(homes.map((n) => n.depth)).toEqual([1, 3]);
    // Two rows, two ids: one selection cannot be both.
    expect(new Set(homes.map((n) => n.id)).size).toBe(2);
    // And what was read for one serves the other: Music, itself a root, is
    // under both.
    expect(nodes.filter((n) => explorerPath(n.id) === "/Users/x/Music").map((n) => n.depth)).toEqual([1, 2, 4]);
  });
});

describe("a folder cut at the backend's cap", () => {
  it("says how many more it holds, as a line under the last shown", () => {
    const nodes = explorerNodes(
      ROOTS,
      new Map([["/Volumes/SD", { names: ["Alpha", "Beta"], total: 14_503 }]]),
    );
    const under = nodes.filter((n) => n.depth === 2).map((n) => [n.name, n.kind]);
    expect(under).toEqual([
      ["Alpha", "directory"], ["Beta", "directory"], ["14,501 more folders not shown", "note"],
    ]);
    const note = nodes.find((n) => n.kind === "note");
    // Not a folder: nothing to open, and no path to open.
    expect(note?.lazy).toBeUndefined();
    expect(moreNote(1)).toBe("1 more folder not shown");
  });
});
