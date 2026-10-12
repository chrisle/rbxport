/**
 * What to say when an export finishes.
 *
 * An export to a stick that already holds one is a sync: most of it is usually
 * "left alone", and saying "exported 200 tracks" when 198 of them were never
 * touched tells someone nothing about what just happened to their stick.
 */

export interface ExportCounts {
  tracks: number;
  reused: number;
  removed: number;
  playlistsAdded?: number;
  playlistsRemoved?: number;
  skipped: string[];
  failed?: string[];
  verified: boolean;
  upToDate?: boolean;
  keptExisting?: number;
  converted?: number;
  warnings?: readonly { title: string }[];
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

export function exportSummary(name: string, report: ExportCounts, t: (text: string, values?: Readonly<Record<string, string | number>>) => string = text => text): string {
  const warnings = report.warnings ?? [];
  if (report.upToDate && warnings.length === 0) return `${name}: ${t("Already up to date")}.`;
  const copied = Math.max(0, report.tracks - report.reused);
  const parts = [
    `${name}: Updated ${plural(copied, "track")}`,
    `Skipped ${plural(report.reused, "track")} (no change)`,
    `Added ${plural(report.playlistsAdded ?? 0, "playlist")}`,
    `Removed ${plural(report.playlistsRemoved ?? 0, "playlist")}`,
  ];

  if (report.skipped.length > 0) {
    parts.push(`${plural(report.skipped.length, "track")} missing`);
  }
  if (report.failed && report.failed.length > 0) {
    parts.push(`${plural(report.failed.length, "track")} failed`);
  }
  if (!report.verified) parts.push("but the result did not read back");
  if ((report.keptExisting ?? 0) > 0) parts.push(t("{count} existing versions kept", { count: report.keptExisting ?? 0 }));
  if ((report.converted ?? 0) > 0) parts.push(t("{count} tracks converted", { count: report.converted ?? 0 }));
  if (warnings.length > 0) parts.push(`${t("Export complete with warnings")}: ${warnings.map(w => w.title).join(", ")}`);
  return `${parts.join(". ")}.`;
}
