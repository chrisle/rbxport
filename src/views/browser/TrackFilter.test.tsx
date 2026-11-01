/** @vitest-environment jsdom */
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { FilterValues } from "@/ipc/types";
import { EMPTY_FILTER, type FilterState } from "@/lib/trackFilter";
import { TrackFilter } from "./TrackFilter";

const VALUES: FilterValues = {
  bpms: [],
  keys: [],
  tags: [
    { name: "Situation", tags: ["Peak", "Warm-up"] },
    { name: "Components", tags: ["Synth", "Vocal"] },
  ],
};

let host: HTMLDivElement;
let root: Root;
let latest: FilterState;

function Harness({ onState }: { onState: (state: FilterState) => void }) {
  const [state, setState] = useState<FilterState>(EMPTY_FILTER);
  return (
    <TrackFilter
      state={state}
      values={VALUES}
      masterBpmX100={null}
      onChange={(next) => {
        onState(next);
        setState(next);
      }}
    />
  );
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  latest = EMPTY_FILTER;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<Harness onState={(s) => { latest = s; }} />));
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

const click = (el: Element, init: MouseEventInit = {}) =>
  act(() => { el.dispatchEvent(new MouseEvent("click", { bubbles: true, ...init })); });

const columns = () => [...host.querySelectorAll<HTMLElement>('[data-testid="filter-tag-column"]')];
const chip = (column: number, name: string) =>
  [...(columns()[column]?.querySelectorAll<HTMLElement>('[role="option"]') ?? [])].find((c) => c.textContent === name);

it("draws the tag chips selectable, not greyed out", () => {
  const list = columns()[0]?.querySelector('[role="listbox"]');
  expect(list?.getAttribute("aria-disabled")).toBeNull();
  expect(list?.hasAttribute("data-inert")).toBe(false);
  expect(chip(0, "Peak")?.getAttribute("aria-selected")).toBe("false");
});

it("a click picks one chip, a Cmd-click adds another, and the same again removes it", () => {
  click(chip(0, "Peak")!);
  expect(latest.tags[0]?.values).toEqual(["Peak"]);
  expect(chip(0, "Peak")?.getAttribute("aria-selected")).toBe("true");

  click(chip(0, "Warm-up")!, { metaKey: true });
  expect(latest.tags[0]?.values).toEqual(["Peak", "Warm-up"]);

  click(chip(0, "Peak")!, { ctrlKey: true });
  expect(latest.tags[0]?.values).toEqual(["Warm-up"]);

  // A plain click replaces the picks.
  click(chip(0, "Peak")!);
  expect(latest.tags[0]?.values).toEqual(["Peak"]);
});

it("picks are kept per category", () => {
  click(chip(0, "Peak")!);
  click(chip(1, "Vocal")!);
  expect(latest.tags[0]?.values).toEqual(["Peak"]);
  expect(latest.tags[1]?.values).toEqual(["Vocal"]);
});

it("the tick box turns a column on and the AND / OR switch sets how its picks combine", () => {
  const column = columns()[1]!;
  const tick = column.querySelector('[role="checkbox"]')!;
  click(tick);
  expect(latest.tags[1]?.enabled).toBe(true);
  expect(latest.tags[1]?.any).toBe(false);

  const [and, or] = [...column.querySelectorAll("button")].filter((b) => /^(AND|OR)$/.test(b.textContent ?? ""));
  click(or!);
  expect(latest.tags[1]?.any).toBe(true);
  expect(or!.hasAttribute("data-on")).toBe(true);
  click(and!);
  expect(latest.tags[1]?.any).toBe(false);
  // Ticking again keeps the picks, as the other columns do.
  click(chip(1, "Synth")!);
  click(tick);
  expect(latest.tags[1]).toMatchObject({ enabled: false, values: ["Synth"] });
});

it("a pad column with no category behind it has nothing to pick", () => {
  const pad = columns()[3]!;
  expect(pad.querySelector('[role="option"]')).toBeNull();
  expect(pad.querySelector('[role="listbox"]')?.getAttribute("aria-disabled")).toBe("true");
});

it("RST clears the tag picks", () => {
  click(chip(0, "Peak")!);
  click(host.querySelector('button[aria-label="Reset"]')!);
  expect(latest.tags.every((t) => t.values.length === 0 && !t.enabled)).toBe(true);
});
