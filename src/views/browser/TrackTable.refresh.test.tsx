/**
 * @vitest-environment jsdom
 *
 * What a library change costs the rows on screen.
 *
 * A rating, a comment, a track a player loads over LINK: each makes the
 * backend reload and announce a new generation, and every open list fetches
 * its rows again. That used to be drawn as a view switch — every visible row
 * to a skeleton, the whole list torn down and rebuilt, every row rebuilt
 * again as its page landed — for a change to one cell of one row. These hold
 * that a refresh keeps the rows' DOM and only rewrites what changed.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { ViewSpec } from "@/ipc/types";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { PreferencesProvider } from "@/store/usePreferences";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { defaultLayout, resolve } from "@/lib/columns";
import { TrackTable } from "./TrackTable";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let opens: number;
let fetches: number;

const COLLECTION: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
const columns = resolve(defaultLayout());
const noop = () => {};

/** The mock answers on the microtask queue; a timer lets a chain of them land. */
const settle = async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
};

function render(props: { spec?: ViewSpec; libraryGeneration?: number; pendingEdits?: Map<string, { rating: number }> }) {
  act(() => {
    root.render(
      <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: noop, reset: noop }}>
        <TrackTable
          spec={props.spec ?? COLLECTION}
          onSortChange={noop}
          title="Collection"
          query=""
          onQueryChange={noop}
          columns={columns}
          onColumnMove={noop}
          onColumnResize={noop}
          onColumnToggle={noop}
          onColumnAutoSize={noop}
          onColumnAutoSizeAll={noop}
          libraryGeneration={props.libraryGeneration ?? 0}
          {...(props.pendingEdits ? { pendingEdits: props.pendingEdits } : {})}
          onRate={noop}
        />
      </PreferencesProvider>,
    );
  });
}

/** The real rows on screen; a skeleton carries no row role. */
const rows = () => [...host.querySelectorAll('[role="row"][aria-selected]')];
const titles = () => rows().map((row) => row.querySelector('[data-col="title"]')?.textContent ?? "");
const list = () => host.querySelector('[data-testid="track-scroll"]')!.children[1]!;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  // The virtualizer sizes its window from the scroller's offset box, which
  // jsdom leaves at zero; a screen's worth of rows needs a screen.
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  // The waveform cells paint onto a canvas jsdom cannot draw; nothing here
  // reads the paint.
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  const mock = createMockBackend({ trackCount: 62, latencyMs: 0, writable: true });
  opens = 0;
  fetches = 0;
  __setBackend({
    ...mock,
    openView: (spec) => {
      opens += 1;
      return mock.openView(spec);
    },
    fetchRows: (...args) => {
      fetches += 1;
      return mock.fetchRows(...args);
    },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("TrackTable, when the library changes under it", () => {
  it("keeps every row on screen and in place while its page is fetched again", async () => {
    render({});
    await settle();
    await settle();
    const before = rows();
    const container = list();
    expect(before.length).toBeGreaterThan(10);
    opens = 0;
    fetches = 0;

    // What the app does on `library:changed`.
    render({ libraryGeneration: 1 });
    // No skeletons in between: the rows that were there stay there.
    expect(rows().length).toBe(before.length);
    expect(list()).toBe(container);

    await settle();
    await settle();
    expect(opens).toBe(1);
    expect(fetches).toBe(1);
    // The same DOM nodes, not rebuilt ones.
    expect(list()).toBe(container);
    const after = rows();
    expect(after.length).toBe(before.length);
    before.forEach((row, i) => expect(after[i]).toBe(row));
  });

  it("keeps a rating lit across the reload that carries it", async () => {
    render({});
    await settle();
    await settle();
    const first = rows()[0]!;
    let id = "";
    await act(async () => {
      const backend = await import("@/ipc/client").then((m) => m.getBackend());
      id = (await backend.fetchRows(1, 0, 1, []))[0]!.id;
    });
    const lit = () => first.querySelector('[role="radio"][aria-checked="true"]')?.getAttribute("aria-label");

    render({ pendingEdits: new Map([[id, { rating: 4 }]]) });
    expect(lit()).toBe("4 of 5");
    // The reload lands: overlay gone, generation bumped, page in flight.
    render({ libraryGeneration: 1 });
    expect(lit()).toBe("4 of 5");
    await settle();
    await settle();
  });

  it("moves the rows into their new order on a re-sort rather than rebuilding them", async () => {
    render({});
    await settle();
    await settle();
    const titleOf = (row: Element) => row.querySelector('[data-col="title"]')?.textContent ?? "";
    const before = new Map(rows().map((row) => [row, titleOf(row)]));

    render({ spec: { ...COLLECTION, sort: "title" } });
    // The old order stands until the sorted page lands; nothing goes blank.
    expect(rows().length).toBe(before.size);
    await settle();
    await settle();
    const after = titles();
    expect(after).toEqual([...after].sort((a, b) => a.localeCompare(b)));
    // The tracks on screen both before and after — at least eighteen of the
    // sixty-two, whichever forty each order puts first — are the same nodes,
    // moved, and each still shows its own track.
    const kept = rows().filter((row) => before.has(row));
    expect(kept.length).toBeGreaterThanOrEqual(18);
    for (const row of kept) expect(titleOf(row)).toBe(before.get(row));
  });

  it("builds the list afresh when the source changes", async () => {
    render({});
    await settle();
    await settle();
    const container = list();
    expect(rows().length).toBeGreaterThan(10);

    render({ spec: { ...COLLECTION, source: { kind: "tagList" } } });
    // Another list's rows are no picture of this one: they go at once.
    expect(rows().length).toBe(0);
    expect(list()).not.toBe(container);
    await settle();
    await settle();
  });
});
