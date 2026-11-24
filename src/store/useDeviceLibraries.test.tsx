/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend, Device } from "@/ipc/types";
import { useDeviceLibraries, type DeviceLibraries } from "./useDeviceLibraries";

const device: Device = { path: "/Volumes/TEST", name: "TEST", volumeId: "test", totalBytes: 100, freeBytes: 50, removable: true, export: null };
let latest: DeviceLibraries;
const host = document.createElement("div");
const root = createRoot(host);
const report = vi.fn();
function Probe({ devices }: { devices: Device[] }) { latest = useDeviceLibraries(devices, report); return null; }
const settle = () => act(async () => { await Promise.resolve(); });
afterEach(() => { act(() => root.unmount()); __setBackend(null); });

it("reopens an already browsed device view after an export or grid-tempo event refreshes volumes", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const read = vi.fn(() => Promise.resolve([]));
  __setBackend({ deviceLibraries: read } as unknown as Backend);
  act(() => root.render(<Probe devices={[device]} />)); await settle();
  act(() => latest.expand({ id: `device:${device.path}`, name: "TEST", kind: "device", depth: 0 })); await settle();
  expect(read).toHaveBeenCalledTimes(1);
  const before = latest.revision;
  act(() => root.render(<Probe devices={[{ ...device, freeBytes: 40 }]} />)); await settle();
  expect(read).toHaveBeenCalledTimes(2);
  expect(latest.revision).toBe(before + 1);
  await settle();
  expect(read).toHaveBeenCalledTimes(2);
});
