/**
 * @vitest-environment jsdom
 *
 * Issue 540: a played track's row is drawn in rekordbox's played green. The
 * rows marked are the backend's answer to `playedTracks`, fetched again when
 * it says they changed (a play the deck recorded, a player's history add over
 * LINK, RESET) and when LINK turns on or off.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { Backend, ViewSpec } from "@/ipc/types";
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
let backend: Backend;
let linkOn = false;
let linkListener: (() => void) | null = null;
const linkPlayed = new Set<string>();

const COLLECTION: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
const columns = resolve(defaultLayout());
const noop = () => {};

const settle = async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
};

function render() {
  act(() => {
    root.render(
      <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: noop, reset: noop }}>
        <TrackTable
          spec={COLLECTION}
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
          libraryGeneration={0}
        />
      </PreferencesProvider>,
    );
  });
}

const rows = () => [...host.querySelectorAll<HTMLElement>('[role="row"][aria-selected]')];
const playedTitles = () => rows()
  .filter((row) => row.dataset.played !== undefined)
  .map((row) => row.querySelector('[data-col="title"]')?.textContent ?? "");

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  const mock = createMockBackend({ trackCount: 20, latencyMs: 0, writable: true });
  linkOn = false;
  linkListener = null;
  linkPlayed.clear();
  // A player's history adds, as the real backend holds them: kept, and in
  // the answer only while LINK is on.
  backend = {
    ...mock,
    playedTracks: async () => [...new Set([...await mock.playedTracks(), ...(linkOn ? linkPlayed : [])])],
    onLinkStatus: (listener) => {
      linkListener = () => { void mock.linkStatus().then(listener); };
      return () => { linkListener = null; };
    },
  };
  __setBackend(backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("TrackTable, played tracks", () => {
  it("colours a track once the deck records its play, and none after RESET", async () => {
    render();
    await settle();
    await settle();
    expect(rows().length).toBeGreaterThan(3);
    expect(playedTitles()).toEqual([]);

    const view = await backend.openView(COLLECTION);
    const [first, , third] = await backend.fetchRows(view.viewId, 0, 3);
    await act(async () => {
      await backend.edits.recordPlay(first!.id);
      await backend.edits.recordPlay(third!.id);
    });
    await settle();
    expect(playedTitles()).toEqual([first!.title, third!.title]);

    await act(async () => { await backend.resetPlayedTracks(); });
    await settle();
    expect(playedTitles()).toEqual([]);
  });

  it("shows a player's plays only while LINK is on", async () => {
    render();
    await settle();
    await settle();
    const view = await backend.openView(COLLECTION);
    const [first] = await backend.fetchRows(view.viewId, 0, 1);
    linkPlayed.add(first!.id);
    // A play the backend announces while LINK is off is held back.
    act(() => { linkListener?.(); });
    await settle();
    expect(playedTitles()).toEqual([]);

    linkOn = true;
    act(() => { linkListener?.(); });
    await settle();
    expect(playedTitles()).toEqual([first!.title]);

    linkOn = false;
    act(() => { linkListener?.(); });
    await settle();
    expect(playedTitles()).toEqual([]);
  });
});
