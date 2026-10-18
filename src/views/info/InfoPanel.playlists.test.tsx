/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, RelatedPlaylist, RowDto } from "@/ipc/types";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { InfoPanel } from "./InfoPanel";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

const track: RowDto = {
  id: "18969",
  trackNo: 4,
  title: "Age Of Love (Dominant Space Remix) [138 EDIT]",
  artist: "Age of Love",
  album: "",
  genre: "",
  label: "",
  comment: "",
  bpmX100: 13_800,
  key: "Fm",
  durationSec: 268,
  rating: 0,
  analysed: 1,
  dateAdded: "",
  releaseDate: "",
  hotCues: [],
  artworkHue: 40,
  hasArtwork: false,
};

/** Path then name: the order the command answers, the row repeats. */
const playlists: RelatedPlaylist[] = [
  { id: "pl-1", name: "Friday", path: "Shows" },
  { id: "pl-2", name: "Warm-up", path: "" },
];

/** The mock answers on the microtask queue; a timer lets the chain land. */
const settle = async () => {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 10));
  });
};

const render = () => {
  const onOpenPlaylist = vi.fn();
  act(() => {
    root.render(
      <PreferencesProvider value={{ preferences: DEFAULT_PREFERENCES, update: () => {}, reset: () => {} }}>
        <InfoPanel
          track={track}
          selection={["18969"]}
          readOnly={false}
          libraryGeneration={0}
          onRate={() => {}}
          onComment={() => {}}
          onEdit={async () => {}}
          onOpenPlaylist={onOpenPlaylist}
        />
      </PreferencesProvider>,
    );
  });
  return onOpenPlaylist;
};

/** The row's links, in list order. */
const links = () => [...host.querySelectorAll("dt")]
  .filter((dt) => dt.textContent === "Playlists")
  .flatMap((dt) => [...dt.nextElementSibling?.querySelectorAll("button") ?? []]);

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const backend: Backend = {
    trackDetails: () => Promise.reject(new Error("the row's fields stand in")),
    trackPlaylists: () => Promise.resolve(playlists),
    logStatusMessage: vi.fn(() => Promise.resolve()),
  } as unknown as Backend;
  __setBackend(backend);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

it("lists the playlists a track is in, under a Playlists row", async () => {
  render();
  await settle();
  const shown = [...host.querySelectorAll("dt")].some((dt) => dt.textContent === "Playlists");
  expect(shown).toBe(true);
  expect(links().map((link) => link.textContent)).toEqual(["Friday", "Warm-up"]);
  // A folder above the playlist goes in the tip, as the path says.
  expect(links()[0]?.getAttribute("title")).toBe("Shows");
  expect(links()[1]?.getAttribute("title")).toBe("Warm-up");
});

it("is no row when the track is in no playlist", async () => {
  __setBackend({
    trackDetails: () => Promise.reject(new Error("")),
    trackPlaylists: () => Promise.resolve([]),
  } as unknown as Backend);
  render();
  await settle();
  const shown = [...host.querySelectorAll("dt")].some((dt) => dt.textContent === "Playlists");
  expect(shown).toBe(false);
});

it("sends a click on a playlist name to the tree", async () => {
  const onOpenPlaylist = render();
  await settle();
  act(() => {
    links()[1]?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
  });
  expect(onOpenPlaylist).toHaveBeenCalledWith("pl-2");
});

it("empties the row and reports it when the track's playlists cannot be read", async () => {
  const logStatusMessage = vi.fn(() => Promise.resolve());
  __setBackend({
    trackDetails: () => Promise.reject(new Error("")),
    trackPlaylists: () => Promise.reject(new Error("That track is no longer in the library.")),
    logStatusMessage,
  } as unknown as Backend);
  void render();
  await settle();
  expect([...host.querySelectorAll("dt")].some((dt) => dt.textContent === "Playlists")).toBe(false);
  // Not a silent empty row: the failure says where every error goes.
  expect(logStatusMessage).toHaveBeenCalledWith(true, "That track is no longer in the library.");
});
