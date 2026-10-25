// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Backend, Device, TreeNode } from "@/ipc/types";
import { App } from "./App";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

const held = vi.hoisted(() => ({
  backend: null as Backend | null,
  devices: [] as Device[],
  devicesChanged: null as (() => void) | null,
  exportPlaylist: vi.fn(),
  menuChoice: null as (() => void) | null,
  currentExport: null as ((node: TreeNode, path: string) => void) | null,
  menuDevices: [] as { id: string; name: string }[],
}));

vi.mock("@/ipc/client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@/ipc/client")>();
  const { createMockBackend } = await import("@/ipc/backend-mock");
  held.backend = createMockBackend({ trackCount: 1 });
  return { ...original, getBackend: () => Promise.resolve(held.backend) };
});

// Keep the menu choice created before a device refresh. The real TreeView
// route from a clicked row to onExport is covered in TreeView.test.tsx.
vi.mock("@/views/tree/TreeView", () => ({
  TreeView: (props: {
    onExport: (node: TreeNode, path: string) => void;
    exportDevices: { id: string; name: string }[];
  }) => {
    held.menuDevices = props.exportDevices;
    held.currentExport = props.onExport;
    return <button onClick={() => {
      const clicked: TreeNode = { id: "playlist-clicked", name: "Clicked", kind: "playlist", depth: 0, childCount: 1 };
      const path = props.exportDevices[0]?.id;
      if (path) held.menuChoice = () => held.currentExport?.(clicked, path);
    }}>Open playlist export menu</button>;
  },
}));

vi.mock("@/views/statusbar/StatusBar", () => ({
  StatusBar: ({ activity, error }: { activity: string; error: string | null }) =>
    <div><span data-testid="activity">{activity}</span><span role="alert">{error}</span></div>,
}));

// Exercise App's own translated strings, without the DOM text localizer.
vi.mock("@/i18n", async () => {
  const { translate } = await vi.importActual<typeof import("@/i18n")>("@/i18n");
  const de = (await import("../../public/locales/de.json")).default;
  return {
    useTranslation: () => (text: string, values: Readonly<Record<string, string | number>> = {}) => {
      let result = translate(text, de);
      for (const [name, value] of Object.entries(values)) result = result.replaceAll(`{${name}}`, String(value));
      return result;
    },
  };
});

const device: Device = {
  name: "USB Test", path: "/Volumes/TEST", totalBytes: 1_000_000,
  freeBytes: 900_000, removable: true, volumeId: "test", export: null,
};

let host: HTMLDivElement;
let root: Root;

beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  vi.clearAllMocks();
  held.devices = [device];
  held.devicesChanged = null;
  held.menuChoice = null;
  held.currentExport = null;
  held.menuDevices = [];
  const { createMockBackend } = await import("@/ipc/backend-mock");
  held.backend = {
    ...createMockBackend({ trackCount: 1 }),
    listDevices: () => Promise.resolve(held.devices),
    onDevicesChanged: (listener) => { held.devicesChanged = listener; return () => { held.devicesChanged = null; }; },
    exportPlaylist: held.exportPlaylist,
  };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function mountAndOpenMenu() {
  await act(async () => { root.render(<App />); await Promise.resolve(); });
  await act(async () => { await Promise.resolve(); });
  expect(held.menuDevices).toEqual([{ id: device.path, name: device.name }]);
  const open = [...host.querySelectorAll("button")].find((button) => button.textContent === "Open playlist export menu");
  expect(open).toBeDefined();
  act(() => { open?.click(); });
  expect(held.menuChoice).not.toBeNull();
}

it("refuses a destination removed after the playlist menu opens", async () => {
  await mountAndOpenMenu();
  held.devices = [];
  await act(async () => { held.devicesChanged?.(); await Promise.resolve(); });
  expect(held.menuDevices).toEqual([]);

  act(() => { held.menuChoice?.(); });
  expect(held.exportPlaylist).not.toHaveBeenCalled();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("Das Gerät ist nicht mehr verbunden.");
});

it("exports the clicked playlist to a current destination and reports translated progress", async () => {
  let finish: ((value: Awaited<ReturnType<Backend["exportPlaylist"]>>) => void) | undefined;
  held.exportPlaylist.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  await mountAndOpenMenu();

  await act(async () => { held.menuChoice?.(); await Promise.resolve(); });
  expect(held.exportPlaylist).toHaveBeenCalledTimes(1);
  expect(held.exportPlaylist.mock.calls[0]?.slice(0, 2)).toEqual(["playlist-clicked", device.path]);
  expect(host.querySelector('[data-testid="activity"]')?.textContent).toBe("Ich schreibe an USB Test…");
  await act(async () => {
    finish?.({ tracks: 0, playlists: 1, bytesCopied: 0, analysisFiles: 0,
      reused: 0, removed: 0, tracksAdded: 0, tracksUpdated: 0,
      playlistsAdded: 1, playlistsRemoved: 0, skipped: [], failed: [], verified: true });
    await Promise.resolve();
  });
});
