import { describe, expect, it } from "vitest";

import type { DeckTick } from "@/ipc/types";
import { continuesFrom, nextRowIndex, playedToEnd } from "./continuousPlay";

const deck = (over: Partial<DeckTick> = {}): DeckTick => ({
  frames: 0,
  totalFrames: 44_100 * 300,
  generation: 1,
  playing: true,
  loaded: true,
  loadId: 7,
  tempo: 1,
  masterTempo: false,
  keyShift: 0,
  startInFrames: 0,
  loopInFrames: 0,
  loopOutFrames: 0,
  looping: false,
  ...over,
});

describe("nextRowIndex", () => {
  // rekordbox 7.2.19, `ListViewer::getNextTrackIndex` @0x10040f920.
  it("moves to the row after the one the track was loaded from", () => {
    expect(nextRowIndex(["a", "b", "c"], "a", 0)).toBe(1);
    expect(nextRowIndex(["a", "b", "c"], "b", 1)).toBe(2);
  });

  it("goes round to the first row after the last", () => {
    expect(nextRowIndex(["a", "b", "c"], "c", 2)).toBe(0);
    expect(nextRowIndex(["a"], "a", 0)).toBe(0);
  });

  it("moves on from the copy that was played when a track is listed twice", () => {
    expect(nextRowIndex(["a", "b", "a", "c"], "a", 2)).toBe(3);
  });

  it("finds the track again when the list has moved under it", () => {
    // Re-sorted since the load: the remembered row holds another track.
    expect(nextRowIndex(["c", "a", "b"], "a", 0)).toBe(2);
    expect(nextRowIndex(["c", "a", "b"], "a", -1)).toBe(2);
  });

  it("starts the list again when the track is no longer in it", () => {
    expect(nextRowIndex(["c", "d"], "a", 1)).toBe(0);
  });

  it("has nothing to give from an empty list", () => {
    expect(nextRowIndex([], "a", 0)).toBe(-1);
  });
});

describe("playedToEnd", () => {
  const end = 44_100 * 300;

  it("takes the engine's word that the track ran out", () => {
    // The fade at the end can stop the head a few frames short of a length
    // the file declares.
    expect(playedToEnd(deck({ frames: end - 600 }), deck({ frames: end - 500, playing: false, ended: true }))).toBe(true);
    expect(playedToEnd(deck({ frames: end - 600 }), deck({ frames: end, playing: false, ended: false }))).toBe(false);
  });

  it("is the tick where a playing deck stops at the end of its track", () => {
    expect(playedToEnd(deck({ frames: end - 100 }), deck({ frames: end, playing: false }))).toBe(true);
  });

  it("is not a pause short of the end", () => {
    expect(playedToEnd(deck({ frames: 1000 }), deck({ frames: 1000, playing: false }))).toBe(false);
  });

  it("is not a deck that was already stopped there", () => {
    expect(playedToEnd(deck({ frames: end, playing: false }), deck({ frames: end, playing: false }))).toBe(false);
    expect(playedToEnd(null, deck({ frames: end, playing: false }))).toBe(false);
  });

  it("is not a loop, an unloaded deck, or a new load", () => {
    expect(playedToEnd(deck(), deck({ frames: end, playing: false, looping: true }))).toBe(false);
    expect(playedToEnd(deck(), deck({ frames: end, playing: false, loaded: false }))).toBe(false);
    expect(playedToEnd(deck(), deck({ frames: end, playing: false, loadId: 8 }))).toBe(false);
    expect(playedToEnd(deck(), deck({ frames: 0, totalFrames: 0, playing: false }))).toBe(false);
  });
});

describe("continuesFrom", () => {
  // rekordbox 7.2.19 EXPORT mode, `ListViewer::canLoadNextTrack` @0x10040f380.
  const playlist = { kind: "playlist", id: "12" } as const;
  const collection = { kind: "collection" } as const;
  const explorer = { kind: "folder", path: "/Music" } as const;
  const history = { kind: "history", id: "3" } as const;
  const folder = { kind: "playlistFolder", id: "5" } as const;
  const tagList = { kind: "tagList" } as const;
  const stickPlaylist = { kind: "device", path: "/Volumes/X", format: "deviceLibrary", playlist: "4" } as const;
  const stickAllTracks = { ...stickPlaylist, playlist: "0" } as const;
  const related = { kind: "related", track: "1", criterion: "bpmKey" } as const;

  it("in 1 PLAYER is a playlist, a stick's included, or the Tag List", () => {
    expect(continuesFrom(playlist, "one")).toBe(true);
    expect(continuesFrom(stickPlaylist, "one")).toBe(true);
    expect(continuesFrom(tagList, "one")).toBe(true);
  });

  it("in 1 PLAYER is not the collection, the Explorer, a history or a stick's All Tracks", () => {
    expect(continuesFrom(collection, "one")).toBe(false);
    expect(continuesFrom(explorer, "one")).toBe(false);
    expect(continuesFrom(history, "one")).toBe(false);
    expect(continuesFrom(stickAllTracks, "one")).toBe(false);
    // [UNKNOWN] which list type rekordbox gives a folder: left stopping.
    expect(continuesFrom(folder, "one")).toBe(false);
  });

  it("in SIMPLE PLAYER is every list the browser shows", () => {
    for (const source of [playlist, collection, explorer, history, folder, tagList, stickPlaylist, stickAllTracks]) {
      expect(continuesFrom(source, "simple")).toBe(true);
    }
  });

  it("is never Related Tracks, and nothing in 2 PLAYER or FULL BROWSER", () => {
    expect(continuesFrom(related, "one")).toBe(false);
    expect(continuesFrom(related, "simple")).toBe(false);
    for (const source of [playlist, collection, tagList]) {
      expect(continuesFrom(source, "two")).toBe(false);
      expect(continuesFrom(source, "browser")).toBe(false);
    }
  });
});
