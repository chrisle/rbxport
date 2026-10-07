/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { HOLD_DELAY_MS, HOLD_REPEAT_MS, useHoldRepeat } from "./useHoldRepeat";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let action: ReturnType<typeof vi.fn>;

function Probe() {
  const hold = useHoldRepeat();
  return <button type="button" {...hold(action)}>Adjust</button>;
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  action = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<Probe />));
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe("useHoldRepeat", () => {
  it("stops repeating when the pointer is released outside the button", () => {
    const button = host.querySelector("button");
    expect(button).not.toBeNull();

    act(() => {
      button!.dispatchEvent(new MouseEvent("pointerdown", { button: 0, bubbles: true }));
      vi.advanceTimersByTime(HOLD_DELAY_MS + HOLD_REPEAT_MS);
    });
    expect(action).toHaveBeenCalledTimes(3);

    act(() => {
      window.dispatchEvent(new MouseEvent("pointerup", { button: 0 }));
      vi.advanceTimersByTime(HOLD_REPEAT_MS * 3);
    });
    expect(action).toHaveBeenCalledTimes(3);
  });

  it("stops repeating when the webview loses focus", () => {
    const button = host.querySelector("button");
    expect(button).not.toBeNull();

    act(() => {
      button!.dispatchEvent(new MouseEvent("pointerdown", { button: 0, bubbles: true }));
      vi.advanceTimersByTime(HOLD_DELAY_MS);
      window.dispatchEvent(new Event("blur"));
      vi.advanceTimersByTime(HOLD_REPEAT_MS * 3);
    });
    expect(action).toHaveBeenCalledTimes(2);
  });
});
