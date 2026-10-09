import { beforeEach, describe, expect, it } from "vitest";

import type { MissingTrack } from "@/ipc/types";
import {
  fileNameOf, folderOf, forgetRelocateFolder, listIds, missingAmong, movedRoots, relocateTracks, type RelocateSteps,
} from "./relocate";

const track = (id: string, path: string): MissingTrack => ({ id, title: `Title ${id}`, artist: "", album: "", path });

function listed(tracks: MissingTrack[]): AsyncIterator<MissingTrack> {
  let at = 0;
  return {
    next: () => Promise.resolve(at < tracks.length
      ? { done: false, value: tracks[at++] as MissingTrack }
      : { done: true, value: undefined }),
  };
}

/** Steps that answer from queues and write down what was asked. */
function scripted(answers: { picks?: (string | null)[]; findOthers?: boolean[]; taken?: string[]; found?: number }) {
  const said: string[] = [];
  const steps: RelocateSteps = {
    choose: (t, folder) => {
      said.push(`choose ${t.id} in ${folder ?? "-"}`);
      const pick = answers.picks?.shift();
      return Promise.resolve(pick === undefined ? null : pick);
    },
    relocate: (t, path) => {
      const ok = !(answers.taken ?? []).includes(path);
      said.push(`${ok ? "relocate" : "refuse"} ${t.id} -> ${path}`);
      return Promise.resolve(ok);
    },
    alreadyInCollection: () => {
      said.push("already in the collection");
      return Promise.resolve();
    },
    askFindOthers: (title) => {
      said.push(`ask about ${title}`);
      return Promise.resolve(answers.findOthers?.shift() ?? false);
    },
    findOthers: (ids, from, to) => {
      said.push(`find ${ids.join(",")} from ${from} to ${to}`);
      return Promise.resolve(answers.found ?? 0);
    },
    found: (count) => {
      said.push(`found ${count}`);
      return Promise.resolve();
    },
  };
  return { said, steps };
}

describe("Relocate over several tracks, as rekordbox's relocateSelectedFiles runs it", () => {
  beforeEach(() => forgetRelocateFolder());

  it("takes the folder names both paths share off their ends", () => {
    expect(movedRoots("/Users/a/Music/X/Y", "/Volumes/B/Music/x/Y")).toEqual(["/Users/a", "/Volumes/B"]);
    expect(movedRoots("C:\\Music\\A", "D:\\Music\\A")).toEqual(["C:", "D:"]);
    // Nothing shared: the folders as they are.
    expect(movedRoots("/old/A", "/new/B")).toEqual(["/old/A", "/new/B"]);
    // Never down to the bare root.
    expect(movedRoots("/A", "/A")).toEqual(["/A", "/A"]);
  });

  it("reads a path's folder and file name either way round", () => {
    expect(folderOf("/a/b/c.mp3")).toBe("/a/b");
    expect(folderOf("C:\\a\\c.mp3")).toBe("C:\\a");
    expect(fileNameOf("C:\\a\\c.mp3")).toBe("c.mp3");
  });

  it("asks for the first file, then offers to find the rest from its location", async () => {
    const { said, steps } = scripted({ picks: ["/new/Music/X/1.mp3"], findOthers: [true], found: 2 });
    const done = await relocateTracks(listed([
      track("1", "/old/Music/X/1.mp3"),
      track("2", "/old/Music/Y/2.mp3"),
      track("3", "/old/Music/Z/3.mp3"),
    ]), steps);
    expect(said).toEqual([
      "choose 1 in -",
      "relocate 1 -> /new/Music/X/1.mp3",
      "ask about Title 1",
      "find 2,3 from /old to /new",
      "found 2",
    ]);
    expect(done).toBe(3);
  });

  it("opens the next chooser where the last file was when the offer is declined", async () => {
    const { said, steps } = scripted({ picks: ["/new/1.mp3", "/other/2.mp3"], findOthers: [false] });
    await relocateTracks(listed([track("1", "/old/1.mp3"), track("2", "/old/2.mp3")]), steps);
    expect(said).toEqual([
      "choose 1 in -",
      "relocate 1 -> /new/1.mp3",
      "ask about Title 1",
      "choose 2 in /new",
      "relocate 2 -> /other/2.mp3",
    ]);
    // The session remembers it for the next run, as rekordbox's does.
    const again = scripted({});
    await relocateTracks(listed([track("3", "/old/3.mp3")]), again.steps);
    expect(again.said).toEqual(["choose 3 in /other"]);
  });

  it("ends the run when a chooser is cancelled", async () => {
    const { said, steps } = scripted({ picks: [null] });
    expect(await relocateTracks(listed([track("1", "/old/1.mp3"), track("2", "/old/2.mp3")]), steps)).toBe(0);
    expect(said).toEqual(["choose 1 in -"]);
  });

  it("refuses a file the collection already holds and goes on", async () => {
    const { said, steps } = scripted({ picks: ["/held.mp3"], taken: ["/held.mp3"], findOthers: [false] });
    expect(await relocateTracks(listed([track("1", "/old/1.mp3"), track("2", "/old/2.mp3")]), steps)).toBe(0);
    expect(said).toEqual([
      "choose 1 in -",
      "refuse 1 -> /held.mp3",
      "already in the collection",
      "ask about Title 1",
      "choose 2 in /",
    ]);
  });

  it("fetches the named tracks a page at a time, in order", async () => {
    const asked: string[][] = [];
    const tracks = missingAmong(["a", "b", "c"], (page) => {
      asked.push(page);
      return Promise.resolve(page.filter((id) => id !== "b").map((id) => track(id, `/x/${id}.mp3`)));
    }, 2);
    const got: string[] = [];
    for await (const t of tracks) got.push(t.id);
    expect(got).toEqual(["a", "c"]);
    expect(asked).toEqual([["a", "b"], ["c"]]);
  });

  it("takes a whole list's ids, a page at a time, before anything changes it", async () => {
    // 5 rows in pages of 2: three fetches, the last short.
    const rows = ["a", "b", "c", "d", "e"].map((id) => track(id, `/x/${id}.mp3`));
    const asked: number[] = [];
    const ids = await listIds((offset, limit) => {
      asked.push(offset);
      return Promise.resolve(rows.slice(offset, offset + limit));
    }, 2);
    expect(ids).toEqual(["a", "b", "c", "d", "e"]);
    expect(asked).toEqual([0, 2, 4]);
    // A list that is a whole number of pages ends on an empty one.
    expect(await listIds((offset, limit) => Promise.resolve(rows.slice(0, 4).slice(offset, offset + limit)), 2))
      .toEqual(["a", "b", "c", "d"]);
  });

  it("relocates every track of a list that rescans without each relocated one", async () => {
    // [#201 review] The manager's list loses a row with every relocate. With
    // the ids taken first, the run reaches all of them; paging the live list
    // as it went would skip "c".
    let live = ["a", "b", "c", "d", "e"].map((id) => track(id, `/old/${id}.mp3`));
    const page = (offset: number, limit: number) => Promise.resolve(live.slice(offset, offset + limit));
    const ids = await listIds(page, 2);
    const tracks = missingAmong(ids, (some) => Promise.resolve(live.filter((t) => some.includes(t.id))), 2);
    const base = scripted({ picks: ["/new/a.mp3", "/new/b.mp3", "/new/c.mp3", "/new/d.mp3", "/new/e.mp3"], findOthers: [false, false, false, false] });
    const steps: RelocateSteps = {
      ...base.steps,
      relocate: (t, path) => {
        live = live.filter((row) => row.id !== t.id);
        return base.steps.relocate(t, path);
      },
    };
    const said = base.said;
    expect(await relocateTracks(tracks, steps)).toBe(5);
    expect(said.filter((line) => line.startsWith("relocate"))).toHaveLength(5);
  });
});
