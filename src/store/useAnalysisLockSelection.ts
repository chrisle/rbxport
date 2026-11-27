import { useEffect, useMemo, useState } from "react";
import { getBackend } from "@/ipc/client";

/** Native menu metadata only; commands keep their existing authoritative write checks. */
export function useAnalysisLockSelection(ids: ReadonlySet<string>, owner: unknown, enabled: boolean): boolean {
  const evaluation = useMemo(() => ({ ids, owner, enabled }), [ids, owner, enabled]);
  const [snapshot, setSnapshot] = useState<{ evaluation: object; locked: boolean } | null>(null);
  useEffect(() => {
    if (!enabled || ids.size === 0) return;
    let live = true;
    let request = 0;
    let stopGrid: (() => void) | undefined;
    let stopLibrary: (() => void) | undefined;
    const refresh = async () => {
      const current = ++request;
      setSnapshot(null);
      try {
        const backend = await getBackend();
        if (!live || current !== request) return;
        const locked = await backend.anyAnalysisLocked([...ids]);
        if (live && current === request) setSnapshot({ evaluation, locked });
      } catch {
        // An unknown lock state must not advertise an unlock operation.
        if (live && current === request) setSnapshot({ evaluation, locked: false });
      }
    };
    const focus = () => { void refresh(); };
    window.addEventListener("focus", focus);
    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      stopGrid = backend.onGridChanged((id) => { if (ids.has(id)) void refresh(); });
      stopLibrary = backend.onLibraryChanged(() => { void refresh(); });
      await refresh();
    })().catch(() => {});
    return () => {
      live = false;
      window.removeEventListener("focus", focus);
      stopGrid?.();
      stopLibrary?.();
    };
  }, [ids, evaluation, enabled]);
  return enabled && snapshot?.evaluation === evaluation && snapshot.locked;
}
