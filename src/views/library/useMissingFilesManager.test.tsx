/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import { createMockBackend } from "@/ipc/backend-mock";
import { useMissingFilesManager } from "./useMissingFilesManager";

let manager: ReturnType<typeof useMissingFilesManager>;
let request: ((id: string) => void) | undefined;
const reply = vi.fn(() => Promise.resolve(true));
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const settle = () => act(async () => { await Promise.resolve(); await Promise.resolve(); });
function Probe() { manager = useMissingFilesManager(); return manager.open ? <div data-manager /> : null; }
beforeEach(async () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  reply.mockReset().mockResolvedValue(true);
  __setBackend({ ...createMockBackend({ trackCount: 0 }), missingFilesReady: reply,
    onMissingFilesRequested: listener => { request = listener; return () => { request = undefined; }; },
  });
  host = document.createElement("div"); root = createRoot(host);
  act(() => root.render(<Probe />)); await settle();
});
afterEach(() => { act(() => root.unmount()); __setBackend(null); });
it("acknowledges only after the existing main dialog reports it is shown", async () => {
  act(() => request?.("1")); await settle();
  expect(host.querySelector("[data-manager]")).not.toBeNull();
  expect(reply).not.toHaveBeenCalled();
  act(() => manager.ready()); await settle();
  expect(reply).toHaveBeenCalledWith("1", true);
});
it("retires a late request without closing a newer manual opening", async () => {
  let answer: ((accepted: boolean) => void) | undefined;
  reply.mockImplementationOnce(() => new Promise<boolean>(resolve => { answer = resolve; }));
  act(() => request?.("1")); act(() => manager.ready()); await settle();
  act(() => { void manager.show(); });
  act(() => answer?.(false)); await settle();
  expect(host.querySelector("[data-manager]")).not.toBeNull();
});
it("closes only its own late opening and ignores requests after unmount", async () => {
  reply.mockResolvedValue(false);
  act(() => request?.("1")); act(() => manager.ready()); await settle();
  expect(host.querySelector("[data-manager]")).toBeNull();
  const obsolete = request;
  act(() => root.unmount()); root = createRoot(host);
  act(() => obsolete?.("2")); await settle();
  expect(reply).toHaveBeenCalledTimes(1);
});
