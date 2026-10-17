import { describe, expect, it } from "vitest";

import { KEYMAP, KEYMAP_GROUPS, keyForPlatform } from "./keymap";
import { BINDINGS } from "./shortcuts";
import { rowsFor } from "@/views/settings/KeyboardPane";

describe("the Export key map", () => {
  it("has the ten groups the pane lists, in its order", () => {
    expect([...KEYMAP_GROUPS]).toEqual([
      "Browse", "Player A", "Player B", "General", "File", "View", "Track", "Playlist", "Help", "Link Export",
    ]);
  });

  it("holds every command the preset binds, and Player B is Player A with shift", () => {
    const bound = Object.values(KEYMAP).flat().filter((r) => r.key !== null);
    expect(bound).toHaveLength(124);
    for (const row of KEYMAP["Player B"]) {
      if (row.key === null) continue;
      expect(row.key.startsWith("shift + ")).toBe(true);
      const twin = KEYMAP["Player A"].find((a) => a.label === row.label);
      expect(twin, row.label).toBeDefined();
    }
  });

  it("opens on the capture's Player A rows in the capture's order", () => {
    const labels = KEYMAP["Player A"].map((r) => r.label);
    expect(labels.slice(0, 12)).toEqual([
      "Play/Pause", "Quantize", "Time Mode", "Cue", "Next Track", "Previous Track", "Memory Cue",
      "Loop In", "Loop Out", "Exit/Reloop", "1/64 Beat Loop", "1/32 Beat Loop",
    ]);
    expect(KEYMAP["Player A"].find((r) => r.label === "1 Beat Loop")?.key).toBe("4");
    expect(KEYMAP["Player A"].find((r) => r.label === "1/64 Beat Loop")?.key).toBeNull();
  });

  it("every binding that names a command names one that exists", () => {
    const ids = new Set(Object.values(KEYMAP).flat().map((r) => r.id));
    for (const binding of BINDINGS) {
      if (binding.command !== undefined) expect(ids.has(binding.command), binding.label).toBe(true);
      else if (!binding.alias) expect(binding.pane, `${binding.label} needs a pane`).toBeDefined();
    }
  });

  it("lists PERFORMANCE mode's fixed beat jumps under each deck, unbound, by rekordbox's ids", () => {
    const fixed = (group: "Player A" | "Player B") =>
      KEYMAP[group].filter((r) => /^(Forward|Backward) /.test(r.label));
    expect(fixed("Player A").map((r) => `${r.id} ${r.label}`)).toEqual([
      "3097 Forward 1/2 beat", "3098 Forward 1 beat", "3099 Forward 2 beats", "309a Forward 4 beats",
      "309b Forward 8 beats", "309c Forward 16 beats", "309d Forward 8 bars",
      "309e Backward 1/2 beat", "309f Backward 1 beat", "30a0 Backward 2 beats", "30a1 Backward 4 beats",
      "30a2 Backward 8 beats", "30a3 Backward 16 beats", "30a4 Backward 8 bars",
    ]);
    expect(fixed("Player B").map((r) => r.id)).toEqual(fixed("Player A").map((r) => r.id.replace(/^30/, "31")));
    for (const row of [...fixed("Player A"), ...fixed("Player B")]) expect(row.key, row.label).toBeNull();
    // The pane draws them live and assignable, for the deck each is filed under.
    expect(rowsFor("Player A", true).find((r) => r.id === "309a"))
      .toMatchObject({ label: "Forward 4 beats", key: null, built: true, bindingId: "jumpForward4Beats", changed: false });
    expect(rowsFor("Player B", true).find((r) => r.id === "31a4"))
      .toMatchObject({ label: "Backward 8 bars", key: null, built: true, bindingId: "b.jumpBackward8Bars" });
    expect(rowsFor("Player A", true, { jumpForward4Beats: { key: "ArrowRight", metaKey: true, altKey: true } })
      .find((r) => r.id === "309a")).toMatchObject({ key: "option + command + cursor right", changed: true });
  });

  it("prints command and option as ctrl and alt on Windows", () => {
    expect(keyForPlatform("shift + command + F", true)).toBe("shift + command + F");
    expect(keyForPlatform("shift + command + F", false)).toBe("shift + ctrl + F");
    expect(keyForPlatform("option + \\", false)).toBe("alt + \\");
  });

  it("the pane's rows are rekordbox's, live where built, plus this app's own", () => {
    const rows = rowsFor("Player A", true);
    expect(rows.find((r) => r.label === "Play/Pause")).toMatchObject({ key: "spacebar", built: true });
    expect(rows.find((r) => r.label === "Loop In")).toMatchObject({ key: "I", built: true, bindingId: "loopIn" });
    expect(rows.find((r) => r.label === "Time Mode")).toMatchObject({ key: "T", built: false, bindingId: null });
    // A key of the person's own is what the row shows, and a key taken
    // away leaves the row without one.
    const own = rowsFor("Player A", true, { loopIn: { key: "l", shiftKey: true }, loopOut: { key: "" } });
    expect(own.find((r) => r.label === "Loop In")).toMatchObject({ key: "shift + L", changed: true });
    expect(own.find((r) => r.label === "Loop Out")).toMatchObject({ key: null, changed: true });
    // The kill buttons have no rekordbox command; they are this app's rows,
    // listed unbound under each deck and assignable.
    expect(rows.find((r) => r.label === "Low Kill")).toMatchObject({ key: null, built: true, bindingId: "a.eqKillLow" });
    expect(rowsFor("Player B", true).find((r) => r.label === "High Kill")).toMatchObject({ bindingId: "b.eqKillHigh" });
    const browse = rowsFor("Browse", true);
    expect(browse.find((r) => r.label === "Search for tracks in the track list")?.built).toBe(true);
    expect(browse.find((r) => r.label === "Select All")).toMatchObject({ key: "command + A", built: true });
    expect(rowsFor("Help", false)).toEqual([]);
    // Playlist holds rekordbox's Renumber Track Order: live, unbound, and
    // assignable, as rekordbox lists it.
    expect(rowsFor("Playlist", true)).toEqual([
      { id: "500e", label: "Renumber Track Order", key: null, built: true, bindingId: "renumberTrackOrder", changed: false },
    ]);
    expect(rowsFor("Playlist", false, { renumberTrackOrder: { key: "r", metaKey: true, altKey: true } })[0])
      .toMatchObject({ key: "alt + ctrl + R", changed: true });
  });
});
