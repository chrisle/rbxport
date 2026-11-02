// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMockBackend } from "@/ipc/backend-mock";
import type { Backend } from "@/ipc/types";
import { PREFERENCES_KEY } from "@/lib/preferences";
import { refusal } from "@/lib/menu";
import { App } from "./App";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

const held = vi.hoisted(() => ({
  backend: null as Backend | null,
  analyseOne: null as ((id: string, title: string) => void) | null,
  analyseSelection: null as (() => void) | null,
  select: null as ((tracks: { id: string; title: string }[]) => void) | null,
}));

vi.mock("@/ipc/client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/ipc/client")>();
  const { createMockBackend } = await import("@/ipc/backend-mock");
  held.backend = createMockBackend({ trackCount: 1 });
  return { ...original, getBackend: () => Promise.resolve(held.backend) };
});

// The real menus reach these callbacks; App owns the refusal under test.
vi.mock("@/views/player/Player", () => ({
  Player: (props: { onAnalyse: (id: string, title: string) => void }) => {
    held.analyseOne = props.onAnalyse;
    return null;
  },
}));
vi.mock("@/views/browser/TrackTable", () => ({
  TrackTable: (props: { onAnalyse: () => void; onSelectedTracks: (tracks: { id: string; title: string }[]) => void }) => {
    held.analyseSelection = props.onAnalyse;
    held.select = props.onSelectedTracks;
    return null;
  },
}));
vi.mock("@/views/statusbar/StatusBar", () => ({
  StatusBar: ({ error }: { error: string | null }) => <span role="alert">{error}</span>,
}));
vi.mock("@/views/analysis/AnalysisDialog", () => ({
  AnalysisDialog: () => <div data-testid="analysis-dialog" />,
}));

let host: HTMLDivElement;
let root: Root;

async function mount(options: { writable: boolean; protectLibrary: boolean }) {
  localStorage.setItem(PREFERENCES_KEY, JSON.stringify({ advanced: { protectLibrary: options.protectLibrary } }));
  held.backend = createMockBackend({ trackCount: 1, writable: options.writable });
  await act(async () => { root.render(<App />); await Promise.resolve(); });
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
}

const alertText = () => host.querySelector('[role="alert"]')?.textContent ?? "";

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  held.analyseOne = null;
  held.analyseSelection = null;
  held.select = null;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("names Library Protection and Preferences when analysis is refused", async () => {
  await mount({ writable: true, protectLibrary: true });
  act(() => held.analyseOne?.("1", "One"));
  expect(alertText()).toBe(refusal(true));
  expect(alertText()).toContain("Library Protection");
  expect(alertText()).toContain("Preferences");

  act(() => held.select?.([{ id: "1", title: "One" }]));
  act(() => held.analyseSelection?.());
  expect(alertText()).toBe(refusal(true));
  expect(host.querySelector('[data-testid="analysis-dialog"]')).toBeNull();
});

it("says rekordbox is running when protection is off and the library is read-only", async () => {
  await mount({ writable: false, protectLibrary: false });
  act(() => held.analyseOne?.("1", "One"));
  expect(alertText()).toBe(refusal(false));
  expect(alertText()).toContain("rekordbox is running");
});

it("opens the settings dialog on a writable, unprotected library", async () => {
  await mount({ writable: true, protectLibrary: false });
  act(() => held.analyseOne?.("1", "One"));
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
  expect(alertText()).toBe("");
  expect(host.querySelector('[data-testid="analysis-dialog"]')).not.toBeNull();
});
