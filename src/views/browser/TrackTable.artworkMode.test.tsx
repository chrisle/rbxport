/**
 * @vitest-environment jsdom
 *
 * The browser's artwork display pattern (#575): rekordbox's two buttons
 * beside the MASTER menu, "Show the upper part of the artwork." and "Show the
 * entire artwork.". The values pinned here are read from rekordbox 7.2.19's
 * `browse::ListViewer` (`getFullArtworkRowHeight` is twice
 * `getPartialArtworkRowHeight`; `updateRowHeight` sets header column 61, the
 * Artwork column, to the doubled height; `movePositionToSelectedRow` puts the
 * first selected row at the top of the list).
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
import type { ArtworkMode } from "@/lib/session";
import { columnsForArtworkMode, rowHeightForArtworkMode, TrackTable } from "./TrackTable";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

const COLLECTION: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
const columns = resolve(defaultLayout());
const noop = () => {};

const settle = async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
};

function render(props: { artworkMode?: ArtworkMode; onArtworkMode?: (mode: ArtworkMode) => void }) {
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
          onRate={noop}
          {...(props.artworkMode ? { artworkMode: props.artworkMode } : {})}
          {...(props.onArtworkMode ? { onArtworkMode: props.onArtworkMode } : {})}
        />
      </PreferencesProvider>,
    );
  });
}

const browser = () => host.querySelector('[data-browser-pane="list"]') as HTMLElement;
const rowHeight = () => browser().style.getPropertyValue("--s-row-height");
const artworkWidth = () => {
  const at = columns.findIndex((col) => col.key === "artwork");
  return browser().style.getPropertyValue("--cols").split(" ")[at];
};
const button = (mode: ArtworkMode) => host.querySelector<HTMLButtonElement>(`[data-testid="artwork-mode-${mode}"]`);

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  Object.defineProperty(HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "scrollHeight", { configurable: true, get: () => 40000 });
  HTMLElement.prototype.scrollTo = function scrollTo(this: HTMLElement, options?: ScrollToOptions | number) {
    if (typeof options === "object" && options.top !== undefined) this.scrollTop = options.top;
    this.dispatchEvent(new Event("scroll"));
  };
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  __setBackend(createMockBackend({ trackCount: 400, latencyMs: 0, writable: true }));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("artwork display pattern", () => {
  it("doubles the row and squares the Artwork column for the whole artwork", () => {
    expect(rowHeightForArtworkMode(25, "partial")).toBe(25);
    expect(rowHeightForArtworkMode(25, "full")).toBe(50);
    const full = columnsForArtworkMode(columns, "full", 50);
    expect(full.find((col) => col.key === "artwork")?.width).toBe(50);
    // Every other column, and the stored layout itself, keep their widths.
    expect(full.filter((col) => col.key !== "artwork")).toEqual(columns.filter((col) => col.key !== "artwork"));
    expect(columns.find((col) => col.key === "artwork")?.width).toBe(80);
    expect(columnsForArtworkMode(columns, "partial", 25)).toBe(columns);
  });

  it("offers no buttons where the browser does not ask for them", async () => {
    render({});
    await settle();
    expect(button("partial")).toBeNull();
    expect(button("full")).toBeNull();
  });

  it("shows the upper part by default, with rekordbox's tooltips", async () => {
    render({ onArtworkMode: noop });
    await settle();
    expect(button("partial")?.getAttribute("aria-pressed")).toBe("true");
    expect(button("full")?.getAttribute("aria-pressed")).toBe("false");
    expect(button("partial")?.getAttribute("aria-label")).toBe("Show the upper part of the artwork.");
    expect(button("full")?.getAttribute("aria-label")).toBe("Show the entire artwork.");
    expect(artworkWidth()).toBe("80px");
  });

  it("switches to the whole artwork: rows twice as tall, the Artwork cell square", async () => {
    const chosen: ArtworkMode[] = [];
    render({ onArtworkMode: (mode) => chosen.push(mode) });
    await settle();
    const partial = Number.parseInt(rowHeight(), 10);
    act(() => button("full")?.click());
    expect(chosen).toEqual(["full"]);
    // Clicking the mode already on does nothing, as rekordbox's handler
    // returns early when the mode is unchanged.
    act(() => button("partial")?.click());
    expect(chosen).toEqual(["full"]);

    render({ artworkMode: "full", onArtworkMode: noop });
    await settle();
    expect(rowHeight()).toBe(`${partial * 2}px`);
    expect(artworkWidth()).toBe(`${partial * 2}px`);
    expect(button("full")?.getAttribute("aria-pressed")).toBe("true");
    // Its width follows the row, so its edge cannot be dragged.
    expect(host.querySelector('[aria-label="Resize Artwork"]')).toBeNull();
    expect(host.querySelector('[aria-label="Resize Track Title"]')).not.toBeNull();
  });

  it("brings the first selected row to the top of the list once the rows change height", async () => {
    let mode: ArtworkMode = "partial";
    const rerender = () => render({ artworkMode: mode, onArtworkMode: (next) => { mode = next; rerender(); } });
    rerender();
    await settle();
    await settle();
    const scroller = host.querySelector('[data-testid="track-scroll"]') as HTMLElement;
    const rows = [...host.querySelectorAll('[role="row"][aria-selected]')];
    const fifth = rows[5] as HTMLElement;
    act(() => {
      fifth.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
      fifth.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, button: 0 }));
      fifth.dispatchEvent(new MouseEvent("click", { bubbles: true, button: 0 }));
    });
    await settle();
    expect(host.querySelectorAll('[role="row"][aria-selected="true"]')).toHaveLength(1);
    scroller.scrollLeft = 120;
    act(() => button("full")?.click());
    await settle();
    await settle();
    const full = Number.parseInt(rowHeight(), 10);
    expect(scroller.scrollTop).toBe(5 * full);
    expect(scroller.scrollLeft).toBe(0);
  });
});
