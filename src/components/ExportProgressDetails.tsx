import { useEffect, useState } from "react";
import type { ExportProgress } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import { canStopExport, formatElapsed, isExportActive, stageLabel, stagePercent } from "@/lib/exportProgress";
import { StopExport } from "./StopExport";
import styles from "./ExportProgressDetails.module.css";

/**
 * A device's export as three fixed lines under the bar: the stage and how far
 * through it, the track it is on, then the current file's figures and the time
 * taken. Every line is always drawn, so the block keeps its height as stages
 * come and go.
 */
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
  const elapsed = (job.elapsedMs ?? 0) + sinceUpdate;
  // The track this file belongs to, where the stage goes track by track.
  const position = (job.tracksTotal ?? 0) > 0 ? { done: (job.tracksDone ?? 0) + 1, total: job.tracksTotal ?? 0 }
    : job.unit === "tracks" && ["planning", "copying"].includes(job.state) && job.total > 0 ? { done: Math.min(job.total, job.done + 1), total: job.total }
    : undefined;
  const stage = [stageLabel(job, t), position ? t("Track {done} of {total}", position) : ""].filter(Boolean).join(" · ");
  // A failed export's title is its error, which Sync Manager shows itself.
  const title = job.state === "failed" ? "" : job.title;
  const figures = [
    job.unit === "bytes" && (job.bytesTotal ?? 0) > 0 ? job.state === "checking"
      ? t("{done} of {total} checked", { done: bytes(job.bytesDone ?? 0), total: bytes(job.bytesTotal ?? 0) })
      : t("{done} of {total} copied", { done: bytes(job.bytesDone ?? 0), total: bytes(job.bytesTotal ?? 0) }) : "",
    job.unit === "milliseconds" ? t("{seconds} seconds processed", { seconds: Math.floor(job.done / 1000) }) : "",
    job.unit === "bytes" && (job.bytesPerSecond ?? 0) > 0 ? t("{rate}/s", { rate: bytes(job.bytesPerSecond ?? 0) }) : "",
    (job.reused ?? 0) + (job.copied ?? 0) + (job.converted ?? 0) > 0 ? t("{reused} reused · {copied} copied · {converted} converted", { reused: job.reused ?? 0, copied: job.copied ?? 0, converted: job.converted ?? 0 }) : "",
  ].filter(Boolean).join(" · ");
  return <div className={styles.progress} data-state={job.state}>
    <progress data-state={job.state} aria-label={t("Exporting {device}", { device: name })} max={100} value={percent} />
    <div className={styles.line}>
      <span className={styles.text} aria-live="polite">{stage}</span>
      <span className={styles.aside}>{percent === undefined ? "" : `${percent}%`}</span>
    </div>
    <div className={styles.line}>
      <span className={styles.text} title={title || undefined}>{title}</span>
    </div>
    <div className={`${styles.line} ${styles.figures}`}>
      <span className={styles.text} title={figures || undefined}>{figures}</span>
      <span className={styles.aside}>{elapsed >= 1000 ? t("{time} elapsed", { time: formatElapsed(elapsed) }) : ""}</span>
    </div>
    {canStopExport(job) ? <StopExport path={job.path} className={buttonClass} /> : null}
  </div>;
}
