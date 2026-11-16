/**
 * @vitest-environment jsdom
 *
 * The Color column names a colour as the library does (`djmdColor.Commnt`),
 * paints it by the colour it is, and a right-click on it offers "Rename
 * <name>…" for each of the eight, which opens where they are renamed.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { Backend, ViewSpec } from "@/ipc/types";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { PreferencesProvider } from "@/store/usePreferences";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { resolve } from "@/lib/columns";
import { TrackTable } from "./TrackTable";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let backend: Backend;

const COLLECTION: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
const columns = resolve({ order: ["title", "color"], widths: {} });
const noop = () => {};

const settle = async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
};

function render(onEditColor: (id: string, color: string) => void, onRenameColors: () => void) {
  act(() => {
    root.render(
      <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: noop, reset: noop }}>
        <TrackTable
          spec={COLLECTION}
          onSortChange={noop}
          title="Collection"
          query=""
          onQueryChange={noop}
          columns={columns}
          onColumnMove={noop}
          onColumnResize={noop}
          onColumnToggle={noop}
          onColumnAutoSize={noop}
          onColumnAutoSizeAll={noop}
          libraryGeneration={0}
          onEditColor={onEditColor}
          onRenameColors={onRenameColors}
        />
      </PreferencesProvider>,
    );
  });
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  backend = createMockBackend({ trackCount: 20, latencyMs: 0, writable: true });
  __setBackend(backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

const colorCells = () => [...host.querySelectorAll<HTMLElement>('[role="row"][aria-selected] [data-col="color"]')];

it("names a colour as the library does, and its right-click offers to rename the eight", async () => {
  await backend.edits.setColorName(6, "Peak");
  const onEditColor = vi.fn();
  const onRenameColors = vi.fn();
  render(onEditColor, onRenameColors);
  await settle();
  await settle();
  const peak = colorCells().find((cell) => cell.textContent === "Peak");
  if (!peak) throw new Error(`no Peak row in ${colorCells().map((c) => c.textContent).join("|")}`);
  expect(peak.querySelector("[data-color]")?.getAttribute("data-color")).toBe("Aqua");

  act(() => { peak.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); });
  const select = host.querySelector<HTMLSelectElement>('[data-col="color"] select');
  if (!select) throw new Error("the Color list did not open");
  const labels = [...select.options].filter((o) => o.value !== "").map((o) => o.textContent);
  expect(labels).toEqual(["No Color", "Pink", "Red", "Orange", "Yellow", "Green", "Peak", "Blue", "Purple"]);
  act(() => { select.dispatchEvent(new Event("blur")); });

  const cell = colorCells().find((c) => c.textContent === "Peak")!;
  act(() => { cell.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 })); });
  const menu = document.querySelector('[role="menu"][aria-label="Color"]');
  if (!menu) throw new Error("no colour menu");
  expect(document.querySelector('[role="menu"][aria-label="Track"]')).toBeNull();
  const items = [...menu.querySelectorAll<HTMLElement>('[role="menuitem"]')];
  expect(items.map((i) => i.textContent)).toEqual([
    "Rename Pink…", "Rename Red…", "Rename Orange…", "Rename Yellow…",
    "Rename Green…", "Rename Peak…", "Rename Blue…", "Rename Purple…",
  ]);
  act(() => { items[5]!.click(); });
  expect(onRenameColors).toHaveBeenCalledTimes(1);
  expect(onEditColor).not.toHaveBeenCalled();
});
