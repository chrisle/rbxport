import { describe, expect, it } from "vitest";
import {
  applyClick, clickSettles, contextPress, emptySelection, inListOrder, modifierFor, pressModifier, pressSelects, selectAll,
  selectedTracks,
} from "./selection";

describe("selection", () => {
  it("reads the modifier from the event", () => {
    expect(modifierFor({ shiftKey: false, metaKey: false, ctrlKey: false })).toBe("none");
    expect(modifierFor({ shiftKey: false, metaKey: true, ctrlKey: false })).toBe("toggle");
    expect(modifierFor({ shiftKey: false, metaKey: false, ctrlKey: true })).toBe("toggle");
    expect(modifierFor({ shiftKey: true, metaKey: true, ctrlKey: false })).toBe("range");
  });

  it("a plain press on a selected row waits for the release; every other press applies", () => {
    const plain = { button: 0, shiftKey: false, metaKey: false, ctrlKey: false };
    // Selected and plain: this may be a drag of the selection.
    expect(pressSelects(plain, true)).toBe(false);
    expect(clickSettles(plain, true)).toBe(true);
    // Not selected: the press selects, and the click that follows changes nothing.
    expect(pressSelects(plain, false)).toBe(true);
    expect(clickSettles(plain, false)).toBe(false);
    // Modified presses are never drags and apply at once.
    expect(pressSelects({ ...plain, shiftKey: true }, true)).toBe(true);
    expect(pressSelects({ ...plain, metaKey: true }, true)).toBe(true);
    expect(clickSettles({ ...plain, metaKey: true }, true)).toBe(false);
  });

  it("the right button keeps a selection it lands in, and selects outside it", () => {
    const right = { button: 2, shiftKey: false, metaKey: false, ctrlKey: false };
    expect(pressSelects(right, true)).toBe(false);
    expect(clickSettles(right, true)).toBe(false);
    expect(pressSelects(right, false)).toBe(true);
  });

  it("a macOS Control-click is the menu's press: it keeps the selection it lands in (#135)", () => {
    // What WebKit and Chromium send on macOS before the contextmenu event.
    const controlClick = { button: 0, shiftKey: false, metaKey: false, ctrlKey: true };
    expect(contextPress(controlClick, true)).toBe(true);
    expect(pressSelects(controlClick, true, true)).toBe(false);
    expect(clickSettles(controlClick, true, true)).toBe(false);
    // Outside the selection it selects that row alone, as a right-click does.
    expect(pressSelects(controlClick, false, true)).toBe(true);
    expect(pressModifier(controlClick, true)).toBe("none");
    // Elsewhere Control is the toggle, as before.
    expect(contextPress(controlClick, false)).toBe(false);
    expect(pressSelects(controlClick, true, false)).toBe(true);
    expect(pressModifier(controlClick, false)).toBe("toggle");
    // ⌘ still toggles on macOS.
    const command = { ...controlClick, ctrlKey: false, metaKey: true };
    expect(contextPress(command, true)).toBe(false);
    expect(pressModifier(command, true)).toBe("toggle");
  });

  it("a plain click replaces the selection and moves the anchor", () => {
    const s = applyClick(emptySelection, { id: "a", index: 3 }, "none");
    expect([...s.ids]).toEqual(["a"]);
    expect(s.anchorIndex).toBe(3);
  });

  it("toggle adds and removes", () => {
    let s = applyClick(emptySelection, { id: "a", index: 0 }, "none");
    s = applyClick(s, { id: "b", index: 1 }, "toggle");
    expect([...s.ids].sort()).toEqual(["a", "b"]);
    s = applyClick(s, { id: "a", index: 0 }, "toggle");
    expect([...s.ids]).toEqual(["b"]);
  });

  it("range uses the ids the backend resolved and keeps the anchor put", () => {
    let s = applyClick(emptySelection, { id: "a", index: 2 }, "none");
    s = applyClick(s, { id: "e", index: 6 }, "range", ["a", "b", "c", "d", "e"]);
    expect(s.ids.size).toBe(5);
    expect(s.anchorIndex).toBe(2);
    // A second shift-click grows from the same origin, not from the last click.
    s = applyClick(s, { id: "c", index: 4 }, "range", ["a", "b", "c"]);
    expect(s.anchorIndex).toBe(2);
    expect(s.ids.size).toBe(3);
  });

  it("range without an anchor degrades to a plain click", () => {
    const s = applyClick(emptySelection, { id: "z", index: 9 }, "range", undefined);
    expect([...s.ids]).toEqual(["z"]);
  });

  it("select all takes every id the backend resolved", () => {
    const s = selectAll(emptySelection, ["a", "b", "c"]);
    expect([...s.ids].sort()).toEqual(["a", "b", "c"]);
  });

  it("select all keeps an existing anchor and defaults to the top", () => {
    // Made from nothing: the anchor sits at the top so a following shift-click
    // has an origin.
    expect(selectAll(emptySelection, ["a", "b"]).anchorIndex).toBe(0);
    // Empty view: nothing to anchor on.
    expect(selectAll(emptySelection, []).anchorIndex).toBeNull();
    // An anchor already set is left where it was.
    const anchored = applyClick(emptySelection, { id: "b", index: 3 }, "none");
    expect(selectAll(anchored, ["a", "b", "c"]).anchorIndex).toBe(3);
  });

  it("reports every selected track even when most rows are not cached", () => {
    // 30,000 selected, titles known for only the ~6,400 the row cache holds.
    const ids = Array.from({ length: 30_000 }, (_, i) => String(100000 + i));
    const titles = new Map(ids.slice(0, 6400).map((id) => [id, `Title ${id}`]));
    const tracks = selectedTracks(selectAll(emptySelection, ids).ids, titles);
    expect(tracks).toHaveLength(30_000);
    expect(tracks[0]).toEqual({ id: "100000", title: "Title 100000" });
    expect(tracks[29_999]).toEqual({ id: "129999", title: "129999" });
  });

  it("orders a ⌘-click selection top to bottom, as rekordbox's selected array is", () => {
    // Clicked c, then a with ⌘: the set holds them in click order.
    let state = applyClick(emptySelection, { id: "c", index: 2 }, "none");
    state = applyClick(state, { id: "a", index: 0 }, "toggle");
    expect([...state.ids]).toEqual(["c", "a"]);
    expect(inListOrder(state.ids, ["a", "b", "c", "d"])).toEqual(["a", "c"]);
  });

  it("puts a selected id the list no longer shows last", () => {
    const ids = new Set(["gone", "d", "b"]);
    expect(inListOrder(ids, ["a", "b", "c", "d"])).toEqual(["b", "d", "gone"]);
    // A track listed twice (a playlist may hold one twice) is reported once.
    expect(inListOrder(new Set(["b"]), ["b", "a", "b"])).toEqual(["b"]);
  });
});
