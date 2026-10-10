import { describe, expect, it, vi } from "vitest";

import type { UnanalysedTracks } from "@/ipc/types";
import { analysesOnLoad, autoAnalysisOffer, takeRemainingPages, UNANALYSED_PAGE } from "./autoAnalysis";

/** A backend over fixed pages, keyed by the row each starts from. */
function pages(byRow: Record<number, UnanalysedTracks>) {
  return {
    unanalysedTracks: vi.fn((from: number, limit: number) => {
      expect(limit).toBe(UNANALYSED_PAGE);
      return Promise.resolve(byRow[from] ?? { tracks: [], next: null });
    }),
  };
}

const track = (id: string) => ({ id, title: `Track ${id}` });

describe("the Auto Analysis launch prompt", () => {
  it("offers the first page of unanalysed tracks when Auto Analysis is on", async () => {
    const backend = pages({ 0: { tracks: [track("1")], next: 40 } });
    expect(await autoAnalysisOffer(backend, { auto: true, readOnly: false }))
      .toEqual({ tracks: [track("1")], next: 40 });
  });

  it("asks nothing when Auto Analysis is off, the library is read-only, or all is analysed", async () => {
    const backend = pages({ 0: { tracks: [track("1")], next: null } });
    expect(await autoAnalysisOffer(backend, { auto: false, readOnly: false })).toBeNull();
    expect(await autoAnalysisOffer(backend, { auto: true, readOnly: true })).toBeNull();
    expect(backend.unanalysedTracks).not.toHaveBeenCalled();
    expect(await autoAnalysisOffer(pages({}), { auto: true, readOnly: false })).toBeNull();
  });

  it("queues every later page in order, skipping empty ones", async () => {
    const backend = pages({
      40: { tracks: [track("2"), track("3")], next: 90 },
      90: { tracks: [], next: 200 },
      200: { tracks: [track("4")], next: null },
    });
    const taken: string[][] = [];
    await takeRemainingPages(backend, 40, tracks => taken.push(tracks.map(t => t.id)));
    expect(taken).toEqual([["2", "3"], ["4"]]);
  });

  it("stops at a page that does not move on", async () => {
    const backend = pages({ 40: { tracks: [track("2")], next: 40 } });
    const take = vi.fn();
    await takeRemainingPages(backend, 40, take);
    expect(take).toHaveBeenCalledTimes(1);
    expect(backend.unanalysedTracks).toHaveBeenCalledTimes(1);
  });
});

describe("analysing a track as it is loaded onto a deck", () => {
  const writable = { readOnly: false, queued: () => false };
  it("analyses a never-analysed Collection track, as rekordbox's analyzeCollectionLoad does", () => {
    expect(analysesOnLoad({ id: "100011", analysed: 0 }, writable)).toBe(true);
  });

  it("leaves analysed and locked tracks alone", () => {
    expect(analysesOnLoad({ id: "100003", analysed: 1 }, writable)).toBe(false);
    // rekordbox's Analysed with the analysis lock: bit 0x80.
    expect(analysesOnLoad({ id: "100004", analysed: 0x80 }, writable)).toBe(false);
  });

  it("leaves files outside the library, missing files and a read-only library alone", () => {
    expect(analysesOnLoad({ id: "file:/Volumes/USB/Contents/a.mp3", analysed: 0 }, writable)).toBe(false);
    expect(analysesOnLoad({ id: "100011", analysed: 0, missing: true }, writable)).toBe(false);
    expect(analysesOnLoad({ id: "100011", analysed: 0 }, { readOnly: true, queued: () => false })).toBe(false);
  });

  it("does not queue a track twice when Auto Analysis already has it", () => {
    expect(analysesOnLoad({ id: "100011", analysed: 0 }, { readOnly: false, queued: (id) => id === "100011" })).toBe(false);
  });
});
