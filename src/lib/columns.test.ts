import { describe, expect, it } from "vitest";

import {
  autoSizeAll,
  autoSizeColumn,
  AVAILABLE_COLUMNS,
  CATALOGUE,
  DEFAULT_VISIBLE,
  FIXED,
  FOLDER_VISIBLE,
  MENU_COLUMNS,
  REQUIRED_COLUMNS,
  defaultLayout,
  folderLayout,
  MAX_COLUMN_WIDTH,
  MIN_COLUMN_WIDTH,
  moveColumn,
  resizeColumn,
  resolve,
  sanitise,
  specOf,
  toggleColumn,
  widthOf,
  type ColumnKey,
} from "./columns";

describe("the catalogue", () => {
  it("matches the header menu that was captured", () => {
    // Thirty-nine columns, in the menu's order — counted off the capture. If
    // this number moves, the capture and the code have drifted apart. `#` is
    // not among them: it is a fixed column, and neither the capture nor
    // german.lang's column names has it.
    expect(MENU_COLUMNS).toHaveLength(39);
    expect(MENU_COLUMNS[0]?.label).toBe("Attribute");
    expect(MENU_COLUMNS.at(-1)?.label).toBe("Cloud");
    expect(MENU_COLUMNS.slice(3, 8).map((column) => column.label)).toEqual([
      "Track Title", "Release Date", "Artist", "Genre", "Comments",
    ]);
    expect(MENU_COLUMNS.some((c) => c.key === "trackNo")).toBe(false);
  });

  it("keeps the row number out of the menu and always in the table", () => {
    // Hiding the position in a playlist would make the order unreadable, and
    // rekordbox does not offer it either.
    expect(FIXED).toEqual(["trackNo"]);
    const hidden = toggleColumn(defaultLayout(), "trackNo");
    expect(resolve(hidden).map((c) => c.key)).toContain("trackNo");
    // And it leads, wherever a move tries to put it.
    expect(resolve(moveColumn(defaultLayout(), "trackNo", 5))[0]?.key).toBe("trackNo");
  });

  it("keeps Track Title checked and non-hideable while allowing it to move", () => {
    expect(REQUIRED_COLUMNS).toEqual(["trackNo", "title"]);
    const layout = defaultLayout();
    expect(toggleColumn(layout, "title")).toBe(layout);
    expect(moveColumn(layout, "title", 8).order.indexOf("title")).toBe(7);
  });

  it("has no duplicate keys or labels", () => {
    expect(new Set(CATALOGUE.map((c) => c.key)).size).toBe(CATALOGUE.length);
    expect(new Set(CATALOGUE.map((c) => c.label)).size).toBe(CATALOGUE.length);
  });

  it("shows the twelve the menu ticks by default", () => {
    expect(DEFAULT_VISIBLE).toHaveLength(12);
    for (const key of DEFAULT_VISIBLE) {
      expect(specOf(key), key).toBeDefined();
    }
  });

  it("gives every column a usable width", () => {
    for (const column of CATALOGUE) {
      expect(column.width, column.key).toBeGreaterThanOrEqual(MIN_COLUMN_WIDTH);
      expect(column.width, column.key).toBeLessThanOrEqual(MAX_COLUMN_WIDTH);
    }
  });

  it("allows every index-backed optional heading to sort the view", () => {
    expect(specOf("comment")?.sortable).toBe(true);
    expect(specOf("djPlayCount")?.sortable).toBe(true);
  });
});

describe("toggleColumn", () => {
  it("hides a visible column", () => {
    const after = toggleColumn(defaultLayout(), "bpm");
    expect(after.order).not.toContain("bpm");
  });

  it("enables columns backed by the requested detail fields", () => {
    const layout = defaultLayout();
    expect(AVAILABLE_COLUMNS).toContain("size");
    expect(toggleColumn(layout, "size").order).toContain("size");
    expect(moveColumn({ order: ["title", "size"], widths: {} }, "size", 1).order)
      .toEqual(["size", "title"]);
  });

  it("appends one that belongs after everything visible", () => {
    // Genre sits past Release Date in the menu, so the end is its place.
    expect(toggleColumn(defaultLayout(), "genre").order.at(-1)).toBe("genre");
  });

  it("round-trips", () => {
    const start = defaultLayout();
    const there = toggleColumn(start, "album");
    const back = toggleColumn(there, "album");
    expect(back.order).toEqual(start.order);
  });

  it("ignores a key that is not a column", () => {
    const start = defaultLayout();
    expect(toggleColumn(start, "nope" as ColumnKey)).toBe(start);
  });

  it("does not mutate the layout it was given", () => {
    const start = defaultLayout();
    const before = [...start.order];
    toggleColumn(start, "bpm");
    expect(start.order).toEqual(before);
  });
});

describe("moveColumn", () => {
  it("moves a column left and right", () => {
    // `to` counts the rendered headers, which lead with the fixed row number,
    // so a drop on header 4 is order position 3. Getting this offset wrong
    // stopped the drag reordering anything at all.
    const start = defaultLayout();
    const first = start.order[0] as ColumnKey;
    const moved = moveColumn(start, first, 3 + FIXED.length);
    expect(moved.order[3]).toBe(first);
    expect(moved.order).toHaveLength(start.order.length);
  });

  it("puts a column where the header it was dropped on sits", () => {
    // The end-to-end meaning: resolve() and moveColumn() must agree on what
    // position 4 refers to.
    const start = defaultLayout();
    const key = start.order[0] as ColumnKey;
    const moved = resolve(moveColumn(start, key, 4));
    expect(moved[4]?.key).toBe(key);
  });

  it("clamps a target past either end", () => {
    const start = defaultLayout();
    const key = start.order[2] as ColumnKey;
    expect(moveColumn(start, key, -10).order[0]).toBe(key);
    expect(moveColumn(start, key, 999).order.at(-1)).toBe(key);
  });

  it("keeps every column", () => {
    const start = defaultLayout();
    const moved = moveColumn(start, "bpm", 0);
    expect([...moved.order].sort()).toEqual([...start.order].sort());
  });

  it("ignores a column that is not visible", () => {
    const start = defaultLayout();
    expect(moveColumn(start, "genre", 0)).toBe(start);
  });
});

describe("resizeColumn", () => {
  it("sets a width", () => {
    expect(widthOf(resizeColumn(defaultLayout(), "bpm", 200), "bpm")).toBe(200);
  });

  it("clamps to something readable and something finite", () => {
    expect(widthOf(resizeColumn(defaultLayout(), "bpm", 1), "bpm")).toBe(MIN_COLUMN_WIDTH);
    expect(widthOf(resizeColumn(defaultLayout(), "bpm", 99_999), "bpm")).toBe(MAX_COLUMN_WIDTH);
    expect(widthOf(resizeColumn(defaultLayout(), "bpm", Number.NaN), "bpm")).toBe(MIN_COLUMN_WIDTH);
  });

  it("rounds to whole pixels", () => {
    expect(widthOf(resizeColumn(defaultLayout(), "bpm", 120.6), "bpm")).toBe(121);
  });

  it("leaves other columns alone", () => {
    const after = resizeColumn(defaultLayout(), "bpm", 200);
    expect(widthOf(after, "title")).toBe(specOf("title")?.width);
  });
});

describe("auto-size", () => {
  it("returns one column to its catalogue width", () => {
    const resized = resizeColumn(defaultLayout(), "bpm", 300);
    const after = autoSizeColumn(resized, "bpm");
    expect(widthOf(after, "bpm")).toBe(specOf("bpm")?.width);
  });

  it("returns every column", () => {
    let layout = defaultLayout();
    layout = resizeColumn(layout, "bpm", 300);
    layout = resizeColumn(layout, "title", 500);
    const after = autoSizeAll(layout);
    expect(after.widths).toEqual({});
    expect(widthOf(after, "title")).toBe(specOf("title")?.width);
  });

  it("is a no-op on a column that was never resized", () => {
    const start = defaultLayout();
    expect(autoSizeColumn(start, "bpm")).toBe(start);
  });
});

describe("resolve", () => {
  it("returns the visible columns in order, at their current widths", () => {
    const layout = resizeColumn(defaultLayout(), "bpm", 200);
    const columns = resolve(layout);
    // The fixed row number leads, then the layout's own order.
    expect(columns.map((c) => c.key)).toEqual([...FIXED, ...layout.order]);
    expect(columns.find((c) => c.key === "bpm")?.width).toBe(200);
  });

  it("restores Track Title even when a caller supplies a layout without it", () => {
    expect(resolve({ order: ["preview", "artwork", "bpm"], widths: {} }).map((column) => column.key))
      .toEqual(["trackNo", "preview", "artwork", "title", "bpm"]);
  });

  it("renders detail columns from a saved layout", () => {
    expect(resolve({ order: ["title", "size", "cloud", "artist"], widths: {} }).map((column) => column.key))
      .toEqual(["trackNo", "title", "size", "cloud", "artist"]);
  });
});

describe("sanitise", () => {
  it("accepts a good layout unchanged", () => {
    const layout = resizeColumn(defaultLayout(), "bpm", 200);
    expect(sanitise(layout)).toEqual(layout);
  });

  it("falls back when the stored value is nonsense", () => {
    // A broken layout must render a working table, not an empty one.
    for (const bad of [null, undefined, 7, "x", [], {}, { order: [] }]) {
      expect(sanitise(bad).order).toEqual(defaultLayout().order);
    }
  });

  it("drops columns that no longer exist", () => {
    const got = sanitise({ order: ["title", "wasRemoved", "bpm"], widths: {} });
    expect(got.order).toEqual(["title", "bpm"]);
  });

  it("drops a repeated column", () => {
    expect(sanitise({ order: ["title", "title", "bpm"], widths: {} }).order)
      .toEqual(["title", "bpm"]);
  });

  it("repairs a saved layout that hid Track Title", () => {
    expect(sanitise({ order: ["preview", "artwork", "bpm"], widths: {} }).order)
      .toEqual(["preview", "artwork", "title", "bpm"]);
  });

  it("preserves saved detail columns and their widths", () => {
    expect(sanitise({ order: ["title", "size", "artist", "cloud"], widths: { size: 100, artist: 250 } }))
      .toEqual({ order: ["title", "size", "artist", "cloud"], widths: { size: 100, artist: 250 } });
    expect(sanitise({ order: ["size", "cloud"], widths: {} }).order).toContain("size");
  });

  it("drops widths that are not usable numbers", () => {
    const got = sanitise({
      order: ["title"],
      widths: { title: 200, bpm: "wide", key: Number.NaN, gone: 100 },
    });
    expect(got.widths).toEqual({ title: 200 });
  });

  it("clamps a stored width that is out of range", () => {
    const got = sanitise({ order: ["title"], widths: { title: 99_999 } });
    expect(got.widths.title).toBe(MAX_COLUMN_WIDTH);
  });
});

describe("the Explorer's layout", () => {
  it("is the FolderTracks header, in the capture's order and widths", () => {
    const columns = resolve(folderLayout()).map((c) => [c.key, c.width]);
    expect(columns).toEqual([
      ["trackNo", 47],
      ["preview", 200], ["artwork", 80], ["title", 128], ["artist", 128], ["album", 128],
      ["genre", 128], ["bpm", 80], ["rating", 90], ["duration", 80], ["key", 128],
      ["fileName", 128],
    ]);
  });

  it("names only real columns", () => {
    for (const key of FOLDER_VISIBLE) expect(specOf(key)).toBeDefined();
  });

  it("is what a broken stored layout falls back to when asked", () => {
    expect(sanitise(null, folderLayout).order).toEqual([...FOLDER_VISIBLE]);
    expect(sanitise({ order: [] }, folderLayout).widths.preview).toBe(200);
    // And not otherwise: the collection's table keeps its own default.
    expect(sanitise(null).order).toEqual(defaultLayout().order);
  });
});
