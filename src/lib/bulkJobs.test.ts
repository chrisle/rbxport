import { describe, expect, it } from "vitest";

import type { ImportReport } from "@/ipc/types";
import { importInSlices, removeInSlices } from "./bulkJobs";

const ids = (n: number, prefix = "t") => Array.from({ length: n }, (_, i) => `${prefix}${i}`);

/**
 * A stand-in library: each call is applied whole or not at all, as one
 * transaction is, so what it holds after a stop can be checked.
 */
function library(held: readonly string[] = []) {
  const tracks = new Set(held);
  const calls: string[][] = [];
  return {
    tracks,
    calls,
    importSlice: (paths: string[]): Promise<ImportReport> => {
      calls.push(paths);
      const report: ImportReport = { imported: 0, skipped: [], tracks: [], existing: [] };
      for (const path of paths) {
        if (path.endsWith(".txt")) report.skipped.push(`${path}: not audio`);
        else if (tracks.has(path)) report.existing.push({ id: path, title: path });
        else {
          tracks.add(path);
          report.imported += 1;
          report.tracks.push({ id: path, title: path });
        }
      }
      return Promise.resolve(report);
    },
    remove: (slice: string[]): Promise<number> => {
      calls.push(slice);
      for (const id of slice) tracks.delete(id);
      return Promise.resolve(slice.length);
    },
  };
}

describe("removeInSlices", () => {
  it("removes 495 tracks in slices of 100 and reports each", async () => {
    const lib = library(ids(495));
    const seen: number[] = [];
    const removed = await removeInSlices(ids(495), lib.remove, new AbortController().signal, (n) => seen.push(n));
    expect(removed).toBe(495);
    expect(lib.calls.map((c) => c.length)).toEqual([100, 100, 100, 100, 95]);
    expect(seen).toEqual([100, 200, 300, 400, 495]);
    expect(lib.tracks.size).toBe(0);
  });

  it("stops between slices: the slices before are removed whole, the rest are untouched", async () => {
    const all = ids(495);
    const lib = library(all);
    const stop = new AbortController();
    const removed = await removeInSlices(all, lib.remove, stop.signal, (n) => { if (n === 200) stop.abort(); });
    expect(removed).toBe(200);
    expect(lib.calls).toHaveLength(2);
    expect([...lib.tracks]).toEqual(all.slice(200));
  });

  it("throws on a failed slice and keeps the count of the slices that landed", async () => {
    const lib = library(ids(300));
    let calls = 0;
    let removed = 0;
    const failing = (slice: string[]) => (++calls === 2 ? Promise.reject(new Error("locked")) : lib.remove(slice));
    await expect(removeInSlices(ids(300), failing, new AbortController().signal, (n) => { removed = n; }))
      .rejects.toThrow("locked");
    expect(removed).toBe(100);
    expect(lib.tracks.size).toBe(200);
  });
});

describe("importInSlices", () => {
  it("adds up every slice and keeps held files as the tracks they stand for", async () => {
    const files = [...ids(150, "a"), "notes.txt", "held"];
    const lib = library(["held"]);
    const done = await importInSlices(files, lib.importSlice, new AbortController().signal);
    expect(done).toMatchObject({ reached: 152, imported: 150, skipped: 1, existing: 1 });
    expect(done.ids).toEqual([...ids(150, "a"), "held"]);
    expect(done.tracks).toHaveLength(150);
  });

  it("stops between slices, and a second run takes up exactly what the first left", async () => {
    const files = ids(495, "f");
    const lib = library();
    const stop = new AbortController();
    const reached: number[] = [];
    const first = await importInSlices(files, lib.importSlice, stop.signal, (n) => {
      reached.push(n);
      if (n === 300) stop.abort();
    });
    expect(reached).toEqual([100, 200, 300]);
    expect(first).toMatchObject({ reached: 300, imported: 300 });
    expect(lib.tracks.size).toBe(300);

    const again = await importInSlices(files, lib.importSlice, new AbortController().signal);
    expect(again).toMatchObject({ reached: 495, imported: 195, existing: 300 });
    expect(again.ids).toEqual(files);
  });

  it("does nothing once stopped before the first slice", async () => {
    const lib = library();
    const stop = new AbortController();
    stop.abort();
    const done = await importInSlices(ids(10), lib.importSlice, stop.signal);
    expect(done.reached).toBe(0);
    expect(lib.calls).toEqual([]);
  });
});
