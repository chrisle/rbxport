import { describe, expect, it } from "vitest";
import {
  applyClick, clickSettles, contextPress, cursorOf, cursorTarget, emptySelection, extendSelection, inListOrder, modifierFor, pressModifier, pressSelects, selectAll,
  selectedTracks, type ExtendMove, type SelectionState,
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

describe("cursorTarget", () => {
  it("starts from the top visible row when nothing is highlighted", () => {
    expect(cursorTarget("moveDown", null, 7, 20, 400)).toBe(7);
    expect(cursorTarget("moveUp", null, 7, 20, 400)).toBe(7);
    expect(cursorTarget("moveDown", null, -1, 20, 400)).toBe(0);
  });

  it("steps from the highlighted row, a page at a time for Page Up and Down", () => {
    expect(cursorTarget("moveDown", 10, 0, 20, 400)).toBe(11);
    expect(cursorTarget("moveUp", 10, 0, 20, 400)).toBe(9);
    expect(cursorTarget("pageDown", 10, 0, 20, 400)).toBe(30);
    expect(cursorTarget("pageUp", 10, 0, 20, 400)).toBe(-10);
    expect(cursorTarget("toTop", 10, 0, 20, 400)).toBe(0);
    expect(cursorTarget("toBottom", 10, 0, 20, 400)).toBe(399);
  });
});

/**
 * Shift + cursor up/down against rekordbox 7.2.19's `browse::CustomListBox::
 * keyPressed` @0x100e7c180 (static, macOS arm64; the decoding is in the PR
 * for #514). Rows are named by their index, "r0" to "r9".
 */
describe("extendSelection", () => {
  const listed = Array.from({ length: 10 }, (_, i) => `r${i}`);
  const rows = (state: SelectionState) => listed.flatMap((id, i) => (state.ids.has(id) ? [i] : []));
  const press = (state: SelectionState, ...moves: ExtendMove[]) => {
    let s = state;
    for (const move of moves) s = extendSelection(s, move, listed)?.selection ?? s;
    return s;
  };
  const click = (index: number) => applyClick(emptySelection, { id: `r${index}`, index }, "none");
  const toggle = (state: SelectionState, index: number) => applyClick(state, { id: `r${index}`, index }, "toggle");

  it("grows a row at a time from the clicked row and moves the cursor with it", () => {
    const down = press(click(4), "extendDown", "extendDown");
    expect(rows(down)).toEqual([4, 5, 6]);
    expect(cursorOf(down)).toBe(6);
    const up = press(click(4), "extendUp", "extendUp");
    expect(rows(up)).toEqual([2, 3, 4]);
    expect(cursorOf(up)).toBe(2);
  });

  it("shrinks back when the direction reverses, and grows past the origin the other way", () => {
    let s = press(click(4), "extendDown", "extendDown", "extendUp");
    expect(rows(s)).toEqual([4, 5]);
    expect(cursorOf(s)).toBe(5);
    s = press(s, "extendUp");
    expect(rows(s)).toEqual([4]);
    s = press(s, "extendUp");
    expect(rows(s)).toEqual([3, 4]);
    expect(cursorOf(s)).toBe(3);
    // And from above: down from the top row shrinks from the top.
    s = press(s, "extendUp", "extendDown");
    expect(rows(s)).toEqual([3, 4]);
    expect(cursorOf(s)).toBe(3);
  });

  it("returns the new cursor row for the list to scroll into view", () => {
    expect(extendSelection(click(4), "extendDown", listed)?.cursor).toBe(5);
    expect(extendSelection(press(click(4), "extendDown"), "extendUp", listed)?.cursor).toBe(4);
  });

  it("stops at either end of the list", () => {
    const top = press(click(1), "extendUp", "extendUp", "extendUp");
    expect(rows(top)).toEqual([0, 1]);
    expect(cursorOf(top)).toBe(0);
    const bottom = press(click(8), "extendDown", "extendDown");
    expect(rows(bottom)).toEqual([8, 9]);
    expect(cursorOf(bottom)).toBe(9);
  });

  it("extends from the row ⌘-clicked last, keeping the rows apart from it", () => {
    const s = press(toggle(click(2), 6), "extendDown");
    expect(rows(s)).toEqual([2, 6, 7]);
    expect(cursorOf(s)).toBe(7);
    // Up from the ⌘-clicked row adds above it; the other row is untouched.
    expect(rows(press(toggle(click(2), 6), "extendUp"))).toEqual([2, 5, 6]);
  });

  it("shrinking toward a ⌘-clicked row fills the gap between them, as rekordbox does", () => {
    // 2 and 6 by ⌘-click, then down to 7, then back up: rekordbox drops 7 and
    // selects every row from the top of the selection to the new cursor.
    const s = press(toggle(click(2), 6), "extendDown", "extendUp");
    expect(rows(s)).toEqual([2, 3, 4, 5, 6]);
    expect(cursorOf(s)).toBe(6);
    // The mirror image: 7 and 3, up to 2, then back down.
    const m = press(toggle(click(7), 3), "extendUp", "extendDown");
    expect(rows(m)).toEqual([3, 4, 5, 6, 7]);
    expect(cursorOf(m)).toBe(3);
  });

  it("goes from the top selected row after the cursor row is ⌘-clicked off", () => {
    let s = toggle(toggle(click(2), 5), 6);
    s = toggle(s, 6);
    expect(cursorOf(s)).toBeNull();
    s = press(s, "extendDown");
    expect(rows(s)).toEqual([2, 3, 5]);
    expect(cursorOf(s)).toBe(3);
  });

  it("⌘-clicking off a row that is not the cursor leaves the cursor where it was", () => {
    const s = toggle(toggle(toggle(click(2), 5), 7), 5);
    expect(cursorOf(s)).toBe(7);
  });

  it("goes on from the row shift-clicked, and from the last row after Select All", () => {
    const shifted = applyClick(click(2), { id: "r5", index: 5 }, "range", ["r2", "r3", "r4", "r5"]);
    expect(shifted.anchorIndex).toBe(2);
    expect(rows(press(shifted, "extendDown"))).toEqual([2, 3, 4, 5, 6]);
    expect(rows(press(shifted, "extendUp"))).toEqual([2, 3, 4]);
    const all = selectAll(click(4), listed);
    expect(cursorOf(all)).toBe(9);
    expect(rows(press(all, "extendUp"))).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it("leaves the key to a plain cursor move when no row of the list is selected", () => {
    expect(extendSelection(emptySelection, "extendDown", listed)).toBeNull();
    expect(extendSelection({ ids: new Set(["gone"]), anchorIndex: 3 }, "extendDown", listed)).toBeNull();
  });

  it("keeps selected ids the list does not show", () => {
    const s = press({ ids: new Set(["gone", "r4"]), anchorIndex: 4 }, "extendDown");
    expect([...s.ids].sort()).toEqual(["gone", "r4", "r5"]);
  });
});
