import styles from "./StatusBar.module.css";
import { Bug, Heart } from "lucide-react";
import type { ReactNode } from "react";
import { useTooltip } from "@/store/usePreferences";
import { refusal } from "@/lib/menu";
import type { ExportProgress } from "@/ipc/types";
import type { JobView } from "@/lib/jobQueue";
import { useTranslation } from "@/i18n";

export interface StatusBarProps {
  /** Long library operations: the running one first, then those waiting. */
  jobs?: readonly JobView[];
  onStopJob?: ((id: number) => void) | undefined;
  exports?: readonly (ExportProgress & { name: string })[];
  /** The build's version, shown beside the name; null until it is read. */
  version?: string | null;
  /** General status text; empty when idle. */
  activity?: string;
  /** `waiting`: nothing is being analysed yet, a library job holds the run. */
  analysisProgress?: { completed: number; total: number; waiting?: boolean } | undefined;
  backupActivity?: string;
  backupProgress?: { phase: string; copiedBytes: number; totalBytes: number } | undefined;
  /**
   * Something went wrong, said in red.
   *
   * Errors belong here rather than over the thing that raised them: the
   * player used to print its own across the pad row and the tree beneath it.
   */
  error?: string | null;
  /** e.g. "Selected: 4 Tracks, 18 minutes, 58.4 MB"; empty when nothing is selected. */
  selection?: string;
  readOnly?: boolean;
  /** Library Protection in Preferences is why, rather than rekordbox running. */
  protectedLibrary?: boolean;
  onExplainReadOnly?: (() => void) | undefined;
  onDisableReadOnly?: (() => void) | undefined;
  onOpenProtection?: (() => void) | undefined;
  onReportBug?: (() => void) | undefined;
  onSupport?: (() => void) | undefined;
  /** Opens the current application log with the OS default handler. */
  onOpenLog?: (() => void) | undefined;
  /** Present only while analysis is running, so it can be stopped. */
  onCancelAnalysis?: (() => void) | undefined;
  /** Tracks that failed analysis in the current run. */
  analysisFailures?: number;
  /** A short-lived status-bar action, such as a downloaded update. */
  updateNotice?: ReactNode;
}

export function StatusBar({
  jobs = [],
  onStopJob,
  exports = [],
  version = null,
  activity = "",
  analysisProgress,
  backupActivity = "",
  backupProgress,
  error = null,
  selection = "",
  readOnly = false,
  protectedLibrary = false,
  onExplainReadOnly,
  onDisableReadOnly,
  onOpenProtection,
  onReportBug,
  onSupport,
  onOpenLog,
  onCancelAnalysis,
  analysisFailures = 0,
  updateNotice,
}: StatusBarProps) {
  const tip = useTooltip();
  const t = useTranslation();
  const analysisPercent = analysisProgress && analysisProgress.total > 0
    ? Math.min(100, Math.max(0, Math.floor(analysisProgress.completed / analysisProgress.total * 100))) : 0;
  const backupPercent = backupProgress && backupProgress.totalBytes > 0
    ? Math.min(100, Math.max(0, Math.floor(backupProgress.copiedBytes / backupProgress.totalBytes * 100))) : 0;
  const exportTotal = exports.reduce((sum, job) => sum + job.total, 0);
  const exportDone = exports.reduce((sum, job) => sum + job.done, 0);
  const exportPercent = exportTotal > 0
    ? Math.min(99, Math.max(0, Math.floor(exportDone / exportTotal * 100))) : 0;
  const activeExportStates = new Set(exports.filter(job => job.state !== "done").map(job => job.state));
  const exportNoun = `${exports.length} ${exports.length === 1 ? "USB" : "USBs"}`;
  const exportLabel = activeExportStates.size !== 1 ? `Syncing ${exportNoun}`
    : activeExportStates.has("preparing") ? `Preparing ${exportNoun}`
    : activeExportStates.has("checking") ? `Checking ${exportNoun}`
    : activeExportStates.has("copying") ? `Exporting ${exportNoun}`
    : activeExportStates.has("database") ? `Building databases for ${exportNoun}`
    : activeExportStates.has("verifying") ? `Verifying ${exportNoun}`
    : activeExportStates.has("publishing") ? `Publishing ${exportNoun}`
    : activeExportStates.has("ejecting") ? `Ejecting ${exportNoun}`
    : `Syncing ${exportNoun}`;
  return (
    <footer className={styles.statusBar}>
      <span
        className={`${styles.logo} ${onOpenLog ? styles.logLink : ""}`}
        title={onOpenLog ? "Right-click to open the application log" : undefined}
        onContextMenu={onOpenLog ? (event) => {
          event.preventDefault();
          onOpenLog();
        } : undefined}
      >
        rbxport
        {version === null ? null : <> <span className={styles.version}>{version}</span></>}
      </span>
      {readOnly ? (
        <button type="button" className={styles.readOnly} title={refusal(protectedLibrary)}
          onClick={onExplainReadOnly} onDoubleClick={onDisableReadOnly}>
          Library read-only
        </button>
      ) : null}
      {/*
        Stop comes before the progress text, not after it. After it the button
        slides left and right as the counter's width changes, which makes it
        hard to hit — a moving target for a person, and unclickable for a test.
      */}
      {onCancelAnalysis ? (
        <button type="button" className={styles.stop} onClick={onCancelAnalysis}>
          Stop
        </button>
      ) : null}
      {jobs.map((job) => {
        const percent = job.total > 0 ? Math.min(100, Math.floor(job.done / job.total * 100)) : 0;
        return (
          <span key={job.id} className={styles.jobMeter} data-state={job.state}>
            {onStopJob && job.stoppable ? (
              <button type="button" className={styles.stop} onClick={() => onStopJob(job.id)}
                disabled={job.state === "stopping"} aria-label={`${t("Stop")}: ${job.label}`}>
                {job.state === "stopping" ? t("Stopping…") : t("Stop")}
              </button>
            ) : null}
            <span className={styles.jobLabel}>{job.label}</span>
            {job.state === "queued" ? <span className={styles.jobQueued}>{t("Queued")}</span> : <>
              <progress className={styles.backupProgress} aria-label={job.label} max={100}
                value={job.total > 0 ? percent : undefined} />
              <span className={styles.backupPercent}>{job.total > 0 ? `(${percent}%)` : ""}</span>
            </>}
          </span>
        );
      })}
      {error === null || error === "" ? null : (
        <span className={styles.error} role="alert">
          {error}
          {onOpenProtection ? (
            <button type="button" className={styles.warningAction} onClick={onOpenProtection}>
              Open Preferences
            </button>
          ) : null}
        </span>
      )}
      {exports.length > 0 ? <span className={styles.backupMeter}>
        <span>{exportLabel}</span>
        <progress className={styles.backupProgress} aria-label={`Exporting ${exports.length} ${exports.length === 1 ? "USB" : "USBs"}`} max={100} value={exportPercent} />
        <span className={styles.backupPercent}>({exportPercent}%)</span>
      </span> : null}
      {backupProgress ? <span className={styles.backupMeter} title={backupActivity}>
        <span>Backup</span>
        <progress className={styles.backupProgress} aria-label="Backup progress"
          max={100} value={backupProgress.totalBytes > 0 ? backupPercent : undefined} />
        <span className={styles.backupPercent}>({backupPercent}%)</span>
      </span>
        : backupActivity ? <span className={styles.backupActivity} role="status">{backupActivity}</span> : null}
      {analysisProgress ? <span className={styles.analysisMeter}>
        <span>Analyzing {analysisProgress.total} {analysisProgress.total === 1 ? "track" : "tracks"}</span>
        {analysisProgress.waiting === true ? <span className={styles.jobQueued}>{t("Queued")}</span> : <>
          <progress className={styles.backupProgress} aria-label="Analysis progress"
            max={100} value={analysisPercent} />
          <span className={styles.backupPercent}>({analysisPercent}%)</span>
        </>}
      </span> : <span className={styles.activity}>{activity}</span>}
      {analysisFailures > 0 ? (
        <span
          className={styles.failures}
          title={tip("These tracks could not be analysed; the run carried on past them")}
        >
          {analysisFailures} failed
        </span>
      ) : null}
      <span className={styles.selection}>{selection}</span>
      <span className={styles.actions}>
        {updateNotice}
        {onSupport ? <button type="button" className={`${styles.reportBug} ${styles.support}`} onClick={onSupport}><Heart size="1em" aria-hidden="true" /> Support rbxport</button> : null}
        {onReportBug ? <button type="button" className={`${styles.reportBug} ${styles.report}`} onClick={onReportBug}><Bug size="1em" aria-hidden="true" /> Report bug</button> : null}
      </span>
    </footer>
  );
}
