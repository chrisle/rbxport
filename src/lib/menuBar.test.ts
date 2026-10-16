import { afterEach, describe, expect, it } from "vitest";

import type { BrowserFocus } from "./browserFocus";
import {
  menuBarEnabled, menuBarItem, menuBarState, onMenuBarState, publishListMenuState, publishTreeMenuState,
  type ListMenuState, type TreeMenuState,
} from "./menuBar";

const LIST: BrowserFocus = { pane: "list", side: "main" };
const TREE: BrowserFocus = { pane: "tree", side: "main" };

function enabled(
  focus: BrowserFocus,
  list: Partial<ListMenuState> | null,
  tree: TreeMenuState["kind"],
  has: (action: string) => boolean = () => true,
): string[] {
  const state: ListMenuState = { source: "collection", selected: 0, rows: [], loose: false, ...list };
  const out = menuBarEnabled({
    focus,
    lists: list === null ? {} : { main: state },
    trees: { main: { kind: tree } },
    has,
  });
  return Object.entries(out).filter(([, on]) => on).map(([id]) => id).sort();
}

// The rekordbox column of each case is what the Windows menu bar showed
// (/Users/chrisle/code/rbxport-worktrees/parity/issue-513/rekordbox-*.png)
// or, where marked, what `ListViewer::isActiveCommand` / `TreeViewer::
// isActiveCommand` say [OBS static, rekordbox 7.2.19].
describe("menuBarEnabled", () => {
  it("greys every item until a list or the tree has the focus", () => {
    // rekordbox-playlist-menu-collection-noselection.png: a fresh start.
    expect(enabled(null, { selected: 1, rows: [{}] }, "allTracks")).toEqual([]);
  });

  it("greys the Track menu with nothing selected", () => {
    // rekordbox-track-menu-collection-noselection.png
    expect(enabled(LIST, { selected: 0 }, "allTracks")).toEqual([
      "create-folder", "create-intelligent-playlist", "create-playlist",
    ]);
  });

  it("offers a present track's items in the Collection", () => {
    // rekordbox-track-menu-collection-one-track-unreachable-path.png:
    // Analyze Track, Analysis Lock (On) and Remove from Collection live,
    // Import To Collection and the Relocate items greyed. That track's file
    // was on another machine, so Reload Tag and Show in Explorer were greyed
    // too; for a file that is there they are live [OBS static: 0x4003 and
    // 0x400d test `isValidMediaPath`].
    expect(enabled(LIST, { selected: 1, rows: [{ missing: false }] }, "allTracks")).toEqual([
      "analysis-lock", "analysis-lock-off", "analysis-lock-on", "analyze-track",
      "create-folder", "create-intelligent-playlist", "create-playlist",
      "reload-tag", "remove-from-collection", "show-in-finder",
    ]);
  });

  it("offers only removal and relocation for a missing track", () => {
    // rekordbox-track-menu-playlist-one-track.png and alphatheta-docs'
    // rekordbox-auto-relocate-win-track-menu.png. rekordbox greys Analysis
    // Lock Off unless a selected track is locked; the rows here do not carry
    // the lock, so Off stays live, as the context menu's does.
    expect(enabled(LIST, { selected: 1, rows: [{ missing: true }] }, "allTracks")).toEqual([
      "analysis-lock", "analysis-lock-off", "auto-relocate",
      "create-folder", "create-intelligent-playlist", "create-playlist",
      "relocate", "remove-from-collection",
    ]);
  });

  it("greys the Track menu while the tree has the focus", () => {
    // rekordbox-track-menu-playlist-tree-focused-noselection.png, and
    // [OBS static] the focused component must be the list box.
    expect(enabled(TREE, { selected: 1, rows: [{}] }, "playlist")).not.toContain("remove-from-collection");
  });

  it("shows only one file in the Finder", () => {
    const two = enabled(LIST, { selected: 2, rows: [{}, {}] }, "playlist");
    expect(two).not.toContain("show-in-finder");
    expect(two).toContain("reload-tag");
  });

  it("imports only loose files, and offers them nothing that needs a collection track", () => {
    expect(enabled(LIST, { source: "folder", selected: 1, rows: [{}], loose: true }, "explorer")).toEqual([
      "import-to-collection", "show-in-finder",
    ]);
  });

  it("does not reload tags in a history or the Tag List [OBS static: case 0x4003]", () => {
    expect(enabled(LIST, { source: "history", selected: 1, rows: [{}] }, "history")).not.toContain("reload-tag");
    expect(enabled(LIST, { source: "tagList", selected: 1, rows: [{}] }, "tagList")).not.toContain("reload-tag");
    expect(enabled(LIST, { source: "history", selected: 1, rows: [{}] }, "history")).toContain("remove-from-collection");
  });

  it("takes rows never fetched to be present", () => {
    expect(enabled(LIST, { selected: 3, rows: [{ missing: true }] }, "allTracks")).toContain("analyze-track");
  });

  it("greys Analyze Track for the sub-browser, whose selection it does not analyse", () => {
    const out = menuBarEnabled({
      focus: { pane: "list", side: "sub" },
      lists: { sub: { source: "collection", selected: 1, rows: [{}], loose: false } },
      trees: {},
      has: () => true,
    });
    expect(out["analyze-track"]).toBe(false);
    expect(out["remove-from-collection"]).toBe(true);
  });

  it("greys a stick's tracks", () => {
    expect(enabled(LIST, { source: "device", selected: 1, rows: [{}] }, "devicePlaylist")).toEqual([]);
  });

  it("offers a playlist's items", () => {
    // rekordbox-playlist-menu-playlist.png: Create x3, Delete Playlist,
    // Renumber Track Order and Export a playlist to a file live; Sort Items
    // greyed.
    expect(enabled(TREE, null, "playlist")).toEqual([
      "create-folder", "create-intelligent-playlist", "create-playlist", "delete-playlist",
      "export-playlist-file", "export-playlist-m3u8", "export-playlist-txt", "renumber-track-order",
    ]);
  });

  it("offers a folder's items", () => {
    // rekordbox-playlist-menu-folder.png: Create x3 and Delete Playlist live,
    // Renumber greyed. rekordbox also offers Export a playlist to a file on a
    // folder; what it writes is not known, so it is greyed here.
    expect(enabled(TREE, null, "folder")).toEqual([
      "create-folder", "create-intelligent-playlist", "create-playlist", "delete-playlist",
    ]);
  });

  it("offers only the Create items on the Playlists heading and the Collection", () => {
    // rekordbox-playlist-menu-playlists-root.png also has Delete Playlist and
    // Export live there; not known what they do, so greyed here.
    // rekordbox-playlist-menu-collection-list-focused.png: Create x3 only.
    const create = ["create-folder", "create-intelligent-playlist", "create-playlist"];
    expect(enabled(TREE, null, "collection")).toEqual(create);
    expect(enabled(LIST, { selected: 1, rows: [{}] }, "allTracks").filter((id) => id.startsWith("create"))).toEqual(create);
  });

  it("answers the Playlist menu from the tree while its list has the focus", () => {
    // rekordbox-playlist-menu-playlist-list-focused.png
    expect(enabled(LIST, { source: "playlist", selected: 1, rows: [{}] }, "playlist")).toContain("delete-playlist");
  });

  it("does not renumber an intelligent playlist", () => {
    expect(enabled(TREE, null, "smartPlaylist")).not.toContain("renumber-track-order");
  });

  it("greys an item nothing on screen answers", () => {
    expect(enabled(TREE, null, "playlist", (action) => action !== "deletePlaylist")).not.toContain("delete-playlist");
  });

  it("names a command for every id it greys", () => {
    for (const id of ["relocate", "create-playlist", "export-playlist-txt", "renumber-track-order"]) {
      expect(menuBarItem(id)).not.toBeNull();
    }
    expect(menuBarItem("analysis-lock")).toBeNull();
  });
});

describe("menu bar state", () => {
  afterEach(() => {
    publishListMenuState("main", null);
    publishTreeMenuState("main", null);
  });

  it("tells its listeners only when a report changes", () => {
    let calls = 0;
    const off = onMenuBarState(() => { calls += 1; });
    const state: ListMenuState = { source: "playlist", selected: 1, rows: [{ missing: false }], loose: false };
    publishListMenuState("main", state);
    publishListMenuState("main", { ...state, rows: [{ missing: false }] });
    publishTreeMenuState("main", { kind: "playlist" });
    publishTreeMenuState("main", { kind: "playlist" });
    expect(calls).toBe(2);
    expect(menuBarState().lists.main?.selected).toBe(1);
    publishListMenuState("main", null);
    expect(menuBarState().lists.main).toBeUndefined();
    off();
  });
});
