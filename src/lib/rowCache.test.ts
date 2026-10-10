import { describe, expect, it } from "vitest";
import { RowCache } from "./rowCache";

const page = (n: number, size = 4) => Array.from({ length: size }, (_, i) => `r${n * size + i}`);

describe("RowCache", () => {
  it("returns rows by absolute index", () => {
    const c = new RowCache<string>(4, 10);
    c.setPage(0, "v1:1", page(0));
    c.setPage(2, "v1:1", page(2));
    expect(c.get(0, "v1:1")).toBe("r0");
    expect(c.get(3, "v1:1")).toBe("r3");
    expect(c.get(9, "v1:1")).toBe("r9");
    expect(c.get(5, "v1:1")).toBeUndefined();
  });

  it("treats a different view token as a miss, so a sort never shows stale rows", () => {
    const c = new RowCache<string>(4, 10);
    c.setPage(0, "v1:1", page(0));
    expect(c.get(1, "v1:1")).toBe("r1");
    expect(c.get(1, "v2:1")).toBeUndefined();
  });

  it("peeks at a page from the view before, until its replacement lands", () => {
    const c = new RowCache<string>(4, 10);
    c.setPage(0, "v1:1", page(0));
    // An edit reopened the view: the page is no longer current, but it is
    // still the best picture of those rows there is.
    expect(c.get(1, "v2:2")).toBeUndefined();
    expect(c.peek(1)).toBe("r1");
    expect(c.missingPages(0, 4, "v2:2")).toEqual([0]);
    c.setPage(0, "v2:2", ["s0", "s1", "s2", "s3"]);
    expect(c.get(1, "v2:2")).toBe("s1");
    expect(c.peek(1)).toBe("s1");
    expect(c.peek(9)).toBeUndefined();
  });

  it("reports exactly the pages a range needs", () => {
    const c = new RowCache<string>(4, 10);
    c.setPage(1, "v1:1", page(1));
    expect(c.missingPages(0, 12, "v1:1")).toEqual([0, 2]);
    expect(c.missingPages(4, 8, "v1:1")).toEqual([]);
  });

  it("evicts least-recently-used pages past the cap", () => {
    const c = new RowCache<string>(4, 3);
    c.setPage(0, "v1:1", page(0));
    c.setPage(1, "v1:1", page(1));
    c.setPage(2, "v1:1", page(2));
    c.get(0, "v1:1");            // touch page 0 so page 1 becomes oldest
    c.setPage(3, "v1:1", page(3));
    expect(c.size).toBe(3);
    expect(c.hasPage(0, "v1:1")).toBe(true);
    expect(c.hasPage(1, "v1:1")).toBe(false);
    expect(c.hasPage(3, "v1:1")).toBe(true);
  });

  it("stays bounded under sustained scrolling", () => {
    const c = new RowCache<string>(64, 100);
    for (let p = 0; p < 1000; p++) c.setPage(p, "v1:1", page(p, 64));
    expect(c.size).toBe(100);
  });
});

describe("view identity", () => {
  it("does not serve pages from another view that shares a generation", () => {
    // The bug this guards: the backend returns the same `gen` for every view,
    // so keying on `gen` alone let a previous view's rows render after a sort.
    const c = new RowCache<string>(4, 10);
    c.setPage(0, "1:1", page(0));
    expect(c.get(0, "1:1")).toBe("r0");
    expect(c.get(0, "2:1")).toBeUndefined();
    expect(c.missingPages(0, 4, "2:1")).toEqual([0]);
  });
});

describe("patch", () => {
  it("rewrites the rows it picks in place and leaves the pages' identity alone", () => {
    const c = new RowCache<{ id: string; cues: string }>(2, 10);
    const rows = (ids: string[]) => ids.map((id) => ({ id, cues: "" }));
    c.setPage(0, "1:1", rows(["a", "b"]));
    c.setPage(1, "1:1", rows(["c", "d"]));
    expect(c.patch((r) => r.id === "c", (r) => ({ ...r, cues: "AB" }))).toBe(true);
    expect(c.get(2, "1:1")).toEqual({ id: "c", cues: "AB" });
    expect(c.get(0, "1:1")).toEqual({ id: "a", cues: "" });
    expect(c.hasPage(1, "1:1")).toBe(true);
    // A row the cache does not hold is nothing to do.
    expect(c.holds((r) => r.id === "d")).toBe(true);
    expect(c.holds((r) => r.id === "zz")).toBe(false);
    expect(c.patch((r) => r.id === "zz", (r) => ({ ...r, cues: "X" }))).toBe(false);
    expect(c.size).toBe(2);
  });
});
