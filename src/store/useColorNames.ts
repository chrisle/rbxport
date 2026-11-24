/** Colour comments belong to a master library or one selected device database.
 * Only mounted owners are cached; requests are shared and stale answers ignored. */
import { useCallback, useSyncExternalStore } from "react";
import { getBackend } from "@/ipc/client";
import type { DeviceTrackSource } from "@/ipc/types";
import { STOCK_COLOR_NAMES } from "@/lib/colorNames";
export { colorDot, colorNameOf, STOCK_COLOR_NAMES } from "@/lib/colorNames";
export type ColorOwner = Pick<DeviceTrackSource, "path" | "format"> & { revision?: number };
type Entry = { names: readonly string[]; listeners: Set<() => void>; sequence: number; offs: (() => void)[] };
const active = new Map<string, Entry>();
const fallback = () => STOCK_COLOR_NAMES;
async function refresh(key: string, entry: Entry) {
  const sequence = ++entry.sequence;
  try {
    const backend = await getBackend();
    const [path, format] = key === "master" ? [] : JSON.parse(key) as [string, DeviceTrackSource["format"], number];
    const source = path !== undefined && format !== undefined ? { path, format } : undefined;
    const names = await backend.colorNames(source);
    if (active.get(key) !== entry || entry.sequence !== sequence || names.length !== 8) return;
    if (names.every((name, at) => name === entry.names[at])) return;
    entry.names = names;
    for (const listener of entry.listeners) listener();
  } catch { /* Unavailable owners retain their own fallback, never master names. */ }
}
function subscribe(key: string, listener: () => void) {
  let entry = active.get(key);
  if (!entry) {
    entry = { names: STOCK_COLOR_NAMES, listeners: new Set(), sequence: 0, offs: [] };
    active.set(key, entry);
    const owner = entry;
    void getBackend().then(backend => {
      if (active.get(key) !== owner) return;
      const changed = () => { void refresh(key, owner); };
      if (key === "master") {
        owner.offs.push(backend.onLibraryChanged(changed), backend.onLibraryReady(changed));
      } else {
        owner.offs.push(backend.onDevicesChanged(changed));
      }
      changed();
    }).catch(() => {});
  }
  const owner = entry;
  owner.listeners.add(listener);
  return () => {
    owner.listeners.delete(listener);
    if (!owner.listeners.size) {
      owner.sequence++;
      for (const off of owner.offs) off();
      if (active.get(key) === owner) active.delete(key);
    }
  };
}
/** Undefined is master; null is an unbound loose/mixed source with stock fallback. */
export function useColorNames(source?: ColorOwner | null): readonly string[] {
  const key = source === null ? "" : source === undefined ? "master" : JSON.stringify([source.path, source.format, source.revision ?? 0]);
  const onChange = useCallback((listener: () => void) => key ? subscribe(key, listener) : () => {}, [key]);
  const snapshot = useCallback(() => active.get(key)?.names ?? STOCK_COLOR_NAMES, [key]);
  return useSyncExternalStore(onChange, snapshot, fallback);
}
