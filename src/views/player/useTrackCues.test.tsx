/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, Cue, RowDto } from "@/ipc/types";
import { useTrackCues } from "./useTrackCues";

let root: Root;
let host: HTMLDivElement;
const trackCues = vi.fn<Backend["trackCues"]>();
function Probe({ track }: { track: RowDto }) {
  return <span>{useTrackCues(track).length}</span>;
}
const settle = () => act(async () => { await Promise.resolve(); });

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  trackCues.mockReset().mockResolvedValue([]);
  __setBackend({ trackCues, onCuesChanged: () => () => {}, onLibraryChanged: () => () => {} } as unknown as Backend);
  host = document.createElement("div");
  root = createRoot(host);
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

it("clears the previous source's cues while the same file loads from another device format", async () => {
  const first = { id: "file:/usb/track.wav", deviceSource: { path: "/usb", format: "oneLibrary", trackId: 42 } } as RowDto;
  trackCues.mockResolvedValueOnce([{} as Cue]);
  act(() => root.render(<Probe track={first} />));
  await settle();
  expect(host.textContent).toBe("1");
  let finish: (cues: Cue[]) => void = () => {};
  trackCues.mockImplementationOnce(() => new Promise<Cue[]>((resolve) => { finish = resolve; }));
  const next = { ...first, deviceSource: { path: "/usb", trackId: 42, format: "deviceLibrary" as const } };
  act(() => root.render(<Probe track={next} />));
  await settle();
  expect(host.textContent).toBe("0");
  await act(async () => { finish([{} as Cue, {} as Cue]); await Promise.resolve(); });
  expect(host.textContent).toBe("2");
});
