import { describe, expect, it } from "vitest";

import { colorName, deckInfo } from "./deckInfoLines";
import type { RowDto, TrackDetails } from "@/ipc/types";

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
  rating: 3,
  analysed: 1,
  dateAdded: "2023-08-06 10:00:00.000 +00:00",
  releaseDate: "",
  hotCues: [],
  artworkHue: 40,
  hasArtwork: false,
};

/** The captured track, as the reference library holds it, rated and coloured Aqua. */
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
  color: "6",
  rating: 3,
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

describe("the deck's INFO tab", () => {
  it("prints the manual's seven items from the row and its record", () => {
    expect(deckInfo(row, details)).toEqual({
      rating: 3,
      color: "Aqua",
      comment: "F - 4A - 138",
      file: ["WAV File", "45.1 MB", "44100 Hz", "1411 kbps"],
    });
  });

  it("shows dashes with no track loaded", () => {
    expect(deckInfo(null, null)).toEqual({
      rating: 0,
      color: "",
      comment: "—",
      file: ["—", "—", "—", "—"],
    });
  });

  it("shows the row and blank file lines until the record arrives", () => {
    expect(deckInfo(row, null)).toEqual({
      rating: 3,
      color: "",
      comment: "F - 4A - 138",
      file: ["", "", "", ""],
    });
  });

  it("never prints another track's record", () => {
    const other = { ...details, id: "1" };
    expect(deckInfo(row, other).file).toEqual(["", "", "", ""]);
    expect(deckInfo(row, other).color).toBe("");
  });

  it("takes the rating and comment from the row, which the browser keeps current", () => {
    const edited = { ...row, rating: 5, comment: "changed in the browser" };
    const info = deckInfo(edited, details);
    expect(info.rating).toBe(5);
    expect(info.comment).toBe("changed in the browser");
  });

  it("leaves a fact blank when the library does not hold it", () => {
    const bare = { ...details, fileType: 6, fileSize: 0, sampleRate: 0, bitrate: 0 };
    // A zero bitrate prints "VBR", as the Summary tab does [ASSUME: the deck
    // INFO tab was not observed in rekordbox; it reuses the Summary facts].
    expect(deckInfo(row, bare).file).toEqual(["", "", "", "VBR"]);
  });

  it("names the eight colours and nothing else", () => {
    expect(colorName("1")).toBe("Pink");
    expect(colorName("8")).toBe("Purple");
    expect(colorName("0")).toBe("");
    expect(colorName("")).toBe("");
    expect(colorName("9")).toBe("");
  });
});
