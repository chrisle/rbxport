/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { useNativeMenu } from "./useNativeMenu";

it("applies saved Performance state before readiness and serializes preset/language changes", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const root = createRoot(document.createElement("div"));
  const calls: { performance1: boolean; importLabel: string; finish: () => void }[] = [];
  __setBackend({ ...createMockBackend(), setMenuLabels: (labels, performance1) => new Promise<void>(finish => {
    calls.push({ performance1, importLabel: labels.Import!, finish });
  }) });
  let ready = false;
  const english = (text: string) => text;
  const german = (text: string) => `de:${text}`;
  function Shell({ performance1, translate = english }: { performance1: boolean; translate?: typeof english }) {
    ready = useNativeMenu(translate, performance1); return null;
  }
  try {
    await act(async () => { root.render(<Shell performance1 />); await Promise.resolve(); });
    expect(ready).toBe(false);
    expect(calls.map(call => call.performance1)).toEqual([true]);
    // Old native update is still pending; newer choices cannot overtake it.
    await act(async () => { root.render(<Shell performance1={false} />); await Promise.resolve(); });
    await act(async () => { root.render(<Shell performance1 translate={german} />); await Promise.resolve(); });
    expect(calls).toHaveLength(1);
    await act(async () => { calls[0]!.finish(); await Promise.resolve(); });
    expect(ready).toBe(false);
    expect(calls.map(call => [call.performance1, call.importLabel])).toEqual([[true, "Import"], [true, "de:Import"]]);
    await act(async () => { calls[1]!.finish(); await Promise.resolve(); });
    expect(ready).toBe(true);
    await act(async () => { root.render(<Shell performance1={false} translate={german} />); await Promise.resolve(); });
    expect(ready).toBe(false);
    await act(async () => { calls[2]!.finish(); await Promise.resolve(); });
    expect(ready).toBe(true);
    expect(calls[2]!.performance1).toBe(false);
  } finally { act(() => root.unmount()); __setBackend(null); }
});

it("does not acknowledge a failed update and a later preset change can retry", async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const root = createRoot(document.createElement("div"));
  let fail = true; let ready = false;
  const translate = (text: string) => text;
  __setBackend({ ...createMockBackend(), setMenuLabels: () => fail ? Promise.reject(new Error("unavailable")) : Promise.resolve() });
  function Shell({ performance1 }: { performance1: boolean }) { ready = useNativeMenu(translate, performance1); return null; }
  try {
    await act(async () => { root.render(<Shell performance1 />); await Promise.resolve(); });
    expect(ready).toBe(false);
    fail = false;
    await act(async () => { root.render(<Shell performance1={false} />); await Promise.resolve(); });
    expect(ready).toBe(true);
  } finally { act(() => root.unmount()); __setBackend(null); }
});
