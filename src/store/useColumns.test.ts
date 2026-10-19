import { describe, expect, it } from "vitest";

import { columnContextFor } from "./useColumns";

describe("columnContextFor", () => {
  it("gives a playlist folder the playlists' layout, as rekordbox does (#596)", () => {
    // rekordbox 7.2 getXmlKeyName: every playlist-type list that is not an
    // intelligent playlist reads and writes TableHeader-PlaylistTracks.
    expect(columnContextFor("folder")).toBe("playlist");
    expect(columnContextFor("deviceFolder")).toBe("playlist");
  });

  it("shares one layout between every playlist rather than keeping one each", () => {
    expect(columnContextFor("playlist")).toBe("playlist");
    expect(columnContextFor("smartPlaylist")).toBe("playlist");
    expect(columnContextFor("devicePlaylist")).toBe("playlist");
  });

  it("keeps the collection, histories and the Explorer apart", () => {
    expect(columnContextFor("allTracks")).toBe("collection");
    expect(columnContextFor("collection")).toBe("collection");
    expect(columnContextFor(undefined)).toBe("collection");
    expect(columnContextFor("history")).toBe("history");
    expect(columnContextFor("directory")).toBe("folder");
    expect(columnContextFor("explorer")).toBe("folder");
  });
});
