/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { PreferencesProvider } from "@/store/usePreferences";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { defaultLayout, resolve } from "@/lib/columns";
import { TrackTable } from "./TrackTable";
let host: HTMLDivElement;
let root: Root;
let missing = false;
const noop = () => {};
const columns = resolve({ ...defaultLayout(), order: ["attr", ...defaultLayout().order] });
const render = (loaded: readonly (string | null)[]) => act(() => root.render(
  <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: noop, reset: noop }}>
    <TrackTable spec={{ source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" }}
      onSortChange={noop} title="Collection" query="" onQueryChange={noop} columns={columns}
      onColumnMove={noop} onColumnResize={noop} onColumnToggle={noop} onColumnAutoSize={noop}
      onColumnAutoSizeAll={noop} libraryGeneration={0} loadedTracks={loaded.map(id => id === null ? null : [id, id === "100003" ? 2 : 1] as const)} />
  </PreferencesProvider>));
const settle = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  missing = false;
  const mock = createMockBackend({ trackCount: 20, latencyMs: 0 });
  await mock.deckLoad("a", "100001", 1);
  await mock.deckLoad("b", "100003", 2);
  __setBackend({ ...mock, fetchRows: async (...args) => (await mock.fetchRows(...args))
    .map(row => missing && row.id === "100001" ? { ...row, missing: true } : row) });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); __setBackend(null); });
it("marks loaded tracks from either deck without changing selection or track numbers", async () => {
  render(["100001", "100003"]); await settle(); await settle();
  const marked = () => [...host.querySelectorAll('[data-col="attr"] [data-loaded]')];
  expect(marked()).toHaveLength(2);
  expect(marked().map(marker => marker.closest('[role="row"]')?.querySelector('[data-col="trackNo"]')?.textContent)).toEqual(["2", "4"]);
  expect(host.querySelectorAll('[aria-selected="true"]')).toHaveLength(0);
  render(["100003", null]); await settle();
  expect(marked()).toHaveLength(1);
  render([null, null]); await settle();
  expect(marked()).toHaveLength(0);
});


it("keeps missing-file precedence and other cue markers", async () => {
  missing = true; render(["100001", null]); await settle(); await settle();
  expect(host.querySelectorAll("[data-loaded]")).toHaveLength(0);
  expect(host.querySelectorAll('[aria-label="File is Missing"]')).toHaveLength(1);
  expect([...host.querySelectorAll('[data-col="attr"]')].some(cell => cell.textContent?.includes("CUE"))).toBe(true);
});
