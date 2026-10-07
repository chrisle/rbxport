import { errorMessage } from "@/lib/errorMessage";
import { useCallback, useEffect, useRef, useState } from "react";
import { getBackend } from "@/ipc/client";
import type { BackupProgress } from "@/ipc/types";
import { formatBytes } from "@/lib/format";
import { useTranslation } from "@/i18n";

const EMPTY: BackupProgress = { running: false, phase: "", copiedBytes: 0, totalBytes: 0, error: null, path: null };

function unchanged(a: BackupProgress, b: BackupProgress): boolean {
  return a.running === b.running && a.phase === b.phase && a.copiedBytes === b.copiedBytes
    && a.totalBytes === b.totalBytes && a.error === b.error && a.path === b.path
    && (a.currentItem ?? null) === (b.currentItem ?? null);
}

type Translate = ReturnType<typeof useTranslation>;

export function backupStatus(progress: BackupProgress, t: Translate): string {
  switch (progress.phase) {
    case "preparing": return t("Preparing backup…");
    case "copying": {
      const status = t("Creating backup: {percent}% — {copied} of {total}", {
        percent: progress.totalBytes > 0 ? Math.min(100, Math.floor(progress.copiedBytes / progress.totalBytes * 100)) : 0,
        copied: formatBytes(progress.copiedBytes),
        total: formatBytes(progress.totalBytes),
      });
      return progress.currentItem ? `${status} · ${progress.currentItem}` : status;
    }
    case "compressing": return t("Compressing backup…");
    case "validating": return t("Verifying backup…");
    case "stopping": return t("Stopping backup…");
    case "complete": return t("Backup created.");
    case "cancelled": return t("Backup stopped.");
    case "failed": return progress.error ?? t("Backup failed.");
    default: return "";
  }
}

/** The backend owns the job; every window can reconnect without restarting it. */
export function useBackupProgress() {
  const t = useTranslation();
  const [progress, setProgress] = useState(EMPTY);
  const [requestError, setRequestError] = useState("");
  const pending = useRef(false);
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        const next = await (await getBackend()).backupProgress();
        if (live && !pending.current) setProgress(current => unchanged(current, next) ? current : next);
      } catch {
        // Preserve the last known job while the backend is temporarily unavailable.
      } finally {
        if (live) timer = setTimeout(() => void refresh(), 500);
      }
    };
    void refresh();
    return () => { live = false; clearTimeout(timer); };
  }, []);
  const start = useCallback(async () => {
    if (pending.current || progress.running) return;
    pending.current = true;
    setRequestError("");
    setProgress({ ...EMPTY, running: true, phase: "preparing" });
    try {
      const backend = await getBackend();
      await backend.startBackup();
      setProgress(await backend.backupProgress());
    } catch (e) {
      setRequestError(errorMessage(e));
      setProgress(EMPTY);
    } finally { pending.current = false; }
  }, [progress.running]);
  const stop = useCallback(async () => {
    setRequestError("");
    try {
      const backend = await getBackend();
      await backend.cancelBackup();
      setProgress(await backend.backupProgress());
    } catch (e) { setRequestError(errorMessage(e)); }
  }, []);
  return { progress, start, stop, text: backupStatus(progress, t), error: requestError || progress.error };
}
