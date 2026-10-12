import type { ExportProgress } from "@/ipc/types";

const active = new Set(["preparing", "planning", "recovering", "checking", "converting", "copying", "database", "verifying", "publishing", "finalizing", "ejecting"]);
export function isExportActive(job: ExportProgress): boolean { return active.has(job.state); }
export function canStopExport(job: ExportProgress): boolean {
  return job.canCancel ?? ["preparing", "planning", "checking", "converting", "copying", "database"].includes(job.state);
}
export function stagePercent(job: ExportProgress): number | undefined {
  if (job.state === "done") return 100;
  if (["preparing", "planning", "recovering", "database", "verifying", "publishing", "finalizing", "ejecting", "failed", "cancelled"].includes(job.state)) return undefined;
  const done = job.unit === "bytes" ? job.bytesDone ?? 0 : job.done;
  const total = job.unit === "bytes" ? job.bytesTotal ?? 0 : job.total;
  return total > 0 ? Math.min(100, Math.max(0, Math.floor(done / total * 100))) : undefined;
}

type Translate = (text: string, values?: Readonly<Record<string, string | number>>) => string;
export function stageLabel(job: ExportProgress, t: Translate): string {
  switch (job.state) {
    case "preparing": return t("Preparing export");
    case "planning": return t("Planning changes");
    case "recovering": return t("Recovering interrupted export");
    case "checking": return job.unit === "bytes" ? t("Checking existing rekordbox export") : t("Checking {done} of {total} tracks", { done: job.done, total: job.total });
    case "converting": return t("Converting audio");
    case "copying": return t("Copying audio");
    case "database": return t("Updating playlists and databases");
    case "verifying": return t("Verifying changed files and databases");
    case "publishing": return t("Saving verified export");
    case "finalizing": return t("Checking saved library and finishing");
    case "ejecting": return t("Safely ejecting");
    case "done": return (job.warningsCount ?? 0) > 0 ? t("Export complete with warnings") : job.upToDate ? t("Already up to date") : t("Export complete");
    case "failed": return t("Export failed");
    case "cancelled": return t("Export stopped");
  }
}
