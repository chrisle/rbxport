/**
 * @vitest-environment jsdom
 *
 * Key Analysis > "Write the value to the ID3 tag" on the Analysis pane
 * (#204): rekordbox's `writeKeyInTag`, off by default, under its own
 * tab alongside Track Analysis.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { DEFAULT_PREFERENCES, type Preferences } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { BUILTIN_ANALYSIS_STAGES } from "@/lib/analysisStages";
import { AnalysisPane, type AnalysisTab } from "./AnalysisPane";

let host: HTMLDivElement;
let root: Root;
let update: ReturnType<typeof vi.fn>;

function mount(preferences: Preferences, tab: AnalysisTab = "key") {
  act(() => {
    root.render(
      <PreferencesProvider value={{ preferences, update, reset: () => {} }}>
        <AnalysisPane tab={tab} />
      </PreferencesProvider>,
    );
  });
}

function keySwitch(label: string): HTMLInputElement {
  const section = host.querySelector<HTMLElement>('section[aria-label="Key Analysis"]');
  const row = [...(section?.querySelectorAll("label") ?? [])].find(element => element.textContent === label);
  const input = row?.querySelector<HTMLInputElement>('input[role="switch"]');
  if (!input) throw new Error(`no Key Analysis switch "${label}"`);
  return input;
}

const writeSwitch = () => keySwitch("Write the value to the ID3 tag");

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
  __setBackend(null);
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

it("offers Analyze key under Key Analysis, on by default, with the Mixed In Key reason", () => {
  mount(DEFAULT_PREFERENCES);
  const input = keySwitch("Analyze key");
  expect(input.checked).toBe(true);
  expect(host.textContent).toContain("Mixed In Key");

  act(() => input.click());
  expect(update).toHaveBeenCalledWith("analysis", { detectKey: false });
});

it("shows Analyze key off when saved off", () => {
  mount({ ...DEFAULT_PREFERENCES, analysis: { ...DEFAULT_PREFERENCES.analysis, detectKey: false } });
  expect(keySwitch("Analyze key").checked).toBe(false);
});

it("moves stages from Advanced and keeps the key switches on their own tab", async () => {
  __setBackend({ ...createMockBackend(), analysisStages: () => Promise.resolve(BUILTIN_ANALYSIS_STAGES) });
  mount(DEFAULT_PREFERENCES, "track");
  expect(host.querySelector('section[aria-label="Analysis order"]')).toBeNull();
  mount(DEFAULT_PREFERENCES, "advanced");
  await act(async () => {});
  const section = host.querySelector('section[aria-label="Analysis order"]');
  expect([...section!.querySelectorAll("li")].map(stage => stage.textContent)).toEqual(["Waveform", "BPM / Grid", "KEY"]);
  expect(host.querySelector('section[aria-label="Key Analysis"]')).toBeNull();
  const key = [...section!.querySelectorAll("li")].find(stage => stage.textContent === "KEY");
  act(() => {
    key?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
  });
  expect(update).toHaveBeenCalledWith("analysis", { stageOrder: ["waveform", "key", "grid"] });

  mount({ ...DEFAULT_PREFERENCES, analysis: { ...DEFAULT_PREFERENCES.analysis, stageOrder: ["waveform", "key", "grid"] } }, "advanced");
  expect([...section!.querySelectorAll("li")].map(stage => stage.textContent)).toEqual(["Waveform", "KEY", "BPM / Grid"]);
  mount(DEFAULT_PREFERENCES, "key");
  expect(host.querySelector('section[aria-label="Analysis order"]')).toBeNull();
  expect(writeSwitch().checked).toBe(false);
});
