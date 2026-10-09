/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { UsbExportPane } from "./UsbExportPane";
import styles from "./UsbExportPane.module.css";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("keeps the blank-drive database option in USB Export and updates its existing preference", () => {
  const update = vi.fn();
  act(() => root.render(
    <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update, reset: vi.fn() }}>
      <UsbExportPane />
    </PreferencesProvider>,
  ));

  const option = host.querySelector<HTMLInputElement>('input[aria-labelledby$="-database-folders"]');
  expect(host.textContent).toContain("Setup PIONEER folder on USB drives");
  expect(host.textContent).toContain("when a new USB drive is synced for the first time");
  expect(option?.checked).toBe(true);
  act(() => option?.click());
  expect(update).toHaveBeenCalledWith("djSystem", { createDatabaseFolders: false });
});

it("explains the sync options and shows their defaults", () => {
  act(() => root.render(
    <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: vi.fn(), reset: vi.fn() }}>
      <UsbExportPane />
    </PreferencesProvider>,
  ));

  expect(host.textContent).toContain("Automatically import CDJ/mixer settings when syncing");
  expect(host.textContent).toContain("Copy valid CDJ and mixer settings from selected USB devices into rbxport when you click SYNC in Sync Manager.");
  expect(host.textContent).toContain("Automatically import play history when syncing");
  expect(host.textContent).toContain("Add new play-history entries from selected USB devices to your library when you click SYNC in Sync Manager.");
  expect(host.textContent).toContain("Free space on your USB stick by removing songs that aren't in any playlist.");
  expect([...host.querySelectorAll(`.${styles.default}`)].map(node => node.textContent)).toEqual([
    "Default: Off", "Default: On", "Default: On", "Default: On", "Default: On", "Default: Off", "Default: On", "Default: Off",
  ]);
});

it("sets what Sync Manager's Import has ticked when it opens", () => {
  const update = vi.fn();
  act(() => root.render(
    <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update, reset: vi.fn() }}>
      <UsbExportPane />
    </PreferencesProvider>,
  ));
  const toggle = (name: string) => [...host.querySelectorAll<HTMLInputElement>('input[role="switch"]')]
    .find(input => document.getElementById(input.getAttribute("aria-labelledby") ?? "")?.textContent === name)!;
  expect(toggle("Import cues and beat grids").checked).toBe(true);
  expect(toggle("Import play history").checked).toBe(true);
  expect(toggle("Import CDJ/mixer settings").checked).toBe(false);
  expect(toggle("Import track ratings").checked).toBe(true);
  expect(toggle("Automatically import track ratings when syncing").checked).toBe(true);
  act(() => toggle("Import CDJ/mixer settings").click());
  expect(update).toHaveBeenCalledWith("usbExport", { importButtonSettings: true });
  act(() => toggle("Import track ratings").click());
  expect(update).toHaveBeenCalledWith("usbExport", { importButtonRatings: false });
  act(() => toggle("Automatically import track ratings when syncing").click());
  expect(update).toHaveBeenCalledWith("usbExport", { importRatings: false });
});

it("offers AIFF as a compatibility conversion target", () => {
  const update = vi.fn();
  const preferences = {
    ...DEFAULT_PREFERENCES,
    usbExport: { ...DEFAULT_PREFERENCES.usbExport, maximumCompatibility: true },
  };
  act(() => root.render(
    <PreferencesProvider value={{ preferences, update, reset: vi.fn() }}>
      <UsbExportPane />
    </PreferencesProvider>,
  ));
  const select = host.querySelector<HTMLSelectElement>('select[id$="-format"]')!;
  expect([...select.options].map(option => [option.value, option.textContent])).toContainEqual(["aiff", "AIFF — larger files"]);
  act(() => {
    select.value = "aiff";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(update).toHaveBeenCalledWith("usbExport", { conversionFormat: "aiff" });
});
