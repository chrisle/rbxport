import { describe, expect, it } from "vitest";

import { INITIAL_SYNC, deckStartedPlaying, isFollowing, pressMaster, toggleBeatSync } from "./syncMaster";

describe("two-deck sync master (#615)", () => {
  it("names no master until a deck plays, then the first to play", () => {
    // rekordbox 7 Export, both decks loaded and stopped: MASTER unlit on both.
    expect(INITIAL_SYNC.master).toBeNull();
    // "The Player which starts the playback first becomes the Sync Master."
    const b = deckStartedPlaying(INITIAL_SYNC, "b");
    expect(b.master).toBe("b");
    expect(deckStartedPlaying(b, "a").master).toBe("b");
  });

  it("lets deck A sync to B when B played first", () => {
    let s = deckStartedPlaying(INITIAL_SYNC, "b");
    s = toggleBeatSync(s, "a");
    expect(s.on.a).toBe(true);
    expect(isFollowing(s, "a")).toBe(true);
    expect(isFollowing(s, "b")).toBe(false);
  });

  it("lets the master's own BEAT SYNC light without following", () => {
    let s = deckStartedPlaying(INITIAL_SYNC, "a");
    s = toggleBeatSync(s, "a");
    expect(s.on.a).toBe(true);
    expect(isFollowing(s, "a")).toBe(false);
    // MASTER handed to B: A's BEAT SYNC stays lit and A now follows B.
    s = pressMaster(s, "b");
    expect(s.on.a).toBe(true);
    expect(isFollowing(s, "a")).toBe(true);
  });

  it("keeps a deck's BEAT SYNC when it takes MASTER", () => {
    let s = toggleBeatSync(deckStartedPlaying(INITIAL_SYNC, "a"), "b");
    expect(isFollowing(s, "b")).toBe(true);
    s = pressMaster(s, "b");
    expect(s.on.b).toBe(true);
    expect(isFollowing(s, "b")).toBe(false);
    expect(isFollowing(s, "a")).toBe(false);
  });

  it("lights BEAT SYNC before either deck plays, and follows once the other deck takes MASTER", () => {
    let s = toggleBeatSync(INITIAL_SYNC, "b");
    expect(s.on.b).toBe(true);
    expect(isFollowing(s, "b")).toBe(false);
    s = deckStartedPlaying(s, "a");
    expect(isFollowing(s, "b")).toBe(true);
  });
});
