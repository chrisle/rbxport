/** @vitest-environment jsdom */
import { act, startTransition, Suspense } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend, Cue, RowDto } from "@/ipc/types";
import { gridSourceKey } from "@/lib/gridEdit";
import { useCuePoint } from "./useCuePoint";
import { useTrackCues } from "./useTrackCues";

let root: Root;
let host: HTMLDivElement;
let point: ReturnType<typeof useCuePoint>;
let uncommitted: ReturnType<typeof useCuePoint>;
const read = vi.fn<Backend["trackCues"]>();
const row = (id: string) => ({ id }) as RowDto;
const cues = (seconds: number): Cue[] => [{ id: "memory", positionMs: seconds * 1000, outMs: 0, letter: "", memory: true, colour: null }];
const blocked = new Promise<void>(() => {});
function Probe({ track, intent = 0, suspend = false }: { track: RowDto; intent?: number; suspend?: boolean }) {
  const current = useCuePoint(`${gridSourceKey(track.id, track.deviceSource)}:${intent}`);
  useTrackCues(track, found => current.initialiseCuePoint((found[0]?.positionMs ?? 0) / 1000), intent);
  if (suspend) {
    uncommitted = current;
    // eslint-disable-next-line @typescript-eslint/only-throw-error -- React Suspense throws a pending promise.
    throw blocked;
  }
  point = current;
  return <span>{current.cuePoint}</span>;
}
const show = (track: RowDto, intent = 0) => act(async () => {
  root.render(<Suspense fallback="Waiting"><Probe track={track} intent={intent} /></Suspense>);
  await Promise.resolve();
});
function deferred() {
  let finish!: (found: Cue[]) => void;
  read.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  return (seconds: number) => act(async () => { finish(cues(seconds)); await Promise.resolve(); });
}
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  read.mockReset().mockResolvedValue(cues(10));
  __setBackend({ trackCues: read, onCuesChanged: () => () => {}, onLibraryChanged: () => () => {} } as unknown as Backend);
  host = document.createElement("div");
  root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); __setBackend(null); });

it("uses zero for a new audio while metadata waits, then its own first memory cue", async () => {
  await show(row("A")); expect(point.cuePoint).toBe(10);
  const old = point;
  const finish = deferred();
  await show(row("B")); expect(point.cuePoint).toBe(0);
  act(() => { old.setCuePoint(12); old.initialiseCuePoint(15); });
  expect(point.cuePoint).toBe(0);
  await finish(3); expect(point.cuePoint).toBe(3);
});

it("preserves deliberate CUE changes before metadata and during same-owner refresh", async () => {
  const track = row("override");
  const finish = deferred();
  await show(track);
  act(() => point.setCuePoint(7));
  await finish(3); expect(point.cuePoint).toBe(7);
  read.mockResolvedValueOnce(cues(15));
  await show({ ...track, title: "Metadata refresh" });
  expect(point.cuePoint).toBe(7);
});

it("resets an explicit same-row reload without reusing the prior temporary cue", async () => {
  const track = row("reload");
  await show(track); act(() => point.setCuePoint(7));
  await show(track); expect(point.cuePoint).toBe(7);
  const finish = deferred();
  await show(track, 1); expect(point.cuePoint).toBe(0);
  await finish(3); expect(point.cuePoint).toBe(3);
});

it("binds two device sources of the same audio independently", async () => {
  const track = { ...row("file:/usb/audio.wav"), deviceSource: { path: "/usb", format: "oneLibrary", trackId: 42 } } as RowDto;
  await show(track); expect(point.cuePoint).toBe(10);
  const finish = deferred();
  await show({ ...track, deviceSource: { ...track.deviceSource!, format: "deviceLibrary" } });
  expect(point.cuePoint).toBe(0);
  await finish(3); expect(point.cuePoint).toBe(3);
});

it("retires A -> pending B -> A and ignores B's late first reply", async () => {
  const a = row("ABA-A");
  await show(a); expect(point.cuePoint).toBe(10);
  const old = deferred(); await show(row("ABA-B"));
  const current = deferred(); await show(a); expect(point.cuePoint).toBe(0);
  await old(3); expect(point.cuePoint).toBe(0);
  await current(7); expect(point.cuePoint).toBe(7);
});

it("keeps the committed load's first reply during an abandoned render", async () => {
  const a = row("committed-A");
  const finish = deferred(); await show(a);
  await act(async () => {
    startTransition(() => root.render(<Suspense fallback="Waiting"><Probe track={row("uncommitted-B")} suspend /></Suspense>));
    await Promise.resolve();
  });
  act(() => uncommitted.setCuePoint(12));
  expect(host.textContent).toBe("0");
  await finish(10);
  expect(host.textContent).toBe("10");
  await show(a); expect(point.cuePoint).toBe(10);
});
