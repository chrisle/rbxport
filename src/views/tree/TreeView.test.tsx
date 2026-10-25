/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TreeNode } from "@/ipc/types";
import { TreeView } from "./TreeView";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const playlist: TreeNode = {
  id: "devlib:pdb:node:7:/Volumes/TEST", name: "Set", kind: "devicePlaylist", depth: 0, childCount: 1,
};

function render(canDropToDevicePlaylists: boolean, onDropDeviceTracks: (node: TreeNode) => void) {
  act(() => root.render(<TreeView
    nodes={[playlist]}
    selectedId={playlist.id}
    onSelect={() => {}}
    dragging
    canDropToDevicePlaylists={canDropToDevicePlaylists}
    onDropDeviceTracks={onDropDeviceTracks}
  />));
}

describe("TreeView device playlist drops", () => {
  it("accepts a collection track drag and routes the exact device playlist", () => {
    const dropped = vi.fn();
    render(true, dropped);
    const row = host.querySelector("[data-device-track-drop]");
    expect(row).not.toBeNull();

    const over = new Event("dragover", { bubbles: true, cancelable: true });
    Object.defineProperty(over, "dataTransfer", { value: { dropEffect: "none", types: [] } });
    act(() => { row?.dispatchEvent(over); });
    expect(over.defaultPrevented).toBe(true);
    expect((over as DragEvent).dataTransfer?.dropEffect).toBe("copy");

    act(() => { row?.dispatchEvent(new Event("drop", { bubbles: true, cancelable: true })); });
    expect(dropped).toHaveBeenCalledWith(playlist);
  });

  it("does not expose a device playlist as a target for device row ids", () => {
    const dropped = vi.fn();
    act(() => root.render(<TreeView
      nodes={[playlist]}
      selectedId={playlist.id}
      onSelect={() => {}}
      dragging
      canDropToDevicePlaylists={false}
      onDropDeviceTracks={dropped}
    />));
    const row = host.querySelector("[data-kind='devicePlaylist']");
    expect(row?.hasAttribute("data-device-track-drop")).toBe(false);
    const over = new Event("dragover", { bubbles: true, cancelable: true });
    Object.defineProperty(over, "dataTransfer", { value: { dropEffect: "none", types: [] } });
    act(() => { row?.dispatchEvent(over); });
    expect(over.defaultPrevented).toBe(false);
    act(() => { row?.dispatchEvent(new Event("drop", { bubbles: true, cancelable: true })); });
    expect(dropped).not.toHaveBeenCalled();
  });
});

describe("TreeView playlist context export", () => {
  const selected: TreeNode = { id: "playlist-selected", name: "Selected", kind: "playlist", depth: 0, childCount: 1 };
  const clicked: TreeNode = { id: "playlist-clicked", name: "Clicked", kind: "playlist", depth: 0, childCount: 1 };

  it("passes the right-clicked playlist and chosen USB path to export", () => {
    const onExport = vi.fn();
    act(() => root.render(<TreeView
      nodes={[selected, clicked]}
      selectedId={selected.id}
      onSelect={() => {}}
      onExport={onExport}
      exportDevices={[{ id: "/Volumes/DJ STICK", name: "DJ STICK" }]}
    />));

    const row = host.querySelector('[role="treeitem"][data-kind="playlist"]:nth-child(2)');
    expect(row).not.toBeNull();
    act(() => { row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })); });
    const exportItem = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')]
      .find((item) => item.textContent === "Export Playlist");
    expect(exportItem).toBeDefined();
    act(() => { exportItem?.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })); });
    const stick = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')]
      .find((item) => item.textContent === "DJ STICK");
    expect(stick).toBeDefined();
    act(() => { stick?.click(); });
    expect(onExport).toHaveBeenCalledExactlyOnceWith(clicked, "/Volumes/DJ STICK");
  });
});
