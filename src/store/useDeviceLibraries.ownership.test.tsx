/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend, Device, DeviceLibrary } from "@/ipc/types";
import { useDeviceLibraries, type DeviceLibraries } from "./useDeviceLibraries";

const device: Device = { path: "/fixture", name: "TEST", volumeId: "first-medium", totalBytes: 100, freeBytes: 50, removable: true, export: null };
const node = { id: "device:/fixture", name: "TEST", kind: "device" as const, depth: 0 };
const libraries = (name: string): DeviceLibrary[] => [{ format: "deviceLibrary", tracks: 0,
  nodes: [{ id: "1", parentId: "0", depth: 0, name, folder: false, count: 0 }] }];
let root: Root | undefined;
let latest: DeviceLibraries;
const report = vi.fn();
function Probe({ devices }: { devices: Device[] }) {
  latest = useDeviceLibraries(devices, report);
  return null;
}
const settle = () => act(async () => { await Promise.resolve(); });
const render = (devices: Device[]) => act(() => {
  root ??= createRoot(document.createElement("div"));
  root.render(<Probe devices={devices} />);
});
afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  report.mockClear();
  __setBackend(null);
});

it("retires a failed read when its hook unmounts and keeps the new session usable", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  let rejectOld: (error: Error) => void = () => {};
  const read = vi.fn()
    .mockImplementationOnce(() => new Promise<DeviceLibrary[]>((_resolve, reject) => { rejectOld = reject; }))
    .mockResolvedValue(libraries("New session playlist"));
  __setBackend({ deviceLibraries: read } as unknown as Backend);
  render([device]); await settle();
  act(() => latest.expand(node)); await settle();
  expect(read).toHaveBeenCalledTimes(1);
  act(() => root?.unmount()); root = undefined;
  render([device]); await settle();
  act(() => latest.expand(node)); await settle();
  expect(latest.nodes.some(item => item.name === "New session playlist")).toBe(true);
  await act(async () => { rejectOld(new Error("Retired session read")); await Promise.resolve(); });
  expect(report).not.toHaveBeenCalled();
  expect(latest.nodes.some(item => item.name === "New session playlist")).toBe(true);
});

it("reads a replacement medium at the same mount path even while its predecessor is pending", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  let releaseOld: (rows: DeviceLibrary[]) => void = () => {};
  const read = vi.fn()
    .mockResolvedValueOnce(libraries("First medium playlist"))
    .mockImplementationOnce(() => new Promise<DeviceLibrary[]>(resolve => { releaseOld = resolve; }))
    .mockResolvedValue(libraries("Replacement medium playlist"));
  __setBackend({ deviceLibraries: read } as unknown as Backend);
  render([device]); await settle();
  act(() => latest.expand(node)); await settle();
  let refresh: Promise<void> | undefined;
  act(() => { refresh = latest.refresh(device.path); }); await settle();
  expect(read).toHaveBeenCalledTimes(2);
  render([{ ...device, volumeId: "replacement-medium" }]); await settle();
  expect(latest.nodes.some(item => item.name === "Replacement medium playlist")).toBe(true);
  await act(async () => { releaseOld(libraries("First medium playlist")); await refresh; });
  expect(latest.nodes.some(item => item.name === "Replacement medium playlist")).toBe(true);
  expect(latest.nodes.some(item => item.name === "First medium playlist")).toBe(false);
});

it("gives the same volume a new owner after removal and re-expansion", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  let releaseOld: (rows: DeviceLibrary[]) => void = () => {};
  const read = vi.fn().mockResolvedValueOnce(libraries("Before removal"))
    .mockImplementationOnce(() => new Promise<DeviceLibrary[]>(resolve => { releaseOld = resolve; }))
    .mockResolvedValue(libraries("After remount"));
  __setBackend({ deviceLibraries: read } as unknown as Backend);
  render([device]); await settle();
  act(() => latest.expand(node)); await settle();
  let old: Promise<void> | undefined;
  act(() => { old = latest.refresh(device.path); }); await settle();
  render([]); await settle();
  expect(latest.nodes).toHaveLength(0);
  render([device]); await settle();
  act(() => latest.expand(node)); await settle();
  expect(read).toHaveBeenCalledTimes(3);
  expect(latest.nodes.some(item => item.name === "After remount")).toBe(true);
  const revision = latest.revision;
  await act(async () => { releaseOld(libraries("Before removal")); await old; });
  expect(latest.nodes.some(item => item.name === "After remount")).toBe(true);
  expect(latest.nodes.some(item => item.name === "Before removal")).toBe(false);
  expect(latest.revision).toBe(revision);
});

it("keeps expansion deduplication owned by the newest pending read", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  let releaseFirst: (rows: DeviceLibrary[]) => void = () => {};
  let releaseLatest: (rows: DeviceLibrary[]) => void = () => {};
  const read = vi.fn()
    .mockImplementationOnce(() => new Promise<DeviceLibrary[]>(resolve => { releaseFirst = resolve; }))
    .mockImplementationOnce(() => new Promise<DeviceLibrary[]>(resolve => { releaseLatest = resolve; }));
  __setBackend({ deviceLibraries: read } as unknown as Backend);
  render([device]); await settle();
  act(() => { latest.expand(node); latest.expand(node); }); await settle();
  expect(read).toHaveBeenCalledTimes(1);
  let refresh: Promise<void> | undefined;
  act(() => { refresh = latest.refresh(device.path); }); await settle();
  expect(read).toHaveBeenCalledTimes(2);
  await act(async () => { releaseFirst(libraries("Obsolete first read")); await Promise.resolve(); });
  act(() => latest.expand(node)); await settle();
  expect(read).toHaveBeenCalledTimes(2);
  expect(latest.nodes.some(item => item.name === "Obsolete first read")).toBe(false);
  await act(async () => { releaseLatest(libraries("Current pending read")); await refresh; });
  expect(latest.nodes.some(item => item.name === "Current pending read")).toBe(true);
  expect(latest.revision).toBe(1);
});

it("reports a current read error and permits a later explicit refresh", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const read = vi.fn().mockRejectedValueOnce(new Error("Current device read failed"))
    .mockResolvedValue(libraries("Recovered playlist"));
  __setBackend({ deviceLibraries: read } as unknown as Backend);
  render([device]); await settle();
  act(() => latest.expand(node)); await settle();
  expect(report).toHaveBeenCalledExactlyOnceWith("Current device read failed");
  expect(latest.nodes).toHaveLength(1);
  await act(async () => { await latest.refresh(device.path); });
  expect(latest.nodes.some(item => item.name === "Recovered playlist")).toBe(true);
  expect(latest.revision).toBe(1);
});
