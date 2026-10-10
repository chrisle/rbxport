import { describe, expect, it, vi } from "vitest";

import type { FolderPlaylistReport } from "@/ipc/types";
import { importFolderDrop } from "./folderDrop";

const t = (text: string, values?: Readonly<Record<string, string | number>>) =>
  text.replace(/\{(\w+)\}/g, (_, key: string) => String(values?.[key] ?? `{${key}}`));

const report = (name: string, extra: Partial<FolderPlaylistReport> = {}): FolderPlaylistReport => ({
  name, playlist: null, conflict: null, folder: true, imported: 0, skipped: [], tracks: [], existing: 0, at: null, ...extra,
});

describe("importFolderDrop", () => {
  it("gives every folder of one drop the index the first one settled on, as rekordbox does", async () => {
    const importFolderPlaylist = vi.fn((path: string, _parent: string, _replace?: string, at?: number | null) =>
      Promise.resolve(report(path, { playlist: `id-${path}`, at: at ?? 7 })));
    const done = await importFolderDrop({ importFolderPlaylist, confirm: vi.fn() }, "root", ["A", "B"], t);
    expect(importFolderPlaylist.mock.calls).toEqual([
      ["A", "root", undefined, null],
      ["B", "root", undefined, 7],
    ]);
    expect(done).toEqual({ message: "Made playlists: A, B.", refused: false, tracks: [] });
  });

  it("asks rekordbox's question on a name clash and carries the lowered index on", async () => {
    const importFolderPlaylist = vi.fn((path: string, _parent: string, replace?: string, at?: number | null) => {
      if (path === "Old" && !replace) return Promise.resolve(report(path, { conflict: "old-id", at: 4 }));
      if (path === "Old") return Promise.resolve(report(path, { playlist: "new-old", at: 3 }));
      return Promise.resolve(report(path, { playlist: `id-${path}`, at: at ?? null }));
    });
    const confirm = vi.fn(() => Promise.resolve(true));
    await importFolderDrop({ importFolderPlaylist, confirm }, "f1", ["Old", "C"], t);
    expect(confirm).toHaveBeenCalledWith(
      "One or several lists with the same name already exist.\nDo you want to replace them with the one you're importing?",
    );
    expect(importFolderPlaylist.mock.calls).toEqual([
      ["Old", "f1", undefined, null],
      ["Old", "f1", "old-id", 4],
      ["C", "f1", undefined, 3],
    ]);
  });

  it("stops between folders: the folders before are playlists, the rest are not touched", async () => {
    const stop = new AbortController();
    const importFolderPlaylist = vi.fn((path: string) =>
      Promise.resolve(report(path, { playlist: `id-${path}`, tracks: [{ id: path, title: path }] })));
    const seen: number[] = [];
    const done = await importFolderDrop({ importFolderPlaylist, confirm: vi.fn() }, "root", ["A", "B", "C"], t,
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
    await importFolderDrop({
      importFolderPlaylist: (path) => Promise.resolve(report(path, { conflict: "x", at: 0 })),
      confirm: () => Promise.resolve(false),
    }, "root", ["Old", "Older"], t, undefined, (n) => seen.push(n));
    expect(seen).toEqual([1, 2]);
  });

  it("keeps the old list when the answer is no, and refuses a drop with no folder", async () => {
    const kept = await importFolderDrop({
      importFolderPlaylist: (path) => Promise.resolve(report(path, { conflict: "x", at: 0 })),
      confirm: () => Promise.resolve(false),
    }, "root", ["Old"], t);
    expect(kept.message).toBe("No playlist was made.");

    const loose = await importFolderDrop({
      importFolderPlaylist: (path) => Promise.resolve(report(path, { folder: false })),
      confirm: vi.fn(),
    }, "root", ["track.mp3"], t);
    expect(loose).toEqual({
      message: "Drop folders onto Playlists or a playlist folder to make playlists of them.",
      refused: true,
      tracks: [],
    });
  });
});
