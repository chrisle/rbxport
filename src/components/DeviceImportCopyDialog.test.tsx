/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { DevicePlaylistImportPlan } from "@/ipc/types";
import { DeviceImportCopyDialog } from "./DeviceImportCopyDialog";

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

const PLAN: DevicePlaylistImportPlan = {
  name: "TAG LIST",
  listed: 5,
  missing: 3,
  files: 2,
  bytes: 20 * 1024 ** 2,
  destination: "/Users/dj/Music/PioneerDJ/Imported from Device",
  freeBytes: 128 * 1024 ** 3,
};

const button = (label: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === label);

it("says what rekordbox says before it copies, with OK focused", () => {
  act(() => root.render(<DeviceImportCopyDialog plan={PLAN} onAnswer={() => {}} />));
  const text = host.textContent ?? "";
  expect(text).toContain("Import Playlist from Device");
  expect(text).toContain("contains music files that are not present in rekordbox on your computer.");
  expect(text).toContain("These music files will be copied to this computer.");
  expect(text).toContain("2 files 20.0 MB to the folder : /Users/dj/Music/PioneerDJ/Imported from Device");
  expect(text).toContain("Music file copy destination:");
  expect(text).toContain("Free space of /Users/dj/Music/PioneerDJ/Imported from Device is 128.0 GB");
  expect(document.activeElement?.textContent).toBe("OK");
});

it("names one file in the singular", () => {
  act(() => root.render(<DeviceImportCopyDialog plan={{ ...PLAN, files: 1, freeBytes: null }} onAnswer={() => {}} />));
  expect(host.textContent).toContain("1 file 20.0 MB to the folder : ");
  expect(host.textContent).not.toContain("Free space of");
});

it("answers OK with a copy and Cancel or Escape with none", () => {
  const onAnswer = vi.fn();
  act(() => root.render(<DeviceImportCopyDialog plan={PLAN} onAnswer={onAnswer} />));
  act(() => button("OK")?.click());
  act(() => button("Cancel")?.click());
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
  expect(onAnswer.mock.calls).toEqual([[true], [false], [false]]);
});
