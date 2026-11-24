/** Colour comments belong to a master library or one selected device database.
 * Only mounted owners are cached; requests are shared and stale answers ignored. */
import { useCallback, useSyncExternalStore } from "react";
import { getBackend } from "@/ipc/client";
import type { DeviceTrackSource } from "@/ipc/types";
import { STOCK_COLOR_NAMES } from "@/lib/colorNames";
export { colorDot, colorNameOf, STOCK_COLOR_NAMES } from "@/lib/colorNames";
export type ColorOwner = Pick<DeviceTrackSource, "path" | "format"> & { revision?: number };
type Entry = { names: readonly string[]; listeners: Set<() => void>; offs: (() => void)[] };
const active = new Map<string, Entry>();
const fallback = () => STOCK_COLOR_NAMES;
function subscribe(key: string, listener: () => void) {
  let entry = active.get(key);
  if (!entry) {
    entry = { names: STOCK_COLOR_NAMES, listeners: new Set(), offs: [] };
    active.set(key, entry);
    const owner = entry;
    void getBackend().then(backend => {
      if (active.get(key) !== owner) return;
      const source = key === "master" ? undefined : JSON.parse(key) as ColorOwner;
      let sequence = 0;
      const changed = () => {
        const request = ++sequence;
        void backend.colorNames(source).then(names => {
          if (active.get(key) !== owner || request !== sequence || names.length !== 8) return;
          if (JSON.stringify(names) === JSON.stringify(owner.names)) return;
          owner.names = names;
          for (const notify of owner.listeners) notify();
        }).catch(() => {}); // An unavailable owner keeps its own fallback.
      };
      owner.offs.push(key === "master" ? backend.onLibraryChanged(changed) : backend.onDevicesChanged(changed));
      if (key === "master") owner.offs.push(backend.onLibraryReady(changed));
      changed();
    }).catch(() => {});
  }
  const owner = entry;
  owner.listeners.add(listener);
  return () => {
    owner.listeners.delete(listener);
    if (!owner.listeners.size) {
      for (const off of owner.offs) off();
      active.delete(key);
    }
  };
}
/** Undefined is master; null is an unbound loose/mixed source with stock fallback. */
export function useColorNames(source?: ColorOwner | null): readonly string[] {
  const key = source === null ? "" : source === undefined ? "master" : JSON.stringify({ path: source.path, format: source.format, revision: source.revision ?? 0 });
  const onChange = useCallback((listener: () => void) => key ? subscribe(key, listener) : () => {}, [key]);
  return useSyncExternalStore(onChange, () => active.get(key)?.names ?? STOCK_COLOR_NAMES, fallback);
}
