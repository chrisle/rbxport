/**
 * @vitest-environment jsdom
 *
 * Each list keeps its own scroll position (#338).
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


beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  // The virtualizer sizes its window from the scroller's offset box, which
  // jsdom leaves at zero; a screen's worth of rows needs a screen.
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
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
    const PLAYLIST: ViewSpec = { source: { kind: "playlist", id: "cue" }, sort: "trackNo", descending: false, query: "" };
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
});
