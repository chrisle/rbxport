import { describe, expect, it } from "vitest";

import {
  DEFAULT_SESSION,
  DEFAULT_SUB_TREE_WIDTH,
  DEFAULT_SUB_WIDTH,
  DEFAULT_TREE_WIDTH,
  sanitiseSession,
  SEEDED_NODES,
  SEEDED_ROWS,
} from "./session";
import { DEFAULT_SORT } from "./viewSpec";

describe("sanitiseSession", () => {
  it("accepts a session it wrote itself", () => {
    const session = {
      treeWidth: 420,
      selectedNodeId: "pl-7",
      treeExpansion: { collapsed: ["folder-1"], expanded: ["dir:0:/Music"] },
      sort: { column: "bpm", descending: true },
      infoOpen: true,
      subOpen: false,
      filterOpen: true,
      tree: [{ id: "pl-7", name: "Set", kind: "playlist", depth: 1 }],
      rows: [{ id: "100", title: "One" }],
      count: 14,
      layout: "two",
      subWidth: 700,
      subTreeWidth: 240,
      trafficLight: "b",
      waveformZoom: { a: 4, b: 32 },
    };
    expect(sanitiseSession(session)).toEqual(session);
  });

  it("gives a session from before the sub-browser had widths the measured ones", () => {
    const session = sanitiseSession({ treeWidth: 300, subWidth: "wide" });
    expect(session.subWidth).toBe(DEFAULT_SUB_WIDTH);
    expect(session.subTreeWidth).toBe(DEFAULT_SUB_TREE_WIDTH);
  });

  it("keeps only records that could be drawn", () => {
    // The opening screen is written by us but read back as untrusted: a row
    // without an id cannot be keyed, and a keyed list is the one thing the
    // table needs.
    const session = sanitiseSession({
      tree: [{ id: "a" }, "nope", null, { name: "no id" }],
      rows: [{ id: "1" }, 7],
      count: -3,
    });
    expect(session.tree).toHaveLength(1);
    expect(session.rows).toHaveLength(1);
    expect(session.count).toBe(0);
  });

  it("caps what it will store, so one library cannot fill the disk", () => {
    const many = Array.from({ length: 9000 }, (_, i) => ({ id: String(i) }));
    const session = sanitiseSession({ tree: many, rows: many });
    expect(session.tree.length).toBeLessThanOrEqual(SEEDED_NODES);
    expect(session.rows.length).toBeLessThanOrEqual(SEEDED_ROWS);
  });

  it("falls back on anything that is not a session", () => {
    // A working window matters more than honouring whatever was stored.
    for (const bad of [null, undefined, 7, "x", [], true]) {
      expect(sanitiseSession(bad)).toEqual(DEFAULT_SESSION);
    }
  });

  it("rejects a width that would collapse or explode the tree", () => {
    expect(sanitiseSession({ treeWidth: 0 }).treeWidth).toBe(DEFAULT_TREE_WIDTH);
    expect(sanitiseSession({ treeWidth: -5 }).treeWidth).toBe(DEFAULT_TREE_WIDTH);
    expect(sanitiseSession({ treeWidth: Number.NaN }).treeWidth).toBe(DEFAULT_TREE_WIDTH);
    expect(sanitiseSession({ treeWidth: "wide" }).treeWidth).toBe(DEFAULT_TREE_WIDTH);
    // A width from a wider screen is kept; the splitter clamps it to the window.
    expect(sanitiseSession({ treeWidth: 4000 }).treeWidth).toBe(4000);
  });

  it("rejects a sort column that is not one", () => {
    expect(sanitiseSession({ sort: { column: "nope" } }).sort).toEqual(DEFAULT_SORT);
    expect(sanitiseSession({ sort: "bpm" }).sort).toEqual(DEFAULT_SORT);
    expect(sanitiseSession({ sort: { column: "bpm" } }).sort).toEqual({
      column: "bpm",
      descending: false,
    });
    expect(sanitiseSession({ sort: { column: "djPlayCount", descending: true } }).sort).toEqual({
      column: "djPlayCount",
      descending: true,
    });
  });

  it("treats a missing panel flag as closed", () => {
    const session = sanitiseSession({ infoOpen: "yes" });
    expect(session.infoOpen).toBe(false);
    expect(session.subOpen).toBe(false);
    expect(session.filterOpen).toBe(false);
  });

  it("keeps a selected node only when it is an id", () => {
    expect(sanitiseSession({ selectedNodeId: "pl-1" }).selectedNodeId).toBe("pl-1");
    expect(sanitiseSession({ selectedNodeId: 42 }).selectedNodeId).toBeNull();
  });

  it("keeps only bounded, unambiguous tree expansion ids", () => {
    const session = sanitiseSession({
      treeExpansion: {
        collapsed: ["closed", "closed", 7],
        expanded: ["open", "closed", null],
      },
    });
    expect(session.treeExpansion).toEqual({ collapsed: ["closed"], expanded: ["open"] });
    expect(sanitiseSession({ treeExpansion: "wide open" }).treeExpansion)
      .toEqual(DEFAULT_SESSION.treeExpansion);
  });

  it("restores only waveform zoom levels the player can select", () => {
    expect(sanitiseSession({ waveformZoom: { a: 0.5, b: 64 } }).waveformZoom).toEqual({
      a: 0.5,
      b: 64,
    });
    expect(sanitiseSession({ waveformZoom: { a: 3, b: "close" } }).waveformZoom).toEqual(
      DEFAULT_SESSION.waveformZoom,
    );
    expect(sanitiseSession({}).waveformZoom).toEqual(DEFAULT_SESSION.waveformZoom);
  });
});
