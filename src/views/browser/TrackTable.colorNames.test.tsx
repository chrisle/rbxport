/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { PreferencesProvider } from "@/store/usePreferences";
import { DEFAULT_PREFERENCES, savePreferences } from "@/lib/preferences";
import { defaultLayout, resolve } from "@/lib/columns";
import type { Backend, RowDto, ViewSpec } from "@/ipc/types";
import { Localization } from "@/i18n";
import de from "../../../public/locales/de.json";
import { TrackTable } from "./TrackTable";
let host: HTMLDivElement;
let root: Root;
let backend: Backend;
let row: RowDto;
let report: ReturnType<typeof vi.fn>;
let reportRow: ReturnType<typeof vi.fn>;
const noop = () => {};
const columns = resolve({ ...defaultLayout(), order: ["color", ...defaultLayout().order] });
const settle = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
function render(source: ViewSpec["source"], onRenameColors?: () => void) {
  act(() => root.render(<Localization><PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: noop, reset: noop }}>
    <TrackTable spec={{ source, sort: "trackNo", descending: false, query: "" }}
      title="Collection" query="" onQueryChange={noop} onSortChange={noop} libraryGeneration={0}
      columns={columns} onColumnMove={noop} onColumnResize={noop} onColumnToggle={noop}
      onColumnAutoSize={noop} onColumnAutoSizeAll={noop} onEditColor={noop} {...onRenameColors ? { onRenameColors } : {}} onSelectedTracks={report} onSelectedRow={reportRow} />
  </PreferencesProvider></Localization>));
}
beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  report = vi.fn();
  reportRow = vi.fn();
  backend = createMockBackend({ trackCount: 2, latencyMs: 0 });
  const view = await backend.openView({ source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" });
  row = { ...(await backend.fetchRows(view.viewId, 0, 1))[0]!, extra: { color: 1 } };
  let viewId = 0;
  backend.openView = () => Promise.resolve({ viewId: ++viewId, len: 1, gen: 1 });
  backend.fetchRows = () => Promise.resolve([row]);
  backend.colorNames = owner => Promise.resolve([owner ? owner.format === "oneLibrary" ? "One label" : "PDB label" : "Play", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"]);
  __setBackend(backend);
  savePreferences({ ...DEFAULT_PREFERENCES, view: { ...DEFAULT_PREFERENCES.view, locale: "de" } });
  vi.stubGlobal("fetch", vi.fn(() => Promise.resolve({ ok: true, json: () => Promise.resolve(de) })));
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); __setBackend(null); localStorage.clear(); vi.unstubAllGlobals(); });
it("shows literal master labels in German and switches to each selected device database", async () => {
  render({ kind: "collection" }); await settle(); await settle();
  const cell = () => host.querySelector('[role="row"] [data-col="color"]')!;
  expect(cell().textContent).toBe("Play");
  expect(cell().querySelector('[data-color="Pink"]')).not.toBeNull();
  render({ kind: "device", path: "/stick", format: "oneLibrary", playlist: "0" }); await settle(); await settle();
  expect(cell().textContent).toBe("One label");
  render({ kind: "device", path: "/stick", format: "deviceLibrary", playlist: "0" }); await settle(); await settle();
  expect(cell().textContent).toBe("PDB label");
  render({ kind: "collection" }); await settle(); await settle();
  expect(cell().textContent).toBe("Play");
});

it("does not replace another pane's selection when an empty browser first mounts", async () => {
  render({ kind: "collection" }); await settle(); await settle();
  expect(report).not.toHaveBeenCalled();
  expect(reportRow).not.toHaveBeenCalled();
});

it("refreshes a selected row when another database returns the same file ID", async () => {
  const pdb = { kind: "device", path: "/stick", format: "deviceLibrary", playlist: "0" } as const;
  row = { ...row, id: "file:/fixture.wav", deviceSource: { path: "/stick", format: "deviceLibrary", trackId: 1 } };
  render(pdb); await settle(); await settle();
  act(() => { host.querySelector('[role="row"] [data-col="title"]')!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })); });
  await settle();
  expect(reportRow).toHaveBeenLastCalledWith(row);
  reportRow.mockClear();
  row = { ...row, extra: { color: 2 }, deviceSource: { path: "/stick", format: "oneLibrary", trackId: 1 } };
  render({ ...pdb, format: "oneLibrary" }); await settle(); await settle();
  expect(reportRow).toHaveBeenLastCalledWith(row);
  expect(report).toHaveBeenLastCalledWith([expect.objectContaining({ id: row.id })], { ...pdb, format: "oneLibrary" });
});

it("offers to rename the master library's eight colours, and not a device's", async () => {
  const onRenameColors = vi.fn();
  const colorMenu = () => document.querySelector('[role="menu"][aria-label="Color"]');
  const rightClick = () => act(() => {
    host.querySelector('[role="row"] [data-col="color"]')!.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
  });
  render({ kind: "collection" }, onRenameColors); await settle(); await settle();
  rightClick();
  const items = [...colorMenu()!.querySelectorAll<HTMLElement>('[role="menuitem"]')];
  expect(items).toHaveLength(8);
  expect(items[0]!.textContent).toContain("Play");
  act(() => { items[0]!.click(); });
  expect(onRenameColors).toHaveBeenCalledTimes(1);
  render({ kind: "device", path: "/stick", format: "oneLibrary", playlist: "0" }, onRenameColors); await settle(); await settle();
  rightClick();
  expect(colorMenu()).toBeNull();
});
