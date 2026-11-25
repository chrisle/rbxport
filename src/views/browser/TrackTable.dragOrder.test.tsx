/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ViewSpec } from "@/ipc/types";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { defaultLayout, resolve } from "@/lib/columns";
import { detectPlatform } from "@/lib/shortcuts";
import { TrackTable, type TrackDrag } from "./TrackTable";

let host: HTMLDivElement;
let root: Root;
const noop = () => {};
const settle = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 15)); });
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); __setBackend(null); vi.restoreAllMocks(); });

it.each(["complete", "missing", "rejected", "cancelled"] as const)("binds virtualized selection order to its source view (%s ranks)", async ranks => {
  const backend = createMockBackend({ trackCount: 320, writable: true, latencyMs: 0 });
  const ids = backend.viewIdsInRange.bind(backend);
  let resolve!: (ids: string[]) => void;
  let reject!: (error: Error) => void;
  let hold = false;
  let captured: string[] = [];
  const read = vi.spyOn(backend, "viewIdsInRange").mockImplementation(async (...args) => {
    const listed = await ids(...args);
    if (!hold) return listed;
    captured = listed;
    return new Promise<string[]>((done, fail) => { resolve = done; reject = fail; });
  });
  __setBackend(backend);
  let payload: TrackDrag | null = null;
  act(() => root.render(<TrackTable
    spec={{ source: { kind: "collection" }, sort: "title", descending: true, query: "" }}
    onSortChange={noop} title="Collection" query="" onQueryChange={noop} columns={resolveColumns}
    onColumnMove={noop} onColumnResize={noop} onColumnToggle={noop} onColumnAutoSize={noop} onColumnAutoSizeAll={noop}
    onDragTracks={drag => { payload = drag; }} />));
  await settle(); await settle();
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true,
    ...(detectPlatform().mac ? { metaKey: true } : { ctrlKey: true }) })); });
  await settle();
  expect(host.querySelectorAll('[role="row"][aria-selected="true"]').length).toBeGreaterThan(0);
  hold = true;
  const row = host.querySelector('[role="row"][aria-selected="true"]');
  if (!row) throw new Error("selected fixture row missing");
  const event = new Event("dragstart", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: { setData: noop, setDragImage: noop } });
  act(() => { row.dispatchEvent(event); });
  await settle();
  const drag = payload as TrackDrag | null;
  expect(drag?.ids).toHaveLength(320);
  expect(drag?.orderedIds).toBeDefined();
  expect(read).toHaveBeenCalledTimes(2); // Select All plus this exact view's ranks.
  expect(captured).toHaveLength(320);
  if (ranks === "cancelled") {
    act(() => { row.dispatchEvent(new Event("dragend", { bubbles: true })); });
    reject(new Error("source view closed after cancellation"));
    await settle(); // No consumer awaits the ranks; rejection is still handled.
    expect(payload).toBeNull();
  } else if (ranks === "rejected") {
    reject(new Error("source view closed"));
    await expect(drag?.orderedIds).rejects.toThrow("source view closed");
  } else {
    resolve(ranks === "missing" ? captured.slice(1) : captured);
    if (ranks === "missing") await expect(drag?.orderedIds).rejects.toThrow("The track list changed");
    else expect(await drag?.orderedIds).toEqual(captured);
  }
});
const resolveColumns = resolve(defaultLayout());


it("keeps repeated content IDs in the order of the visibly selected rows", async () => {
  const backend = createMockBackend({ trackCount: 10, writable: true, latencyMs: 0 });
  await backend.edits.createPlaylist("Duplicate source", "root");
  const playlist = (await backend.playlistTree()).find(node => node.name === "Duplicate source")!;
  await backend.edits.addTracksToPlaylist(playlist.id, ["100000", "100002", "100000", "100001"], true);
  const spec = { source: { kind: "playlist" as const, id: playlist.id }, sort: "trackNo" as const, descending: false, query: "" };
  const view = await backend.openView(spec);
  const expected = await backend.viewIdsInRange(view.viewId, 0, 3);
  expect(expected).toHaveLength(4);
  expect(new Set(expected).size).toBe(3);
  __setBackend(backend);
  let payload: TrackDrag | null = null;
  act(() => root.render(<TrackTable spec={spec} onSortChange={noop} title="Duplicates" query="" onQueryChange={noop} columns={resolveColumns}
    onColumnMove={noop} onColumnResize={noop} onColumnToggle={noop} onColumnAutoSize={noop} onColumnAutoSizeAll={noop}
    onDragTracks={drag => { payload = drag; }} />));
  await settle(); await settle();
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true,
    ...(detectPlatform().mac ? { metaKey: true } : { ctrlKey: true }) })); });
  await settle();
  const row = host.querySelector('[role="row"][aria-selected="true"]');
  if (!row) throw new Error("selected fixture row missing");
  const event = new Event("dragstart", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: { setData: noop, setDragImage: noop } });
  act(() => { row.dispatchEvent(event); });
  const drag = payload as TrackDrag | null;
  expect(drag?.ids).toEqual(expected);
});


function mountSource(spec: ViewSpec, onDrag: (drag: TrackDrag | null) => void) {
  act(() => root.render(<TrackTable spec={spec} onSortChange={noop} title="Source" query={spec.query} onQueryChange={noop} columns={resolveColumns}
    onColumnMove={noop} onColumnResize={noop} onColumnToggle={noop} onColumnAutoSize={noop} onColumnAutoSizeAll={noop}
    onDragTracks={onDrag} />));
}
function selectAllRows() {
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true,
    ...(detectPlatform().mac ? { metaKey: true } : { ctrlKey: true }) })); });
}
function dragSelected() {
  const row = host.querySelector('[role="row"][aria-selected="true"]');
  if (!row) throw new Error("selected fixture row missing");
  const event = new Event("dragstart", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: { setData: noop, setDragImage: noop } });
  act(() => { row.dispatchEvent(event); });
}

it("projects a filtered descending source rather than collection or click order", async () => {
  const backend = createMockBackend({ trackCount: 4, writable: true, latencyMs: 0 });
  for (const [at, title] of ["Keep 0", "Discard", "Keep 2", "Keep 3"].entries()) {
    await backend.edits.setTrackField([String(100000 + at)], "title", title);
  }
  const spec: ViewSpec = { source: { kind: "collection" }, sort: "title", descending: true, query: "Keep" };
  const view = await backend.openView(spec);
  const expected = await backend.viewIdsInRange(view.viewId, 0, view.len - 1);
  expect(expected).toEqual(["100003", "100002", "100000"]);
  __setBackend(backend);
  let payload: TrackDrag | null = null;
  mountSource(spec, drag => { payload = drag; }); await settle(); await settle();
  selectAllRows(); await settle(); dragSelected();
  expect((payload as TrackDrag | null)?.ids).toEqual(expected);
});

it("refuses cached old row ranks while a same-source sort projection is replacing", async () => {
  const backend = createMockBackend({ trackCount: 4, writable: true, latencyMs: 0 });
  const open = backend.openView.bind(backend);
  let hold = false;
  let release!: () => void;
  backend.openView = async spec => {
    if (hold) await new Promise<void>(resolve => { release = resolve; });
    return open(spec);
  };
  __setBackend(backend);
  let payload: TrackDrag | null = null;
  const receive = (drag: TrackDrag | null) => { payload = drag; };
  const spec: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
  mountSource(spec, receive); await settle(); await settle();
  selectAllRows(); await settle();
  hold = true;
  mountSource({ ...spec, sort: "title", descending: true }, receive); await settle();
  expect(release).toBeDefined();
  dragSelected();
  await expect((payload as TrackDrag | null)?.orderedIds).rejects.toThrow("The track list changed");
  release(); await settle();
});
