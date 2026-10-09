// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { EXTRA_COLUMNS } from "@/lib/columns";
import type { RowDto } from "@/ipc/types";
import { cellText } from "./TrackTable";

describe("browser detail columns", () => {
  it("renders every requested database field when that field has a value", () => {
    const row: RowDto = {
      id: "1", trackNo: 1, title: "Track", artist: "Artist", album: "Album",
      genre: "House", label: "Label", comment: "Comment", bpmX100: 12800,
      key: "Am", durationSec: 240, rating: 5, analysed: 1,
      dateAdded: "2026-09-24", releaseDate: "2026-09-24", artworkHue: 0,
      hasArtwork: false, fileName: "track.mp3",
      memoryCues: [],
      hotCues: [["A", 1000, null]],
      extra: {
        size: 8_000_000, discNo: 1, albumArtist: "Artist", composer: "Composer",
        lyricist: "Lyricist", fileType: 1, year: 2026, mixName: "Extended Mix",
        remixer: "Remixer", originalArtist: "Original", sampleRate: 44100,
        bitrate: 320, bitDepth: 16, location: "/Music/track.mp3",
        dateCreated: "2026-09-24", publishTrackInfo: true, message: "Available",
        color: 1, djPlayCount: 3, myTag: "Peak", trackNumber: 4, cloud: true,
      },
    };
    for (const column of EXTRA_COLUMNS) {
      expect(cellText(row, column), column).not.toBe("");
    }
    expect(cellText(row, "hotCue")).toBe("A");
  });

  it("prints VBR for a bitrate the library holds at zero, and nothing for one it did not send", () => {
    const row = { extra: { bitrate: 0 } } as unknown as RowDto;
    expect(cellText(row, "bitrate")).toBe("VBR");
    expect(cellText({ ...row, extra: { bitrate: 256 } }, "bitrate")).toBe("256 kbps");
    expect(cellText({ ...row, extra: {} }, "bitrate")).toBe("");
  });
});
