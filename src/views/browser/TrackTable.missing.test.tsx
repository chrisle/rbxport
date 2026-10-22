/**
 * @vitest-environment jsdom
 *
 * Issue 682: a track whose file went missing after its row was read showed no
 * `[!]` until something made the list fetch its page again. rekordbox's flag
 * is a picture too, and it looks at the disk again for the one row selected
 * (`BrowseBasicView::changedSelectedRows` @0x100321688 ->
 * `updateMissingStatus` @0x100321010, then `repaintRow`); the table does the
 * same.
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
/** Track ids whose files are gone, as the disk says now. */
const gone = new Set<string>();
/** The ids looked at again, in order. */
let looked: string[] = [];

const COLLECTION: ViewSpec = { source: { kind: "collection" }, sort: "trackNo", descending: false, query: "" };
// The Attribute column, where the [!] is drawn, beside the defaults.
const columns = resolve({ ...defaultLayout(), order: ["attr", ...defaultLayout().order] });
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
const flagged = (row: Element | undefined) => row?.querySelector('[aria-label="File is Missing"]') != null;
const press = (row: Element | undefined, init: MouseEventInit = {}) => {
  act(() => {
    row?.firstElementChild?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0, ...init }));
  });
};

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 1200 });
  HTMLCanvasElement.prototype.getContext = (() => null) as typeof HTMLCanvasElement.prototype.getContext;
  const mock = createMockBackend({ trackCount: 20, latencyMs: 0 });
  gone.clear();
  looked = [];
  backend = {
    ...mock,
    trackMissing: (trackId) => {
      looked.push(trackId);
      return Promise.resolve(gone.has(trackId));
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

describe("TrackTable, a file that goes missing while the list is up", () => {
  it("shows the [!] once its row is selected, and takes it away when the file is back", async () => {
    render();
    await settle();
    await settle();
    const view = await backend.openView(COLLECTION);
    const [, second] = await backend.fetchRows(view.viewId, 0, 2);
    expect(flagged(rows()[1])).toBe(false);

    // Gone after the page was read: nothing shows it yet.
    gone.add(second!.id);
    await settle();
    expect(flagged(rows()[1])).toBe(false);

    press(rows()[1]);
    await settle();
    expect(looked).toEqual([second!.id]);
    expect(flagged(rows()[1])).toBe(true);

    gone.delete(second!.id);
    press(rows()[0]);
    await settle();
    press(rows()[1]);
    await settle();
    expect(flagged(rows()[1])).toBe(false);
  });

  it("looks only when one row is selected", async () => {
    render();
    await settle();
    await settle();
    press(rows()[0]);
    await settle();
    expect(looked).toHaveLength(1);

    // A second row added to the selection is not looked at.
    press(rows()[2], { shiftKey: true });
    await settle();
    expect(looked).toHaveLength(1);
  });
});
