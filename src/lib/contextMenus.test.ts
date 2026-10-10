import { describe, expect, it } from "vitest";

import {
  deckMenu, deleteKeyAction, deviceTrackMenu, deviceTreeMenu, enabled, entriesOf, MISSING_TRACK_MENU, MISSING_TRACK_TITLE,
  SEPARATOR, shortcutMenu, TRACK_MENU, trackMenuFor, treeMenu, type MenuContext, type MenuRow,
} from "./contextMenus";

const OPEN: MenuContext = { inPlaylist: true, hasFile: true, readOnly: false };

describe("MISSING_TRACK_MENU", () => {
  it("is rekordbox's menu over a missing track, under its heading", () => {
    // [OBS rekordbox 7.2.14, Winrig chris-win11 2026-10-08, issue #201]
    expect(MISSING_TRACK_TITLE).toBe("File is Missing");
    expect(MISSING_TRACK_MENU.map((row) => (row === SEPARATOR ? row : row.label))).toEqual([
      "Auto Relocate",
      "Relocate",
      "Remove from Collection",
    ]);
  });

  it("writes, so a read-only library greys all three", () => {
    for (const entry of entriesOf(MISSING_TRACK_MENU)) {
      expect(enabled(entry, OPEN), entry.label).toBe(true);
      expect(enabled(entry, { ...OPEN, readOnly: true }), entry.label).toBe(false);
    }
  });
});

describe("TRACK_MENU", () => {
  it("is rekordbox's own list, in its own order, less the cloud", () => {
    // Transcribed from a capture of the menu open. If this drifts, the app has
    // stopped matching the thing it is a clone of — with the deliberate
    // exception of Cloud Library Sync, which is left out rather than greyed.
    expect(entriesOf(TRACK_MENU).map((e) => e.label)).toEqual([
      "Load",
      "Import To Collection",
      "Analyze Track",
      "Analysis Lock",
      "Add To Playlist",
      "Add To Tag List",
      "Reload Tag",
      "Get Info from iTunes",
      "Track Type",
      "Export Track",
      "Auto Load Hot Cue",
      "Reset DJ Play Count",
      "Add New Analysis Data",
      "Convert Memory Cues to Hot Cues",
      "Remove from Playlist",
      "Remove from Collection",
      "Remove from History",
      "Show information",
      "Show in Finder",
      "Track information",
    ]);
  });

  it("keeps the capture's seven separators", () => {
    expect(TRACK_MENU.filter((row) => row === SEPARATOR)).toHaveLength(7);
  });

  it("marks the entries that open a submenu", () => {
    const arrows = entriesOf(TRACK_MENU).filter((e) => e.submenu).map((e) => e.label);
    expect(arrows).toEqual([
      "Load",
      "Analysis Lock",
      "Add To Playlist",
      "Track Type",
      "Export Track",
      "Auto Load Hot Cue",
      "Track information",
    ]);
  });
});

describe("trackMenuFor", () => {
  const load = (players: number) =>
    entriesOf(trackMenuFor(players)).find((e) => e.label === "Load");

  it("offers the players the layout is drawing and no others", () => {
    expect(load(0)?.items).toBeUndefined();
    expect(entriesOf(load(1)?.items ?? []).map((e) => e.label)).toEqual([
      "Load track to player 1",
    ]);
    expect(entriesOf(load(2)?.items ?? []).map((e) => e.label)).toEqual([
      "Load track to player 1",
      "Load track to player 2",
    ]);
  });

  it("leaves the rest of rekordbox's list exactly as it was", () => {
    expect(entriesOf(trackMenuFor(2)).map((e) => e.label)).toEqual(
      entriesOf(TRACK_MENU).map((e) => e.label),
    );
    expect(trackMenuFor(2).filter((row) => row === SEPARATOR)).toHaveLength(7);
  });

  it("keeps Load greyed with no player to load into, and live with one", () => {
    // The arrow is drawn either way, which is what rekordbox does with an
    // entry a given selection cannot use.
    expect(load(0)?.submenu).toBe(true);
    expect(enabled(load(0) ?? { label: "Load", action: null }, OPEN)).toBe(false);
    expect(enabled(load(2) ?? { label: "Load", action: null }, OPEN)).toBe(true);
  });
});

describe("treeMenu", () => {
  it("offers root creation actions on the Playlists collection", () => {
    expect(entriesOf(treeMenu("collection")).map((e) => e.label)).toEqual([
      "Create New Playlist",
      "Create New Intelligent Playlist",
      "Create New Folder",
    ]);
  });

  it("is rekordbox's own list over a playlist, in its own order, less the cloud", () => {
    // docs/screenshots context-menu-tree@2x: thirteen entries in eight groups.
    // The three cloud rows shared the first group with Export Playlist, and
    // Collaborative playlist had a group of its own; leaving the four out
    // takes it to nine in seven groups; Rename, which the capture has no row
    // for, joins Delete's group and makes ten.
    expect(entriesOf(treeMenu("playlist")).map((e) => e.label)).toEqual([
      "Export Playlist",
      "Create New Playlist",
      "Create New Intelligent Playlist",
      "Create New Folder",
      "Playlist display setting",
      "Add Artwork",
      "Rename Playlist",
      "Delete Playlist",
      "Export a playlist to a file",
      "Add To Shortcut",
    ]);
    expect(treeMenu("playlist").filter((row) => row === SEPARATOR)).toHaveLength(6);
  });

  it("draws no cloud entry at all, rather than a greyed one", () => {
    const labels = entriesOf(treeMenu("playlist")).map((e) => e.label);
    expect(labels).not.toContain("Cloud Library Sync");
    expect(labels).not.toContain("Auto Upload");
    expect(labels).not.toContain("Batch Auto Upload setting");
    expect(labels).not.toContain("Collaborative playlist");
  });

  it("marks the entries that open a submenu", () => {
    const arrows = entriesOf(treeMenu("playlist")).filter((e) => e.submenu).map((e) => e.label);
    expect(arrows).toEqual(["Export Playlist", "Export a playlist to a file"]);
  });

  it("offers the rule editor on an intelligent playlist, and a shortcut's removal only on the shortcut", () => {
    // rekordbox 7.2.19 opens an intelligent playlist's menu with the editor
    // and a separator, ahead of Export Playlist (showTreeViewPopupMenu
    // @0x10011af90); the rest is a playlist's menu.
    const smart = treeMenu("smartPlaylist");
    expect(smart[0]).toEqual({ label: "Edit the Intelligent Playlist", action: "editSmartPlaylist" });
    expect(smart[1]).toBe(SEPARATOR);
    expect(smart.slice(2)).toEqual(treeMenu("playlist"));
    expect(entriesOf(treeMenu("playlist")).map((e) => e.label)).not.toContain("Edit the Intelligent Playlist");
    // rekordbox 7.2.11 keeps "Add To Shortcut" on a playlist that is one
    // already; the shortcut's own menu is the one row that deletes it.
    expect(entriesOf(treeMenu("playlist")).map((e) => e.label)).not.toContain("Remove from Shortcut");
    expect(entriesOf(shortcutMenu()).map((e) => e.label)).toEqual(["Delete Shortcut"]);
  });

  it("fills Add To Playlist and Export Track with what there is, and imports only loose files", () => {
    const rows = trackMenuFor(0, [{ id: "p1", name: "Sets › Warm Up" }], [{ id: "/Volumes/USB A", name: "USB A" }]);
    const add = entriesOf(rows).find((e) => e.label === "Add To Playlist");
    expect(add?.items && entriesOf(add.items).map((e) => e.action)).toEqual(["addToPlaylist:p1"]);
    const stick = entriesOf(rows).find((e) => e.label === "Export Track");
    expect(stick?.items && entriesOf(stick.items).map((e) => e.action)).toEqual(["exportTrack:/Volumes/USB A"]);
    // With nothing to offer, the arrows are greyed as before.
    const bare = entriesOf(trackMenuFor(0));
    expect(bare.find((e) => e.label === "Add To Playlist")?.items).toBeUndefined();
    const importRow = bare.find((e) => e.label === "Import To Collection") ?? { label: "", action: null };
    expect(enabled(importRow, OPEN)).toBe(false);
    expect(enabled(importRow, { ...OPEN, loose: true })).toBe(true);
    // Over a loose file, a track's writes are off and loading stays on.
    const lock = bare.find((e) => e.label === "Analysis Lock") ?? { label: "", action: null };
    expect(enabled(lock, { ...OPEN, loose: true })).toBe(false);
    const tagged = entriesOf(trackMenuFor(0, [], [], { tagList: true })).map((e) => e.label);
    expect(tagged).toContain("Remove from Tag List");
    expect(tagged).not.toContain("Remove from Playlist");
  });

  it("lists the connected sticks under Export Playlist and Export Folder, with no folder picker", () => {
    // #142: Export Playlist opened a folder picker. rekordbox's submenu is
    // the connected drives, one row each [OBS rekordbox 7, Winrig 2026-10-08].
    const sticks = [{ id: "E:\\", name: "USB" }, { id: "/Volumes/DJ STICK", name: "DJ STICK" }];
    for (const [kind, label] of [["playlist", "Export Playlist"], ["smartPlaylist", "Export Playlist"], ["folder", "Export Folder"]] as const) {
      const row = entriesOf(treeMenu(kind, sticks)).find((e) => e.label === label);
      expect(row?.submenu).toBe(true);
      expect(row?.action).toBeNull();
      expect(row?.items && entriesOf(row.items).map((e) => [e.label, e.action])).toEqual([
        ["USB", "exportTo:E:\\"],
        ["DJ STICK", "exportTo:/Volumes/DJ STICK"],
      ]);
      expect(enabled(row ?? { label, action: null }, OPEN)).toBe(true);
      // An export writes the stick, not the library: it stays live while
      // rekordbox holds the library.
      expect(enabled(row ?? { label, action: null }, { ...OPEN, readOnly: true })).toBe(true);
    }
    // With nothing connected the arrow is there and greyed, not a picker.
    const bare = entriesOf(treeMenu("playlist")).find((e) => e.label === "Export Playlist");
    expect(bare?.items).toBeUndefined();
    expect(enabled(bare ?? { label: "", action: null }, OPEN)).toBe(false);
  });

  it("a folder's menu is rekordbox's own: no artwork or file export, and Sort Items", () => {
    expect(entriesOf(treeMenu("folder")).map((e) => e.label)).toEqual([
      "Export Folder",
      "Create New Playlist",
      "Create New Intelligent Playlist",
      "Create New Folder",
      "Playlist display setting",
      "Rename Folder",
      "Delete Folder",
      "Sort Items",
      "Add To Shortcut",
    ]);
  });

  it("names rekordbox's submenu rows under Analysis Lock, Auto Load Hot Cue and Track information", () => {
    const rows = (label: string) => {
      const entry = entriesOf(TRACK_MENU).find((e) => e.label === label);
      return entry?.items ? entriesOf(entry.items).map((e) => e.label) : [];
    };
    expect(rows("Analysis Lock")).toEqual(["On", "Off"]);
    expect(rows("Auto Load Hot Cue")).toEqual(["Enable Auto Load Hot Cue", "Disable Auto Load Hot Cue"]);
    expect(rows("Track information")).toEqual(["Publish", "Do not publish"]);
    // Drawn but not done: the two KUVO rows and the two Auto Load rows.
    const lock = entriesOf(TRACK_MENU).find((e) => e.label === "Track information");
    expect(enabled(lock ?? { label: "", action: null }, OPEN)).toBe(false);
  });

  it("offers Create New Intelligent Playlist between the other two create rows, as rekordbox does", () => {
    // #151: the row was missing. rekordbox 7.2.19 draws it on the Playlists
    // heading, a folder, a playlist and an intelligent playlist, always
    // between Create New Playlist and Create New Folder
    // (showTreeViewPopupMenu @0x100119568 and @0x10011b0b8).
    for (const kind of ["collection", "folder", "playlist", "smartPlaylist"] as const) {
      const rows = entriesOf(treeMenu(kind));
      const at = rows.findIndex((e) => e.label === "Create New Intelligent Playlist");
      expect(rows[at - 1]?.label, kind).toBe("Create New Playlist");
      expect(rows[at]?.action, kind).toBe("createSmartPlaylist");
      expect(rows[at + 1]?.label, kind).toBe("Create New Folder");
      // It writes the library, so it is greyed while rekordbox holds it.
      expect(enabled(rows[at]!, OPEN), kind).toBe(true);
      expect(enabled(rows[at]!, { ...OPEN, readOnly: true }), kind).toBe(false);
    }
  });

  it("takes the node's own word for what is being deleted", () => {
    expect(entriesOf(treeMenu("folder")).map((e) => e.label)).toContain("Delete Folder");
    expect(entriesOf(treeMenu("playlist")).map((e) => e.label)).toContain("Delete Playlist");
    expect(entriesOf(treeMenu("folder")).map((e) => e.label)).toContain("Rename Folder");
    expect(entriesOf(treeMenu("playlist")).map((e) => e.label)).toContain("Rename Playlist");
    expect(entriesOf(treeMenu("folder")).map((e) => e.label)).toContain("Export Folder");
    expect(entriesOf(treeMenu("playlist")).map((e) => e.label)).toContain("Export Playlist");
  });
});

describe("enabled", () => {
  const entry = (label: string) =>
    entriesOf(TRACK_MENU).find((e) => e.label === label) ?? { label, action: null };

  it("greys what rekordbox has and this does not", () => {
    expect(enabled(entry("Load"), OPEN)).toBe(false);
    expect(enabled(entry("Get Info from iTunes"), OPEN)).toBe(false);
  });

  it("offers Analyze Track, and greys it while rekordbox holds the library", () => {
    expect(enabled(entry("Analyze Track"), OPEN)).toBe(true);
    expect(enabled(entry("Analyze Track"), { ...OPEN, readOnly: true })).toBe(false);
  });

  it("draws no cloud entry in the track menu either", () => {
    expect(entriesOf(TRACK_MENU).map((e) => e.label)).not.toContain("Cloud Library Sync");
  });

  it("greys removing from a playlist when the view is not one", () => {
    expect(enabled(entry("Remove from Playlist"), { ...OPEN, inPlaylist: false })).toBe(false);
    expect(enabled(entry("Remove from Playlist"), OPEN)).toBe(true);
  });

  it("greys showing a file that is not there", () => {
    expect(enabled(entry("Show in Finder"), { ...OPEN, hasFile: false })).toBe(false);
  });

  it("greys every write while rekordbox holds the library", () => {
    // The same rule the rest of the app follows: a refusal, not a race.
    const locked = { ...OPEN, readOnly: true };
    expect(enabled(entry("Remove from Playlist"), locked)).toBe(false);
    // Reading is still fine.
    expect(enabled(entry("Show information"), locked)).toBe(true);
  });
});

describe("the Explorer's track menu, against rekordbox's", () => {
  // rekordbox 7, right-clicking a file in the Explorer [OBS Winrig
  // 2026-10-08, issue #105]: a file the collection does not hold
  // (Music/RBX-ARTWORK-TEST, rekordbox-06-menu-not-imported.png) and one it
  // does (the sampler's 4-Floor Breaks Kit, rekordbox-17-menu-imported.png).
  // Live entries only; every other row was drawn greyed. Cloud Library Sync
  // is left out here on purpose, and Show in Explorer is Show in Finder.
  const REKORDBOX_LIVE = {
    loose: [
      "Import To Collection", "Add To Playlist", "Add To Tag List", "Reload Tag", "Get Info from iTunes",
      "Export Track", "Auto Load Hot Cue", "Reset DJ Play Count", "Show information", "Show in Finder",
      "Track information",
    ],
    imported: [
      "Analyze Track", "Analysis Lock", "Add To Playlist", "Add To Tag List", "Reload Tag",
      "Get Info from iTunes", "Export Track", "Auto Load Hot Cue", "Reset DJ Play Count",
      "Remove from Collection", "Show information", "Show in Finder", "Track information",
    ],
  };
  // Where this still differs, and why. In every view: rows this does not do
  // (iTunes, Auto Load, KUVO) and Load, which rekordbox greyed in its Export
  // layout. Over a loose file only: entries rekordbox leaves live whose effect
  // on a file outside the collection has not been observed [UNKNOWN].
  const EVERY_VIEW = ["Load", "Get Info from iTunes", "Auto Load Hot Cue", "Track information"];
  const LOOSE_UNOBSERVED = ["Add To Tag List", "Reload Tag", "Export Track", "Reset DJ Play Count", "Show information"];

  const rows = trackMenuFor(2, [{ id: "p1", name: "Warm Up" }], [{ id: "/Volumes/USB", name: "USB" }], { explorer: true });
  const explorer: MenuContext = { inPlaylist: false, inHistory: false, hasFile: true, readOnly: false };
  const live = (loose: boolean) =>
    entriesOf(rows).filter((e) => enabled(e, { ...explorer, loose })).map((e) => e.label);

  it("draws rekordbox's rows, with no Convert Memory Cues to Hot Cues", () => {
    expect(entriesOf(rows).map((e) => e.label)).toEqual([
      "Load", "Import To Collection", "Analyze Track", "Analysis Lock", "Add To Playlist", "Add To Tag List",
      "Reload Tag", "Get Info from iTunes", "Track Type", "Export Track", "Auto Load Hot Cue",
      "Reset DJ Play Count", "Add New Analysis Data", "Remove from Playlist", "Remove from Collection",
      "Remove from History", "Show information", "Show in Finder", "Track information",
    ]);
    expect(rows.filter((row) => row === SEPARATOR)).toHaveLength(7);
    // The rest of the browser keeps it, as rekordbox's Collection does.
    expect(entriesOf(trackMenuFor(2)).map((e) => e.label)).toContain("Convert Memory Cues to Hot Cues");
  });

  it("over an imported file, is a track's menu: the one rekordbox draws", () => {
    const differs = new Set(EVERY_VIEW);
    expect(live(false).filter((l) => !differs.has(l))).toEqual(
      REKORDBOX_LIVE.imported.filter((l) => !differs.has(l)),
    );
  });

  it("over a file the library does not hold, imports it and adds it to a playlist", () => {
    const differs = new Set([...EVERY_VIEW, ...LOOSE_UNOBSERVED]);
    expect(live(true).filter((l) => !differs.has(l))).toEqual(
      REKORDBOX_LIVE.loose.filter((l) => !differs.has(l)),
    );
    // Pinned so closing a gap is a deliberate edit here.
    expect(live(true).filter((l) => differs.has(l))).toEqual(["Load"]);
    const add = entriesOf(rows).find((e) => e.label === "Add To Playlist");
    expect(entriesOf(add?.items ?? []).every((e) => enabled(e, { ...explorer, loose: true }))).toBe(true);
    expect(enabled(add ?? { label: "", action: null }, { ...explorer, loose: true, readOnly: true })).toBe(false);
  });
});

describe("deckMenu", () => {
  const state = { waveformColor: "3band" as const, beatCount: "position" as const, waveformClick: true };

  it("is rekordbox's player menu, top to bottom, with its greyed entries", () => {
    const labels = entriesOf(deckMenu(state)).map((e) => e.label);
    expect(labels).toEqual([
      "Change waveform color", "Analyze Track", "Beat Count Display", "Export Track", "Export Loop As WAV",
      "Active Loop Playback", "Click on the waveform for PLAY and CUE",
    ]);
    const context = { inPlaylist: false, hasFile: true, readOnly: false };
    const live = entriesOf(deckMenu(state)).filter((e) => enabled(e, context)).map((e) => e.label);
    expect(live).toEqual([
      "Change waveform color", "Analyze Track", "Beat Count Display", "Click on the waveform for PLAY and CUE",
    ]);
  });

  it("ticks the choice in force in each submenu", () => {
    const rows = deckMenu({ ...state, waveformColor: "rgb", beatCount: "toMemoryBeats", waveformClick: false });
    const items = (label: string) => entriesOf(rows).find((e) => e.label === label)?.items ?? [];
    expect(entriesOf(items("Change waveform color")).map((e) => [e.label, e.checked ?? false]))
      .toEqual([["BLUE", false], ["RGB", true], ["3Band", false]]);
    expect(entriesOf(items("Beat Count Display")).filter((e) => e.checked).map((e) => e.label))
      .toEqual(["Count to the next MEMORY CUE (Beats)"]);
    expect(entriesOf(items("Click on the waveform for PLAY and CUE")).filter((e) => e.checked).map((e) => e.label))
      .toEqual(["Disable"]);
  });

  it("offers Analyze Track for the loaded track", () => {
    const entry = entriesOf(deckMenu(state)).find((e) => e.label === "Analyze Track");
    expect(entry?.action).toBe("analyse");
  });
});

describe("deleteKeyAction", () => {
  it("is the removal the list's own menu offers, as rekordbox's Delete key is (#136)", () => {
    expect(deleteKeyAction("collection")).toBe("removeFromCollection");
    expect(deleteKeyAction("playlist")).toBe("removeFromPlaylist");
    expect(deleteKeyAction("history")).toBe("removeFromHistory");
    expect(deleteKeyAction("tagList")).toBe("removeFromTagList");
  });

  it("does nothing where there is nothing to remove from", () => {
    for (const source of ["folder", "playlistFolder", "related"]) {
      expect(deleteKeyAction(source)).toBeNull();
    }
  });
});

describe("a stick's own library", () => {
  const live = <A extends string>(rows: readonly MenuRow<A>[], context: MenuContext = { inPlaylist: false, hasFile: false, readOnly: false }) =>
    entriesOf(rows).filter((e) => enabled(e, context)).map((e) => e.label);

  it("draws rekordbox 7.2.14's tree menus [OBS Winrig 2026-10-08], with the edits this app makes live", () => {
    expect(entriesOf(deviceTreeMenu("devicePlaylists")).map((e) => e.label)).toEqual([
      "Create New Playlist", "Create New Folder", "Import Folder", "Delete All", "Sort Items", "Add To Shortcut",
    ]);
    expect(live(deviceTreeMenu("devicePlaylists"))).toEqual(["Create New Playlist", "Create New Folder"]);
    expect(entriesOf(deviceTreeMenu("devicePlaylist")).map((e) => e.label)).toEqual([
      "Add Artwork", "Import Playlist", "Delete Playlist", "Export a playlist to a file", "Add To Shortcut",
    ]);
    expect(live(deviceTreeMenu("devicePlaylist"))).toEqual(["Delete Playlist"]);
    expect(live(deviceTreeMenu("deviceFolder"))).toEqual(["Create New Playlist", "Create New Folder", "Delete Folder"]);
  });

  it("greys every edit to a stick while rekordbox holds it or the stick is busy", () => {
    const busy = { inPlaylist: false, hasFile: false, readOnly: true };
    for (const kind of ["devicePlaylists", "deviceFolder", "devicePlaylist"] as const) {
      expect(live(deviceTreeMenu(kind), busy)).toEqual([]);
    }
    expect(live(deviceTrackMenu([{ id: "3", name: "Top List" }], true), { inPlaylist: true, hasFile: true, readOnly: true })).toEqual([]);
  });

  it("offers a library's playlists to its tracks, and Remove from Playlist only inside one", () => {
    const inAll = deviceTrackMenu([{ id: "3", name: "Top List" }], false);
    expect(entriesOf(inAll).map((e) => e.label)).toEqual([
      "Add To Playlist", "Delete Track", "Retrieve the waveform from collection", "Update Collection", "Show information",
    ]);
    const add = entriesOf(inAll)[0];
    expect(add?.items && entriesOf(add.items).map((e) => [e.label, e.action])).toEqual([["Top List", "deviceAddToPlaylist:3"]]);
    const open = { inPlaylist: true, hasFile: true, readOnly: false };
    expect(entriesOf(deviceTrackMenu([], true)).filter((e) => enabled(e, open)).map((e) => e.label)).toEqual(["Remove from Playlist"]);
    expect(entriesOf(deviceTrackMenu([], false)).some((e) => e.label === "Remove from Playlist")).toBe(false);
  });
});
