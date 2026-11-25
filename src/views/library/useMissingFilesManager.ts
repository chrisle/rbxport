import { useCallback, useEffect, useRef, useState } from "react";
import { getBackend } from "@/ipc/client";

/** The main window's existing manager, observed before navigation is accepted. */
export function useMissingFilesManager(prepare?: () => Promise<unknown>) {
  const [open, setOpen] = useState(false);
  const shown = useRef(false);
  const version = useRef(0);
  const mounted = useRef(true);
  const waiting = useRef<{ promise: Promise<void>; resolve: () => void; reject: () => void } | null>(null);
  const close = useCallback(() => {
    version.current++;
    shown.current = false;
    waiting.current?.reject();
    waiting.current = null;
    setOpen(false);
  }, []);
  const show = useCallback(async () => {
    version.current++;
    if (prepare) await prepare();
    if (!mounted.current) throw new Error("Missing file manager owner closed");
    setOpen(true);
    if (shown.current) return Promise.resolve();
    if (!waiting.current) {
      let resolve!: () => void;
      let reject!: () => void;
      const promise = new Promise<void>((yes, no) => { resolve = yes; reject = () => no(new Error("Missing file manager closed before opening")); });
      waiting.current = { promise, resolve, reject };
    }
    return waiting.current.promise;
  }, [prepare]);
  const ready = useCallback(() => {
    shown.current = true;
    waiting.current?.resolve();
    waiting.current = null;
  }, []);
  useEffect(() => {
    mounted.current = true;
    let live = true;
    let stop: (() => void) | undefined;
    void getBackend().then(backend => {
      if (!live) return;
      stop = backend.onMissingFilesRequested(id => {
        if (!live) return;
        const alreadyShown = shown.current;
        const opening = show();
        const mine = version.current;
        void opening.then(async () => {
          if (!live) return;
          const accepted = await backend.missingFilesReady(id, true);
          if (live && !accepted && !alreadyShown && mine === version.current) close();
        }).catch(async () => {
          if (live) {
            await backend.missingFilesReady(id, false).catch(() => {});
            if (!alreadyShown && mine === version.current) close();
          }
        });
      });
    });
    return () => { live = false; mounted.current = false; stop?.(); shown.current = false; waiting.current?.reject(); waiting.current = null; };
  }, [show, close]);
  return { open, show, close, ready };
}
