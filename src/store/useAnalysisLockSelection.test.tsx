/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { useAnalysisLockSelection } from "./useAnalysisLockSelection";
let root: Root;
let locked = false;
let changed: (id: string) => void;
let reloaded: () => void;
const query = vi.fn<Backend["anyAnalysisLocked"]>();
function Probe({ ids, owner = "collection", enabled = true }: { ids: ReadonlySet<string>; owner?: string; enabled?: boolean }) {
  locked = useAnalysisLockSelection(ids, owner, enabled);
  return <button disabled={!locked}>Analysis Lock Off</button>;
}
const settle = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });
const render = (ids: ReadonlySet<string>, owner = "collection", enabled = true) => act(() => root.render(<Probe ids={ids} owner={owner} enabled={enabled} />));
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  query.mockReset().mockResolvedValue(false);
  __setBackend({ anyAnalysisLocked: query,
    onGridChanged: (listener: (id: string) => void) => { changed = listener; return () => {}; },
    onLibraryChanged: (listener: () => void) => { reloaded = listener; return () => {}; },
  } as unknown as Backend);
  root = createRoot(document.createElement("div"));
});
afterEach(() => { act(() => root.unmount()); __setBackend(null); });
it("uses authoritative lock state and refreshes lock then unlock without changing the selection", async () => {
  const ids = new Set(["1"]); render(ids); await settle(); expect(locked).toBe(false);
  query.mockResolvedValue(true); act(() => changed("1")); await settle(); expect(locked).toBe(true);
  query.mockResolvedValue(false); act(() => changed("1")); await settle(); expect(locked).toBe(false);
  act(() => changed("other")); await settle(); expect(query).toHaveBeenCalledTimes(3);
});
it("retires a pending answer when selection or source changes", async () => {
  let old!: (value: boolean) => void;
  query.mockReturnValueOnce(new Promise(resolve => { old = resolve; }));
  render(new Set(["1"])); await settle(); render(new Set(["2"]), "other-source"); await settle();
  act(() => old(true)); await settle(); expect(locked).toBe(false);
});
it("retires a pending pre-unlock answer on a newer grid event", async () => {
  let old!: (value: boolean) => void;
  query.mockReturnValueOnce(new Promise(resolve => { old = resolve; }));
  render(new Set(["1"])); await settle(); act(() => changed("1")); await settle();
  act(() => old(true)); await settle(); expect(locked).toBe(false);
});
it("uses one request for a large selection", async () => {
  query.mockResolvedValue(true);
  render(new Set(Array.from({ length: 171_000 }, (_, i) => String(i + 1)))); await settle();
  expect(locked).toBe(true); expect(query).toHaveBeenCalledTimes(1); expect(query.mock.calls[0]?.[0]).toHaveLength(171_000);
});
it("retires an old true across disable and reenable with unchanged IDs and source", async () => {
  const ids = new Set(["1"]); query.mockResolvedValueOnce(true); render(ids); await settle(); expect(locked).toBe(true);
  query.mockReturnValue(new Promise(() => {})); render(ids, "collection", false); expect(locked).toBe(false);
  render(ids); expect(locked).toBe(false); await settle(); expect(locked).toBe(false);
});
it("does not query empty or disabled device/folder selections", async () => {
  render(new Set()); await settle(); render(new Set(["file:/owned/audio.wav"]), "device", false); await settle();
  expect(query).not.toHaveBeenCalled(); expect(locked).toBe(false);
});
it("refreshes after library reload and focus and refuses failed reads", async () => {
  query.mockResolvedValueOnce(true); render(new Set(["1"])); await settle(); expect(locked).toBe(true);
  query.mockRejectedValue(new Error("read failed")); act(() => reloaded()); await settle(); expect(locked).toBe(false);
  query.mockResolvedValue(true); act(() => { window.dispatchEvent(new Event("focus")); }); await settle(); expect(locked).toBe(true);
});
it("does not publish a pending response after unmount", async () => {
  let old!: (value: boolean) => void;
  query.mockReturnValueOnce(new Promise(resolve => { old = resolve; })); render(new Set(["1"])); await settle();
  act(() => root.render(null)); act(() => old(true)); await settle(); expect(locked).toBe(false);
});
