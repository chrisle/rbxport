import { describe, expect, it } from "vitest";

import { EMPTY_FILTER, isNarrowing, pick, toSpecFilter, wholeBpm, type FilterState } from "./trackFilter";

function withBpm(values: number[], tolerancePct = 0, enabled = true): FilterState {
  return { ...EMPTY_FILTER, bpm: { enabled, values, tolerancePct } };
}

describe("pick", () => {
  it("a plain click picks one value", () => {
    expect(pick([128, 130], 126, false)).toEqual([126]);
  });

  it("the toggle modifier adds and removes", () => {
    expect(pick([128], 130, true)).toEqual([128, 130]);
    expect(pick([128, 130], 128, true)).toEqual([130]);
  });

  it("All clears the picks either way", () => {
    expect(pick([128, 130], null, false)).toEqual([]);
    expect(pick([128, 130], null, true)).toEqual([]);
  });
});

describe("toSpecFilter", () => {
  it("sends nothing while no column is ticked", () => {
    const picked: FilterState = { ...withBpm([128], 2, false), key: { enabled: false, values: ["Fm"] } };
    expect(isNarrowing(picked)).toBe(false);
    expect(toSpecFilter(picked, 12800)).toBeUndefined();
  });

  it("carries a ticked BPM column with the master player's BPM", () => {
    expect(toSpecFilter(withBpm([128], 2), 12600)).toEqual({
      bpm: { values: [128], tolerancePct: 2, masterBpmX100: 12600 },
    });
    // No deck loaded: the ± list has nothing to centre on.
    expect(toSpecFilter(withBpm([], 2), null)?.bpm?.masterBpmX100).toBeNull();
    expect(toSpecFilter(withBpm([], 2), 0)?.bpm?.masterBpmX100).toBeNull();
  });

  it("leaves a ticked set column at All off the wire", () => {
    const state: FilterState = {
      ...EMPTY_FILTER,
      key: { enabled: true, values: [] },
      rating: { enabled: true, values: [5] },
      color: { enabled: true, values: ["Red", "Aqua"] },
    };
    expect(toSpecFilter(state, null)).toEqual({ ratings: [5], colors: ["Red", "Aqua"] });
    // Every ticked column at All is no filter at all.
    expect(toSpecFilter({ ...EMPTY_FILTER, key: { enabled: true, values: [] } }, null)).toBeUndefined();
  });
});

describe("tag columns", () => {
  const tagged = (at: number, patch: Partial<FilterState["tags"][number]>): FilterState => ({
    ...EMPTY_FILTER,
    tags: EMPTY_FILTER.tags.map((tag, i) => (i === at ? { ...tag, ...patch } : tag)),
  });

  it("a ticked column with picks goes out by category position with its AND / OR switch", () => {
    const state = tagged(1, { enabled: true, values: ["Peak", "Warm-up"], any: true });
    expect(isNarrowing(state)).toBe(true);
    expect(toSpecFilter(state, null)).toEqual({
      tags: [{ category: 1, any: true, tags: ["Peak", "Warm-up"] }],
    });
    expect(toSpecFilter(tagged(0, { enabled: true, values: ["Synth"] }), null)).toEqual({
      tags: [{ category: 0, any: false, tags: ["Synth"] }],
    });
  });

  it("carries several columns, each with its own switch", () => {
    const state: FilterState = {
      ...EMPTY_FILTER,
      tags: [
        { enabled: true, values: ["A", "B"], any: false },
        { enabled: false, values: ["X"], any: false },
        { enabled: true, values: ["C"], any: true },
        { enabled: true, values: [], any: true },
      ],
    };
    expect(toSpecFilter(state, null)).toEqual({
      tags: [
        { category: 0, any: false, tags: ["A", "B"] },
        { category: 2, any: true, tags: ["C"] },
      ],
    });
  });

  it("a category index past the default columns still goes out", () => {
    const tags = [...EMPTY_FILTER.tags];
    while (tags.length < 6) tags.push({ enabled: false, values: [], any: false });
    tags[5] = { enabled: true, values: ["X", "Y"], any: true };
    const state: FilterState = { ...EMPTY_FILTER, tags };
    expect(isNarrowing(state)).toBe(true);
    expect(toSpecFilter(state, null)).toEqual({ tags: [{ category: 5, any: true, tags: ["X", "Y"] }] });
  });

  it("an unticked column, or a ticked one at All, sends nothing", () => {
    expect(toSpecFilter(tagged(0, { enabled: false, values: ["Synth"] }), null)).toBeUndefined();
    expect(toSpecFilter(tagged(0, { enabled: true, values: [] }), null)).toBeUndefined();
  });
});

describe("wholeBpm", () => {
  it("rounds to the nearest whole number", () => {
    expect(wholeBpm(12798)).toBe(128);
    expect(wholeBpm(12850)).toBe(129);
    expect(wholeBpm(0)).toBe(0);
  });
});
