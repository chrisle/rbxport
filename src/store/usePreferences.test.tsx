/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DEFAULT_PREFERENCES, loadPreferences, PREFERENCES_KEY, type Preferences } from "@/lib/preferences";
import { usePreferencesStore, type PreferencesStore } from "./usePreferences";

let host: HTMLDivElement;
let root: Root;
let store: PreferencesStore;

function Harness() {
  store = usePreferencesStore();
  return null;
}

beforeEach(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root.render(<Harness />); await Promise.resolve(); });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

/** The Preferences window's write, with no `storage` event reaching this one. */
function otherWindowTurnsOnFirstBeatCue(): void {
  const stored: Preferences = { ...loadPreferences(), analysis: { ...loadPreferences().analysis, firstBeatCue: true } };
  localStorage.setItem(PREFERENCES_KEY, JSON.stringify(stored));
}

it("keeps another window's choice when this window changes a different one (#343)", async () => {
  expect(store.preferences.analysis.firstBeatCue).toBe(false);
  otherWindowTurnsOnFirstBeatCue();
  await act(async () => { store.update("view", { tempoSlider: !DEFAULT_PREFERENCES.view.tempoSlider }); await Promise.resolve(); });
  const reloaded = loadPreferences();
  expect(reloaded.analysis.firstBeatCue).toBe(true);
  expect(reloaded.view.tempoSlider).toBe(!DEFAULT_PREFERENCES.view.tempoSlider);
  expect(store.preferences.analysis.firstBeatCue).toBe(true);
});

it("keeps another window's choice when this window resets a different pane", async () => {
  otherWindowTurnsOnFirstBeatCue();
  await act(async () => { store.reset("view"); await Promise.resolve(); });
  expect(loadPreferences().analysis.firstBeatCue).toBe(true);
});

it("survives a restart: the switch reads back as it was left", async () => {
  await act(async () => { store.update("analysis", { firstBeatCue: true }); await Promise.resolve(); });
  act(() => root.unmount());
  root = createRoot(host);
  await act(async () => { root.render(<Harness />); await Promise.resolve(); });
  expect(store.preferences.analysis.firstBeatCue).toBe(true);
});
