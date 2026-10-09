/**
 * @vitest-environment jsdom
 *
 * The intelligent playlist editor's own rules: which operators a property
 * offers, what changing one does to the fields beside it, and what reaches
 * `onSave`.
 *
 * The backend's half is covered in `src-tauri/tests/commands.rs`, which
 * makes a rule and edits it. None of that says what the form does when a
 * text property's operator is carried over to a number — which is where a
 * rule the index cannot answer would come from — nor does anything else
 * exercise the relative-date conditions, whose unit is a field the other
 * operators do not have.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { SmartRule } from "@/ipc/types";
import { SmartPlaylistEditor, emptyCondition, type SmartPlaylistEditorProps } from "./SmartPlaylistEditor";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let onSave: ReturnType<typeof vi.fn>;
let onCancel: ReturnType<typeof vi.fn>;

function open(overrides: Partial<SmartPlaylistEditorProps> = {}) {
  act(() => {
    root.render(
      <SmartPlaylistEditor
        title="Create New Intelligent Playlist"
        name="Untitled Intelligent List"
        rule={{ logic: "all", conditions: [] }}
        onSave={onSave}
        onCancel={onCancel}
        {...overrides}
      />,
    );
  });
}

const byLabel = (label: string) => host.querySelector<HTMLElement>(`[aria-label="${label}"]`);
const nameField = () => host.querySelector<HTMLInputElement>('[aria-label="List name"]')!;
const row = (n: number) => host.querySelector<HTMLElement>(`[aria-label="Condition ${n}"]`);
const sel = (n: number, label: string) =>
  row(n)?.querySelector<HTMLSelectElement>(`[aria-label="${label}"]`) ?? null;
const inp = (n: number, label: string) =>
  row(n)?.querySelector<HTMLInputElement>(`[aria-label="${label}"]`) ?? null;
const rowBtn = (n: number, label: string) =>
  row(n)?.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`) ?? null;
const button = (text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent?.trim() === text)!;

/**
 * Changes a field the way a user does.
 *
 * React keeps its own copy of the last value it wrote on the element, so a
 * plain assignment looks like its own write and the change never reaches
 * `onChange`; going through the prototype's setter is what a keystroke does.
 */
function set(el: HTMLInputElement | HTMLSelectElement, value: string) {
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value")?.set?.call(el, value);
  act(() => {
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}

/**
 * The rule the form would save, by pressing OK and reading what it handed
 * over.
 *
 * Conditions are read back through this rather than off the popups. A select
 * holding a value none of its options carry falls back to showing the first
 * one, so an operator wrongly carried over to a property that cannot take it
 * renders as that property's first operator — exactly what a correct fallback
 * would render — and the difference is only visible in what gets saved.
 */
function saved(): SmartRule {
  act(() => button("OK").click());
  return onSave.mock.calls[onSave.mock.calls.length - 1]?.[1] as SmartRule;
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  onSave = vi.fn();
  onCancel = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("what the dialog opens on", () => {
  it("starts a new rule at Artist = (empty), as rekordbox's new row is", () => {
    open();
    expect(emptyCondition()).toEqual({ property: "artist", operator: "1", left: "", right: "", unit: "" });
    expect(sel(1, "Property")?.value).toBe("artist");
    expect(sel(1, "Operator")?.value).toBe("1");
    expect(inp(1, "Value")?.value).toBe("");
    expect(row(2)).toBeNull();
  });

  it("opens on the rule it was given, field for field", () => {
    open({
      title: "Edit the Intelligent Playlist",
      name: "Peak time",
      rule: {
        logic: "any",
        conditions: [
          { property: "bpm", operator: "5", left: "126", right: "132", unit: "" },
          { property: "genre", operator: "8", left: "house", right: "", unit: "" },
        ],
      },
    });
    expect(nameField().value).toBe("Peak time");
    expect((byLabel("Match") as HTMLSelectElement).value).toBe("any");
    expect(sel(1, "Property")?.value).toBe("bpm");
    expect(sel(1, "Operator")?.value).toBe("5");
    expect(inp(1, "Value")?.value).toBe("126");
    expect(inp(1, "Upper value")?.value).toBe("132");
    expect(sel(2, "Operator")?.value).toBe("8");
    expect(host.querySelector('[role="dialog"]')?.getAttribute("aria-label")).toBe(
      "Edit the Intelligent Playlist",
    );
  });

  it("edits a copy, so the rule it was handed is left alone", () => {
    // The tree holds the rule it fetched; an edit that reached through to it
    // would leave the panel showing changes that were never saved.
    const rule = {
      logic: "all" as const,
      conditions: [{ property: "artist", operator: "1", left: "x", right: "", unit: "" }],
    };
    open({ rule });
    set(inp(1, "Value")!, "y");
    expect(rule.conditions[0]?.left).toBe("x");
  });

  it("offers every property rekordbox's dialog lists, under the names SmartList holds", () => {
    open();
    const options = [...(sel(1, "Property")?.options ?? [])];
    expect(options).toHaveLength(23);
    expect(options[0]?.textContent).toBe("Album");
    expect(options.filter((o) => o.disabled)).toEqual([]);
    const byText = Object.fromEntries(options.map((o) => [o.textContent, o.value]));
    expect(byText).toMatchObject({
      "Album artist": "albumArtist",
      Composer: "producer",
      "Mix name": "mixName",
      "My Tag": "myTag",
      "Original artist": "originalArtist",
      Remixer: "remixedBy",
    });
  });

  it("shows a property it does not know as it is, not as the first in the list", () => {
    // Issue #84: an unreadable property arrived as "" and a select showed
    // the first option carrying that value, "Album artist".
    open({ rule: { logic: "all", conditions: [{ property: "", operator: "8", left: "12", right: "", unit: "" }] } });
    const property = sel(1, "Property")!;
    expect(property.value).toBe("");
    expect(property.selectedOptions[0]?.textContent).toBe("—");
    expect(property.selectedOptions[0]?.disabled).toBe(true);
  });
});

describe("a My Tag condition", () => {
  const myTags = [
    { name: "Subgenre", tags: [{ id: "101", name: "Deep" }, { id: "102", name: "Tech" }] },
    { name: "Situation", tags: [{ id: "201", name: "Peak" }] },
  ];

  it("opens on the tag a saved rule names, with the two operators rekordbox answers", () => {
    open({
      myTags,
      rule: { logic: "all", conditions: [{ property: "myTag", operator: "8", left: "102", right: "", unit: "" }] },
    });
    expect(sel(1, "Property")?.value).toBe("myTag");
    expect([...(sel(1, "Operator")?.options ?? [])].map((o) => o.value)).toEqual(["8", "9"]);
    const value = sel(1, "Value")!;
    expect(value.tagName).toBe("SELECT");
    expect(value.value).toBe("102");
    expect(value.selectedOptions[0]?.textContent).toBe("Tech");
    expect([...value.querySelectorAll("optgroup")].map((g) => g.label)).toEqual(["Subgenre", "Situation"]);
  });

  it("keeps a tag id the library no longer has rather than picking another", () => {
    open({
      myTags,
      rule: { logic: "all", conditions: [{ property: "myTag", operator: "9", left: "999", right: "", unit: "" }] },
    });
    expect(sel(1, "Value")?.value).toBe("999");
    expect(saved().conditions[0]).toEqual({ property: "myTag", operator: "9", left: "999", right: "", unit: "" });
  });

  it("picks a tag by id and saves it", () => {
    open({ myTags, name: "Peak" });
    set(inp(1, "Value")!, "Daft Punk");
    set(sel(1, "Property")!, "myTag");
    // Typed text names no tag, so it is not carried over.
    expect(sel(1, "Value")?.value).toBe("");
    expect(button("OK").disabled).toBe(true);
    set(sel(1, "Value")!, "201");
    expect(saved().conditions[0]).toEqual({ property: "myTag", operator: "8", left: "201", right: "", unit: "" });
  });

  it("does not carry a tag id into a text property", () => {
    open({
      myTags,
      rule: { logic: "all", conditions: [{ property: "myTag", operator: "8", left: "101", right: "", unit: "" }] },
    });
    set(sel(1, "Property")!, "genre");
    expect(inp(1, "Value")?.value).toBe("");
    expect(sel(1, "Operator")?.value).toBe("8");
  });
});

describe("changing a property", () => {
  it("keeps an operator the new property can take", () => {
    open({ name: "Crate" });
    set(sel(1, "Operator")!, "8");
    // Genre is text as Artist is, so "contains" still means something.
    set(sel(1, "Property")!, "genre");
    set(inp(1, "Value")!, "house");
    expect(sel(1, "Operator")?.value).toBe("8");
    expect(saved().conditions[0]).toEqual({
      property: "genre", operator: "8", left: "house", right: "", unit: "",
    });
  });

  it("drops one it cannot, falling back to the first the property offers", () => {
    open({ name: "Crate" });
    set(sel(1, "Operator")!, "8");
    // Nothing numeric contains anything, so "contains" cannot come along.
    set(sel(1, "Property")!, "bpm");
    expect([...(sel(1, "Operator")?.options ?? [])].map((o) => o.value)).toEqual([
      "1", "2", "3", "4", "5",
    ]);
    // Read back through what is saved, not through the popup. A select holding
    // a value none of its options carry falls back to showing the first one,
    // so the popup reads "=" either way and would hide an operator that had
    // been carried over.
    set(inp(1, "Value")!, "128");
    expect(saved().conditions[0]).toEqual({
      property: "bpm", operator: "1", left: "128", right: "", unit: "",
    });
  });

  it("offers each kind of property its own operators", () => {
    open();
    const operators = () => [...(sel(1, "Operator")?.options ?? [])].map((o) => o.value);
    expect(operators()).toEqual(["1", "2", "8", "9", "10", "11"]);
    set(sel(1, "Property")!, "rating");
    expect(operators()).toEqual(["1", "2", "3", "4", "5"]);
    set(sel(1, "Property")!, "stockDate");
    expect(operators()).toEqual(["1", "2", "3", "4", "6", "7", "5"]);
  });

  it("brings the unit along when the operator it kept needs one", () => {
    open({ name: "Fresh" });
    set(sel(1, "Property")!, "stockDate");
    set(sel(1, "Operator")!, "6");
    expect(sel(1, "Unit")?.value).toBe("day");
    // Date Created is a date too, so "is in the last" survives the move and
    // has to keep a unit with it — a relative condition with no unit is one
    // the index cannot answer.
    set(sel(1, "Property")!, "dateCreated");
    set(inp(1, "Value")!, "14");
    expect(sel(1, "Operator")?.value).toBe("6");
    expect(sel(1, "Unit")?.value).toBe("day");
    expect(saved().conditions[0]).toEqual({
      property: "dateCreated", operator: "6", left: "14", right: "", unit: "day",
    });
  });
});

describe("changing an operator", () => {
  it("shows a unit for the relative dates and takes it away again", () => {
    open();
    set(sel(1, "Property")!, "stockDate");
    expect(inp(1, "Value")?.type).toBe("date");
    expect(sel(1, "Unit")).toBeNull();

    set(sel(1, "Operator")!, "6");
    expect(sel(1, "Unit")?.value).toBe("day");
    // A relative date is counted, not dated.
    expect(inp(1, "Value")?.type).toBe("number");
    expect([...(sel(1, "Unit")?.options ?? [])].map((o) => o.value)).toEqual([
      "day", "week", "month", "year",
    ]);

    set(sel(1, "Operator")!, "7");
    expect(sel(1, "Unit")?.value).toBe("day");

    set(sel(1, "Operator")!, "1");
    expect(sel(1, "Unit")).toBeNull();
    expect(inp(1, "Value")?.type).toBe("date");
  });

  it("keeps a chosen unit while the operator still takes one", () => {
    open();
    set(sel(1, "Property")!, "stockDate");
    set(sel(1, "Operator")!, "6");
    set(sel(1, "Unit")!, "month");
    set(sel(1, "Operator")!, "7");
    expect(sel(1, "Unit")?.value).toBe("month");
  });

  it("shows the upper value only for the range, and forgets it on the way out", () => {
    open();
    set(sel(1, "Property")!, "bpm");
    expect(inp(1, "Upper value")).toBeNull();

    set(sel(1, "Operator")!, "5");
    set(inp(1, "Value")!, "126");
    set(inp(1, "Upper value")!, "132");
    expect(inp(1, "Upper value")?.value).toBe("132");

    set(sel(1, "Operator")!, "3");
    expect(inp(1, "Upper value")).toBeNull();
    // Coming back gives a fresh range rather than the old one's far end.
    set(sel(1, "Operator")!, "5");
    expect(inp(1, "Upper value")?.value).toBe("");
  });
});

describe("the condition rows", () => {
  it("keeps the last one, and only asks all-or-any once there are two", () => {
    open();
    expect(byLabel("Match")).toBeNull();
    expect(host.textContent).toContain("Match the following condition:");
    expect(rowBtn(1, "Remove condition")?.disabled).toBe(true);

    act(() => byLabel("Add condition")?.click());
    expect(byLabel("Match")).not.toBeNull();
    expect(host.textContent).toContain("following conditions:");
    expect(row(2)).not.toBeNull();
    expect(rowBtn(1, "Remove condition")?.disabled).toBe(false);
  });

  it("removes the row its ⊖ belongs to, not the last one", () => {
    open({
      rule: {
        logic: "all",
        conditions: [
          { property: "artist", operator: "1", left: "one", right: "", unit: "" },
          { property: "genre", operator: "1", left: "two", right: "", unit: "" },
          { property: "label", operator: "1", left: "three", right: "", unit: "" },
        ],
      },
    });
    act(() => rowBtn(2, "Remove condition")?.click());
    expect(inp(1, "Value")?.value).toBe("one");
    expect(inp(2, "Value")?.value).toBe("three");
    expect(row(3)).toBeNull();
  });
});

describe("saving", () => {
  it("will not save without a name, or with any condition left blank", () => {
    open();
    // A fresh condition has no value, so there is nothing to save yet.
    expect(button("OK").disabled).toBe(true);
    set(inp(1, "Value")!, "Daft Punk");
    expect(button("OK").disabled).toBe(false);

    set(nameField(), "   ");
    expect(button("OK").disabled).toBe(true);
    set(nameField(), "");
    expect(button("OK").disabled).toBe(true);
    set(nameField(), "Crate");
    expect(button("OK").disabled).toBe(false);

    // A second row with nothing in it blocks the save the first one allowed.
    act(() => byLabel("Add condition")?.click());
    expect(button("OK").disabled).toBe(true);
    set(inp(2, "Value")!, "Justice");
    expect(button("OK").disabled).toBe(false);
  });

  it("trims the name and the values", () => {
    open({ name: "  Crate  " });
    set(inp(1, "Value")!, "  Daft Punk  ");
    act(() => button("OK").click());
    expect(onSave).toHaveBeenCalledWith("Crate", {
      logic: "all",
      conditions: [{ property: "artist", operator: "1", left: "Daft Punk", right: "", unit: "" }],
    });
  });

  it("saves a relative date whole: property, operator, count and unit", () => {
    open({ name: "Fresh" });
    set(sel(1, "Property")!, "stockDate");
    set(sel(1, "Operator")!, "7");
    set(sel(1, "Unit")!, "month");
    set(inp(1, "Value")!, "6");
    act(() => button("OK").click());
    expect(onSave).toHaveBeenCalledWith("Fresh", {
      logic: "all",
      conditions: [{ property: "stockDate", operator: "7", left: "6", right: "", unit: "month" }],
    });
  });

  it("saves a range with both ends, and the logic the Match popup is on", () => {
    open({ name: "Peak" });
    set(sel(1, "Property")!, "bpm");
    set(sel(1, "Operator")!, "5");
    set(inp(1, "Value")!, "126");
    set(inp(1, "Upper value")!, "132");
    act(() => byLabel("Add condition")?.click());
    set(sel(2, "Property")!, "genre");
    set(sel(2, "Operator")!, "8");
    set(inp(2, "Value")!, "house");
    set(byLabel("Match") as HTMLSelectElement, "any");
    act(() => button("OK").click());
    expect(onSave).toHaveBeenCalledWith("Peak", {
      logic: "any",
      conditions: [
        { property: "bpm", operator: "5", left: "126", right: "132", unit: "" },
        { property: "genre", operator: "8", left: "house", right: "", unit: "" },
      ],
    });
  });
});

describe("focus", () => {
  it("lands on the name, selected, when the dialog opens", () => {
    open();
    expect(document.activeElement).toBe(nameField());
    expect(nameField().selectionStart).toBe(0);
    expect(nameField().selectionEnd).toBe(nameField().value.length);
  });

  it("stays on a dropdown when the parent renders again with a new onCancel", () => {
    // #131, #215: App passes `onCancel` as a fresh arrow and renders on its
    // own (the window regaining focus refreshes devices and LINK status).
    // Each new `onCancel` used to focus the name again, which shut the
    // Property dropdown the moment it opened.
    open();
    const property = sel(1, "Property")!;
    act(() => property.focus());
    expect(document.activeElement).toBe(property);

    const later = vi.fn();
    open({ onCancel: later });
    expect(document.activeElement).toBe(property);

    // Escape still reaches the newest handler, and only that one.
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(later).toHaveBeenCalledTimes(1);
    expect(onCancel).not.toHaveBeenCalled();
  });
});

describe("closing without saving", () => {
  it("closes on Escape, on Cancel, and on a click outside — but not on one inside", () => {
    open();
    const dialog = host.querySelector<HTMLElement>('[role="dialog"]')!;
    act(() => {
      dialog.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    });
    expect(onCancel).not.toHaveBeenCalled();

    const backdrop = host.querySelector<HTMLElement>('[role="presentation"]')!;
    act(() => {
      backdrop.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    });
    expect(onCancel).toHaveBeenCalledTimes(1);

    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(onCancel).toHaveBeenCalledTimes(2);

    act(() => button("Cancel").click());
    expect(onCancel).toHaveBeenCalledTimes(3);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("stops listening for Escape once it is gone", () => {
    open();
    act(() => root.unmount());
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(onCancel).not.toHaveBeenCalled();
    // The afterEach unmount has to find something to unmount.
    root = createRoot(host);
  });
});
