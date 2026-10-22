/**
 * @vitest-environment jsdom
 *
 * One column layout per artwork mode (#575). rekordbox 7.2.19 keeps the whole
 * artwork's header under the table's name with `-FullArtwork` appended
 * (`BrowseHeaderManager::getXmlKeyName`) and restores whichever the mode
 * names when the mode changes (`ListViewer::updateRowHeight` calls
 * `changeTableHeader`), so a column changed in one mode leaves the other's
 * alone.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { ArtworkMode } from "@/lib/session";
import { columnStorageKey, useColumns, type ColumnContext, type Columns } from "./useColumns";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let latest: Columns;

function Probe({ context, mode }: { context: ColumnContext; mode: ArtworkMode }) {
  latest = useColumns(context, mode);
  return null;
}

const render = (context: ColumnContext, mode: ArtworkMode) =>
  act(() => root.render(<Probe context={context} mode={mode} />));
const keys = () => latest.columns.map((column) => column.key);

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("useColumns per artwork mode", () => {
  it("stores the whole artwork's layout under its own key", () => {
    expect(columnStorageKey("playlist")).toBe("rbl.columns.v2.playlist");
    expect(columnStorageKey("playlist", "full")).toBe("rbl.columns.v2.playlist.full");
    expect(columnStorageKey("subBrowser", "full")).toBe("rbl.columns.v2.subBrowser.full");
  });

  it("opens the whole artwork with its own default and gives the upper part's back", () => {
    render("collection", "partial");
    const partial = keys();
    expect(partial).toContain("title");
    expect(partial).not.toContain("summary");

    render("collection", "full");
    expect(keys()).toEqual([
      "trackNo", "preview", "artwork", "summary", "duration", "rating", "comment", "label", "dateAdded", "releaseDate",
    ]);

    render("collection", "partial");
    expect(keys()).toEqual(partial);
  });

  it("keeps a change made in one mode out of the other", () => {
    render("playlist", "full");
    act(() => latest.resize("summary", 300));
    act(() => latest.toggle("genre"));
    expect(latest.columns.find((column) => column.key === "summary")?.width).toBe(300);
    expect(keys()).toContain("genre");

    render("playlist", "partial");
    expect(keys()).not.toContain("genre");
    act(() => latest.resize("title", 222));

    render("playlist", "full");
    expect(latest.columns.find((column) => column.key === "summary")?.width).toBe(300);
    expect(latest.columns.find((column) => column.key === "title")).toBeUndefined();
    expect(JSON.parse(localStorage.getItem("rbl.columns.v2.playlist") ?? "{}").widths.title).toBe(222);
    expect(JSON.parse(localStorage.getItem("rbl.columns.v2.playlist.full") ?? "{}").widths.summary).toBe(300);
  });

  it("keeps the Explorer's text columns beside Summary", () => {
    render("folder", "full");
    expect(keys().slice(0, 6)).toEqual(["trackNo", "preview", "artwork", "summary", "title", "artist"]);
  });
});
