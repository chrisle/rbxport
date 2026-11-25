/**
 * The Devices tree's sticks and what their own libraries hold.
 *
 * A stick's libraries are read the first time it is opened and kept; they
 * are read again after an edit to them, and whenever the device list
 * changes (an export, a stick plugged in or out), since either can have
 * rewritten them. A stick that is gone is forgotten. Nothing is polled.
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { Device, DeviceFormat, DeviceLibrary, DevicePlaylistEdit, DevicePlaylistEditResult, TreeNode } from "@/ipc/types";
import { withDeviceLibraries } from "@/lib/deviceLibrary";
import { errorMessage } from "@/lib/errorMessage";
import { deviceNodes, devicePath } from "@/lib/devices";

export interface DeviceLibraries {
  /** The Devices section: each stick, and under an opened one its libraries. */
  nodes: TreeNode[];
  /** A stick was opened: read its libraries, once. */
  expand: (node: TreeNode) => void;
  /**
   * Changes one library's playlists on a stick, then reads the stick again.
   * Rejects with the backend's reason, having changed nothing.
   */
  edit: (path: string, format: DeviceFormat, edit: DevicePlaylistEdit) => Promise<DevicePlaylistEditResult>;
  refresh: (path: string) => Promise<void>;
  /** Bumped after every edit, so a view of a stick's library opens again. */
  revision: number;
}

interface DeviceOwner { volumeId: string }
interface LibraryRead { owner: DeviceOwner }
interface LoadedLibraries { owner: DeviceOwner; libraries: readonly DeviceLibrary[] }

export function useDeviceLibraries(devices: readonly Device[], onError: (message: string) => void): DeviceLibraries {
  const [loaded, setLoaded] = useState<ReadonlyMap<string, LoadedLibraries>>(() => new Map());
  const [owners, setOwners] = useState<ReadonlyMap<string, DeviceOwner>>(() =>
    new Map(devices.map(device => [device.path, { volumeId: device.volumeId }])));
  const current = useRef<ReadonlyMap<string, DeviceOwner> | null>(null);
  const [revision, setRevision] = useState(0);
  // Sticks asked for and not yet answered, so a double-click on a twisty
  // does not read the same stick twice.
  const pending = useRef(new Map<string, LibraryRead>());
  // Only committed devices own replies. A return to the same mount spelling
  // after removal gets a new owner, even when its volume ID is unchanged.
  useLayoutEffect(() => {
    const previous = current.current ?? owners;
    const next = new Map(devices.map(device => {
      const owner = previous.get(device.path);
      return [device.path, owner?.volumeId === device.volumeId ? owner : { volumeId: device.volumeId }] as const;
    }));
    current.current = next;
    for (const [path, request] of pending.current) {
      if (next.get(path) !== request.owner) pending.current.delete(path);
    }
    if (owners.size !== next.size || [...next].some(([path, owner]) => owners.get(path) !== owner)) setOwners(next);
  }, [devices, owners]);
  useLayoutEffect(() => () => {
    current.current = null;
    pending.current.clear();
  }, []);
  const known = useRef(loaded);
  useEffect(() => {
    known.current = loaded;
  }, [loaded]);

  const read = useCallback(async (path: string) => {
    const owner = current.current?.get(path);
    if (!owner) return false;
    const request = { owner };
    pending.current.set(path, request);
    const isCurrent = () => current.current?.get(path) === owner && pending.current.get(path) === request;
    let found: readonly DeviceLibrary[] = [];
    let published = false;
    try {
      const backend = await getBackend();
      if (!isCurrent()) return false;
      found = await backend.deviceLibraries(path);
    } catch (error) {
      if (isCurrent()) onError(errorMessage(error));
    } finally {
      if (isCurrent()) {
        pending.current.delete(path);
        setLoaded(previous => new Map(previous).set(path, { owner, libraries: found }));
        published = true;
      }
    }
    return published;
  }, [onError]);

  const expand = useCallback(
    (node: TreeNode) => {
      const path = devicePath(node.id);
      if (path === null) return;
      const owner = current.current?.get(path);
      if (!owner || known.current.get(path)?.owner === owner || pending.current.get(path)?.owner === owner) return;
      void read(path);
    },
    [read],
  );

  // A stick that left is forgotten; one still here is read again, since the
  // list changes when an export has just rewritten it.
  useEffect(() => {
    const present = new Set(devices.map((d) => d.path));
    setLoaded((current) => {
      const kept = new Map([...current].filter(([path]) => present.has(path)));
      return kept.size === current.size ? current : kept;
    });
    let refreshed = false;
    for (const path of known.current.keys()) {
      if (present.has(path) && pending.current.get(path)?.owner !== current.current?.get(path)) {
        refreshed = true;
        void read(path);
      }
    }
    if (refreshed) setRevision(current => current + 1);
  }, [devices, read]);

  const edit = useCallback(
    async (path: string, format: DeviceFormat, change: DevicePlaylistEdit) => {
      const owner = current.current?.get(path);
      const backend = await getBackend();
      const result = await backend.devicePlaylistEdit(path, format, change);
      if (result.changed > 0 && current.current?.get(path) === owner) {
        if (await read(path)) setRevision((r) => r + 1);
      }
      return result;
    },
    [read],
  );

  const refresh = useCallback(async (path: string) => {
    if (await read(path)) setRevision((revision) => revision + 1);
  }, [read]);

  const nodes = useMemo(() => {
    const visible = new Map<string, readonly DeviceLibrary[]>();
    for (const device of devices) {
      const found = loaded.get(device.path);
      if (found?.owner === owners.get(device.path) && found?.owner.volumeId === device.volumeId) {
        visible.set(device.path, found.libraries);
      }
    }
    return withDeviceLibraries(deviceNodes(devices), visible);
  }, [devices, loaded, owners]);

  return useMemo(() => ({ nodes, expand, edit, refresh, revision }), [nodes, expand, edit, refresh, revision]);
}
