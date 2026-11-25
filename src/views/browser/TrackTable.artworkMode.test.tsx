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
import { defaultLayout, fullArtworkLayout, resolve, toggleColumn, type ColumnSpec } from "@/lib/columns";
import type { ArtworkMode } from "@/lib/session";
import {
  columnsForArtworkMode, fullArtworkWidth, previewBand, rowHeightForArtworkMode, TrackTable,
} from "./TrackTable";

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

function render(props: {
  spec?: ViewSpec;
  libraryGeneration?: number;
  artworkMode?: ArtworkMode;
  onArtworkMode?: (mode: ArtworkMode) => void;
  columns?: readonly ColumnSpec[];
  keyDisplay?: typeof DEFAULT_PREFERENCES.view.keyDisplay;
}) {
  act(() => {
    root.render(
      <PreferencesProvider value={{
        preferences: props.keyDisplay
          ? { ...DEFAULT_PREFERENCES, view: { ...DEFAULT_PREFERENCES.view, keyDisplay: props.keyDisplay } }
          : DEFAULT_PREFERENCES,
        update: noop,
        reset: noop,
      }}>
        <TrackTable
          spec={props.spec ?? COLLECTION}
          onSortChange={noop}
          title="Collection"
          query=""
          onQueryChange={noop}
          columns={props.columns ?? columns}
          onColumnMove={noop}
          onColumnResize={noop}
          onColumnToggle={noop}
          onColumnAutoSize={noop}
          onColumnAutoSizeAll={noop}
          libraryGeneration={props.libraryGeneration ?? 0}
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

  it("restores the selection when a saved artwork layout opens a view for extra columns", async () => {
    const backend = createMockBackend({ trackCount: 400, latencyMs: 0, writable: true });
    const idsInRange = backend.viewIdsInRange.bind(backend);
    const openView = backend.openView.bind(backend);
    let holdNext = false;
    let holdOpen = false;
    let projectionPending = false;
    let obsoleteReads = 0;
    let finish: (() => void) | undefined;
    let finishOpen: (() => void) | undefined;
    backend.openView = async (...args) => {
      if (holdOpen) {
        holdOpen = false;
        projectionPending = true;
        await new Promise<void>((resolve) => { finishOpen = resolve; });
        projectionPending = false;
      }
      return openView(...args);
    };
    backend.viewIdsInRange = async (...args) => {
      if (projectionPending) {
        obsoleteReads += 1;
        throw new Error("the previous view closed during projection replacement");
      }
      const ids = await idsInRange(...args);
      if (!holdNext) return ids;
      holdNext = false;
      return new Promise<string[]>((resolve) => { finish = () => resolve(ids); });
    };
    __setBackend(backend);
    let mode: ArtworkMode = "partial";
    const customColumns = resolve(toggleColumn(fullArtworkLayout(defaultLayout(), true), "composer", "full"), "full");
    const rerender = () => render({ artworkMode: mode, columns: mode === "full" ? customColumns : columns,
      onArtworkMode: next => { mode = next; rerender(); } });
    rerender();
    await settle(); await settle();
    const row = host.querySelectorAll<HTMLElement>('[role="row"][aria-selected]')[5];
    if (!row) throw new Error("fixture row missing");
    act(() => {
      row.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
      row.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, button: 0 }));
      row.dispatchEvent(new MouseEvent("click", { bubbles: true, button: 0 }));
    });
    await settle();
    holdNext = true;
    holdOpen = true;
    act(() => button("full")?.click());
    await settle(); await settle();
    expect(finishOpen).toBeDefined();
    expect(obsoleteReads).toBe(0);
    expect(finish).toBeUndefined();
    act(() => finishOpen?.());
    await settle(); await settle();
    expect(finish).toBeDefined();
    act(() => finish?.());
    await settle();
    const scroller = host.querySelector('[data-testid="track-scroll"]') as HTMLElement;
    expect(scroller.scrollTop).toBe(5 * Number.parseInt(rowHeight(), 10));
  });

  it.each(["mode", "source", "selection", "generation"] as const)("ignores an artwork resize lookup after its %s changes", async (change) => {
    const backend = createMockBackend({ trackCount: 400, latencyMs: 0, writable: true, playlistFixture: "cueOnly" });
    const idsInRange = backend.viewIdsInRange.bind(backend);
    let holdNext = false;
    let finish: (() => void) | undefined;
    backend.viewIdsInRange = async (...args) => {
      const ids = await idsInRange(...args);
      if (!holdNext) return ids;
      holdNext = false;
      return new Promise<string[]>((resolve) => { finish = () => resolve(ids); });
    };
    __setBackend(backend);
    let mode: ArtworkMode = "partial";
    let spec = COLLECTION;
    let generation = 0;
    const rerender = () => render({ spec, libraryGeneration: generation, artworkMode: mode, onArtworkMode: (next) => { mode = next; rerender(); } });
    const select = (index: number) => {
      const row = host.querySelectorAll<HTMLElement>('[role="row"][aria-selected]')[index];
      if (!row) throw new Error("fixture row missing");
      act(() => {
        row.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
        row.dispatchEvent(new MouseEvent("mouseup", { bubbles: true, button: 0 }));
        row.dispatchEvent(new MouseEvent("click", { bubbles: true, button: 0 }));
      });
    };
    rerender();
    await settle(); await settle();
    select(5);
    await settle();
    holdNext = true;
    act(() => button("full")?.click());
    await settle(); await settle();
    expect(finish).toBeDefined();
    if (change === "mode") act(() => button("partial")?.click());
    else if (change === "source") {
      spec = { ...COLLECTION, source: { kind: "playlist", id: "cue" } };
      rerender();
    } else if (change === "generation") {
      generation += 1;
      rerender();
    } else select(7);
    await settle(); await settle();
    const scroller = host.querySelector('[data-testid="track-scroll"]') as HTMLElement;
    scroller.scrollTop = 73;
    scroller.scrollLeft = 31;
    act(() => finish?.());
    await settle();
    expect(scroller.scrollTop).toBe(73);
    expect(scroller.scrollLeft).toBe(31);
  });
});

describe("the whole artwork's list (#575)", () => {
  // rekordbox 7.2.19, static: the whole artwork's own header layout puts
  // Summary (id 63) after the Artwork (id 61, no heading), and the Preview
  // waveform is inset a fifth of the row's height above and below.
  const fullColumns = resolve(fullArtworkLayout(defaultLayout(), true), "full");
  const cell = (key: string) => host.querySelector<HTMLElement>(`[role="row"][aria-selected] [data-col="${key}"]`);

  it("holds the Artwork width between rekordbox's minimum and maximum", () => {
    expect(fullArtworkWidth(50)).toBe(50);
    expect(fullArtworkWidth(20)).toBe(25);
    expect(fullArtworkWidth(100)).toBe(85);
  });

  it("insets the waveform by a fifth of the row above and below", () => {
    expect(previewBand(25)).toEqual({ top: 5, height: 15 });
    expect(previewBand(50)).toEqual({ top: 10, height: 30 });
    expect(previewBand(33)).toEqual({ top: 6, height: 21 });
  });

  it("draws Summary's two lines, a blank Artwork heading and a centred waveform band", async () => {
    render({ artworkMode: "full", onArtworkMode: noop, columns: fullColumns });
    await settle();
    await settle();
    const summary = cell("summary");
    expect(summary).not.toBeNull();
    const [first, second] = [...(summary?.children ?? [])] as HTMLElement[];
    // The deterministic collection fixture's first row pins the values, not
    // only the shape of the Summary renderer.
    expect(first?.textContent).toBe("Something In The Air (Radio Edit)");
    expect([...(second?.children ?? [])].map((part) => part.textContent)).toEqual([
      "The Temper Trap", "122.00", "Dbm",
    ]);
    // An unanalysed row leaves both unavailable values empty.
    const missing = [...host.querySelectorAll<HTMLElement>('[data-col="summary"]')]
      .find((entry) => entry.firstElementChild?.textContent === "Your Eyes (Extended Remix)");
    expect(missing).toBeDefined();
    expect([...(missing?.children.item(1)?.children ?? [])].map((part) => part.textContent)).toEqual([
      "Tujamo", "", "",
    ]);
    expect(cell("title")).toBeNull();
    const headings = [...host.querySelectorAll('[role="columnheader"]')].map((heading) => heading.textContent);
    expect(headings.slice(0, 4)).toEqual(["#", "Preview", "", "Summary"]);
    expect(host.querySelector('[aria-label="Resize Artwork"]')).toBeNull();
    expect(browser().style.getPropertyValue("--s-preview-band-top")).toBe("10px");
    expect(browser().dataset.artwork).toBe("full");
  });

  it("formats Summary's key with the selected key display", async () => {
    render({ artworkMode: "full", onArtworkMode: noop, columns: fullColumns, keyDisplay: "alphanumeric" });
    await settle();
    await settle();
    const summary = cell("summary");
    const line = summary?.children.item(1);
    expect(line?.children.item(2)?.textContent).toBe("12A");
  });

  it("offers Summary rather than Artwork in the whole artwork's column menu", async () => {
    render({ artworkMode: "full", onArtworkMode: noop, columns: fullColumns });
    await settle();
    const heading = [...host.querySelectorAll<HTMLElement>('[role="columnheader"]')][3];
    act(() => {
      heading?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 10, clientY: 10 }));
    });
    const items = [...document.querySelectorAll('[role="menuitemcheckbox"]')].map((item) => item.textContent);
    expect(items.some((text) => text?.endsWith("Summary"))).toBe(true);
    expect(items.some((text) => text?.endsWith("Artwork"))).toBe(false);
  });

  it("keeps the upper part's Artwork edge fixed too", async () => {
    render({ onArtworkMode: noop });
    await settle();
    expect(host.querySelector('[aria-label="Resize Artwork"]')).toBeNull();
    expect(browser().style.getPropertyValue("--s-preview-band-top")).toBe("5px");
    expect(browser().dataset.artwork).toBeUndefined();
  });
});
