import { useState } from "react";
import { getBackend } from "@/ipc/client";
import { useTranslation } from "@/i18n";

export function StopExport({ path, className }: { path: string; className?: string | undefined }) {
  const t = useTranslation();
  const [stopping, setStopping] = useState(false);
  const [error, setError] = useState("");
  const stop = () => {
    setStopping(true);
    setError("");
    void getBackend().then(backend => backend.cancelExport(path)).catch(() => {
      setStopping(false);
      setError(t("Could not stop the export. Try again."));
    });
  };
  return <>
    <button type="button" className={className} onClick={stop} disabled={stopping}
      aria-label={t("Stop export to {device}", { device: path })} title={t("Stop after the current file operation finishes")}>
      {stopping ? t("Stopping…") : t("Stop")}
    </button>
    {error ? <span role="alert">{error}</span> : null}
  </>;
}
