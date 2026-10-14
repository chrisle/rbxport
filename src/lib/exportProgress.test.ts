import { describe, expect, it } from "vitest";
import type { ExportProgress } from "@/ipc/types";
import { canStopExport, formatElapsed, isExportActive, stageLabel, stagePercent } from "./exportProgress";

const job: ExportProgress = { path: "/usb", state: "copying", done: 1, total: 4, title: "Track" };
describe("export stage feedback", () => {
  it("uses bytes for copying and processed duration for conversion", () => {
    expect(stagePercent({ ...job, unit: "bytes", bytesDone: 800, bytesTotal: 1000 })).toBe(80);
    expect(stagePercent({ ...job, state: "converting", unit: "milliseconds", done: 20_000, total: 60_000 })).toBe(33);
  });
  it("measures a per-file stage across its tracks, not the file alone", () => {
    // Track 3 of 4, halfway through its file: 2.5 of 4 tracks.
    const file = { ...job, state: "preparing" as const, unit: "bytes" as const, done: 0, total: 0, bytesDone: 50, bytesTotal: 100, tracksDone: 2, tracksTotal: 4 };
    expect(stagePercent(file)).toBe(62);
    expect(stagePercent({ ...file, state: "copying" })).toBe(62);
    expect(stagePercent({ ...file, state: "converting", unit: "milliseconds", done: 30_000, total: 60_000 })).toBe(62);
    // Preparing before any file reports is not measured.
    expect(stagePercent({ ...job, state: "preparing", unit: "tracks", done: 0, total: 40 })).toBeUndefined();
  });
  it("formats elapsed time as a clock", () => {
    expect(formatElapsed(107_400)).toBe("1:47");
    expect(formatElapsed(5_000)).toBe("0:05");
    expect(formatElapsed(3_725_000)).toBe("1:02:05");
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
