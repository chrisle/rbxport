/**
 * The eight track colours' names, as the library has them.
 *
 * rekordbox lets a library rename its colours (Preferences › Advanced ›
 * Browse › Color); the names are `djmdColor.Commnt`, read by `rbl-index` with
 * the library. One store for every list that names a colour: fetched once
 * when the first one mounts and again whenever the library changes, so a
 * rename shows everywhere without each view asking.
 */
import { useSyncExternalStore } from "react";

import { getBackend } from "@/ipc/client";
import { STOCK_COLOR_NAMES } from "@/lib/colorNames";

export { colorDot, colorNameOf, STOCK_COLOR_NAMES } from "@/lib/colorNames";

let names: readonly string[] = STOCK_COLOR_NAMES;
const listeners = new Set<() => void>();
let stop: (() => void) | null = null;

async function fetchNames(): Promise<void> {
  try {
    const backend = await getBackend();
    const fetched = await backend.colorNames();
    if (fetched.length !== STOCK_COLOR_NAMES.length) return;
    if (fetched.every((name, at) => name === names[at])) return;
    names = fetched;
    for (const listener of listeners) listener();
  } catch {
    // No library yet: the stock names stand until it loads.
  }
}

function start(): void {
  let live = true;
  const offs: (() => void)[] = [];
  stop = () => {
    live = false;
    for (const off of offs) off();
  };
  void (async () => {
    try {
      const backend = await getBackend();
      if (!live) return;
      offs.push(backend.onLibraryChanged(() => void fetchNames()));
      offs.push(backend.onLibraryReady(() => void fetchNames()));
    } catch {
      return;
    }
    await fetchNames();
  })();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (stop === null) start();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      stop?.();
      stop = null;
      names = STOCK_COLOR_NAMES;
    }
  };
}

const snapshot = () => names;

/** The eight names, 1 to 8 at indices 0 to 7. */
export function useColorNames(): readonly string[] {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}
