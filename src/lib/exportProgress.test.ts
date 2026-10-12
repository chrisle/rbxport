import { describe, expect, it } from "vitest";
import type { ExportProgress } from "@/ipc/types";
import { canStopExport, isExportActive, stageLabel, stagePercent } from "./exportProgress";

const job: ExportProgress = { path: "/usb", state: "copying", done: 1, total: 4, title: "Track" };
describe("export stage feedback", () => {
  it("uses bytes for copying and processed duration for conversion", () => {
    expect(stagePercent({ ...job, unit: "bytes", bytesDone: 800, bytesTotal: 1000 })).toBe(80);
    expect(stagePercent({ ...job, state: "converting", unit: "milliseconds", done: 20_000, total: 60_000 })).toBe(33);
  });
  it("never invents a percentage for database, recovery, or publication", () => {
    for (const state of ["database", "recovering", "verifying", "publishing", "finalizing"] as const) {
      expect(stagePercent({ ...job, state, done: 4 })).toBeUndefined();
      expect(isExportActive({ ...job, state })).toBe(true);
    }
  });
  it("disables stopping once publication has started", () => {
    expect(canStopExport({ ...job, state: "publishing", canCancel: false })).toBe(false);
    expect(canStopExport({ ...job, canCancel: true })).toBe(true);
  });
  it("shows a full stage without declaring the export finished", () => {
    const end = { ...job, done: 4 };
    expect(stagePercent(end)).toBe(100); expect(isExportActive(end)).toBe(true);
  });
  it("names no-op and incomplete outcomes independently", () => {
    const t = (text: string) => text;
    expect(stageLabel({ ...job, state: "done", upToDate: true }, t)).toBe("Already up to date");
    expect(stageLabel({ ...job, state: "done", warningsCount: 1 }, t)).toBe("Export complete with warnings");
  });
});
