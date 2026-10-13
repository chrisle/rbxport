import { describe, expect, it, vi } from "vitest";

import type { FolderPlaylistReport } from "@/ipc/types";
import type { FolderDropClash } from "@/lib/preferences";
import { importFolderDrop, type FolderClashAnswer } from "./folderDrop";

const t = (text: string, values?: Readonly<Record<string, string | number>>) =>
  text.replace(/\{(\w+)\}/g, (_, key: string) => String(values?.[key] ?? `{${key}}`));

const report = (name: string, extra: Partial<FolderPlaylistReport> = {}): FolderPlaylistReport => ({
  name, renamed: null, playlist: null, conflict: null, folder: true, imported: 0, skipped: [], tracks: [], existing: 0,
  at: null, ...extra,
});

const never = () => Promise.reject(new Error("asked"));
const policy = (mode: FolderDropClash, ask: (name: string) => Promise<FolderClashAnswer> = never) => ({ mode, ask });

/**
 * A backend whose target already holds `Old` and `Older`: a call without an
 * answer comes back as a conflict, `replace` replaces, `copy` names the new
 * list `<name> (1)` as the real backend does.
 */
function clashing() {
  return vi.fn((path: string, _parent: string, replace?: string, at?: number | null, copy?: boolean) => {
    const held = path === "Old" || path === "Older";
    if (held && copy) return Promise.resolve(report(path, { playlist: `copy-${path}`, renamed: `${path} (1)`, at: at ?? 0 }));
    if (held && replace) return Promise.resolve(report(path, { playlist: `new-${path}`, at: at ?? 0 }));
    if (held) return Promise.resolve(report(path, { conflict: `id-${path}`, at: at ?? 0 }));
    return Promise.resolve(report(path, { playlist: `id-${path}`, at: at ?? 0 }));
  });
}

describe("importFolderDrop", () => {
  it("gives every folder of one drop the index the first one settled on, as rekordbox does", async () => {
    const importFolderPlaylist = vi.fn((path: string, _parent: string, _replace?: string, at?: number | null) =>
      Promise.resolve(report(path, { playlist: `id-${path}`, at: at ?? 7 })));
    const done = await importFolderDrop({ importFolderPlaylist }, "root", ["A", "B"], t, policy("copy"));
    expect(importFolderPlaylist.mock.calls).toEqual([
      ["A", "root", undefined, null, true],
      ["B", "root", undefined, 7, true],
    ]);
    expect(done).toEqual({ message: "Made playlists: A, B.", refused: false, tracks: [] });
  });

  it("imports a same-named folder as a copy without asking by default, and says so (#355)", async () => {
    const importFolderPlaylist = clashing();
    const ask = vi.fn(never);
    const done = await importFolderDrop({ importFolderPlaylist }, "root", ["Old", "New"], t, policy("copy", ask));
    expect(ask).not.toHaveBeenCalled();
    expect(importFolderPlaylist.mock.calls.map((call) => call.slice(2))).toEqual([
      [undefined, null, true],
      [undefined, 0, true],
    ]);
    expect(done.message).toBe(
      "Made playlists: Old (1), New. A list named Old was already there, so the folder was imported as Old (1).",
    );
  });

  it("replaces without asking when Preferences says Replace existing", async () => {
    const importFolderPlaylist = clashing();
    const done = await importFolderDrop({ importFolderPlaylist }, "f1", ["Old"], t, policy("replace"));
    expect(importFolderPlaylist.mock.calls).toEqual([
      ["Old", "f1", undefined, null, false],
      ["Old", "f1", "id-Old", 0],
    ]);
    expect(done.message).toBe("Made playlists: Old.");
  });

  it("leaves the folder untouched when Stop is pressed while Ask me is waiting", async () => {
    const stop = new AbortController();
    const importFolderPlaylist = clashing();
    const ask = vi.fn(() => {
      stop.abort();
      return Promise.resolve<FolderClashAnswer>({ choice: "replace", all: false });
    });
    const done = await importFolderDrop({ importFolderPlaylist }, "root", ["Old", "New"], t, policy("ask", ask),
      stop.signal);
    expect(ask).toHaveBeenCalledTimes(1);
    expect(importFolderPlaylist.mock.calls.map((call) => call.slice(0, 3))).toEqual([["Old", "root", undefined]]);
    expect(done.message).toBe("No playlist was made.");
  });

  it("keeps the old list without asking when Preferences says Keep existing", async () => {
    const importFolderPlaylist = clashing();
    const done = await importFolderDrop({ importFolderPlaylist }, "root", ["Old", "New"], t, policy("keep"));
    expect(importFolderPlaylist.mock.calls.map((call) => call[0])).toEqual(["Old", "New"]);
    expect(done.message).toBe("Made playlists: New.");

    const none = await importFolderDrop({ importFolderPlaylist: clashing() }, "root", ["Old"], t, policy("keep"));
    expect(none.message).toBe("No playlist was made.");
  });

  it("asks only under Ask me, once per folder unless the answer is for the rest of the drop", async () => {
    const one = clashing();
    const each = vi.fn((name: string) =>
      Promise.resolve<FolderClashAnswer>({ choice: name === "Old" ? "replace" : "copy", all: false }));
    const done = await importFolderDrop({ importFolderPlaylist: one }, "root", ["Old", "Older"], t, policy("ask", each));
    expect(each.mock.calls).toEqual([["Old"], ["Older"]]);
    expect(one.mock.calls.map((call) => call.slice(0, 5))).toEqual([
      ["Old", "root", undefined, null, false],
      ["Old", "root", "id-Old", 0],
      ["Older", "root", undefined, 0, false],
      ["Older", "root", undefined, 0, true],
    ]);
    expect(done.message).toBe(
      "Made playlists: Old, Older (1). A list named Older was already there, so the folder was imported as Older (1).",
    );

    const batch = clashing();
    const once = vi.fn(() => Promise.resolve<FolderClashAnswer>({ choice: "keep", all: true }));
    const kept = await importFolderDrop({ importFolderPlaylist: batch }, "root", ["Old", "Older", "New"], t,
      policy("ask", once));
    expect(once).toHaveBeenCalledTimes(1);
    expect(batch.mock.calls.map((call) => call[0])).toEqual(["Old", "Older", "New"]);
    expect(kept.message).toBe("Made playlists: New.");

    // A copy chosen for the rest of the drop is asked for up front, as the
    // Import as a copy setting is.
    const copies = clashing();
    await importFolderDrop({ importFolderPlaylist: copies }, "root", ["Old", "Older"], t,
      policy("ask", () => Promise.resolve({ choice: "copy", all: true })));
    expect(copies.mock.calls.map((call) => [call[0], call[4]])).toEqual([
      ["Old", false], ["Old", true], ["Older", true],
    ]);
  });

  it("stops between folders: the folders before are playlists, the rest are not touched", async () => {
    const stop = new AbortController();
    const importFolderPlaylist = vi.fn((path: string) =>
      Promise.resolve(report(path, { playlist: `id-${path}`, tracks: [{ id: path, title: path }] })));
    const seen: number[] = [];
    const done = await importFolderDrop({ importFolderPlaylist }, "root", ["A", "B", "C"], t, policy("copy"),
      stop.signal, (n) => { seen.push(n); if (n === 2) stop.abort(); });
    expect(importFolderPlaylist.mock.calls.map((call) => call[0])).toEqual(["A", "B"]);
    expect(seen).toEqual([1, 2]);
    expect(done).toEqual({
      message: "Made playlists: A, B.", refused: false,
      tracks: [{ id: "A", title: "A" }, { id: "B", title: "B" }],
    });
  });

  it("counts a folder kept on a name clash as done", async () => {
    const seen: number[] = [];
    await importFolderDrop({ importFolderPlaylist: clashing() }, "root", ["Old", "Older"], t,
      policy("ask", () => Promise.resolve({ choice: "keep", all: false })), undefined, (n) => seen.push(n));
    expect(seen).toEqual([1, 2]);
  });

  it("refuses a drop with no folder", async () => {
    const loose = await importFolderDrop({
      importFolderPlaylist: (path) => Promise.resolve(report(path, { folder: false })),
    }, "root", ["track.mp3"], t, policy("copy"));
    expect(loose).toEqual({
      message: "Drop folders onto Playlists or a playlist folder to make playlists of them.",
      refused: true,
      tracks: [],
    });
  });
});
