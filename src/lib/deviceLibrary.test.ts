import { describe, expect, it } from "vitest";

import type { DeviceLibrary, TreeNode } from "@/ipc/types";
import {
  deviceNodeId, deviceParentFor, devicePlaylistsOf, deviceSource, isDeviceLibraryKind, parseDeviceNodeId,
  withDeviceLibraries,
} from "./deviceLibrary";
import { nodesForSource, sourceOf } from "./tree";
import { specForNode } from "./viewSpec";

const stick: TreeNode = { id: "device:D:\\", name: "ssd", kind: "device", depth: 0 };

const libraries: DeviceLibrary[] = [
  {
    format: "deviceLibrary",
    tracks: 4,
    nodes: [
      { id: "5", parentId: "0", name: "Untitled Folder", folder: true, depth: 0, count: 0 },
      { id: "1", parentId: "0", name: "Folder A", folder: true, depth: 0, count: 1 },
      { id: "2", parentId: "1", name: "Inside A", folder: false, depth: 1, count: 2 },
      { id: "3", parentId: "0", name: "Top Renamed", folder: false, depth: 0, count: 2 },
    ],
  },
  { format: "oneLibrary", tracks: 4, nodes: [{ id: "3", parentId: "0", name: "Top List", folder: false, depth: 0, count: 3 }] },
];

describe("a stick's own libraries in the tree", () => {
  it("round-trips what a node is through its id, colons in the path and all", () => {
    const ref = { path: "D:\\", format: "oneLibrary" as const, role: "node" as const, id: "3" };
    expect(parseDeviceNodeId(deviceNodeId(ref))).toEqual(ref);
    expect(parseDeviceNodeId("device:D:\\")).toBeNull();
    expect(parseDeviceNodeId("17")).toBeNull();
  });

  it("puts each library under its stick as rekordbox does: All Tracks, then Playlists and its tree", () => {
    const nodes = withDeviceLibraries([stick], new Map([["D:\\", libraries]]));
    expect(nodes.map((n) => [n.name, n.kind, n.depth])).toEqual([
      ["ssd", "device", 0],
      ["Device Library", "deviceLibrary", 1],
      ["All Tracks", "deviceAllTracks", 2],
      ["Playlists", "devicePlaylists", 2],
      ["Untitled Folder", "deviceFolder", 3],
      ["Folder A", "deviceFolder", 3],
      ["Inside A", "devicePlaylist", 4],
      ["Top Renamed", "devicePlaylist", 3],
      ["OneLibrary", "deviceLibrary", 1],
      ["All Tracks", "deviceAllTracks", 2],
      ["Playlists", "devicePlaylists", 2],
      ["Top List", "devicePlaylist", 3],
    ]);
    // The stick can be opened before anything under it is read.
    expect(nodes[0]).toMatchObject({ lazy: true, expanded: false });
    expect(withDeviceLibraries([stick], new Map())).toHaveLength(1);
  });

  it("files every node under the Devices rail", () => {
    const nodes = withDeviceLibraries([stick], new Map([["D:\\", libraries]]));
    expect(nodesForSource(nodes, "devices")).toHaveLength(nodes.length);
    expect(nodes.every((n) => sourceOf(nodes, n.id) === "devices")).toBe(true);
    expect(nodes.slice(1).every((n) => isDeviceLibraryKind(n.kind))).toBe(true);
  });

  it("opens a playlist, or every track for All Tracks and Playlists, from the stick; a folder opens empty", () => {
    const nodes = withDeviceLibraries([stick], new Map([["D:\\", libraries]]));
    const byName = (name: string) => nodes.find((n) => n.name === name) as TreeNode;
    expect(deviceSource(byName("Top Renamed"), 2)).toEqual({ kind: "device", path: "D:\\", format: "deviceLibrary", playlist: "3", revision: 2 });
    expect(deviceSource(nodes[2] as TreeNode)).toEqual({ kind: "device", path: "D:\\", format: "deviceLibrary", playlist: "0", revision: 0 });
    expect(deviceSource(byName("Playlists"))).toMatchObject({ kind: "device", playlist: "0" });
    expect(deviceSource(byName("Folder A"))).toEqual({ kind: "folder", path: "" });
    expect(deviceSource(byName("Device Library"))).toEqual({ kind: "folder", path: "" });
    expect(specForNode(byName("Top List"), "", null, "classic", "alphabetical", null, 5).source)
      .toEqual({ kind: "device", path: "D:\\", format: "oneLibrary", playlist: "3", revision: 5 });
    expect(specForNode(stick, "", null).source).toEqual({ kind: "collection" });
  });

  it("offers a library's own playlists to Add To Playlist, and only those", () => {
    const nodes = withDeviceLibraries([stick], new Map([["D:\\", libraries]]));
    expect(devicePlaylistsOf(nodes, "D:\\", "deviceLibrary")).toEqual([
      { id: "2", name: "Inside A" }, { id: "3", name: "Top Renamed" },
    ]);
    expect(devicePlaylistsOf(nodes, "D:\\", "oneLibrary")).toEqual([{ id: "3", name: "Top List" }]);
  });

  it("makes a new playlist inside a folder or under the Playlists heading, and nowhere else", () => {
    const nodes = withDeviceLibraries([stick], new Map([["D:\\", libraries]]));
    expect(deviceParentFor(nodes.find((n) => n.kind === "devicePlaylists") as TreeNode)).toBe("0");
    expect(deviceParentFor(nodes.find((n) => n.name === "Folder A") as TreeNode)).toBe("1");
    expect(deviceParentFor(nodes.find((n) => n.name === "Inside A") as TreeNode)).toBeNull();
    expect(deviceParentFor(stick)).toBeNull();
  });
});
