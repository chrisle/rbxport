import { describe, expect, it } from "vitest";
import { formatBitrate, formatBpm, formatBytes, formatDuration, formatEpochDate, formatListTotals, formatShortDate, formatStatusSize, formatStatusTime, statusBarListText } from "./format";

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

describe("formatEpochDate", () => {
  it("prints the local day of a moment in seconds as the date columns do", () => {
    expect(formatEpochDate(new Date(2026, 8, 6, 0, 0, 1).getTime() / 1000)).toBe("9/6/26");
    expect(formatEpochDate(new Date(2005, 11, 25, 23, 59).getTime() / 1000)).toBe("12/25/05");
  });
  it("is blank for a missing or unreadable time", () => {
    expect(formatEpochDate(null)).toBe("");
    expect(formatEpochDate(undefined)).toBe("");
    expect(formatEpochDate(Number.NaN)).toBe("");
    expect(formatEpochDate(Number.MAX_VALUE)).toBe("");
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

describe("formatBytes", () => {
  it("switches units at a gigabyte", () => {
    expect(formatBytes(15.2 * 1024 ** 2)).toBe("15.2 MB");
    expect(formatBytes(144.8 * 1024 ** 3)).toBe("144.8 GB");
  });
});

describe("status-bar totals", () => {
  const t = (text: string) => text;
  it("says whole hours and minutes, always plural, as rekordbox does", () => {
    expect(formatStatusTime(5 * 3600 + 2 * 60 + 59, t)).toBe("5 hours 2 minutes");
    expect(formatStatusTime(18 * 60 + 30, t)).toBe("18 minutes");
    expect(formatStatusTime(60, t)).toBe("1 minutes");
    expect(formatStatusTime(3600, t)).toBe("1 hours 0 minutes");
    expect(formatStatusTime(59, t)).toBe("0 minutes");
    expect(formatStatusTime(0, t)).toBe("0 minutes");
  });
  it("sizes in binary units with at most one decimal", () => {
    expect(formatStatusSize(1, t)).toBe("1 byte");
    expect(formatStatusSize(512, t)).toBe("512 bytes");
    expect(formatStatusSize(1024, t)).toBe("1 KB");
    expect(formatStatusSize(161 * 1024 ** 2, t)).toBe("161 MB");
    expect(formatStatusSize(759.7 * 1024 ** 2, t)).toBe("759.7 MB");
    expect(formatStatusSize(1000 * 1024 ** 2, t)).toBe("1,000 MB");
    expect(formatStatusSize(1.9 * 1024 ** 3, t)).toBe("1.9 GB");
  });
  it("uses the language's own number format", () => {
    expect(formatStatusSize(66.1 * 1024 ** 2, t, "de")).toBe("66,1 MB");
  });
  it("matches the bar rekordbox shows for an open playlist", () => {
    // rekordbox-playlist-61-tracks.png: "61 Tracks, 5 hours 2 minutes, 759.7 MB".
    expect(formatListTotals({ tracks: 61, seconds: 5 * 3600 + 2 * 60 + 41, bytes: 759.7 * 1024 ** 2 }, t))
      .toBe("61 Tracks, 5 hours 2 minutes, 759.7 MB");
    expect(formatListTotals({ tracks: 1, seconds: 366, bytes: 15.2 * 1024 ** 2 }, t)).toBe("1 Tracks, 6 minutes, 15.2 MB");
    // A track with no length or size still counts; it adds nothing else.
    expect(formatListTotals({ tracks: 2, seconds: 0, bytes: 0 }, t)).toBe("2 Tracks, 0 minutes, 0 bytes");
    expect(formatListTotals({ tracks: 0, seconds: 0, bytes: 0 }, t)).toBe("0 Tracks");
  });
});

describe("statusBarListText", () => {
  const t = (text: string) => text;
  const totals = { tracks: 3, seconds: 18 * 60 + 12, bytes: 161 * 1024 ** 2 };
  it("shows the tally until tracks are picked in the list", () => {
    expect(statusBarListText({ picked: false, selected: 0, totals }, t)).toBe("3 Tracks, 18 minutes, 161 MB");
    // A selection carried over from another list is not a pick in this one.
    expect(statusBarListText({ picked: false, selected: 4, totals }, t)).toBe("3 Tracks, 18 minutes, 161 MB");
    // The collection and histories have no tally.
    expect(statusBarListText({ picked: false, selected: 0, totals: undefined }, t)).toBe("");
  });
  it("counts fewer than two picked tracks as none, as rekordbox does", () => {
    // rekordbox-track-selected-cleared.png: one row selected, blank bar.
    expect(statusBarListText({ picked: true, selected: 1, totals }, t)).toBe("");
    // Letting go does not bring the tally back.
    expect(statusBarListText({ picked: true, selected: 0, totals }, t)).toBe("");
    expect(statusBarListText({ picked: true, selected: 2, totals }, t)).toBe("Selected: 2 Tracks");
    expect(statusBarListText({ picked: true, selected: 61, totals: undefined }, t)).toBe("Selected: 61 Tracks");
  });
  it("builds the words from the catalog", () => {
    const de = (text: string) => ({ Selected: "Gewählt", Tracks: "Tracks" })[text] ?? text;
    expect(statusBarListText({ picked: true, selected: 5, totals }, de)).toBe("Gewählt: 5 Tracks");
  });
});
