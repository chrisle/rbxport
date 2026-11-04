/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, RowDto } from "@/ipc/types";
import { useTrackCues } from "./useTrackCues";

let root: Root;
const trackCues = vi.fn<Backend["trackCues"]>();
function Probe({ track }: { track: RowDto }) {
  useTrackCues(track);
  return null;
}
const settle = () => act(async () => { await Promise.resolve(); });

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  trackCues.mockReset().mockResolvedValue([]);
  __setBackend({ trackCues, onCuesChanged: () => () => {}, onLibraryChanged: () => () => {} } as unknown as Backend);
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  __setBackend(null);
});

it("reads a Devices row from its selected on-stick format and track id", async () => {
  const track = {
    id: "file:/run/media/tester/USB/Contents/track.wav",
    deviceSource: { path: "/run/media/tester/USB", format: "oneLibrary", trackId: 42 },
  } as unknown as RowDto;
  act(() => root.render(<Probe track={track} />));
  await settle();
  expect(trackCues).toHaveBeenCalledWith(track.id, { path: "/run/media/tester/USB", format: "oneLibrary", trackId: 42 });
});

it("leaves an Explorer file on the collection cue route", async () => {
  const track = { id: "file:/run/media/tester/USB/Contents/track.wav" } as RowDto;
  act(() => root.render(<Probe track={track} />));
  await settle();
  expect(trackCues).toHaveBeenCalledWith(track.id, undefined);
});
