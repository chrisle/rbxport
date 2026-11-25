import { useCallback, useEffect, useRef, useState } from "react";
import { getBackend } from "@/ipc/client";

/** The latest request owns the native result or its inline fallback. */
export function useReportWindow() {
  const [reportOpen, setReportOpen] = useState(false);
  const request = useRef(0);
  const retire = useCallback(() => { request.current++; }, []);
  useEffect(() => retire, [retire]);
  const closeReport = useCallback(() => {
    retire();
    setReportOpen(false);
  }, [retire]);
  const openReport = useCallback(() => {
    const generation = ++request.current;
    void getBackend().then(backend => {
      if (request.current !== generation) return;
      return backend.openReportWindow();
    }).then(opened => {
      if (request.current === generation && opened === false) setReportOpen(true);
    }).catch(() => {
      if (request.current === generation) setReportOpen(true);
    });
  }, []);
  return { reportOpen, openReport, closeReport };
}
