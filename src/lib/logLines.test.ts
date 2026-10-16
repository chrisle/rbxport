import { describe, expect, it } from "vitest";
import type { LogLine } from "@/ipc/types";
import { LOG_WINDOW_LINES, appendLogLines, formatLogLine, visibleLogLines } from "./logLines";

const line = (seq: number, level: LogLine["level"], message: string, target = "rbxport_lib::commands"): LogLine =>
  ({ seq, timeMs: Date.UTC(2026, 9, 16, 10, 55, 7, 123), level, target, message });

describe("log lines", () => {
  const lines = [
    line(1, "trace", "packet"),
    line(2, "debug", "opened the library"),
    line(3, "info", "Copied 3 tracks.", "rbxport::status"),
    line(4, "warn", "The playlist could not be renamed.", "rbxport::status"),
    line(5, "error", "internal error error.detail=disk full"),
  ];

  it("keeps a level and every more severe one", () => {
    expect(visibleLogLines(lines, "warn", "").map(l => l.seq)).toEqual([4, 5]);
    expect(visibleLogLines(lines, "trace", "").map(l => l.seq)).toEqual([1, 2, 3, 4, 5]);
    expect(visibleLogLines(lines, "error", "").map(l => l.seq)).toEqual([5]);
  });

  it("finds text in the message or the module, case aside", () => {
    expect(visibleLogLines(lines, "trace", "  DISK ").map(l => l.seq)).toEqual([5]);
    expect(visibleLogLines(lines, "trace", "status").map(l => l.seq)).toEqual([3, 4]);
    expect(visibleLogLines(lines, "warn", "copied")).toEqual([]);
  });

  it("writes a line the way the log file does", () => {
    expect(formatLogLine(lines[3]!)).toBe(
      "2026-10-16T10:55:07.123Z  WARN rbxport::status: The playlist could not be renamed.");
  });

  it("appends only lines after the newest held, up to the window's limit", () => {
    expect(appendLogLines(lines.slice(0, 3), lines.slice(1)).map(l => l.seq)).toEqual([1, 2, 3, 4, 5]);
    const held = lines.slice(0, 2);
    expect(appendLogLines(held, [lines[0]!])).toBe(held);
    const many = Array.from({ length: LOG_WINDOW_LINES + 5 }, (_, i) => line(i + 1, "info", `n${i + 1}`));
    const kept = appendLogLines([], many);
    expect(kept).toHaveLength(LOG_WINDOW_LINES);
    expect(kept[0]?.seq).toBe(6);
  });
});
