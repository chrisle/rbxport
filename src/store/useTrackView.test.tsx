/**
 * @vitest-environment jsdom
 *
 * What a view does when it is opened before the library is up.
 *
 * The backend reads the library on its own thread, so the first `open_view`
 * can arrive before there is anything to answer with and comes back
 * "The library has not finished loading yet." That is not a failure worth
 * showing anyone — it is a race the app is expected to lose sometimes and
 * recover from. It did not recover: the open effect keys on the spec, so a
 * view that failed stayed at zero rows until the spec changed, and a table
 * with a count of zero draws nothing at any scroll position.
 *
 * `App.tsx` already learned this for the tree; these hold the same line for
 * the rows.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Backend, RowDto, ViewSpec } from "@/ipc/types";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let useTrackView: typeof import("./useTrackView").useTrackView;
let setBackend: typeof import("@/ipc/client").__setBackend;

/** Everything the test drives from outside. */
let ready: boolean;
let readyListeners: Set<() => void>;
let opens: number;
let tagListListeners: Set<() => void>;
/** What the rows say past their number, so a fetched-again row can be told from the one before. */
let stamp: string;
/** Ratings the library holds, as a reload would carry them. */
let ratings: Record<string, number>;
/** Pages answered by hand, in whichever order the test chooses. */
let holdFetches: boolean;
let heldFetches: Array<() => void>;
/** The library generation the app passes down. */
let generation: number;

const SPEC: ViewSpec = {
  source: { kind: "collection" },
  sort: "trackNo",
  descending: false,
  query: "",
};

function row(i: number): RowDto {
  return {
    id: String(i),
    trackNo: i + 1,
    title: `Track ${i}${stamp}`,
    artist: "",
    album: "",
    genre: "",
    label: "",
    comment: "",
    bpmX100: 0,
    key: "",
    durationSec: 0,
    rating: ratings[String(i)] ?? 0,
    analysed: 0,
    dateAdded: "",
    releaseDate: "",
    hotCues: [],
    hasArtwork: false,
    artworkHue: 0,
  };
}

/** A backend that refuses everything until the library is released. */
function makeBackend(): Backend {
  return {
    openView: (_spec: ViewSpec) => {
      opens += 1;
      // Every open is a new view with a new generation, as the backend
      // answers one after an edit.
      return ready
        ? Promise.resolve({ viewId: opens, len: 500, gen: opens })
        : Promise.reject(new Error("The library has not finished loading yet."));
    },
    fetchRows: (_viewId: number, offset: number, len: number) => {
      if (!ready) return Promise.reject(new Error("The library has not finished loading yet."));
      const rows = () => Array.from({ length: len }, (_, i) => row(offset + i));
      if (!holdFetches) return Promise.resolve(rows());
      // Answered when the test says, with the rows as they read then.
      return new Promise<RowDto[]>((resolve) => {
        heldFetches.push(() => resolve(rows()));
      });
    },
    onLibraryReady: (listener: () => void) => {
      readyListeners.add(listener);
      return () => readyListeners.delete(listener);
    },
    // The hook also listens for cue edits, to patch a cached row's letters
    // in place. Nothing here edits a cue, so the subscription is inert.
    onCuesChanged: () => () => {},
    onTagListChanged: (listener: () => void) => {
      tagListListeners.add(listener);
      return () => tagListListeners.delete(listener);
    },
  } as unknown as Backend;
}

function release() {
  ready = true;
  for (const listener of readyListeners) listener();
}

/** Drains the promise queue; several passes, the chain is a few deep. */
const settle = async () => {
  await act(async () => {
    for (let i = 0; i < 30; i++) await Promise.resolve();
  });
};

let latest: {
  count: number;
  error: string | null;
  loading: boolean;
  rowAt: (i: number) => RowDto | undefined;
  fresh: (i: number) => boolean;
};

/** The scroll position the stand-in table is showing. */
let window_: { start: number; end: number } = { start: 0, end: 32 };
/** The last screen of the previous run, as `App` hands it over. */
let seed: { count: number; rows: RowDto[] } | undefined;
let edits: ReadonlyMap<string, Partial<RowDto>> | undefined;

let spec: ViewSpec = SPEC;

let extraColumns: string[] = [];

function Probe() {
  const view = useTrackView(spec, generation, edits, seed, extraColumns);
  latest = view;
  // A table asks for the window it is showing; this stands in for that.
  view.ensureRange(window_.start, window_.end);
  return null;
}

beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.resetModules();
  ready = false;
  readyListeners = new Set();
  opens = 0;
  tagListListeners = new Set();
  spec = SPEC;
  window_ = { start: 0, end: 32 };
  seed = undefined;
  edits = undefined;
  stamp = "";
  ratings = {};
  holdFetches = false;
  heldFetches = [];
  generation = 0;
  extraColumns = [];
  ({ __setBackend: setBackend } = await import("@/ipc/client"));
  setBackend(makeBackend());
  ({ useTrackView } = await import("./useTrackView"));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setBackend(null);
});

describe("useTrackView, against a library that is not up yet", () => {
  it("reuses pending overlay rows until the patch changes or clears", async () => {
    ready = true;
    edits = new Map([["0", { rating: 5 }]]);
    act(() => root.render(<Probe />));
    await settle();
    const first = latest.rowAt(0);
    expect(first?.rating).toBe(5);
    expect(latest.rowAt(0)).toBe(first);
    edits = new Map([["0", { rating: 3 }]]);
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.rating).toBe(3);
    expect(latest.rowAt(0)).not.toBe(first);
    edits = undefined;
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.rating).toBe(0);
  });
  it("opens the view itself once the library becomes ready", async () => {
    act(() => {
      root.render(<Probe />);
    });
    await settle();
    // Correct so far: there genuinely is nothing to show.
    expect(latest.count).toBe(0);

    release();
    await settle();

    // The spec never changed. Nobody clicked anything. The rows must arrive
    // anyway, or the window stays blank until the app is restarted.
    expect(latest.count).toBe(500);
    expect(latest.error).toBeNull();
  });

  it("fills the rows of the window it was already showing", async () => {
    act(() => {
      root.render(<Probe />);
    });
    await settle();
    release();
    await settle();
    await settle();

    expect(latest.rowAt(0)?.title).toBe("Track 0");
  });

  it("serves real rows past the seed once the library arrives", async () => {
    // What the window actually looked like. `App` hands the table the last
    // screen of the previous run so it is not empty while the library loads:
    // the *count* is the whole library, so the list has its full height and a
    // working scrollbar, but only the first screenful of rows exists.
    //
    // So a view that failed to open looked perfectly healthy at the top and
    // went blank the moment it was scrolled — every row past the seed is a
    // placeholder, for ever. That is the bug as it was reported: pick the
    // playlists root, drag the scrollbar halfway, see nothing.
    seed = { count: 500, rows: Array.from({ length: 30 }, (_, i) => row(i)) };

    act(() => {
      root.render(<Probe />);
    });
    await settle();

    // The shape of the trap: full height, a good first screen, nothing under it.
    expect(latest.count).toBe(500);
    expect(latest.rowAt(0)?.title).toBe("Track 0");
    expect(latest.rowAt(400)).toBeUndefined();

    // Now scroll halfway, exactly as the report describes.
    window_ = { start: 400, end: 432 };
    release();
    await settle();
    await settle();

    expect(latest.rowAt(400)?.title).toBe("Track 400");
  });

  it("does not reopen a view that opened perfectly well", async () => {
    ready = true;
    act(() => {
      root.render(<Probe />);
    });
    await settle();
    expect(latest.count).toBe(500);

    const before = opens;
    // A later ready event — a library reload, say — must not make every open
    // view refetch itself.
    for (const listener of readyListeners) listener();
    await settle();
    expect(opens).toBe(before);
  });
});

describe("useTrackView, when the library changes under an open view", () => {
  /** An opened view with its first page on screen. */
  const opened = async () => {
    ready = true;
    act(() => root.render(<Probe />));
    await settle();
    await settle();
    expect(latest.rowAt(0)?.title).toBe("Track 0");
    expect(latest.fresh(0)).toBe(true);
  };

  it("keeps drawing the rows it has until the fetched-again page lands", async () => {
    await opened();
    const before = opens;

    // An edit: the backend reloaded and announced a new generation. The
    // rows it had are the picture until the new page arrives — not a
    // screen of skeletons, and not a view that reads as loading.
    stamp = " (edited)";
    generation = 1;
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.title).toBe("Track 0");
    expect(latest.fresh(0)).toBe(false);
    expect(latest.loading).toBe(false);
    expect(latest.count).toBe(500);

    await settle();
    await settle();
    expect(opens).toBe(before + 1);
    expect(latest.rowAt(0)?.title).toBe("Track 0 (edited)");
    expect(latest.fresh(0)).toBe(true);
  });

  it("keeps the overlay on a stale row until its fresh page lands", async () => {
    await opened();
    // The star is lit from the overlay the moment it is clicked.
    edits = new Map([["0", { rating: 5 }]]);
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.rating).toBe(5);

    // The reload lands: the app drops the overlay and bumps the generation
    // in the same breath. The star must not go out while the page is
    // fetched again.
    ratings = { "0": 5 };
    edits = undefined;
    generation = 1;
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.rating).toBe(5);
    expect(latest.fresh(0)).toBe(false);

    await settle();
    await settle();
    expect(latest.fresh(0)).toBe(true);
    expect(latest.rowAt(0)?.rating).toBe(5);
  });

  it("forgets a held edit once a fresh row arrives without it", async () => {
    await opened();
    edits = new Map([["0", { rating: 5 }]]);
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.rating).toBe(5);

    // Refused: the overlay goes, nothing reloads, the row shows what the
    // library holds.
    edits = undefined;
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.rating).toBe(0);

    // A later, unrelated reload must not resurrect the refused star for the
    // length of its fetch.
    generation = 1;
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.rating).toBe(0);
    await settle();
    await settle();
    expect(latest.rowAt(0)?.rating).toBe(0);
  });

  it("fetches the pages again when an extra column is shown", async () => {
    await opened();
    const before = opens;
    // The spec the backend sees is the same; the rows it answers with are
    // not, since the column's field is only fetched when it is on show. The
    // pages kept from before have no such field, so they must be replaced —
    // which rests on every open answering with a view of its own.
    stamp = " (with size)";
    extraColumns = ["size"];
    act(() => root.render(<Probe />));
    expect(latest.rowAt(0)?.title).toBe("Track 0");
    await settle();
    await settle();
    expect(opens).toBe(before + 1);
    expect(latest.rowAt(0)?.title).toBe("Track 0 (with size)");
  });

  it("empties the rows when the source changes", async () => {
    await opened();
    spec = { ...SPEC, source: { kind: "tagList" } };
    act(() => root.render(<Probe />));
    // One playlist's rows are no picture of another's.
    expect(latest.rowAt(0)).toBeUndefined();
    expect(latest.loading).toBe(true);
    await settle();
    await settle();
    expect(latest.loading).toBe(false);
    expect(latest.rowAt(0)?.title).toBe("Track 0");
  });

  it("drops a page answered for the view before, whichever order the answers come in", async () => {
    ready = true;
    holdFetches = true;
    act(() => root.render(<Probe />));
    await settle();
    await settle();
    // The first view's page 0 is in flight.
    expect(heldFetches.length).toBe(1);
    const stale = heldFetches[0]!;

    generation = 1;
    act(() => root.render(<Probe />));
    await settle();
    await settle();
    // So is the reopened view's.
    expect(heldFetches.length).toBe(2);
    const fresh = heldFetches[1]!;

    stamp = " (fresh)";
    act(() => fresh());
    await settle();
    expect(latest.rowAt(0)?.title).toBe("Track 0 (fresh)");

    // The old answer lands last. It belongs to a view that is gone, and
    // writing it would put the stale rows over the fresh ones.
    stamp = " (stale)";
    act(() => stale());
    await settle();
    expect(latest.rowAt(0)?.title).toBe("Track 0 (fresh)");
    expect(latest.fresh(0)).toBe(true);
  });
});

describe("useTrackView, when a player or the menu tags a track", () => {
  const tagListChanged = async () => {
    act(() => {
      for (const listener of tagListListeners) listener();
    });
    await settle();
  };

  it("keeps a playlist's view and pages", async () => {
    ready = true;
    act(() => root.render(<Probe />));
    await settle();
    await settle();
    const before = opens;
    const shown = latest.rowAt(0);
    expect(shown?.title).toBe("Track 0");

    await tagListChanged();

    expect(opens).toBe(before);
    expect(latest.rowAt(0)).toBe(shown);
  });

  it("reopens the Tag List", async () => {
    ready = true;
    spec = { ...SPEC, source: { kind: "tagList" } };
    act(() => root.render(<Probe />));
    await settle();
    const before = opens;

    await tagListChanged();

    expect(opens).toBe(before + 1);
  });
});
