// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMockBackend } from "@/ipc/backend-mock";
import type { Backend } from "@/ipc/types";
import type { NativeFileDrop } from "@/ipc/client";
import { DEFAULT_SESSION } from "@/lib/session";
import { App } from "./App";

const held = vi.hoisted(() => ({
  backend: null as Backend | null,
  nativeDrop: null as ((drop: NativeFileDrop) => void) | null,
  paths: vi.fn<(files: File[]) => Promise<string[]>>(),
  mainDrop: undefined as ((files: File[]) => void) | undefined,
  subDrop: undefined as ((files: File[]) => void) | undefined,
  readOnly: true,
}));
vi.mock("@/ipc/client", async importOriginal => {
  const original = await importOriginal<typeof import("@/ipc/client")>();
  const { createMockBackend } = await import("@/ipc/backend-mock");
  held.backend = createMockBackend({ trackCount: 1 });
  return { ...original, getBackend: () => Promise.resolve(held.backend), droppedFilePaths: held.paths,
    subscribeNativeFileDrops: (listener: (drop: NativeFileDrop) => void) => {
      held.nativeDrop = listener;
      return () => { if (held.nativeDrop === listener) held.nativeDrop = null; };
    } };
});
vi.mock("@/views/player/Player", () => ({ Player: () => null }));
vi.mock("@/views/browser/TrackTable", () => ({
  TrackTable: (props: { subBrowser?: boolean; onDropFiles?: (files: File[]) => void; readOnly?: boolean }) => {
    if (props.subBrowser) held.subDrop = props.onDropFiles;
    else { held.mainDrop = props.onDropFiles; held.readOnly = props.readOnly ?? false; }
    return null;
  },
}));
vi.mock("@/views/statusbar/StatusBar", () => ({
  StatusBar: ({ error }: { error: string | null }) => <span role="alert">{error}</span>,
}));
let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  localStorage.setItem("rbl.preferences", JSON.stringify({ advanced: { protectLibrary: false }, analysis: { auto: false } }));
  window.history.replaceState(null, "", "/?dropFixture=1");
  held.paths.mockReset().mockResolvedValue(["/tmp/generated-ready.mp3"]);
  held.mainDrop = held.subDrop = undefined;
  held.nativeDrop = null;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  localStorage.clear();
  window.history.replaceState(null, "", "/");
  Reflect.deleteProperty(document, "elementFromPoint");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
const settle = () => act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
async function mountLibrary(kind: "allTracks" | "playlist") {
  const backend = createMockBackend({ trackCount: 1, writable: true });
  held.backend = backend;
  await backend.edits.createPlaylist("Cached import target", "root");
  const nodes = await backend.playlistTree();
  const selected = nodes.find(node => kind === "allTracks" ? node.kind === kind : node.name === "Cached import target")!;
  localStorage.setItem("rbl.session", JSON.stringify({ ...DEFAULT_SESSION, selectedNodeId: selected.id, tree: nodes, subOpen: true }));
  let ready = false;
  const listeners = new Set<() => void>();
  backend.onLibraryReady = listener => { listeners.add(listener); return () => { listeners.delete(listener); }; };
  const originalTree = backend.playlistTreeSnapshot.bind(backend);
  const originalSummary = backend.librarySummary.bind(backend);
  backend.playlistTreeSnapshot = () => ready ? originalTree() : Promise.reject(new Error("The library has not finished loading yet."));
  backend.librarySummary = () => ready ? originalSummary() : Promise.reject(new Error("The library has not finished loading yet."));
  backend.libraryProblem = () => Promise.resolve(null);
  const actualSlice = backend.importPathsSlice.bind(backend);
  const imports = vi.spyOn(backend, "importPathsSlice").mockImplementation(paths => ready ? actualSlice(paths)
    : Promise.reject(new Error("The library has not finished loading yet.")));
  const target = () => [...host.querySelectorAll<HTMLElement>(`nav[data-browser-side="main"] [role="treeitem"][data-kind="${kind}"]`)]
    .find(node => node.textContent?.includes(selected.name))!;
  Object.defineProperty(document, "elementFromPoint", { configurable: true, value: () => target() });
  await act(async () => { root.render(<App />); await new Promise(resolve => setTimeout(resolve, 50)); });
  await vi.waitFor(async () => { await settle(); expect(host.querySelector('nav[data-browser-side="sub"]')).not.toBeNull(); });
  const subNode = [...host.querySelectorAll<HTMLElement>('nav[data-browser-side="sub"] [role="treeitem"]')]
    .find(node => kind === "allTracks" ? node.dataset.kind === kind : node.textContent?.includes("Cached import target"))!;
  await act(async () => {
    subNode.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await Promise.resolve();
  });
  const announceReady = async () => {
    ready = true;
    await act(async () => { listeners.forEach(listener => listener()); await new Promise(resolve => setTimeout(resolve, 50)); });
  };
  const drop = async () => act(async () => {
    const event = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "dataTransfer", { value: { types: ["Files"], files: [new File([], "generated-ready.mp3")] } });
    target().dispatchEvent(event);
    await new Promise(resolve => setTimeout(resolve, 30));
  });
  return { backend, imports, target, announceReady, drop, actualSlice, selected };
}

it.each(["allTracks", "playlist"] as const)("requires fresh library ownership for cached %s file-drop targets", async kind => {
  const { backend, imports, target, announceReady, drop, selected } = await mountLibrary(kind);
  expect(target()).not.toBeNull();
  expect(target().getAttribute("data-file-drop-playlist")).toBeNull();
  expect(held.mainDrop).toBeUndefined();
  expect(held.subDrop).toBeUndefined();
  expect(held.readOnly).toBe(false); // Device/read-only policy is independent.
  await drop();
  expect(held.paths).not.toHaveBeenCalled();
  act(() => held.nativeDrop?.({ paths: ["/tmp/generated-ready.mp3"], x: 0, y: 0 }));
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("The library has not finished loading yet.");
  expect(imports).not.toHaveBeenCalled();
  await announceReady();
  expect(target().getAttribute("data-file-drop-playlist")).not.toBeNull();
  expect(held.mainDrop).toBeTypeOf("function");
  expect(held.subDrop).toBeTypeOf("function");
  await drop();
  expect(imports).toHaveBeenCalledTimes(1);
  const report = await imports.mock.results[0]!.value as Awaited<ReturnType<Backend["importPathsSlice"]>>;
  expect(report.imported).toBe(1);
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("");
  const view = await backend.openView({ source: kind === "playlist" ? { kind: "playlist", id: selected.id } : { kind: "collection" },
    sort: "trackNo", descending: false, query: "generated-ready" });
  const rows = await backend.fetchRows(view.viewId, 0, 10, []);
  expect(rows.map(row => row.id)).toEqual([report.tracks[0]!.id]);
});

it("preserves admitted direct and queued imports through a readiness refresh", async () => {
  const { imports, announceReady, drop, actualSlice } = await mountLibrary("allTracks");
  await announceReady();
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  imports.mockImplementationOnce(async paths => { await pending; return actualSlice(paths); });
  await drop();
  expect(imports).toHaveBeenCalledTimes(1);
  act(() => held.nativeDrop?.({ paths: ["/tmp/second-ready.mp3"], x: 0, y: 0 }));
  await settle();
  expect(imports).toHaveBeenCalledTimes(1);
  await announceReady();
  await act(async () => { release(); await new Promise(resolve => setTimeout(resolve, 50)); });
  expect(imports).toHaveBeenCalledTimes(2);
  expect(imports.mock.calls.map(call => call[0])).toEqual([["/tmp/generated-ready.mp3"], ["/tmp/second-ready.mp3"]]);
  expect(held.paths).toHaveBeenCalledTimes(1);
});
