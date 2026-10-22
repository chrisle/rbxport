// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { LinkStatus } from "@/ipc/types";
import { LinkPane } from "./LinkPane";

const held = vi.hoisted(() => ({
  update: vi.fn(),
  backend: { linkStatus: vi.fn(), onLinkStatus: vi.fn(), startLinkExport: vi.fn(), stopLinkExport: vi.fn() },
}));
vi.mock("@/ipc/client", () => ({ getBackend: () => Promise.resolve(held.backend) }));
vi.mock("@/store/usePreferences", () => ({ usePreferencesContext: () => ({ preferences: { djSystem: { linkInterface: null, waveformColor: "rgb", waveformPosition: "left", overviewWaveform: "full", keyDisplay: "alphanumeric", linkKeySort: "musical", autoJoinLink: false } }, update: held.update }) }));
const off: LinkStatus = { on: false, problem: null, interface: null, interfaces: [], players: [], master: false, masterBpm: 120, state: "off", number: null };
const blocked = { ...off, problem: "rekordbox is running and holds the link ports. Quit it to turn LINK on." };
let host: HTMLDivElement;
let root: Root;
let unsubscribe: ReturnType<typeof vi.fn>;
const connect = () => [...host.querySelectorAll("button")].find(button => button.textContent === "Connect to PRO DJ LINK");
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  vi.resetAllMocks();
  unsubscribe = vi.fn();
  held.backend.onLinkStatus.mockReturnValue(unsubscribe);
  host = document.createElement("div");
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); vi.useRealTimers(); });
it("detects rekordbox quitting and reopening without a LINK event", async () => {
  held.backend.linkStatus.mockResolvedValue(blocked);
  await act(async () => { root.render(<LinkPane />); await Promise.resolve(); });
  expect(host.querySelector('[role="alert"]')?.textContent).toBe(blocked.problem);
  expect(connect()).toBeUndefined();
  held.backend.linkStatus.mockResolvedValue(off);
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(connect()?.disabled).toBe(false);
  held.backend.linkStatus.mockResolvedValue(blocked);
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(connect()).toBeUndefined();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe(blocked.problem);
});
it("does not overwrite a newer LINK event with a delayed status read", async () => {
  let resolve!: (status: LinkStatus) => void;
  held.backend.linkStatus.mockReturnValue(new Promise<LinkStatus>(done => { resolve = done; }));
  await act(async () => { root.render(<LinkPane />); await Promise.resolve(); });
  const listener = held.backend.onLinkStatus.mock.calls[0]![0] as (status: LinkStatus) => void;
  act(() => listener({ ...off, on: true, state: "up" }));
  await act(async () => { resolve(blocked); await Promise.resolve(); });
  expect(host.querySelector('[role="status"]')?.textContent).toBe("Connected");
  expect(host.querySelector('[role="alert"]')).toBeNull();
});
it("recovers from a failed status check and stops polling when closed", async () => {
  held.backend.linkStatus.mockRejectedValueOnce(new Error("temporary failure")).mockResolvedValue(off);
  await act(async () => { root.render(<LinkPane />); await Promise.resolve(); });
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("temporary failure");
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(connect()?.disabled).toBe(false);
  act(() => root.render(null));
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(held.backend.linkStatus).toHaveBeenCalledTimes(2);
  expect(unsubscribe).toHaveBeenCalledTimes(1);
});

it("shows key sort examples and saves the alphabetical choice", async () => {
  held.backend.linkStatus.mockResolvedValue(off);
  await act(async () => { root.render(<LinkPane />); await Promise.resolve(); });
  expect(host.textContent).toContain("Alphabetically — A, Ab, B, …");
  expect(host.textContent).toContain("Musically — Abm, B, Ebm, F#, Bbm, …");
  const radio = host.querySelector<HTMLInputElement>('input[name="link-key-sort"]');
  act(() => radio?.click());
  expect(held.update).toHaveBeenCalledWith("djSystem", {linkKeySort: "alphabetical"});
});

it("starts LINK with device settings and independent key order", async () => {
  held.backend.linkStatus.mockResolvedValue(off);
  held.backend.startLinkExport.mockResolvedValue({ ...off, on: true, state: "up" });
  await act(async () => { root.render(<LinkPane />); await Promise.resolve(); });
  await act(async () => { connect()?.click(); await Promise.resolve(); });
  expect(held.backend.startLinkExport).toHaveBeenCalledWith(undefined, {
    waveformColor: "rgb",
    waveformPosition: "left",
    overviewWaveform: "full",
    keyDisplay: "alphanumeric",
  }, "musical");
});

it("offers automatic LINK joining and saves it as an opt-in", async () => {
  held.backend.linkStatus.mockResolvedValue(off);
  await act(async () => { root.render(<LinkPane />); await Promise.resolve(); });
  const toggle = [...host.querySelectorAll<HTMLInputElement>('input[role="switch"]')]
    .find((input) => input.parentElement?.textContent?.includes("Auto-join LINK when available"));
  expect(toggle?.checked).toBe(false);
  act(() => toggle?.click());
  expect(held.update).toHaveBeenCalledWith("djSystem", { autoJoinLink: true });
});

it("lists devices up to ID 100 and leaves out TC-SHOWKONTROL above it (issue #582)", async () => {
  const device = (number: number, name: string, kind = "device") => ({
    number, name, kind, address: `10.0.0.${number % 255}`, loaded: null, playing: false, master: false,
    sync: false, cued: false, linkCue: false, mounted: false,
  });
  held.backend.linkStatus.mockResolvedValue({
    ...off, on: true, state: "up", interface: { name: "en0", address: "10.0.0.2" } as LinkStatus["interface"],
    players: [device(33, "XDJ-AZ", "player"), device(100, "Edge device"), device(101, "TC-SHOWKONTROL")],
  });
  await act(async () => { root.render(<LinkPane />); await Promise.resolve(); });
  const table = host.querySelector('table[aria-label="Devices on the link"]');
  expect(table?.textContent).toContain("XDJ-AZ");
  expect(table?.textContent).toContain("Edge device");
  expect(table?.textContent).not.toContain("TC-SHOWKONTROL");
  expect(table?.querySelectorAll("tbody tr")).toHaveLength(2);
  expect(host.querySelector("h4 span")?.textContent).toBe("2");
});
