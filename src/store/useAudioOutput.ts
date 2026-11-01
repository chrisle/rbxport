/**
 * The remembered output device, handed to the engine.
 *
 * The choice lives in Preferences › Audio (`outputDevice`); the engine starts
 * on the system default and builds its stream on the first thing played, so
 * the choice is pushed at start, before anything can play, and again on every
 * change. A device that is not there is not an error: the engine opens the
 * default instead, and the stored choice is left alone for the next time it
 * is.
 */
import { useEffect } from "react";

import { getBackend } from "@/ipc/client";

export function useAudioOutput(outputDevice: string | null): void {
  useEffect(() => {
    void (async () => {
      try {
        const backend = await getBackend();
        await backend.setAudioDevice(outputDevice);
      } catch {
        // No engine behind this build; the choice is still remembered.
      }
    })();
  }, [outputDevice]);
}
