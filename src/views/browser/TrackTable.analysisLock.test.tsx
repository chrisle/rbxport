/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";

import { menuBarEnabled, menuBarState } from "@/lib/menuBar";
import { runCommand } from "@/lib/commands";
import type { Backend, ViewSpec } from "@/ipc/types";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { PreferencesProvider } from "@/store/usePreferences";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { defaultLayout, resolve } from "@/lib/columns";
import { TrackTable } from "./TrackTable";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let backend: Backend;

const COLLECTION: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
// The Attribute column, where the [!] is drawn, beside the defaults.
const columns = resolve({ ...defaultLayout(), order: ["attr", ...defaultLayout().order] });
const noop = () => {};

const settle = async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
};

function render(subBrowser = false) {
  act(() => {
    root.render(
      <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: noop, reset: noop }}>
        <TrackTable
          spec={COLLECTION}
          subBrowser={subBrowser}
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
          onAnalysisLock={(ids,on) => { for (const id of ids) void backend.edits.gridLock(id,on); }}
        />
      </PreferencesProvider>,
    );
  });
}

const rows = () => [...host.querySelectorAll<HTMLElement>('[role="row"][aria-selected]')];

const press = (row: Element | undefined, init: MouseEventInit = {}) => {
  act(() => {
    row?.firstElementChild?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0, ...init }));
  });
};

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  backend = createMockBackend({ trackCount: 20, latencyMs: 0 });
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


const off = (side: "main" | "sub") => menuBarEnabled({ ...menuBarState(), focus: { pane: "list", side }, has: () => true })["analysis-lock-off"];
it.each(["main", "sub"] as const)("%s normal selection and shared lock commands update native Off", async (side) => {
  render(side === "sub"); await settle(); await settle(); press(rows()[1]); await settle(); await settle();
  expect(menuBarState().lists[side]?.selected).toBe(1);
  expect(off(side)).toBe(false);
  act(() => { expect(runCommand("analysisLockOn", { scope: side })).toBe(true); });
  await settle(); await settle(); expect(off(side)).toBe(true);
  act(() => { expect(runCommand("analysisLockOff", { scope: side })).toBe(true); });
  await settle(); await settle(); expect(off(side)).toBe(false);
});
