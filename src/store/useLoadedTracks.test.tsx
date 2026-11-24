/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import type { LoadedTrack } from "@/ipc/types";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { confirmedLoads, useLoadedTracks } from "./useLoadedTracks";
import { useLoadedRows } from "./useLoadedRows";

it("invalidates completed and pending markers on reset while Players are absent", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div"); const root = createRoot(host);
  let reset = () => {};
  let publish: (tracks: readonly (LoadedTrack | null)[]) => void;
  let marked: ReadonlySet<string> = new Set();
  const pending: ((rows: string[]) => void)[] = [];
  __setBackend({ ...createMockBackend(),
    onDeckReset: listener => { reset = listener; return () => {}; },
    loadedRows: () => new Promise<string[]>(done => { pending.push(done); }),
  });
  function Shell() {
    const [owners, setOwners] = useLoadedTracks(); publish = setOwners;
    marked = useLoadedRows(["row"], owners, "same-view"); return null;
  }
  try {
    await act(async () => { root.render(<Shell />); await Promise.resolve(); });
    const owner = ["loaded-a", 31] as const;
    confirmedLoads.set("a", owner);
    await act(async () => { publish([owner]); await Promise.resolve(); });
    await act(async () => { pending[0]!(["row"]); await Promise.resolve(); });
    expect([...marked]).toEqual(["row"]);
    await act(async () => { reset(); await Promise.resolve(); });
    expect([...marked]).toEqual([]);
    expect(confirmedLoads.size).toBe(0);
    await act(async () => { publish([owner]); await Promise.resolve(); });
    expect([...marked]).toEqual([]); // A→reset→A must not reuse a completed answer.
    await act(async () => { reset(); await Promise.resolve(); });
    await act(async () => { pending[1]!(["row"]); await Promise.resolve(); });
    expect([...marked]).toEqual([]); // Nor may an answer pending across reset land.
  } finally {
    act(() => root.unmount()); __setBackend(null); confirmedLoads.clear();
  }
});
