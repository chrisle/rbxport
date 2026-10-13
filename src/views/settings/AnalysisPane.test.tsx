/**
 * @vitest-environment jsdom
 *
 * Key Analysis > "Write the value to the ID3 tag" on the Analysis pane
 * (#204): rekordbox's `writeKeyInTag`, off by default, under its own
 * heading below Track Analysis.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { DEFAULT_PREFERENCES, type Preferences } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { AnalysisPane } from "./AnalysisPane";

let host: HTMLDivElement;
let root: Root;
let update: ReturnType<typeof vi.fn>;

function mount(preferences: Preferences) {
  act(() => {
    root.render(
      <PreferencesProvider value={{ preferences, update, reset: () => {} }}>
        <AnalysisPane tab="track" />
      </PreferencesProvider>,
    );
  });
}

function writeSwitch(): HTMLInputElement {
  const section = host.querySelector<HTMLElement>('section[aria-label="Key Analysis"]');
  const input = section?.querySelector<HTMLInputElement>('input[role="switch"]');
  if (!input) throw new Error("no Key Analysis switch");
  return input;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  update = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

it("offers Write the value to the ID3 tag under Key Analysis, off by default", () => {
  mount(DEFAULT_PREFERENCES);
  const input = writeSwitch();
  expect(input.closest("label")?.textContent).toContain("Write the value to the ID3 tag");
  expect(input.checked).toBe(false);

  act(() => input.click());
  expect(update).toHaveBeenCalledWith("analysis", { writeKeyTag: true });
});

it("shows the saved choice", () => {
  mount({ ...DEFAULT_PREFERENCES, analysis: { ...DEFAULT_PREFERENCES.analysis, writeKeyTag: true } });
  expect(writeSwitch().checked).toBe(true);
});
