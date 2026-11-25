import { afterEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { loadDetailWaveform, subscribeDetailWaveform, watchDetailWaveforms } from "./detailWaveformCache";

const stops: (() => void)[] = [];
afterEach(() => {
  stops.splice(0).forEach(stop => stop());
  __setBackend(null);
});
function fixture() {
  const changed = new Set<(id: string) => void>();
  const read = vi.fn((_id: string, _kind: string): Promise<Uint8Array> => Promise.resolve(new Uint8Array([1, 2, 3])));
  const backend = { trackWaveform: read, onAnalysisChanged: (listener: (id: string) => void) => {
    changed.add(listener);
    return () => { changed.delete(listener); };
  } } as unknown as Backend;
  __setBackend(backend);
  return { read, backend, emit: (id: string) => changed.forEach(listener => listener(id)), changed };
}

it.each([false, true])("invalidates before notifying visible readers regardless of attach order (shell first=%s)", async shellFirst => {
  const { read, backend, emit } = fixture();
  const id = `generated-visible-cache-${shellFirst}`;
  const before = await loadDetailWaveform(id, "bandsDetail");
  await loadDetailWaveform(`${id}-unrelated`, "bandsDetail");
  const after = new Uint8Array([90, 50, 20]);
  read.mockResolvedValue(after);
  const replies: Promise<Uint8Array>[] = [];
  const visible = () => { stops.push(subscribeDetailWaveform(id, () => { replies.push(loadDetailWaveform(id, "bandsDetail")); })); };
  if (shellFirst) stops.push(watchDetailWaveforms(backend));
  visible(); visible();
  if (!shellFirst) stops.push(watchDetailWaveforms(backend));
  emit(id);
  expect(await Promise.all(replies)).toEqual([after, after]);
  expect(read.mock.calls.filter(([track]) => track === id)).toHaveLength(2);
  // A completion for another track neither evicts nor refreshes this entry.
  emit(`${id}-other`);
  expect(replies).toHaveLength(2);
  expect(await loadDetailWaveform(`${id}-unrelated`, "bandsDetail")).toEqual(before);
  expect(read).toHaveBeenCalledTimes(3);
});

it("an offscreen completion retires a same-key pending read before it can refill cache", async () => {
  const { read, backend, emit } = fixture();
  stops.push(watchDetailWaveforms(backend));
  let oldReply!: (bytes: Uint8Array) => void;
  let newReply!: (bytes: Uint8Array) => void;
  read.mockImplementationOnce(() => new Promise(resolve => { oldReply = resolve; }));
  const old = loadDetailWaveform("generated-offscreen-pending", "bandsDetail");
  await Promise.resolve();
  emit("generated-offscreen-pending");
  read.mockImplementationOnce(() => new Promise(resolve => { newReply = resolve; }));
  const fresh = loadDetailWaveform("generated-offscreen-pending", "bandsDetail");
  await Promise.resolve();
  oldReply(new Uint8Array([1, 1, 1]));
  await old;
  const joined = loadDetailWaveform("generated-offscreen-pending", "bandsDetail");
  await Promise.resolve();
  expect(read).toHaveBeenCalledTimes(2);
  const current = new Uint8Array([90, 50, 20]);
  newReply(current);
  expect(await fresh).toEqual(current);
  expect(await joined).toEqual(current);
  expect(await loadDetailWaveform("generated-offscreen-pending", "bandsDetail")).toEqual(current);
  expect(read).toHaveBeenCalledTimes(2);
});

it("detaches the old shell backend and attaches its replacement without duplicate observers", async () => {
  const first = fixture();
  const stopFirst = watchDetailWaveforms(first.backend);
  const id = "generated-shell-rebind";
  const original = await loadDetailWaveform(id, "bandsDetail");
  const queuedOld = [...first.changed][0];
  stopFirst();
  expect(first.changed.size).toBe(0);
  const second = fixture();
  stops.push(watchDetailWaveforms(second.backend));
  expect(second.changed.size).toBe(1);
  first.emit(id);
  queuedOld?.(id);
  expect(await loadDetailWaveform(id, "bandsDetail")).toEqual(original);
  expect(second.read).not.toHaveBeenCalled();
  second.emit(id);
  await loadDetailWaveform(id, "bandsDetail");
  expect(second.read).toHaveBeenCalledTimes(1);
});
