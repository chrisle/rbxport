/**
 * The Preferences as a window of its own.
 *
 * The shell opens a second webview at `index.html#preferences/<pane>`, and
 * this is what it renders: the same Preferences the browser build draws over
 * itself, filling a window the platform gives a title bar to drag by. Its
 * choices go to the storage both windows share; what only the main window
 * holds — the columns, the panes' widths — is asked for over an event.
 */
import { useCallback, useEffect, useState } from "react";

import { getBackend } from "@/ipc/client";
import { useShowWindowWhenReady } from "@/lib/windowReady";
import type { LibrarySummary } from "@/ipc/types";
import { useLimiter } from "@/store/useLimiter";
import { useMaster } from "@/store/useMaster";
import { PreferencesProvider, usePreferencesStore } from "@/store/usePreferences";
import { asTarget, Preferences, type PreferencesTarget } from "./Preferences";

/** The pane, or place in one, the hash names: `#preferences/advanced` is Advanced. */
export function paneFromHash(hash: string): PreferencesTarget {
  return asTarget(hash.replace(/^#preferences\/?/, "").split(/[/?]/)[0]);
}

export function PreferencesWindow() {
  const store = usePreferencesStore();
  const [pane, setPane] = useState<PreferencesTarget>(() => paneFromHash(window.location.hash));
  const [summary, setSummary] = useState<LibrarySummary | null>(null);
  // The window shows once the first library read has answered, so the
  // library facts and backup controls are there when it appears.
  const [firstRead, setFirstRead] = useState(false);
  useShowWindowWhenReady(firstRead);
  // The limiter is the engine's, so this window reads and sets it the same
  // way the shell does, and its meter ticks arrive here as they do there.
  const limiter = useLimiter();
  const master = useMaster(store.preferences.view.vuMeter);

  const [navigation, setNavigation] = useState(0);

  // Turned to another pane by the shell while open: it sets the hash.
  useEffect(() => {
    const onHash = () => {
      setPane(paneFromHash(window.location.hash));
      setNavigation((value) => value + 1);
    };
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  // This window has no shell polling the process state for it. Keep the
  // backup controls and library facts current while Preferences stays open.
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        const backend = await getBackend();
        if (!live) return;
        const read = await backend.librarySummary();
        if (live) setSummary(read);
      } catch {
        // Keep the last known status while the library is unavailable.
      } finally {
        if (live) {
          setFirstRead(true);
          timer = setTimeout(() => void refresh(), 2000);
        }
      }
    };
    void refresh();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, []);

  const close = useCallback(() => {
    void getBackend().then((backend) => backend.closeWindow());
  }, []);
  const ask = useCallback((what: "columns" | "layout") => {
    void getBackend().then((backend) => backend.requestPreferencesReset(what));
  }, []);

  return (
    <PreferencesProvider value={store}>
      <Preferences
        windowed
        summary={summary}
        limiter={limiter.limiter}
        onLimiterChange={limiter.set}
        reduction={master.reduction}
        vu={master.vu}
        peakLeft={master.peakLeft}
        peakRight={master.peakRight}
        key={navigation}
        initialPane={pane}
        onResetColumns={() => ask("columns")}
        onResetLayout={() => ask("layout")}
        onClose={close}
      />
    </PreferencesProvider>
  );
}
