/**
 * The Devices tree's sticks and what their own libraries hold.
 *
 * A stick's libraries are read the first time it is opened and kept; they
 * are read again after an edit to them, and whenever the device list
 * changes (an export, a stick plugged in or out), since either can have
 * rewritten them. A stick that is gone is forgotten. Nothing is polled.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { Device, DeviceFormat, DeviceLibrary, DevicePlaylistEdit, DevicePlaylistEditResult, TreeNode } from "@/ipc/types";
import { withDeviceLibraries } from "@/lib/deviceLibrary";
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
  /** Bumped after every edit, so a view of a stick's library opens again. */
  revision: number;
}

export function useDeviceLibraries(devices: readonly Device[]): DeviceLibraries {
  const [loaded, setLoaded] = useState<ReadonlyMap<string, readonly DeviceLibrary[]>>(() => new Map());
  const [revision, setRevision] = useState(0);
  // Sticks asked for and not yet answered, so a double-click on a twisty
  // does not read the same stick twice.
  const pending = useRef<Set<string>>(new Set());
  const known = useRef(loaded);
  useEffect(() => {
    known.current = loaded;
  }, [loaded]);

  const read = useCallback(async (path: string) => {
    pending.current.add(path);
    const backend = await getBackend();
    let found: readonly DeviceLibrary[] = [];
    try {
      found = await backend.deviceLibraries(path);
    } catch {
      // Unreadable, or gone: a stick with nothing under it, as the Explorer
      // shows a folder it may not open.
    }
    pending.current.delete(path);
    setLoaded((current) => new Map(current).set(path, found));
  }, []);

  const expand = useCallback(
    (node: TreeNode) => {
      const path = devicePath(node.id);
      if (path === null || known.current.has(path) || pending.current.has(path)) return;
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
    for (const path of known.current.keys()) {
      if (present.has(path) && !pending.current.has(path)) void read(path);
    }
  }, [devices, read]);

  const edit = useCallback(
    async (path: string, format: DeviceFormat, change: DevicePlaylistEdit) => {
      const backend = await getBackend();
      const result = await backend.devicePlaylistEdit(path, format, change);
      if (result.changed > 0) {
        await read(path);
        setRevision((r) => r + 1);
      }
      return result;
    },
    [read],
  );

  const nodes = useMemo(() => withDeviceLibraries(deviceNodes(devices), loaded), [devices, loaded]);

  return useMemo(() => ({ nodes, expand, edit, revision }), [nodes, expand, edit, revision]);
}
