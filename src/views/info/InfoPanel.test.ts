import { describe, expect, it } from "vitest";

import {
  acceptable, dateSegments, fieldText, fileTypeLabel, selectionView, singleView, summaryFacts,
} from "./fields";
import type { RowDto, SelectionDetails, TrackDetails } from "@/ipc/types";

const row: RowDto = {
  id: "18969",
  trackNo: 4,
  title: "Age Of Love (Dominant Space Remix) [138 EDIT]",
  artist: "Age of Love",
  album: "",
  genre: "",
  label: "",
  comment: "F - 4A - 138",
  bpmX100: 13_800,
  key: "Fm",
  durationSec: 268,
  rating: 0,
  analysed: 1,
  dateAdded: "2023-08-06 10:00:00.000 +00:00",
  releaseDate: "",
  hotCues: [["A", 46, "#77E866"], ["B", 90_000, "#77E866"], ["C", 120_000, null]],
  artworkHue: 40,
  hasArtwork: false,
};

/** The captured track, as the reference library holds it. */
const details: TrackDetails = {
  id: "18969",
  title: row.title,
  artist: row.artist,
  album: "",
  albumArtist: "",
  originalArtist: "",
  composer: "",
  remixer: "",
  lyricist: "",
  genre: "",
  label: "",
  key: "Fm",
  comment: row.comment,
  mixName: "",
  message: "",
  color: "0",
  rating: 0,
  bpmX100: 13_800,
  durationSec: 268,
  year: 0,
  trackNumber: 0,
  discNumber: 0,
  playCount: 0,
  fileType: 11,
  fileSize: 47_322_584,
  bitrate: 1411,
  sampleRate: 44_100,
  bitDepth: 16,
  dateCreated: "2023-08-06",
  releaseDate: "",
  path: "/Volumes/SD/RB/_2025-10-moved4/unknownartist/unknownalbum/age.wav",
  hotCueAutoLoad: true,
  publish: false,
  hasArtwork: false,
    myTags: [],
};

describe("the Summary tab's table", () => {
  it("prints the captured track the way the capture shows it", () => {
    const byLabel = Object.fromEntries(summaryFacts(row, details).map((f) => [f.label, f.value]));
    expect(byLabel).toEqual({
      "Time": "04:28",
      "File Type": "WAV File",
      "Size": "45.1 MB",
      "Date Created": "8/6/23",
      "Sample Rate": "44100 Hz",
      "Bitrate": "1411 kbps",
      "DJ Play Count": "0",
      "Location": details.path,
    });
  });

  it("keeps the rows in the captured order", () => {
    expect(summaryFacts(row, details).map((f) => f.label)).toEqual([
      "Time", "File Type", "Size", "Date Created", "Sample Rate", "Bitrate", "DJ Play Count", "Location",
    ]);
  });

  it("shows the row's own duration and blanks while the record is on its way", () => {
    const facts = summaryFacts(row, null);
    expect(facts.find((f) => f.label === "Time")?.value).toBe("04:28");
    expect(facts.filter((f) => f.label !== "Time").every((f) => f.value === "")).toBe(true);
    expect(facts).toHaveLength(8);
  });

  it("does not print another track's record", () => {
    const stale = { ...details, id: "1" };
    expect(summaryFacts(row, stale).find((f) => f.label === "Size")?.value).toBe("");
  });

  it("says nothing for a size or rate the library left at zero", () => {
    const bare = { ...details, fileSize: 0, sampleRate: 0 };
    const byLabel = Object.fromEntries(summaryFacts(row, bare).map((f) => [f.label, f.value]));
    expect(byLabel["Size"]).toBe("");
    expect(byLabel["Sample Rate"]).toBe("");
  });

  it("prints VBR for a bitrate the library left at zero, as rekordbox does", () => {
    // [OBS] rekordbox 7.2.14 Summary tab: FLAC, VBR MP3 and M4A rows stored
    // at BitRate 0 all read "VBR".
    const flac = { ...details, fileType: 5, bitrate: 0 };
    const byLabel = Object.fromEntries(summaryFacts(row, flac).map((f) => [f.label, f.value]));
    expect(byLabel["Bitrate"]).toBe("VBR");
  });
});

describe("the file type label", () => {
  it("names the five codes counted against the reference library's extensions", () => {
    expect(fileTypeLabel(1)).toBe("MP3 File");
    expect(fileTypeLabel(4)).toBe("M4A File");
    expect(fileTypeLabel(5)).toBe("FLAC File");
    expect(fileTypeLabel(11)).toBe("WAV File");
    expect(fileTypeLabel(12)).toBe("AIFF File");
  });

  it("prints nothing for a code it cannot vouch for, rather than a guess", () => {
    expect(fileTypeLabel(6)).toBe("");
    expect(fileTypeLabel(0)).toBe("");
  });
});

describe("the Info tab's fields", () => {
  it("prints numbers as numbers, zero included, as rekordbox's own boxes do", () => {
    expect(fieldText(details, "year")).toBe("0");
    expect(fieldText(details, "trackNumber")).toBe("0");
    expect(fieldText({ ...details, year: 2023 }, "year")).toBe("2023");
    expect(fieldText(details, "title")).toBe(row.title);
  });

  it("refuses what the writer would refuse before the round trip", () => {
    expect(acceptable("year", "2023")).toBe(true);
    expect(acceptable("year", " 7 ")).toBe(true);
    expect(acceptable("year", "")).toBe(false);
    expect(acceptable("year", "abc")).toBe(false);
    expect(acceptable("year", "-1")).toBe(false);
    expect(acceptable("year", "10000")).toBe(false);
    expect(acceptable("discNumber", "1000")).toBe(false);
    expect(acceptable("title", "")).toBe(true);
  });

  it("splits a release date into day, month name and year", () => {
    expect(dateSegments("2026-09-11")).toEqual(["11", "September", "2026"]);
    expect(dateSegments("2023-08-01")).toEqual(["1", "August", "2023"]);
    expect(dateSegments("")).toEqual(["", "", ""]);
    expect(dateSegments("not a date")).toEqual(["", "", ""]);
  });
});

describe("the Info tab for several selected tracks", () => {
  // The two-track selection captured on rekordbox 7 (Windows 11): "I Wish"
  // and "Dancing In The Street", both 138.00 BPM, keys Eb and C, no artist.
  const first: TrackDetails = {
    ...details, id: "1", title: "I Wish (TRIODE Edit)", artist: "", key: "Eb", bpmX100: 13_800,
    year: 0, trackNumber: 0, discNumber: 0, playCount: 0, rating: 0, hotCueAutoLoad: true,
  };
  const pair: SelectionDetails = { first, count: 2, mixed: ["title", "key", "durationSec", "path"] };

  it("shows what the tracks share and blanks what they do not, as rekordbox does", () => {
    const view = selectionView(["1", "2"], pair);
    expect(view.multiple).toBe(true);
    expect(view.ids).toEqual(["1", "2"]);
    // [OBS] Track Title and Key blank, BPM 138.00, Year/Track/Disc/Play Count 0, auto-load ticked.
    expect(view.text("title")).toBe("");
    expect(view.text("key")).toBe("");
    expect(view.bpm).toBe("138.00");
    expect(view.text("artist")).toBe("");
    expect(view.text("year")).toBe("0");
    expect(view.text("trackNumber")).toBe("0");
    expect(view.text("discNumber")).toBe("0");
    expect(view.text("playCount")).toBe("0");
    expect(view.hotCueAutoLoad).toBe(true);
    expect(view.myTags).toBeNull();
  });

  it("reads 0.00 for a BPM the tracks do not share, and no stars for a rating", () => {
    // [OBS] three tracks at 138, 136 and 128 BPM: the BPM box read 0.00.
    const three: SelectionDetails = { first: { ...first, rating: 3 }, count: 3, mixed: ["bpmX100", "rating"] };
    const view = selectionView(["1", "2", "3"], three);
    expect(view.bpm).toBe("0.00");
    expect(view.rating).toBe(0);
  });

  it("blanks a number box the tracks do not share rather than printing the first one's", () => {
    // [static] getTrackProp returns an empty string for an unshared Year,
    // Track number, Disc number or DJ Play Count.
    const view = selectionView(["1", "2"], {
      first: { ...first, year: 2019, trackNumber: 4, discNumber: 1, playCount: 7 },
      count: 2,
      mixed: ["year", "trackNumber", "discNumber", "playCount"],
    });
    expect(view.text("year")).toBe("");
    expect(view.text("trackNumber")).toBe("");
    expect(view.text("discNumber")).toBe("");
    expect(view.text("playCount")).toBe("");
  });

  it("ticks a box only when every track has it ticked, and keeps the first track's colour", () => {
    const view = selectionView(["1", "2"], {
      first: { ...first, color: "3", publish: true },
      count: 2,
      mixed: ["hotCueAutoLoad", "publish", "color"],
    });
    expect(view.hotCueAutoLoad).toBe(false);
    expect(view.publish).toBe(false);
    // [static] getBrowseInfoIntValue reads the colour without comparing.
    expect(view.color).toBe("3");
  });

  it("is blank until the record arrives", () => {
    const view = selectionView(["1", "2"], null);
    expect(view.text("artist")).toBe("");
    expect(view.text("year")).toBe("");
    expect(view.bpm).toBe("");
    expect(view.color).toBe("0");
  });

  it("leaves one track as it was: its own record, its My Tags, its own id", () => {
    const view = singleView(row, details);
    expect(view.multiple).toBe(false);
    expect(view.ids).toEqual([row.id]);
    expect(view.text("title")).toBe(row.title);
    expect(view.text("year")).toBe("0");
    expect(view.bpm).toBe("138.00");
    expect(view.myTags).toEqual([]);
    expect(singleView(row, null).myTags).toBeNull();
  });
});
