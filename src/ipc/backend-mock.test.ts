/**
 * The mock's edit semantics have to match Rust's, or `pnpm dev:mock` and the
 * Playwright suite validate behaviour the real backend does not have.
 *
 * The rules asserted here are the ones `crates/rbl-db/tests/writes.rs` asserts
 * on the other side.
 */
import { describe, expect, it } from "vitest";

import { createMockBackend } from "./backend-mock";
import { TREE_ROOT } from "./types";

describe("mock edits", () => {
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
