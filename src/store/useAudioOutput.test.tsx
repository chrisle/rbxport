/**
 * @vitest-environment jsdom
 *
 * The remembered output device reaches the engine at start and on change, and
 * a device that is not there never surfaces as an error (#799).
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { useAudioOutput } from "./useAudioOutput";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

function Probe({ device }: { device: string | null }) {
  useAudioOutput(device);
  return null;
}

let host: HTMLDivElement;
let root: Root;
const settle = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("useAudioOutput", () => {
  it("sends the saved device at start and again when it changes", async () => {
    const setAudioDevice = vi.fn(() => Promise.resolve());
    __setBackend({ setAudioDevice } as unknown as Backend);
    act(() => root.render(<Probe device="m4" />));
    await settle();
    expect(setAudioDevice).toHaveBeenCalledTimes(1);
    expect(setAudioDevice).toHaveBeenLastCalledWith("m4");
    act(() => root.render(<Probe device={null} />));
    await settle();
    expect(setAudioDevice).toHaveBeenLastCalledWith(null);
  });

  it("does not throw when the engine refuses or is absent", async () => {
    __setBackend({ setAudioDevice: () => Promise.reject(new Error("no engine")) } as unknown as Backend);
    act(() => root.render(<Probe device="gone" />));
    await settle();
    expect(host.isConnected).toBe(true);
  });
});
