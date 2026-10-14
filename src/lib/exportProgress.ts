import type { ExportProgress } from "@/ipc/types";

const active = new Set(["preparing", "planning", "recovering", "checking", "converting", "copying", "database", "verifying", "publishing", "finalizing", "ejecting"]);
export function isExportActive(job: ExportProgress): boolean { return active.has(job.state); }
export function canStopExport(job: ExportProgress): boolean {
  return job.canCancel ?? ["preparing", "planning", "checking", "converting", "copying", "database"].includes(job.state);
}
const unmeasured = new Set(["recovering", "database", "verifying", "publishing", "finalizing", "ejecting", "failed", "cancelled"]);
/** How far through the current stage the export is, or undefined when the stage reports no measure. */
export function stagePercent(job: ExportProgress): number | undefined {
  if (job.state === "done") return 100;
  if (unmeasured.has(job.state)) return undefined;
  const percent = (done: number, total: number) => Math.min(100, Math.max(0, Math.floor(done / total * 100)));
  const file = job.unit === "bytes" ? fraction(job.bytesDone ?? 0, job.bytesTotal ?? 0)
    : job.unit === "milliseconds" ? fraction(job.done, job.total) : undefined;
  // A per-file event that names its place in the stage measures the stage:
  // the tracks before it plus how far through this one it is.
  const tracks = job.tracksTotal ?? 0;
  if (job.unit !== "tracks" && tracks > 0) return percent((job.tracksDone ?? 0) + (file ?? 0), tracks);
  // Preparing reports a measure only per file; until it does, it has none.
  if (job.state === "preparing") return undefined;
  if (job.unit === "bytes" || job.unit === "milliseconds") return file === undefined ? undefined : percent(file, 1);
  return job.total > 0 ? percent(job.done, job.total) : undefined;
}
function fraction(done: number, total: number): number | undefined {
  return total > 0 ? Math.min(1, Math.max(0, done / total)) : undefined;
}

/** Elapsed time as m:ss, or h:mm:ss from an hour. */
export function formatElapsed(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  const s = String(seconds % 60).padStart(2, "0");
  const minutes = Math.floor(seconds / 60);
  return minutes >= 60 ? `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, "0")}:${s}` : `${minutes}:${s}`;
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
