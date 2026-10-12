import { useEffect, useState } from "react";
import type { ExportProgress } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import { canStopExport, isExportActive, stageLabel, stagePercent } from "@/lib/exportProgress";
import { StopExport } from "./StopExport";

export function ExportProgressDetails({ job, name, buttonClass }: { job: ExportProgress; name: string; buttonClass?: string | undefined }) {
  const t = useTranslation();
  const [sinceUpdate, setSinceUpdate] = useState(0);
  useEffect(() => {
    setSinceUpdate(0);
    if (!isExportActive(job)) return;
    const received = Date.now();
    const timer = window.setInterval(() => setSinceUpdate(Date.now() - received), 1000);
    return () => window.clearInterval(timer);
  }, [job]);
  const percent = stagePercent(job);
  const bytes = (value: number) => value >= 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(1)} GB` : `${(value / 1024 ** 2).toFixed(1)} MB`;
  return <>
    <progress data-state={job.state} aria-label={t("Exporting {device}", { device: name })} max={100} value={percent} />
    <span aria-live="polite">{stageLabel(job, t)}{job.title && job.state !== "failed" ? ` · ${job.title}` : ""}{percent === undefined ? "" : ` (${percent}%)`}</span>
    {job.unit === "bytes" && (job.bytesTotal ?? 0) > 0 ? <span>{job.state === "checking"
      ? t("{done} of {total} checked", { done: bytes(job.bytesDone ?? 0), total: bytes(job.bytesTotal ?? 0) })
      : t("{done} of {total} copied", { done: bytes(job.bytesDone ?? 0), total: bytes(job.bytesTotal ?? 0) })}</span> : null}
    {job.unit === "milliseconds" ? <span>{t("{seconds} seconds processed", { seconds: Math.floor(job.done / 1000) })}</span> : null}
    {(job.reused ?? 0) + (job.copied ?? 0) + (job.converted ?? 0) > 0 ? <span>{t("{reused} reused · {copied} copied · {converted} converted", { reused: job.reused ?? 0, copied: job.copied ?? 0, converted: job.converted ?? 0 })}</span> : null}
    {(job.bytesPerSecond ?? 0) > 0 ? <span>{t("{rate}/s", { rate: bytes(job.bytesPerSecond ?? 0) })}</span> : null}
    {(job.elapsedMs ?? 0) + sinceUpdate > 1000 ? <span>{t("{seconds} seconds elapsed", { seconds: Math.floor(((job.elapsedMs ?? 0) + sinceUpdate) / 1000) })}</span> : null}
    {canStopExport(job) ? <StopExport path={job.path} className={buttonClass} /> : null}
  </>;
}
