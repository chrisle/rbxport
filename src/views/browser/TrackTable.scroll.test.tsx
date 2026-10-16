/**
 * @vitest-environment jsdom
 *
 * Each list keeps its own scroll position, and command + L locates the
 * track a deck holds, as rekordbox 7.2.19 does (#338; see
 * `src/lib/locateLoaded.ts` for the functions read).
 */
import { locateLoadedTrack } from "@/lib/locateLoaded";
import { hasCommand, runCommand } from "@/lib/commands";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { RowDto, ViewSpec } from "@/ipc/types";
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
let viewHeight = 600;
let root: Root;

const COLLECTION: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
const PLAYLIST: ViewSpec = { source: { kind: "playlist", id: "cue" }, sort: "trackNo", descending: false, query: "" };
const columns = resolve(defaultLayout());
const noop = () => {};

/** The mock answers on the microtask queue; a timer lets a chain of them land. */
const settle = async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
};

interface Props {
  spec?: ViewSpec;
  libraryGeneration?: number;
  pendingEdits?: Map<string, { rating: number }>;
  deckTrackIds?: readonly (string | null)[];
  onShowCollection?: () => void;
  onSelectedRow?: (row: RowDto | null) => void;
}

function render(props: Props) {
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
          {...(props.deckTrackIds ? { deckTrackIds: props.deckTrackIds } : {})}
          {...(props.onShowCollection ? { onShowCollection: props.onShowCollection } : {})}
          {...(props.onSelectedRow ? { onSelectedRow: props.onSelectedRow } : {})}
        />
      </PreferencesProvider>,
    );
  });
}


beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  // The virtualizer sizes its window from the scroller's offset box, which
  // jsdom leaves at zero; a screen's worth of rows needs a screen.
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  viewHeight = 600;
  // jsdom has no scrolling of its own: the virtualizer's scrollToIndex lands
  // through scrollTo, so move the scroller as a browser would.
  HTMLElement.prototype.scrollTo = function scrollTo(this: HTMLElement, options?: ScrollToOptions | number) {
    if (typeof options === "object" && options.top !== undefined) this.scrollTop = options.top;
    this.dispatchEvent(new Event("scroll"));
  };
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => viewHeight });
  // The heading plus 400 rows: how far the virtualizer may scroll.
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", { configurable: true, get: () => 24 + 400 * 24 });
  // The waveform cells paint onto a canvas jsdom cannot draw; nothing here
  // reads the paint.
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  const mock = createMockBackend({ trackCount: 400, latencyMs: 0, writable: true });
  __setBackend(mock);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("TrackTable scroll position per list", () => {
  it("returns to where a list was left after visiting another", async () => {
    render({});
    await settle();
    await settle();
    const scroller = host.querySelector('[data-testid="track-scroll"]') as HTMLElement;
    act(() => {
      scroller.scrollTop = 3000;
      scroller.dispatchEvent(new Event("scroll"));
    });
    render({ spec: PLAYLIST });
    await settle();
    await settle();
    expect(scroller.scrollTop).toBe(0);
    render({});
    await settle();
    await settle();
    expect(scroller.scrollTop).toBe(3000);
  });

  async function scrollAway() {
    render({});
    await settle();
    await settle();
    const scroller = host.querySelector('[data-testid="track-scroll"]') as HTMLElement;
    act(() => {
      scroller.scrollTop = 3000;
      scroller.dispatchEvent(new Event("scroll"));
    });
    render({ spec: PLAYLIST });
    await settle();
    await settle();
    return scroller;
  }

  it("opens a list at the top when it is sorted differently from when it was left", async () => {
    const scroller = await scrollAway();
    render({ spec: { ...COLLECTION, sort: "bpm" } });
    await settle();
    await settle();
    expect(scroller.scrollTop).toBe(0);
  });

  it("opens a list at the top when the list area is not as tall as it was", async () => {
    const scroller = await scrollAway();
    viewHeight = 400;
    render({});
    await settle();
    await settle();
    expect(scroller.scrollTop).toBe(0);
  });
});

describe("Locate track loaded on deck (command + L)", () => {
  const ROW_H = 24;
  const press = () => {
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "l", code: "KeyL", ctrlKey: true, bubbles: true }));
    });
  };
  const selected = () => [...host.querySelectorAll('[role="row"][aria-selected="true"]')].length;

  it("selects the deck's track and scrolls it to the middle when it is off screen", async () => {
    const rows: (RowDto | null)[] = [];
    render({ deckTrackIds: ["100300", null], onSelectedRow: (row) => rows.push(row) });
    await settle();
    await settle();
    const scroller = host.querySelector('[data-testid="track-scroll"]') as HTMLElement;
    press();
    await settle();
    await settle();
    expect(rows.at(-1)?.id).toBe("100300");
    // Scrolled to the row (where in the view is the virtualizer's
    // `align: "center"`, which jsdom's zero-sized boxes cannot show).
    expect(scroller.scrollTop).toBeGreaterThan(24 + 300 * ROW_H - 600);
    expect(selected()).toBe(1);
  });

  it("does the same when the Track menu runs the shared command", async () => {
    const rows: (RowDto | null)[] = [];
    render({ deckTrackIds: ["100300", null], onSelectedRow: (row) => rows.push(row) });
    await settle();
    await settle();
    act(() => {
      expect(locateLoadedTrack()).toBe(true);
    });
    await settle();
    await settle();
    expect(rows.at(-1)?.id).toBe("100300");
    expect(selected()).toBe(1);
  });

  it("leaves the list where it is when the row is already on screen", async () => {
    const rows: (RowDto | null)[] = [];
    render({ deckTrackIds: ["100003", null], onSelectedRow: (row) => rows.push(row) });
    await settle();
    await settle();
    const scroller = host.querySelector('[data-testid="track-scroll"]') as HTMLElement;
    press();
    await settle();
    expect(rows.at(-1)?.id).toBe("100003");
    expect(scroller.scrollTop).toBe(0);
  });

  it("takes the loaded decks in turn, Deck 1 first", async () => {
    const rows: (RowDto | null)[] = [];
    render({ deckTrackIds: ["100001", "100002"], onSelectedRow: (row) => rows.push(row) });
    await settle();
    await settle();
    const seen: (string | undefined)[] = [];
    for (let i = 0; i < 3; i++) {
      press();
      await settle();
      seen.push(rows.at(-1)?.id);
    }
    expect(seen).toEqual(["100001", "100002", "100001"]);
  });

  it("opens All Tracks for a track the open list lacks, and selects it there", async () => {
    const mock = createMockBackend({ trackCount: 400, latencyMs: 0, writable: true });
    __setBackend(mock);
    const view = await mock.openView(PLAYLIST);
    const inPlaylist = new Set(await mock.viewIdsInRange(view.viewId, 0, view.len));
    const missing = Array.from({ length: 400 }, (_, i) => String(100000 + i)).find((id) => !inPlaylist.has(id));
    expect(missing).toBeDefined();
    const rows: (RowDto | null)[] = [];
    let shown = 0;
    const deckTrackIds = [missing ?? null, null];
    const onSelectedRow = (row: RowDto | null) => rows.push(row);
    const onShowCollection = () => {
      shown += 1;
    };
    render({ spec: PLAYLIST, deckTrackIds, onShowCollection, onSelectedRow });
    await settle();
    await settle();
    press();
    await settle();
    expect(shown).toBe(1);
    expect(selected()).toBe(0);
    render({ deckTrackIds, onShowCollection, onSelectedRow });
    await settle();
    await settle();
    await settle();
    expect(rows.at(-1)?.id).toBe(missing);
  });

  it("does nothing for an empty deck or one playing a file the collection lacks", async () => {
    let shown = 0;
    render({ spec: PLAYLIST, deckTrackIds: ["file:/Music/loose.mp3", null], onShowCollection: () => { shown += 1; } });
    await settle();
    await settle();
    press();
    await settle();
    expect(shown).toBe(0);
    expect(selected()).toBe(0);
  });
});

describe("the list's commands, from a key and from anywhere else (#431)", () => {
  const key = (init: KeyboardEventInit) => {
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    act(() => {
      window.dispatchEvent(event);
    });
    return event;
  };
  const selectedIds = () =>
    [...host.querySelectorAll('[role="row"][aria-selected="true"]')].map((row) => row.textContent);

  it("moves the cursor the same way for Cursor Down and its command", async () => {
    const rows: (RowDto | null)[] = [];
    render({ onSelectedRow: (row) => rows.push(row) });
    await settle();
    await settle();
    expect(key({ key: "ArrowDown", code: "ArrowDown" }).defaultPrevented).toBe(true);
    await settle();
    const byKey = rows.at(-1)?.id;
    expect(key({ key: "ArrowDown", code: "ArrowDown" }).defaultPrevented).toBe(true);
    await settle();
    const secondByKey = rows.at(-1)?.id;
    act(() => {
      expect(runCommand("moveUp", { scope: "main" })).toBe(true);
    });
    await settle();
    expect(rows.at(-1)?.id).toBe(byKey);
    act(() => {
      expect(runCommand("moveDown", { scope: "main" })).toBe(true);
    });
    await settle();
    expect(rows.at(-1)?.id).toBe(secondByKey);
  });

  it("selects the whole list the same way for Select All and its command", async () => {
    render({});
    await settle();
    await settle();
    expect(key({ key: "a", code: "KeyA", ctrlKey: true }).defaultPrevented).toBe(true);
    await settle();
    const byKey = selectedIds();
    expect(byKey.length).toBeGreaterThan(1);
    // Home rather than Down: after Select All the cursor is on the last row,
    // as rekordbox's is, so Down would scroll the list to its end.
    key({ key: "Home", code: "Home" });
    await settle();
    expect(selectedIds()).toHaveLength(1);
    act(() => {
      expect(runCommand("selectAll", { scope: "main" })).toBe(true);
    });
    await settle();
    expect(selectedIds()).toEqual(byKey);
  });

  it("extends and shrinks the selection the same way for shift + cursor and its commands (#514)", async () => {
    render({});
    await settle();
    await settle();
    key({ key: "ArrowDown", code: "ArrowDown" });
    await settle();
    const first = selectedIds();
    expect(first).toHaveLength(1);
    expect(key({ key: "ArrowDown", code: "ArrowDown", shiftKey: true }).defaultPrevented).toBe(true);
    await settle();
    expect(key({ key: "ArrowDown", code: "ArrowDown", shiftKey: true }).defaultPrevented).toBe(true);
    await settle();
    const three = selectedIds();
    expect(three).toHaveLength(3);
    expect(three[0]).toBe(first[0]);
    // Reversing shrinks back toward the row it started from.
    expect(key({ key: "ArrowUp", code: "ArrowUp", shiftKey: true }).defaultPrevented).toBe(true);
    await settle();
    expect(selectedIds()).toEqual(three.slice(0, 2));
    act(() => {
      expect(runCommand("extendDown", { scope: "main" })).toBe(true);
    });
    await settle();
    expect(selectedIds()).toEqual(three);
    act(() => {
      expect(runCommand("extendUp", { scope: "main" })).toBe(true);
      expect(runCommand("extendUp", { scope: "main" })).toBe(true);
    });
    await settle();
    expect(selectedIds()).toEqual(first);
  });

  it("answers only for the list it is, and not once the list has gone", async () => {
    render({});
    await settle();
    expect(hasCommand("selectAll", "main")).toBe(true);
    expect(hasCommand("selectAll", "sub")).toBe(false);
    act(() => {
      root.render(<></>);
    });
    expect(hasCommand("selectAll", "main")).toBe(false);
  });
});
