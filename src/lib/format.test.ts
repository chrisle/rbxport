import { describe, expect, it } from "vitest";
import { formatBitrate, formatBpm, formatBytes, formatDuration, formatSelectionSummary, formatShortDate, formatTotalTime } from "./format";

describe("formatDuration", () => {
  it("pads to two digits like rekordbox", () => {
    expect(formatDuration(266)).toBe("04:26");
    expect(formatDuration(5)).toBe("00:05");
  });
  it("does not roll over into hours", () => {
    expect(formatDuration(3661)).toBe("61:01");
  });
  it("returns empty for nonsense rather than NaN", () => {
    expect(formatDuration(Number.NaN)).toBe("");
    expect(formatDuration(-1)).toBe("");
  });
});

describe("formatBpm", () => {
  it("renders the x100 storage as two decimals", () => {
    expect(formatBpm(12800)).toBe("128.00");
    expect(formatBpm(12345)).toBe("123.45");
  });
  it("treats unanalysed (0) as blank, not 0.00", () => {
    expect(formatBpm(0)).toBe("");
  });
});

describe("formatBitrate", () => {
  it("prints kbps for a stored bitrate", () => {
    expect(formatBitrate(320)).toBe("320 kbps");
    expect(formatBitrate(1411)).toBe("1411 kbps");
  });
  it("prints VBR for the zero rekordbox stores on FLAC, VBR MP3 and some M4A rows", () => {
    // [OBS] rekordbox 7.2.14: BitRate 0 shows "VBR" in the column and Summary tab.
    expect(formatBitrate(0)).toBe("VBR");
  });
  it("is blank for nonsense", () => {
    expect(formatBitrate(Number.NaN)).toBe("");
    expect(formatBitrate(-1)).toBe("");
  });
});

describe("formatShortDate", () => {
  it("drops leading zeros and shortens the year", () => {
    expect(formatShortDate("2026-09-06")).toBe("9/6/26");
    expect(formatShortDate("2026-12-25")).toBe("12/25/26");
  });
  it("accepts a full rekordbox timestamp", () => {
    expect(formatShortDate("2026-09-05 03:09:51.109 +00:00")).toBe("9/5/26");
  });
  it("is blank for missing dates", () => {
    expect(formatShortDate(null)).toBe("");
    expect(formatShortDate("")).toBe("");
  });
});

describe("totals", () => {
  it("formats hours and minutes", () => {
    expect(formatTotalTime(821 * 3600 + 32 * 60)).toBe("821 hours 32 minutes");
    expect(formatTotalTime(60)).toBe("1 minute");
  });
  it("switches units at a gigabyte", () => {
    expect(formatBytes(15.2 * 1024 ** 2)).toBe("15.2 MB");
    expect(formatBytes(144.8 * 1024 ** 3)).toBe("144.8 GB");
  });
  it("matches the status bar wording", () => {
    expect(formatSelectionSummary(1, 366, 15.2 * 1024 ** 2)).toBe("Selected: 1 Track, 6 minutes, 15.2 MB");
    expect(formatSelectionSummary(0, 0, 0)).toBe("");
  });
});
