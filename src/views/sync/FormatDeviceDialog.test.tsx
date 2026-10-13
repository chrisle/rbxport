/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { Device } from "@/ipc/types";
import { currentLayout, FormatDeviceDialog } from "./FormatDeviceDialog";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  HTMLDialogElement.prototype.showModal = vi.fn();
  HTMLDialogElement.prototype.close = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const stick = (fileSystem: string): Device => ({
  name: "USB A", path: "/Volumes/USB A", totalBytes: 0, freeBytes: 0, fileSystem, removable: true, volumeId: "dev:1", export: null,
});

const chosen = () => [...host.querySelectorAll<HTMLInputElement>('input[type="radio"]')]
  .find(input => input.checked)?.closest("label")?.querySelector("strong")?.textContent;

it("starts on the filesystem the stick has now", () => {
  act(() => root.render(<FormatDeviceDialog device={stick("hfs")} onFormat={vi.fn()} onClose={vi.fn()} />));
  expect(chosen()).toBe("HFS+");
  act(() => root.render(<FormatDeviceDialog key="fat" device={stick("FAT32")} onFormat={vi.fn()} onClose={vi.fn()} />));
  expect(chosen()).toBe("FAT32");
});

it("reads every name an OS gives HFS+, and starts anything else on FAT32", () => {
  for (const name of ["hfs", "HFS+", "hfsplus", "Mac OS Extended (Journaled)"]) expect(currentLayout(name), name).toBe("hfsPlus");
  for (const name of ["FAT32", "msdos", "vfat", "exFAT", "NTFS", "", undefined]) expect(currentLayout(name), String(name)).toBe("fat32");
});
