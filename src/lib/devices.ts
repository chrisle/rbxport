/**
 * Connected volumes, as the tree and the device panel need them.
 *
 * Kept apart from the components because the interesting parts — what a stick
 * already holds, how full it is, and whether the next export can be a sync —
 * are all decisions rather than rendering.
 */
import type { Device, TreeNode } from "@/ipc/types";
import { formatBytes } from "./format";

/** A device's id in the tree. Prefixed so it cannot collide with a playlist. */
export function deviceId(device: Device): string {
  return `device:${device.path}`;
}

/** The mount point behind a tree id, or null when the id is not a device. */
export function devicePath(id: string): string | null {
  return id.startsWith("device:") ? id.slice("device:".length) : null;
}

/**
 * The device a stale path now lives at, when it was renamed rather than
 * unplugged: the one in the fresh list with the same volume id, if any. Its
 * old name is what the path ended in, for saying what happened.
 */
export function renamedDevice(
  devices: readonly Device[],
  stalePath: string,
  previous: readonly Device[],
): { device: Device; oldName: string } | null {
  const before = previous.find((device) => device.path === stalePath);
  if (before === undefined) return null;
  const device = devices.find((d) => d.volumeId === before.volumeId && d.path !== stalePath);
  return device ? { device, oldName: before.name } : null;
}

/**
 * Devices as tree nodes, so the Devices section renders like every other
 * section rather than needing its own list component.
 */
export function deviceNodes(devices: readonly Device[]): TreeNode[] {
  return devices.map((device) => ({
    id: deviceId(device),
    name: device.name,
    kind: "device" as const,
    depth: 0,
  }));
}

/** `12.4 GB free of 32.0 GB`, or nothing when the size is not known. */
export function capacityText(device: Device): string {
  if (device.totalBytes <= 0) return "";
  return `${formatBytes(device.freeBytes)} free of ${formatBytes(device.totalBytes)}`;
}

/**
 * `1,430.3 GB` — the space table's figure: gibibytes to one decimal with a
 * thousands separator, which is what rekordbox prints for a 1.5 TB card
 * [OBS: capture 9.08.22 PM]. Empty when the size is not known.
 */
export function formatSpace(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  const gb = (bytes / 1024 ** 3).toFixed(1);
  const [whole = "", fraction = "0"] = gb.split(".");
  return `${whole.replace(/\B(?=(\d{3})+(?!\d))/g, ",")}.${fraction} GB`;
}

/** How full the volume is, 0 to 1, or null when the size is not known. */
export function fullness(device: Device): number | null {
  if (device.totalBytes <= 0) return null;
  const used = Math.max(0, device.totalBytes - device.freeBytes);
  return Math.min(1, used / device.totalBytes);
}

/** What is already on the volume, in a sentence. */
export function contentsText(device: Device): string {
  const found = device.export;
  if (device.busy) return "Being written to now. What it holds shows when the sync finishes.";
  if (!found) return "No export on this device yet.";
  const tracks = `${found.tracks} track${found.tracks === 1 ? "" : "s"}`;
  const playlists = `${found.playlists} playlist${found.playlists === 1 ? "" : "s"}`;
  if (!found.ours) {
    // Without our manifest there is nothing to diff against, so the next
    // export rewrites the stick. Saying so beforehand beats surprising
    // someone with a twenty-minute copy.
    return `${tracks} in ${playlists}, written by rekordbox. Exporting here writes everything again.`;
  }
  const when = found.written.slice(0, 16);
  return `${tracks} in ${playlists}, last synced ${when}. Only what changed will be copied.`;
}

/**
 * Whether there is enough room for a rough estimate of what an export needs.
 *
 * An estimate, and shown as one: the real figure depends on what is already
 * there, which is only known once the sync has diffed.
 */
export function hasRoomFor(device: Device, bytes: number): boolean {
  return device.totalBytes <= 0 || bytes <= device.freeBytes;
}
