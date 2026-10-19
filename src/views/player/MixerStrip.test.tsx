/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { runCommand } from "@/lib/commands";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { MixerStrip } from "./MixerStrip";

declare global { var IS_REACT_ACT_ENVIRONMENT: boolean; }

const setChannelKill = vi.fn((_deck: string, _band: string, _killed: boolean) => Promise.resolve());
const setChannelTrim = vi.fn((_deck: string, _trim: number) => Promise.resolve());
const setCrossfade = vi.fn((_position: number) => Promise.resolve());
vi.mock("@/ipc/client", () => ({
  getBackend: () => Promise.resolve({ setChannelKill, setChannelTrim, setCrossfade }),
}));

let host: HTMLDivElement;
let root: Root;

function render(overrides: Record<string, { key: string; shiftKey?: boolean }>, strip = true) {
  const preferences = { ...DEFAULT_PREFERENCES, keyboard: { ...DEFAULT_PREFERENCES.keyboard, overrides } };
  act(() => root.render(
    <PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
      {strip ? <MixerStrip /> : null}
    </PreferencesProvider>,
  ));
}

/** Mounts the strip and forgets the flat channel it sends on the way in. */
async function mount(overrides: Record<string, { key: string; shiftKey?: boolean }>) {
  render(overrides);
  await act(async () => {});
  for (const fn of [setChannelKill, setChannelTrim, setCrossfade]) fn.mockClear();
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  for (const fn of [setChannelKill, setChannelTrim, setCrossfade]) fn.mockClear();
});
afterEach(() => { act(() => root.unmount()); host.remove(); });

const press = (init: KeyboardEventInit) =>
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init })); });

it("toggles the assigned deck's band kill from the keyboard", async () => {
  await mount({ "a.eqKillLow": { key: "z" }, "b.eqKillHigh": { key: "u" } });
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
  await mount({});
  press({ key: "z" });
  await act(async () => {});
  expect(setChannelKill).not.toHaveBeenCalled();
});

it("kills a band the same way from its command as from its key (#431)", async () => {
  await mount({ "b.eqKillMid": { key: "y" } });
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

/** What the engine was last told for each deck's bands and trim. */
function engine() {
  const kills = new Map<string, boolean>();
  for (const [deck, band, killed] of setChannelKill.mock.calls) kills.set(`${deck}.${band}`, killed);
  const trims = new Map<string, number>();
  for (const [deck, trim] of setChannelTrim.mock.calls) trims.set(deck, trim);
  return { kills, trims, crossfade: setCrossfade.mock.calls.at(-1)?.[0] };
}

it("leaves no band killed in the engine after 2 PLAYER, 1 PLAYER and back (#602)", async () => {
  // rekordbox 7.2.19 resets the 3 BAND EQ of both channels on every layout
  // change (`PlayCuePanel::notifyChangeLayoutExport` @0x100012b40 ->
  // `reset3BandEQ` @0x100012900) and draws the buttons from the engine.
  render({});
  await act(async () => {});
  const low = () => host.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")[2];
  expect(low()?.textContent).toBe("LOW");
  act(() => low()?.click());
  await act(async () => {});
  expect(low()?.getAttribute("aria-pressed")).toBe("true");
  expect(engine().kills.get("a.low")).toBe(true);

  // 1 PLAYER: no strip, and the deck that is left plays flat.
  render({}, false);
  await act(async () => {});
  expect(engine().kills.get("a.low")).toBe(false);
  expect(engine().trims.get("a")).toBe(1);
  expect(engine().crossfade).toBe(0.5);

  // Back to 2 PLAYER: the strip draws LOW unlit, and the engine agrees.
  setChannelKill.mockClear();
  render({});
  await act(async () => {});
  expect(low()?.getAttribute("aria-pressed")).toBe("false");
  for (const deck of ["a", "b"]) {
    for (const band of ["high", "mid", "low"]) expect(engine().kills.get(`${deck}.${band}`)).toBe(false);
  }
});

it("starts the engine flat with the strip, whatever an earlier strip left", async () => {
  render({});
  await act(async () => {});
  expect(setChannelKill).toHaveBeenCalledTimes(6);
  expect(setChannelKill.mock.calls.every(([, , killed]) => killed === false)).toBe(true);
  expect(setChannelTrim).toHaveBeenCalledWith("a", 1);
  expect(setChannelTrim).toHaveBeenCalledWith("b", 1);
});
