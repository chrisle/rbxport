/**
 * A USB stick's own libraries in the Devices tree.
 *
 * rekordbox lists each library a stick holds under the stick, Device Library
 * (`export.pdb`) and OneLibrary (`exportLibrary.db`), and under each its All
 * Tracks and its Playlists [static, rekordbox 7.2.11 macOS:
 * `BrowseProperty::isDeviceLibraryRoot` / `isDeviceLibraryPlusRoot` at tree
 * depth 3, `isDeviceAllTracks` and `isDevicePlaylistsRoot` at depth 4]. An
 * edit there changes the library it was made in and not the other [DOC:
 * rekordbox FAQ "Device Library Plus"]. Hot Cue Bank Lists and Histories,
 * which rekordbox also lists there, are not shown.
 *
 * Pure functions over plain data, so the shape is testable without a backend.
 */
import type { DeviceFormat, DeviceLibrary, TreeNode, ViewSpec } from "@/ipc/types";
import { devicePath } from "./devices";

/** What a node under a stick is. */
export type DeviceNodeRole = "library" | "allTracks" | "playlists" | "node";

export interface DeviceNodeRef {
  /** The stick's mount point. */
  path: string;
  format: DeviceFormat;
  role: DeviceNodeRole;
  /** The playlist or folder's id in that library; `"0"` for the headings. */
  id: string;
}

const PREFIX = "devlib:";
const ROLES: Record<DeviceNodeRole, string> = { library: "lib", allTracks: "all", playlists: "root", node: "node" };
const FORMATS: Record<DeviceFormat, string> = { deviceLibrary: "pdb", oneLibrary: "one" };
const ID = /^devlib:(pdb|one):(lib|all|root|node):(\d+):([^]*)$/;

/** A node's id: what it is, in which library, on which stick. The path goes last, since it may hold colons. */
export function deviceNodeId(ref: DeviceNodeRef): string {
  return `${PREFIX}${FORMATS[ref.format]}:${ROLES[ref.role]}:${ref.id}:${ref.path}`;
}

/** The parts of a node id, or null for an id that is not one. */
export function parseDeviceNodeId(id: string): DeviceNodeRef | null {
  const match = ID.exec(id);
  if (!match) return null;
  const [, format = "", role = "", node = "", path = ""] = match;
  return {
    path,
    format: format === "pdb" ? "deviceLibrary" : "oneLibrary",
    role: (Object.keys(ROLES) as DeviceNodeRole[]).find((r) => ROLES[r] === role) ?? "node",
    id: node,
  };
}

/** The kinds the Devices tree draws under a stick. */
export function isDeviceLibraryKind(kind: TreeNode["kind"]): boolean {
  return kind === "deviceLibrary" || kind === "deviceAllTracks" || kind === "devicePlaylists" ||
    kind === "deviceFolder" || kind === "devicePlaylist";
}

/**
 * rekordbox's own questions before a stick's playlist, folder or entries go,
 * word for word [OBS 7.2.14, Winrig 2026-10-08: `rekordbox-19` for a
 * playlist, `rekordbox-13` for Remove from Playlist; the folder's is its
 * sibling in rekordbox's catalog, `[ASSUME]` shown the same way]. Kept as
 * they are so each language gets rekordbox's own translation from its
 * `.lang` catalog (`pnpm locales`).
 */
export const DEVICE_ASKS = {
  deletePlaylist: "Are you sure you want to delete this playlist?\nPlaylist will be deleted from all synced devices.",
  deleteFolder: "Are you sure you want to delete this folder?\nFolder will be deleted from all synced devices.",
  removeTracks: "Are you sure you want to remove the selected track(s) from the playlist?\nTrack(s) will be removed from the playlists of all synced devices.",
} as const;

/** A library's name in the tree, as rekordbox spells it. */
export const LIBRARY_NAMES: Record<DeviceFormat, string> = {
  deviceLibrary: "Device Library",
  oneLibrary: "OneLibrary",
};

/**
 * The Devices section: each stick, and under a stick that has been opened,
 * its libraries. A stick is a branch whether or not it has been read, so it
 * can be opened; what is under it is read then and not before.
 */
export function withDeviceLibraries(
  devices: readonly TreeNode[],
  loaded: ReadonlyMap<string, readonly DeviceLibrary[]>,
): TreeNode[] {
  const out: TreeNode[] = [];
  for (const device of devices) {
    out.push({ ...device, lazy: true, expanded: false });
    const path = devicePath(device.id);
    const libraries = path === null ? undefined : loaded.get(path);
    if (path === null || !libraries) continue;
    for (const library of libraries) {
      const ref = (role: DeviceNodeRole, id = "0") => deviceNodeId({ path, format: library.format, role, id });
      out.push({ id: ref("library"), name: LIBRARY_NAMES[library.format], kind: "deviceLibrary", depth: device.depth + 1 });
      out.push({ id: ref("allTracks"), name: "All Tracks", kind: "deviceAllTracks", depth: device.depth + 2, childCount: library.tracks });
      out.push({ id: ref("playlists"), name: "Playlists", kind: "devicePlaylists", depth: device.depth + 2 });
      for (const node of library.nodes) {
        out.push({
          id: ref("node", node.id),
          name: node.name,
          kind: node.folder ? "deviceFolder" : "devicePlaylist",
          depth: device.depth + 3 + node.depth,
          childCount: node.count,
          // Folders arrive closed, as the collection's do in a fresh tree.
          ...(node.folder ? { expanded: false } : {}),
        });
      }
    }
  }
  return out;
}

/**
 * The view a node under a stick opens: a playlist's tracks, or every track
 * of the library under All Tracks and under the Playlists heading, which
 * rekordbox fills with the library's tracks too [OBS 7.2.14, Winrig
 * 2026-10-08: "Playlists (4 Tracks)" on a four-track stick]. The library's
 * own heading and a folder open empty [UNKNOWN: not captured].
 */
export function deviceSource(node: TreeNode, revision = 0): ViewSpec["source"] | null {
  const ref = parseDeviceNodeId(node.id);
  if (!ref) return null;
  const playlist = node.kind === "deviceAllTracks" || node.kind === "devicePlaylists" ? "0" : node.kind === "devicePlaylist" ? ref.id : null;
  if (playlist === null) return { kind: "folder", path: "" };
  return { kind: "device", path: ref.path, format: ref.format, playlist, revision };
}

/** A library's playlists, by node, for an Add To Playlist submenu. */
export function devicePlaylistsOf(
  nodes: readonly TreeNode[],
  path: string,
  format: DeviceFormat,
): { id: string; name: string }[] {
  return nodes.flatMap((node) => {
    if (node.kind !== "devicePlaylist") return [];
    const ref = parseDeviceNodeId(node.id);
    return ref && ref.path === path && ref.format === format ? [{ id: ref.id, name: node.name }] : [];
  });
}

/**
 * Where a new playlist or folder made from `node`'s menu goes in its
 * library: inside a folder, under the Playlists heading for the heading
 * itself. `null` for a node no playlist can be made under.
 */
export function deviceParentFor(node: TreeNode): string | null {
  const ref = parseDeviceNodeId(node.id);
  if (!ref) return null;
  if (node.kind === "devicePlaylists") return "0";
  if (node.kind === "deviceFolder") return ref.id;
  return null;
}
