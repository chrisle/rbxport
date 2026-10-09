import { describe, expect, it, vi } from "vitest";

import type { UnanalysedTracks } from "@/ipc/types";
import { autoAnalysisOffer, takeRemainingPages, UNANALYSED_PAGE } from "./autoAnalysis";

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
