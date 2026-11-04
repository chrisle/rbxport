import { describe, expect, it } from "vitest";

import {
  capacityText, contentsText, deviceId, deviceNodes, devicePath, formatSpace, fullness, hasRoomFor,
  renamedDevice,
} from "./devices";
import type { Device } from "@/ipc/types";

const stick: Device = {
  name: "DJ STICK",
  path: "/Volumes/DJ STICK",
  totalBytes: 32 * 1024 ** 3,
  freeBytes: 8 * 1024 ** 3,
  removable: true,
  volumeId: "dev:1",
  export: null,
};

describe("devices", () => {
  it("round-trips a mount point through a tree id", () => {
    expect(devicePath(deviceId(stick))).toBe("/Volumes/DJ STICK");
    expect(devicePath("playlist-17")).toBeNull();
  });

  it("makes a flat set of tree nodes", () => {
    const nodes = deviceNodes([stick]);
    expect(nodes).toEqual([
      { id: "device:/Volumes/DJ STICK", name: "DJ STICK", kind: "device", depth: 0 },
    ]);
  });

  it("reports capacity in the units a stick is sold in", () => {
    expect(capacityText(stick)).toBe("8.0 GB free of 32.0 GB");
    expect(fullness(stick)).toBeCloseTo(0.75);
  });

  it("prints the space table's figures the way rekordbox does", () => {
    // The TEST stick's capture: a 1.5 TB card reads 1,430.3 GB.
    expect(formatSpace(1_535_800_000_000)).toBe("1,430.3 GB");
    expect(formatSpace(216_800_000_000)).toBe("201.9 GB");
    expect(formatSpace(32 * 1024 ** 3)).toBe("32.0 GB");
    expect(formatSpace(0)).toBe("");
  });

  it("says nothing about a size it does not know", () => {
    const unknown = { ...stick, totalBytes: 0, freeBytes: 0 };
    expect(capacityText(unknown)).toBe("");
    expect(fullness(unknown)).toBeNull();
    // And never claims a device is too full to write to.
    expect(hasRoomFor(unknown, 10 ** 12)).toBe(true);
  });

  it("says an empty device is empty", () => {
    expect(contentsText(stick)).toBe("No export on this device yet.");
  });

  it("says a stick being written to is busy, not empty", () => {
    expect(contentsText({ ...stick, busy: true })).toMatch(/Being written to/);
  });

  it("says a sync will be incremental when it can be", () => {
    const ours = {
      ...stick,
      export: { tracks: 120, playlists: 3, ours: true, written: "2026-09-08 00:12:00.000 +00:00" },
    };
    expect(contentsText(ours)).toBe(
      "120 tracks in 3 playlists, last synced 2026-09-08 00:12. Only what changed will be copied.",
    );
  });

  it("warns that a rekordbox stick will be written in full", () => {
    const theirs = {
      ...stick,
      export: { tracks: 1, playlists: 1, ours: false, written: "" },
    };
    expect(contentsText(theirs)).toBe(
      "1 track in 1 playlist, written by rekordbox. Exporting here writes everything again.",
    );
  });

  it("knows when a stick is too full", () => {
    expect(hasRoomFor(stick, 1024 ** 3)).toBe(true);
    expect(hasRoomFor(stick, 30 * 1024 ** 3)).toBe(false);
  });

  it("follows a renamed stick by its volume id, not by its path", () => {
    const renamed: Device = { ...stick, name: "USB B", path: "/Volumes/USB B" };
    const other: Device = { ...stick, name: "SD", path: "/Volumes/SD", volumeId: "dev:9" };
    const moved = renamedDevice([other, renamed], stick.path, [stick, other]);
    expect(moved?.device.path).toBe("/Volumes/USB B");
    expect(moved?.oldName).toBe("DJ STICK");
    // Unplugged, not renamed: nothing in the fresh list carries its id.
    expect(renamedDevice([other], stick.path, [stick, other])).toBeNull();
    // Never seen before: nothing to follow.
    expect(renamedDevice([renamed], stick.path, [])).toBeNull();
  });
});
