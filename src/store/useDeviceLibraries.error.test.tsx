/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Device } from "@/ipc/types";
import { useDeviceLibraries, type DeviceLibraries } from "./useDeviceLibraries";

const backend = vi.hoisted(() => ({ deviceLibraries: vi.fn() }));
vi.mock("@/ipc/client", () => ({ getBackend: () => Promise.resolve(backend) }));

it("reports a failed library read instead of silently showing an empty stick", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const message = "Both PIONEER and .PIONEER contain libraries. Reconcile them before syncing.";
  backend.deviceLibraries.mockRejectedValue(new Error(message));
  const report = vi.fn();
  const devices = [{ path: "/fixture", name: "TEST" }] as Device[];
  let libraries: DeviceLibraries;
  function Probe() { libraries = useDeviceLibraries(devices, report); return null; }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => { root.render(<Probe />); await Promise.resolve(); });
    await act(async () => { libraries.expand({ id: "device:/fixture", name: "TEST", kind: "device", depth: 0 }); await Promise.resolve(); });
    expect(report).toHaveBeenCalledExactlyOnceWith(message);
    expect(libraries!.nodes).toHaveLength(1);
  } finally { act(() => root.unmount()); }
});
