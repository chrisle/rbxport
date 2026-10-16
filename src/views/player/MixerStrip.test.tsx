/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { runCommand } from "@/lib/commands";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { MixerStrip } from "./MixerStrip";

declare global { var IS_REACT_ACT_ENVIRONMENT: boolean; }

const setChannelKill = vi.fn(() => Promise.resolve());
vi.mock("@/ipc/client", () => ({
  getBackend: () => Promise.resolve({
    setChannelKill, setChannelTrim: () => Promise.resolve(), setCrossfade: () => Promise.resolve(),
  }),
}));

let host: HTMLDivElement;
let root: Root;

function mount(overrides: Record<string, { key: string; shiftKey?: boolean }>) {
  const preferences = { ...DEFAULT_PREFERENCES, keyboard: { ...DEFAULT_PREFERENCES.keyboard, overrides } };
  act(() => root.render(
    <PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
      <MixerStrip />
    </PreferencesProvider>,
  ));
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  setChannelKill.mockClear();
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const press = (init: KeyboardEventInit) =>
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init })); });

it("toggles the assigned deck's band kill from the keyboard", async () => {
  mount({ "a.eqKillLow": { key: "z" }, "b.eqKillHigh": { key: "u" } });
  press({ key: "z" });
  await act(async () => {});
  expect(setChannelKill).toHaveBeenCalledWith("a", "low", true);
  press({ key: "z" });
  await act(async () => {});
  expect(setChannelKill).toHaveBeenLastCalledWith("a", "low", false);
  press({ key: "u" });
  await act(async () => {});
  expect(setChannelKill).toHaveBeenLastCalledWith("b", "high", true);
});

it("does nothing while the kill keys are unbound", async () => {
  mount({});
  press({ key: "z" });
  await act(async () => {});
  expect(setChannelKill).not.toHaveBeenCalled();
});

it("kills a band the same way from its command as from its key (#431)", async () => {
  mount({ "b.eqKillMid": { key: "y" } });
  press({ key: "y" });
  await act(async () => {});
  expect(setChannelKill).toHaveBeenLastCalledWith("b", "mid", true);
  act(() => {
    expect(runCommand("eqKillMid", { scope: "b" })).toBe(true);
  });
  await act(async () => {});
  expect(setChannelKill).toHaveBeenLastCalledWith("b", "mid", false);
  // A held key's repeat is the same one press.
  act(() => {
    runCommand("eqKillMid", { scope: "b", repeat: true });
  });
  await act(async () => {});
  expect(setChannelKill).toHaveBeenCalledTimes(2);
});
