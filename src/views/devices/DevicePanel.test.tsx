/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend, Device, DeviceSettings } from "@/ipc/types";
import { referenceDeviceSettings } from "@/ipc/mock-device-settings";
import { DevicePanel } from "./DevicePanel";

let host: HTMLDivElement;
let root: Root;
const error = vi.fn();
const device = (path = "/generated-a"): Device => ({ path, name: path, totalBytes: 0, freeBytes: 0, removable: true, volumeId: path, export: null });
const settings = (column = 2) => Object.assign(referenceDeviceSettings("Generated", true), { subColumn: column, hasMenuSettings: true });
const settle = () => act(async () => { for (let i = 0; i < 10; i++) await Promise.resolve(); });
function pending() {
  let resolve!: (value: DeviceSettings) => void;
  let reject!: (value: Error) => void;
  const promise = new Promise<DeviceSettings>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function render(path = "/generated-a") {
  act(() => root.render(<DevicePanel device={device(path)} playlists={[]} onSync={vi.fn()} onRefresh={vi.fn()} onError={error} />));
}
function column() { return host.querySelector<HTMLSelectElement>('select[aria-label="Default right column"]')!; }
function tab(name: string) {
  act(() => Array.from(host.querySelectorAll<HTMLButtonElement>('[role="tab"]')).find(button => button.textContent === name)!.click());
}
function choose(value: number) {
  act(() => { column().value = String(value); column().dispatchEvent(new Event("change", { bubbles: true })); });
}
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear(); error.mockReset();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); __setBackend(null); });

it("edits legacy menus without enabling OneLibrary-owned name or colour settings", async () => {
  const legacy = { ...settings(), hasOneLibrary: false, hasLibrarySettings: false };
  const save = vi.fn((_path: string, next: DeviceSettings) => Promise.resolve(next));
  __setBackend({ deviceSettings: () => Promise.resolve(legacy), saveDeviceSettings: save } as unknown as Backend);
  render(); await settle(); tab("Column");
  expect(column().disabled).toBe(false);
  expect(column().value).toBe("2");
  choose(5); await settle();
  expect(save).toHaveBeenCalledExactlyOnceWith("/generated-a", { ...legacy, subColumn: 5 });
  for (const name of ["Category", "Sort"]) {
    tab(name);
    expect(host.querySelector('[role="listbox"]')?.getAttribute("aria-disabled")).toBeNull();
  }
  tab("Color");
  expect(Array.from(host.querySelectorAll<HTMLInputElement>("input")).every(input => input.disabled)).toBe(true);
  tab("General");
  expect(host.querySelector<HTMLInputElement>('input[maxlength="64"]')?.disabled).toBe(true);
});

it("reverts the current failed save to confirmed settings", async () => {
  __setBackend({ deviceSettings: () => Promise.resolve(settings()), saveDeviceSettings: () => Promise.reject(new Error("generated refusal")) } as unknown as Backend);
  render(); await settle(); tab("Column"); choose(5); await settle();
  expect(column().value).toBe("2"); expect(error).toHaveBeenCalledOnce();
});

it("does not let an older successful save overwrite a newer choice", async () => {
  const older = pending(), newer = pending();
  const save = vi.fn().mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
  __setBackend({ deviceSettings: () => Promise.resolve(settings()), saveDeviceSettings: save } as unknown as Backend);
  render(); await settle(); tab("Column"); choose(5); await settle(); choose(6); await settle();
  newer.resolve(settings(6)); await settle(); older.resolve(settings(5)); await settle();
  expect(column().value).toBe("6"); expect(error).not.toHaveBeenCalled();
});

it.each(["superseded", "another device", "unmounted"] as const)("retires a failed save after %s", async retired => {
  const older = pending();
  const save = vi.fn().mockReturnValueOnce(older.promise).mockResolvedValue(settings(6));
  __setBackend({ deviceSettings: (path: string) => Promise.resolve(settings(path === "/generated-b" ? 6 : 2)), saveDeviceSettings: save } as unknown as Backend);
  render(); await settle(); tab("Column"); choose(5); await settle();
  if (retired === "superseded") choose(6);
  else if (retired === "another device") render("/generated-b");
  else act(() => root.render(<div>another pane</div>));
  await settle(); older.reject(new Error("old refusal")); await settle();
  expect(error).not.toHaveBeenCalled();
  if (retired !== "unmounted") expect(column().value).toBe("6");
});

it("serializes accepted writes and reverts only the newest failure to the preceding committed save", async () => {
  const older = pending(), newer = pending();
  const save = vi.fn().mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
  __setBackend({ deviceSettings: () => Promise.resolve(settings()), saveDeviceSettings: save } as unknown as Backend);
  render(); await settle(); tab("Column"); choose(5); await settle(); choose(6); await settle();
  expect(save).toHaveBeenCalledTimes(1);
  older.resolve(settings(5)); await settle();
  expect(column().value).toBe("6"); expect(save).toHaveBeenCalledTimes(2);
  newer.reject(new Error("current refusal")); await settle();
  expect(column().value).toBe("5"); expect(error).toHaveBeenCalledExactlyOnceWith("current refusal");
});

it("retires queued writes when the same path is remounted and waits for the new owner's read", async () => {
  const dispatched = pending(), newRead = pending();
  const save = vi.fn().mockReturnValueOnce(dispatched.promise).mockResolvedValue(settings(6));
  const read = vi.fn().mockResolvedValueOnce(settings()).mockReturnValueOnce(newRead.promise);
  __setBackend({ deviceSettings: read, saveDeviceSettings: save } as unknown as Backend);
  render(); await settle(); tab("Column"); choose(5); await settle(); choose(6); await settle();
  expect(save).toHaveBeenCalledTimes(1);
  act(() => root.render(<div>removed device</div>));
  render(); await settle(); tab("Column");
  expect(column()).toBeNull();
  dispatched.resolve(settings(5)); await settle();
  expect(save).toHaveBeenCalledTimes(1);
  newRead.resolve(settings(2)); await settle();
  expect(column().value).toBe("2");
  choose(6); await settle();
  expect(save).toHaveBeenCalledTimes(2);
  expect(save.mock.calls[1]).toEqual(["/generated-a", settings(6)]);
});


function renderVolume(volumeId: string) {
  const mounted = { ...device(), volumeId, export: { tracks: 1, playlists: 1, ours: true, written: "same-export-stamp" } };
  act(() => root.render(<DevicePanel device={mounted} playlists={[]} onSync={vi.fn()} onRefresh={vi.fn()} onError={error} />));
}

it("retains confirmed settings and accepted queued edits for the same mounted identity", async () => {
  const first = pending();
  const read = vi.fn().mockResolvedValue(settings());
  const save = vi.fn().mockReturnValueOnce(first.promise).mockResolvedValue(settings(6));
  __setBackend({ deviceSettings: read, saveDeviceSettings: save } as unknown as Backend);
  renderVolume("volume-a"); await settle(); tab("Column"); choose(5); await settle(); choose(6); await settle();
  renderVolume("volume-a"); await settle();
  expect(read).toHaveBeenCalledTimes(1);
  expect(column().value).toBe("6");
  first.resolve(settings(5)); await settle();
  expect(save).toHaveBeenCalledTimes(2);
  expect(column().value).toBe("6"); expect(error).not.toHaveBeenCalled();
});

it("re-reads replacement identity with the same path and export stamp before offering edits", async () => {
  const replacement = pending();
  const read = vi.fn().mockResolvedValueOnce(settings()).mockReturnValueOnce(replacement.promise);
  const save = vi.fn();
  __setBackend({ deviceSettings: read, saveDeviceSettings: save } as unknown as Backend);
  renderVolume("volume-a"); await settle(); tab("Column");
  renderVolume("volume-b"); await settle();
  const pendingControl = column();
  replacement.resolve(settings(3)); await settle();
  expect(pendingControl).toBeNull();
  expect(read).toHaveBeenCalledTimes(2);
  expect(column().value).toBe("3");
  expect(save).not.toHaveBeenCalled();
});

it("retires not-yet-dispatched edits when a replacement keeps the path and export stamp", async () => {
  const dispatched = pending(), replacement = pending();
  const read = vi.fn().mockResolvedValueOnce(settings()).mockReturnValueOnce(replacement.promise);
  const save = vi.fn().mockReturnValueOnce(dispatched.promise).mockResolvedValue(settings(5));
  __setBackend({ deviceSettings: read, saveDeviceSettings: save } as unknown as Backend);
  renderVolume("volume-a"); await settle(); tab("Column"); choose(5); await settle(); choose(6); await settle();
  expect(save).toHaveBeenCalledTimes(1);
  renderVolume("volume-b"); await settle();
  dispatched.resolve(settings(5)); await settle();
  const callsBeforeReplacementRead = save.mock.calls.length;
  replacement.resolve(settings(3)); await settle();
  expect(callsBeforeReplacementRead).toBe(1);
  expect(read).toHaveBeenCalledTimes(2);
  expect(column().value).toBe("3");
  choose(5); await settle();
  expect(save).toHaveBeenCalledTimes(2);
  expect(save.mock.calls[1]).toEqual(["/generated-a", settings(5)]);
  expect(error).not.toHaveBeenCalled();
});

it.each(["success", "failure"] as const)("retires an initial read's late %s after same-path identity replacement", async outcome => {
  const initial = pending(), replacement = pending();
  const read = vi.fn().mockReturnValueOnce(initial.promise).mockReturnValueOnce(replacement.promise);
  __setBackend({ deviceSettings: read } as unknown as Backend);
  renderVolume("volume-a"); await settle(); tab("Column");
  renderVolume("volume-b"); await settle();
  replacement.resolve(settings(3)); await settle();
  if (outcome === "success") initial.resolve(settings(2));
  else initial.reject(new Error("retired volume refusal"));
  await settle();
  expect(error).not.toHaveBeenCalled();
  expect(read).toHaveBeenCalledTimes(2);
  expect(column().value).toBe("3");
});

it("does not seed defaults from an old pending read after same-path identity replacement", async () => {
  const initial = pending();
  const read = vi.fn().mockReturnValueOnce(initial.promise).mockResolvedValue(settings(3));
  const defaults = vi.fn().mockResolvedValue(settings(2));
  __setBackend({ deviceSettings: read, writeDeviceDefaults: defaults } as unknown as Backend);
  renderVolume("volume-a"); await settle();
  renderVolume("volume-b"); await settle();
  initial.resolve({ ...settings(2), hasDevSetting: false }); await settle();
  expect(defaults).not.toHaveBeenCalled();
  expect(read).toHaveBeenCalledTimes(2);
});


it("seeds the replacement owner's missing DEVSETTING and retains its current tab", async () => {
  const initial = pending(), seeded = pending();
  const read = vi.fn().mockReturnValueOnce(initial.promise).mockResolvedValueOnce({ ...settings(3), hasDevSetting: false });
  const defaults = vi.fn().mockReturnValueOnce(seeded.promise);
  __setBackend({ deviceSettings: read, writeDeviceDefaults: defaults } as unknown as Backend);
  renderVolume("volume-a"); await settle(); tab("Column");
  renderVolume("volume-b"); await settle();
  expect(read).toHaveBeenCalledTimes(2);
  expect(defaults).toHaveBeenCalledTimes(1);
  expect(defaults.mock.calls[0]?.[0]).toBe("/generated-a");
  expect(column()).toBeNull();
  initial.resolve({ ...settings(2), hasDevSetting: false }); await settle();
  expect(defaults).toHaveBeenCalledTimes(1);
  seeded.resolve(settings(3)); await settle();
  expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Column");
  expect(column().value).toBe("3");
  expect(error).not.toHaveBeenCalled();
});
