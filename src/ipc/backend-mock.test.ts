/**
 * The mock's edit semantics have to match Rust's, or `pnpm dev:mock` and the
 * Playwright suite validate behaviour the real backend does not have.
 *
 * The rules asserted here are the ones `crates/rbl-db/tests/writes.rs` asserts
 * on the other side.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import { createMockBackend } from "./backend-mock";
import { TREE_ROOT } from "./types";

describe("related playlists", () => {
  it("lists edited playlists a track belongs to, alongside the seeded ones", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const track = "100000";
    const before = new Set((await backend.playlistTree()).map((node) => node.id));
    await backend.edits.createPlaylist("Alpha", TREE_ROOT);
    await backend.edits.createPlaylist("Beta", TREE_ROOT);
    const [alpha = "", beta = ""] = (await backend.playlistTree())
      .filter((node) => !before.has(node.id)).map((node) => node.id);
    await backend.edits.addTracksToPlaylist(alpha, [track], false);
    await backend.edits.addTracksToPlaylist(beta, [track], true);
    expect((await backend.trackPlaylists(track)).map((p) => p.name))
      .toEqual(expect.arrayContaining(["Alpha", "Beta"]));
  });

  it("leaves a playlist out once the track has been taken off it", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const track = "100000";
    const id = (await backend.playlistTree()).filter((node) => node.kind === "playlist")[0]?.id ?? "";
    await backend.edits.addTracksToPlaylist(id, [track], false);
    await backend.edits.removeTracksFromPlaylist(id, [track]);
    expect((await backend.trackPlaylists(track)).map((p) => p.id)).not.toContain(id);
  });

  it("spells the folders above a playlist as its path, ordered path then name", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const track = "100000";
    await backend.edits.createPlaylist("Alpha", TREE_ROOT);
    await backend.edits.createFolder("Gigs", TREE_ROOT);
    const gigs = (await backend.playlistTree()).find((node) => node.name === "Gigs");
    await backend.edits.createPlaylist("Friday", gigs?.id ?? TREE_ROOT);
    const friday = (await backend.playlistTree()).find((node) => node.name === "Friday")?.id ?? "";
    await backend.edits.addTracksToPlaylist(friday, [track], false);
    const got = await backend.trackPlaylists(track);
    expect(got).toEqual(expect.arrayContaining([{ id: friday, name: "Friday", path: "Gigs" }]));
    // The real command sorts by path, then name, and the tree repeats that.
    const names = got.map((p) => p.name);
    expect(names.indexOf("Alpha")).toBeLessThan(names.indexOf("Friday"));
  });
});

describe("mock edits", () => {
  it("reports stored playlist additions instead of attempted track IDs", async () => {
    const backend = createMockBackend({ trackCount: 2 });
    await backend.edits.createPlaylist("Empty 444", TREE_ROOT);
    const playlist = (await backend.playlistTree()).find((node) => node.name === "Empty 444")?.id ?? "";
    const first = await backend.edits.addTracksToPlaylist(playlist, ["100000", "100000"]);
    expect(first.added).toBe(1);
    const again = await backend.edits.addTracksToPlaylist(playlist, ["100000"]);
    expect(again.added).toBe(0);
    expect(await backend.edits.playlistDuplicates(playlist, ["100000"])).toEqual(["100000"]);
  });

  it("sorts a folder's children by name as one Undo step, as sort_playlist_items does", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createFolder("Crate", TREE_ROOT);
    const folder = (await backend.playlistTree()).find((n) => n.name === "Crate")!;
    await backend.edits.createFolder("Bravo", folder.id);
    const bravo = (await backend.playlistTree()).find((n) => n.name === "Bravo")!;
    await backend.edits.createPlaylist("zulu", bravo.id);
    for (const name of ["delta", "élan", "alpha", "elan"]) await backend.edits.createPlaylist(name, folder.id);
    const children = async () => {
      const tree = await backend.playlistTree();
      const at = tree.findIndex((n) => n.id === folder.id);
      const out: string[] = [];
      for (let i = at + 1; i < tree.length && (tree[i]?.depth ?? 0) > folder.depth; i += 1) {
        if (tree[i]?.depth === folder.depth + 1) out.push(tree[i]!.name);
      }
      return out;
    };
    expect(await children()).toEqual(["Bravo", "delta", "élan", "alpha", "elan"]);
    const history = await backend.edits.sortItems(folder.id, "en");
    expect(history.undoLabel).toBe("Sort Items");
    expect(await children()).toEqual(["alpha", "Bravo", "delta", "elan", "élan"]);
    // The folder keeps what is inside it.
    const tree = await backend.playlistTree();
    expect(tree[tree.findIndex((n) => n.name === "Bravo") + 1]?.name).toBe("zulu");
    // Sorted already: nothing recorded.
    expect((await backend.edits.sortItems(folder.id, "en")).undoLabel).toBe("Sort Items");
    await backend.edits.undoEdit();
    expect(await children()).toEqual(["Bravo", "delta", "élan", "alpha", "elan"]);
    await expect(backend.edits.sortItems(tree.find((n) => n.name === "alpha")!.id)).rejects.toThrow();
  });

  it("bumps the generation on every edit, as a real write does", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const first = await backend.edits.createPlaylist("A", TREE_ROOT);
    const second = await backend.edits.createPlaylist("B", TREE_ROOT);
    expect(second).toBeGreaterThan(first);
  });

  it("adds a playlist to the tree and renames it in place", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Friday", TREE_ROOT);
    let tree = await backend.playlistTree();
    const made = tree.find((n) => n.name === "Friday");
    expect(made).toBeDefined();

    await backend.edits.renamePlaylist(made!.id, "Saturday");
    tree = await backend.playlistTree();
    expect(tree.find((n) => n.id === made!.id)?.name).toBe("Saturday");
    expect(tree.filter((n) => n.name === "Saturday")).toHaveLength(1);
  });

  it("removes a deleted playlist from the tree", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Doomed", TREE_ROOT);
    const before = await backend.playlistTree();
    const doomed = before.find((n) => n.name === "Doomed")!;

    await backend.edits.deletePlaylist(doomed.id);
    const after = await backend.playlistTree();
    expect(after.find((n) => n.id === doomed.id)).toBeUndefined();
    expect(after).toHaveLength(before.length - 1);
  });

  it("creates a playlist at the given place among its siblings", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createFolder("Sets", TREE_ROOT);
    const folder = (await backend.playlistTree()).find((n) => n.name === "Sets")!;
    await backend.edits.createPlaylist("A", folder.id);
    await backend.edits.createPlaylist("B", folder.id);
    await backend.edits.createPlaylist("First", folder.id, 0);
    await backend.edits.createPlaylist("Middle", folder.id, 2);
    const names = (await backend.playlistTree()).filter((n) => n.depth === folder.depth + 1 && ["A", "B", "First", "Middle"].includes(n.name)).map((n) => n.name);
    expect(names).toEqual(["First", "A", "Middle", "B"]);
  });

  it("undoes and redoes a folder deletion with its subtree", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createFolder("Sets", TREE_ROOT);
    const folder = (await backend.playlistTree()).find((n) => n.name === "Sets")!;
    await backend.edits.createPlaylist("Friday", folder.id);
    const playlist = (await backend.playlistTree()).find((n) => n.name === "Friday")!;
    await backend.edits.addTracksToPlaylist(playlist.id, ["100000", "100001"]);

    const deleted = await backend.edits.deletePlaylist(folder.id);
    expect(deleted).toMatchObject({ canUndo: true, canRedo: false, undoLabel: "Delete Playlist" });
    expect((await backend.playlistTree()).some((n) => n.id === playlist.id)).toBe(false);

    const undone = await backend.edits.undoEdit();
    expect(undone).toMatchObject({ canRedo: true });
    expect((await backend.playlistTree()).find((n) => n.id === playlist.id)?.depth).toBe(folder.depth + 1);

    const redone = await backend.edits.redoEdit();
    expect(redone).toMatchObject({ canUndo: true, canRedo: false });
    expect((await backend.playlistTree()).some((n) => n.id === playlist.id)).toBe(false);
  });

  it("undoes and redoes a folder drop's Replace existing as one step (#426)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createFolder("Warm Up", TREE_ROOT);
    const old = (await backend.playlistTree()).find((n) => n.name === "Warm Up")!;
    await backend.edits.createPlaylist("Early", old.id);
    const early = (await backend.playlistTree()).find((n) => n.name === "Early")!;
    await backend.edits.renamePlaylist(early.id, "Opening");
    const before = (await backend.playlistTree()).map((n) => n.id);

    const replaced = await backend.importFolderPlaylist("/music/Warm Up", TREE_ROOT, old.id, null);
    expect(replaced.replaced).toEqual({ lists: 2, tracks: 0 });
    const undone = await backend.edits.undoEdit();
    expect(undone).toMatchObject({ redoLabel: "Replace Playlist", undoLabel: "Rename Playlist" });
    expect((await backend.playlistTree()).map((n) => n.id)).toEqual(before);

    const redone = await backend.edits.redoEdit();
    expect(redone).toMatchObject({ undoLabel: "Replace Playlist", canRedo: false });
    expect((await backend.playlistTree()).some((n) => n.id === old.id)).toBe(false);
    expect(await backend.edits.undoEdit()).toMatchObject({ undoLabel: "Rename Playlist" });
  });

  it("undoes a Replace existing around the drop's new folder and a later playlist, as the real backend does (#426)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Warm Up", TREE_ROOT);
    const old = (await backend.playlistTree()).find((n) => n.name === "Warm Up")!;
    await backend.edits.addTracksToPlaylist(old.id, ["100001", "100002"]);
    await backend.edits.renamePlaylist(old.id, "Warm Up");
    const topLevel = async () => (await backend.playlistTree())
      .filter((n) => n.depth === 1 && n.kind !== "history").map((n) => n.id);
    const before = await topLevel();

    // One drop: the clash replaced at index 0, then a new folder at the
    // drop's same index, then a playlist made later by another drop; none
    // but the first is in the history (a playlist made from the menu would
    // be, since #500).
    const replaced = await backend.importFolderPlaylist("/music/Warm Up", TREE_ROOT, old.id, 0);
    const fresh = await backend.importFolderPlaylist("/music/Brand New", TREE_ROOT, undefined, replaced.at);
    const laterDrop = await backend.importFolderPlaylist("/music/Later", TREE_ROOT, undefined, null);
    const later = (await backend.playlistTree()).find((n) => n.id === laterDrop.playlist)!;
    const dropped = await topLevel();
    expect(dropped.slice(0, 2)).toEqual([fresh.playlist, replaced.playlist]);

    const undone = await backend.edits.undoEdit();
    expect(undone).toMatchObject({ redoLabel: "Replace Playlist", undoLabel: "Rename Playlist" });
    expect(await topLevel()).toEqual([fresh.playlist!, ...before, later.id]);
    expect((await backend.playlistTree()).find((n) => n.id === old.id)?.childCount).toBe(2);

    await backend.edits.redoEdit();
    expect(await topLevel()).toEqual(dropped);
  });

  it("refuses to undo a Replace existing once its playlist changed, and drops only that entry (#426, #506)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Warm Up", TREE_ROOT);
    const old = (await backend.playlistTree()).find((n) => n.name === "Warm Up")!;
    const replaced = await backend.importFolderPlaylist("/music/Warm Up", TREE_ROOT, old.id, null);
    await backend.edits.addTracksToPlaylist(replaced.playlist!, ["100003", "100004"]);
    // Another program takes one out again. The mock has no second writer;
    // Remove from History writes the list's tracks outside the history.
    await backend.edits.removeFromHistory(replaced.playlist!, ["100004"]);
    const tree = (await backend.playlistTree()).map((n) => n.id);
    // The add (#499) is refused first, then the replacement.
    await expect(backend.edits.undoEdit()).rejects.toMatchObject({
      kind: "conflict",
      message: "Can't undo Add Tracks to Playlist: the playlist's tracks have changed since. The newer change was kept.",
    });
    await expect(backend.edits.undoEdit()).rejects.toMatchObject({
      kind: "conflict",
      message: "Can't undo Replace Playlist: the playlists have changed since. The newer change was kept.",
    });
    expect((await backend.playlistTree()).map((n) => n.id)).toEqual(tree);
    // Gone, as the real backend drops it, so it blocks nothing older: next
    // is the replaced playlist's own creation, which is refused too since
    // the replacement took it away (#500).
    await expect(backend.edits.undoEdit()).rejects.toMatchObject({
      kind: "conflict",
      message: "Can't undo Create Playlist: the playlist is no longer there. The newer change was kept.",
    });
    await expect(backend.edits.undoEdit()).rejects.toThrow("no library edit to undo");
  });

  it("undoes and redoes a playlist, folder and intelligent playlist creation, one step each (#500)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const ids = async () => (await backend.playlistTree()).map((n) => n.id);
    const before = await ids();
    await backend.edits.createFolder("Crates", TREE_ROOT);
    const folder = (await backend.playlistTree()).find((n) => n.name === "Crates")!;
    await backend.edits.createPlaylist("Friday", folder.id);
    await backend.edits.createSmartPlaylist("Sevens", TREE_ROOT, { logic: "all", conditions: [] });
    await backend.edits.setSmartRule((await backend.playlistTree()).find((n) => n.name === "Sevens")!.id, {
      logic: "any",
      conditions: [],
    });
    const after = await ids();

    expect(await backend.edits.undoEdit()).toMatchObject({ redoLabel: "Edit Intelligent Playlist", undoLabel: "Create Intelligent Playlist" });
    expect(await backend.edits.undoEdit()).toMatchObject({ undoLabel: "Create Playlist" });
    expect(await backend.edits.undoEdit()).toMatchObject({ undoLabel: "Create Folder" });
    expect(await backend.edits.undoEdit()).toMatchObject({ canUndo: false });
    expect(await ids()).toEqual(before);
    for (let i = 0; i < 4; i += 1) await backend.edits.redoEdit();
    expect(await ids()).toEqual(after);
  });

  it("refuses to undo a creation once something was put in it, and keeps it (#500, #506)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Friday", TREE_ROOT);
    const friday = (await backend.playlistTree()).find((n) => n.name === "Friday")!;
    await backend.edits.addTracksToPlaylist(friday.id, ["100001", "100002"]);
    // Another program takes one out again, so the add (#499) is refused
    // first. The mock has no second writer; Remove from History writes the
    // list's tracks outside the history.
    await backend.edits.removeFromHistory(friday.id, ["100002"]);
    await expect(backend.edits.undoEdit()).rejects.toMatchObject({
      kind: "conflict",
      message: "Can't undo Add Tracks to Playlist: the playlist's tracks have changed since. The newer change was kept.",
    });
    await expect(backend.edits.undoEdit()).rejects.toMatchObject({
      kind: "conflict",
      message: "Can't undo Create Playlist: tracks have been added to the playlist since. The newer change was kept.",
    });
    expect((await backend.playlistTree()).some((n) => n.id === friday.id)).toBe(true);
  });

  it("clears library redo when a new edit branches from an undo", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Doomed", TREE_ROOT);
    const doomed = (await backend.playlistTree()).find((n) => n.name === "Doomed")!;
    await backend.edits.deletePlaylist(doomed.id);
    await backend.edits.undoEdit();

    await backend.edits.renamePlaylist(doomed.id, "Kept");
    await expect(backend.edits.redoEdit()).rejects.toThrow("no library edit");
    expect((await backend.playlistTree()).find((n) => n.id === doomed.id)?.name).toBe("Kept");
    await backend.edits.undoEdit();
    expect((await backend.playlistTree()).find((n) => n.id === doomed.id)?.name).toBe("Doomed");
  });

  it("does not add a track that is already in the playlist", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Set", TREE_ROOT);
    const list = (await backend.playlistTree()).find((n) => n.name === "Set")!;

    await backend.edits.addTracksToPlaylist(list.id, ["100000", "100001"]);
    await backend.edits.addTracksToPlaylist(list.id, ["100000", "100002"]);
    // Asserted through reorder, which reports the surviving order.
    const generation = await backend.edits.reorderPlaylist(list.id, []);
    expect(generation).toBeGreaterThan(0);
  });

  it("keeps tracks a partial reorder did not mention", async () => {
    // Dropping them would silently empty a playlist when a caller passes only
    // the visible window. Same rule as the Rust writer.
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Set", TREE_ROOT);
    const list = (await backend.playlistTree()).find((n) => n.name === "Set")!;
    const tracks = ["100000", "100001", "100002", "100003"];
    await backend.edits.addTracksToPlaylist(list.id, tracks);

    await backend.edits.reorderPlaylist(list.id, ["100003"]);
    await backend.edits.removeTracksFromPlaylist(list.id, ["100000"]);
    // Nothing above should have thrown, and the generation keeps advancing.
    const generation = await backend.edits.reorderPlaylist(list.id, []);
    expect(generation).toBeGreaterThan(4);
  });

  it("undoes one add sent in slices, and a reorder, as one step each (#499)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Set", TREE_ROOT);
    const list = (await backend.playlistTree()).find((n) => n.name === "Set")!;
    const order = async () => (await backend.fetchRows(
      (await backend.openView({ source: { kind: "playlist", id: list.id }, sort: "trackNo", descending: false, query: "" })).viewId,
      0, 20,
    )).map((row) => row.id);

    await backend.edits.addTracksToPlaylist(list.id, ["100000"]);
    await backend.edits.addTracksToPlaylist(list.id, ["100001", "100002"], false, "job-499");
    await backend.edits.addTracksToPlaylist(list.id, ["100003"], false, "job-499");
    expect(await order()).toEqual(["100000", "100001", "100002", "100003"]);
    await backend.edits.reorderPlaylist(list.id, ["100003", "100000"]);
    expect(await order()).toEqual(["100003", "100000", "100001", "100002"]);

    expect((await backend.edits.undoEdit()).undoLabel).toBe("Add Tracks to Playlist");
    expect(await order()).toEqual(["100000", "100001", "100002", "100003"]);
    const undone = await backend.edits.undoEdit();
    expect(await order()).toEqual(["100000"]);
    expect(undone.redoLabel).toBe("Add Tracks to Playlist");
    await backend.edits.redoEdit();
    expect(await order()).toEqual(["100000", "100001", "100002", "100003"]);
    expect((await backend.edits.redoEdit()).undoLabel).toBe("Reorder Playlist");
    expect(await order()).toEqual(["100003", "100000", "100001", "100002"]);
  });

  it("undoes Remove from Tag List and Clear Tag List one step each, in order, and refuses a list changed since (#508)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const tagged = async () => (await backend.fetchRows(
      (await backend.openView({ source: { kind: "tagList" }, sort: "trackNo", descending: false, query: "" })).viewId,
      0, 20,
    )).map((row) => row.id);
    await backend.edits.addToTagList(["100003", "100001", "100004", "100002"]);
    await backend.edits.removeFromTagList(["100001", "100002"]);
    expect(await tagged()).toEqual(["100003", "100004"]);
    await backend.edits.clearTagList();
    expect(await tagged()).toEqual([]);

    expect(await backend.edits.undoEdit()).toMatchObject({ undoLabel: "Remove from Tag List", redoLabel: "Clear Tag List" });
    expect(await tagged()).toEqual(["100003", "100004"]);
    await backend.edits.undoEdit();
    expect(await tagged()).toEqual(["100003", "100001", "100004", "100002"]);
    expect(await backend.edits.redoEdit()).toMatchObject({ undoLabel: "Remove from Tag List" });
    expect(await tagged()).toEqual(["100003", "100004"]);

    // A removal of nothing records nothing; a tag added since refuses the Undo.
    await backend.edits.removeFromTagList(["100009"]);
    await backend.edits.addToTagList(["100007"]);
    await expect(backend.edits.undoEdit()).rejects.toMatchObject({
      kind: "conflict",
      message: "Can't undo Remove from Tag List: the Tag List has changed since. The newer change was kept.",
    });
    expect(await tagged()).toEqual(["100003", "100004", "100007"]);
  });

  it("clamps a rating to the range the backend accepts", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.setTrackRating(["100000"], 9);
    const rows = await backend.fetchRows(
      (await backend.openView({ source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" })).viewId,
      0,
      1,
    );
    expect(rows[0]?.rating).toBeLessThanOrEqual(5);
  });

  it("writes one edit to every track of a selection and undoes it in one step", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const ids = ["100000", "100001", "100002"];
    await backend.edits.setTrackField(ids, "genre", "Techno");
    await backend.edits.setTrackRating(ids, 4);
    for (const id of ids) {
      const d = await backend.trackDetails(id);
      expect(d.genre).toBe("Techno");
      expect(d.rating).toBe(4);
    }
    await backend.edits.undoEdit();
    for (const id of ids) expect((await backend.trackDetails(id)).rating).not.toBe(4);
    expect((await backend.trackDetails("100001")).genre).toBe("Techno");
  });

  it("records an info-panel BPM as the grid's Set Tempo and keeps the earlier grid Undo (#505)", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    let id = "";
    for (let n = 100000; n < 100020 && !id; n++) {
      if (await backend.gridState(String(n)).then(() => true, () => false)) id = String(n);
    }
    expect(id).not.toBe("");
    const original = (await backend.gridState(id)).bpmX100;
    await backend.edits.gridEdit(id, { kind: "nudge", ms: 5 });
    await backend.edits.setTrackField([id], "bpm", "150");
    const typed = await backend.gridState(id);
    expect(typed.bpmX100).toBe(15_000);
    expect(typed.undoLabel).toBe("Set Tempo");
    expect((await backend.trackDetails(id)).bpmX100).toBe(15_000);
    const undone = await backend.edits.gridUndo(id);
    expect(undone.bpmX100).toBe(original);
    expect(undone.undoLabel).toBe("Shift Beat Grid Right");
    expect((await backend.trackDetails(id)).bpmX100).toBe(original);
    expect((await backend.edits.gridRedo(id)).bpmX100).toBe(15_000);
  });

  it("refuses a title for several tracks, as rekordbox greys the box", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await expect(backend.edits.setTrackField(["100000", "100001"], "title", "Same")).rejects.toThrow();
  });

  it("reads a selection as the first track with the differing fields named", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const ids = ["100000", "100001"];
    await backend.edits.setTrackField(ids, "genre", "Techno");
    const selection = await backend.selectionDetails(ids);
    expect(selection.count).toBe(2);
    expect(selection.first.id).toBe("100000");
    expect(selection.mixed).toContain("title");
    expect(selection.mixed).not.toContain("genre");
  });

  it("writes a comment through to the rows the table reads", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.setTrackComment(["100000"], "5A - Am - 128");
    const handle = await backend.openView({
      source: { kind: "collection" }, sort: "trackNo", descending: false, query: "",
    });
    const rows = await backend.fetchRows(handle.viewId, 0, 1);
    expect(rows[0]?.comment).toBe("5A - Am - 128");
  });
});

describe("the mock's Explorer", () => {
  it("starts from the four roots of the capture and opens one level at a time", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const roots = await backend.explorerRoots();
    expect(roots.map((r) => r.name)).toEqual(["Music", "mock", "Macintosh HD", "SD"]);
    expect((await backend.explorerChildren("/")).names).toEqual(["Applications", "Library", "System", "Users"]);
    expect((await backend.explorerChildren("/no/such")).names).toEqual([]);
  });

  it("lists a folder's files: library rows where it holds them, loose files where not", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const spec = { source: { kind: "folder", path: "/Users/mock/Music/Downloads" }, sort: "trackNo", descending: false, query: "" } as const;
    const view = await backend.openView(spec);
    expect(view.len).toBe(12);
    const rows = await backend.fetchRows(view.viewId, 0, 12);
    expect(rows.slice(0, 6).every((r) => /^\d+$/.test(r.id))).toBe(true);
    expect(rows.slice(6).every((r) => r.id.startsWith("file:"))).toBe(true);
    expect(rows[6]?.fileName).toBe("Untitled Bounce 1.wav");
    expect(rows.map((r) => r.trackNo)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    expect(await backend.viewIdsInRange(view.viewId, 10, 11)).toEqual([rows[10]?.id, rows[11]?.id]);
  });

  it("opens an unknown folder, and the heading, empty", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    for (const path of ["", "/Users/mock/Music/Rekordbox", "/nope"]) {
      const view = await backend.openView({ source: { kind: "folder", path }, sort: "trackNo", descending: false, query: "" });
      expect(view.len).toBe(0);
    }
  });

  it("searches a folder by title and by file name", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const view = await backend.openView({ source: { kind: "folder", path: "/Users/mock/Music/Downloads" }, sort: "trackNo", descending: false, query: "bounce" });
    expect(view.len).toBe(2);
  });
});

describe("USB music cleanup", () => {
  it("keeps individually exported music by default, then removes only music outside all synced playlists", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.edits.createPlaylist("Cleanup A", TREE_ROOT);
    await backend.edits.createPlaylist("Cleanup B", TREE_ROOT);
    const tree = await backend.playlistTree();
    const a = tree.find(n => n.name === "Cleanup A")!.id;
    const b = tree.find(n => n.name === "Cleanup B")!.id;
    await backend.edits.addTracksToPlaylist(a, ["100000"]);
    await backend.edits.addTracksToPlaylist(b, ["100001"]);
    const destination = (await backend.listDevices()).find(d => !d.export)?.path;
    expect(destination).toBeDefined();
    await backend.exportTracksToDevice(["100001", "100002"], destination!);
    const kept = await backend.syncDevices([a, b], [destination!], undefined);
    expect(kept[0]?.report).toMatchObject({ tracks: 3, removed: 0 });
    const cleaned = await backend.syncDevices([a, b], [destination!], undefined, false, false, true);
    expect(cleaned[0]?.report).toMatchObject({ tracks: 2, removed: 1 });
    await backend.exportTracksToDevice(["100002"], destination!);
    // Export Playlist adds to what the stick holds (#304): B stays, and only
    // the loose track outside every playlist is cleaned up.
    const exported = await backend.exportPlaylist(a, destination!, undefined, true);
    expect(exported).toMatchObject({ tracks: 2, removed: 1, playlists: 2 });
  });
});

describe("mock preview", () => {
  it("previews a track from a point, pauses the deck, and stops where it is", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await backend.deckLoad("a", "100001", 1);
    await backend.deckPlay("a");
    expect((await backend.deckState()).a.playing).toBe(true);

    await backend.previewPlay("100002", 12_000);
    // Outside PERFORMANCE mode rekordbox pauses the decks for a preview.
    expect((await backend.deckState()).a.playing).toBe(false);
    const playing = await backend.previewState();
    expect(playing.track).toBe("100002");
    expect(playing.playing).toBe(true);
    expect(playing.positionMs).toBeGreaterThanOrEqual(12_000);
    expect(playing.durationMs).toBeGreaterThan(12_000);

    await backend.previewStop();
    const stopped = await backend.previewState();
    expect(stopped.playing).toBe(false);
    expect(stopped.positionMs).toBeGreaterThanOrEqual(12_000);
  });

  it("refuses a track that is not there", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    await expect(backend.previewPlay("no-such-track", 0)).rejects.toMatchObject({ kind: "notFound" });
    expect((await backend.previewState()).track).toBeNull();
  });
});

describe("mock File Created column (#308)", () => {
  it("sends seconds or null only when asked, and sorts the blanks first", async () => {
    const backend = createMockBackend({ trackCount: 20 });
    const plain = await backend.openView({ source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" });
    const [first] = await backend.fetchRows(plain.viewId, 0, 1);
    expect(first?.extra?.fileCreated).toBeUndefined();

    const view = await backend.openView({ source: { kind: "collection" }, sort: "fileCreated", descending: false, query: "" });
    const rows = await backend.fetchRows(view.viewId, 0, 20, ["fileCreated"]);
    const values = rows.map((row) => row.extra?.fileCreated);
    expect(values.slice(0, 2)).toEqual([null, null]);
    const known = values.slice(2) as number[];
    expect(known.every((value) => typeof value === "number")).toBe(true);
    expect([...known].sort((a, b) => a - b)).toEqual(known);
  });
});

describe("mock Relocate Undo (#502)", () => {
  // The mock's relocate keeps what it was asked on `window`.
  afterEach(() => vi.unstubAllGlobals());
  const backendWithMissing = () => {
    vi.stubGlobal("window", {});
    return createMockBackend({ trackCount: 20, missingEvery: 5 });
  };
  const missingIds = async (backend: ReturnType<typeof createMockBackend>) =>
    (await backend.missingTracks(0, 100, true)).tracks.map((track) => track.id);

  it("undoes one Relocate and one Relocate by location as one step each", async () => {
    const backend = backendWithMissing();
    const missing = await missingIds(backend);
    expect(missing.length).toBe(4);
    const [first = "", ...rest] = missing;

    expect(await backend.relocateTrack(first, "/Users/mock/Music/Moved/a.mp3")).toBe(true);
    expect(await backend.relocateByLocation(rest, "/old", "/new")).toBe(3);
    expect(await missingIds(backend)).toEqual([]);

    const undone = await backend.edits.undoEdit();
    expect(undone).toMatchObject({ undoLabel: "Relocate", redoLabel: "Relocate" });
    expect(await missingIds(backend)).toEqual(rest);
    await backend.edits.undoEdit();
    expect(await missingIds(backend)).toEqual(missing);
    await backend.edits.redoEdit();
    expect(await missingIds(backend)).toEqual(rest);
  });

  it("undoes an Auto Relocate run as one step", async () => {
    const backend = backendWithMissing();
    const missing = await missingIds(backend);
    const report = await backend.autoRelocate({ folders: ["/x"], music: false, video: false, desktop: false }, null);
    expect(report.relocated).toBe(2);
    const history = await backend.edits.undoEdit();
    expect(history).toMatchObject({ canUndo: false, redoLabel: "Relocate" });
    expect(await missingIds(backend)).toEqual(missing);
  });
});
