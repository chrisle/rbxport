/**
 * The preferences, held once at the top and read anywhere.
 *
 * A context rather than a prop threaded through every view: a tooltip in the
 * deck and the key column in the browser both read the same choice, and
 * neither has any business knowing where the other lives.
 */
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

import {
  DEFAULT_PREFERENCES,
  loadPreferences,
  PREFERENCES_KEY,
  savePreferences,
  storedPreferences,
  type PreferencePane,
  type Preferences,
} from "@/lib/preferences";

export interface PreferencesStore {
  preferences: Preferences;
  /** Changes some of one pane's choices, keeping the rest. */
  update: <P extends PreferencePane>(pane: P, patch: Partial<Preferences[P]>) => void;
  /** The pane's "Reset to defaults" button. */
  reset: (pane: PreferencePane) => void;
}

const PreferencesContext = createContext<PreferencesStore>({
  preferences: DEFAULT_PREFERENCES,
  update: () => {},
  reset: () => {},
});

export const PreferencesProvider = PreferencesContext.Provider;

/** Owns the preferences: read once at start, written on every change. */
export function usePreferencesStore(initial?: Preferences): PreferencesStore {
  const [preferences, setPreferences] = useState<Preferences>(() => initial ?? loadPreferences());

  const update = useCallback(
    <P extends PreferencePane>(pane: P, patch: Partial<Preferences[P]>) => {
      setPreferences((current) => {
        // On top of what is stored now, not this window's copy: another
        // window's choices since then stay as that window left them.
        const base = storedPreferences() ?? current;
        const next = { ...base, [pane]: { ...base[pane], ...patch } };
        savePreferences(next);
        window.dispatchEvent(new CustomEvent("rbl-preferences"));
        return next;
      });
    },
    [],
  );

  const reset = useCallback((pane: PreferencePane) => {
    setPreferences((current) => {
      const base = storedPreferences() ?? current;
      const next = { ...base, [pane]: DEFAULT_PREFERENCES[pane] };
      savePreferences(next);
      return next;
    });
  }, []);

  // The Preferences window is a window of its own with the same storage: a
  // choice made there lands here as a `storage` event, which only ever fires
  // in the other windows. Read back rather than parsed from the event, so it
  // goes through the same checks as a stored set does at start.
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key === PREFERENCES_KEY || event.key === null) setPreferences(loadPreferences());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  // Memoised as a whole so a consumer re-renders on a change and not on
  // every render of the owner.
  return useMemo(() => ({ preferences, update, reset }), [preferences, update, reset]);
}

export function usePreferencesContext(): PreferencesStore {
  return useContext(PreferencesContext);
}

export function usePreferences(): Preferences {
  return useContext(PreferencesContext).preferences;
}

/**
 * The `title` a control should carry: its tooltip, or nothing when the
 * window has turned tooltips off.
 */
export function useTooltip(): (text: string | undefined) => string | undefined {
  const on = usePreferences().view.tooltips;
  return useCallback((text: string | undefined) => (on ? text : undefined), [on]);
}
