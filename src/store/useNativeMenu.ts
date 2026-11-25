import { useEffect, useRef, useState } from "react";
import { getBackend } from "@/ipc/client";
import { nativeMenuLabels } from "@/lib/nativeMenu";

/** Main window owns the menu; serialize language and preset updates together. */
export function useNativeMenu(t: (source: string) => string, performance1: boolean): boolean {
  const pending = useRef(Promise.resolve());
  const [applied, setApplied] = useState<boolean | null>(null);
  useEffect(() => {
    let live = true;
    pending.current = pending.current.catch(() => undefined).then(async () => {
      if (!live) return;
      const backend = await getBackend();
      if (!live) return;
      await backend.setMenuLabels(nativeMenuLabels(t), performance1);
      if (live) setApplied(performance1);
    }).catch(() => undefined);
    return () => { live = false; };
  }, [t, performance1]);
  return applied === performance1;
}
