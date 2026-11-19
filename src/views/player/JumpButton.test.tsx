/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { JumpButton, PRESS_HOLD_MS } from "./JumpButton";

declare global { var IS_REACT_ACT_ENVIRONMENT: boolean; }

let host: HTMLDivElement;
let root: Root;
const jumped = vi.fn();

const button = () => host.querySelector("button")!;
const fire = (type: string) => act(() => { button().dispatchEvent(new Event(type, { bubbles: true })); });

beforeEach(() => {
  vi.useFakeTimers();
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  jumped.mockClear();
  act(() => root.render(<JumpButton label="Beat jump forward" glyph="›" disabled={false} onJump={jumped} />));
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.useRealTimers(); });

it("lights on press and stays lit briefly after release", () => {
  expect(button().hasAttribute("data-pressed")).toBe(false);
  fire("pointerdown");
  expect(button().hasAttribute("data-pressed")).toBe(true);
  fire("pointerup");
  expect(button().hasAttribute("data-pressed")).toBe(true);
  act(() => { vi.advanceTimersByTime(PRESS_HOLD_MS); });
  expect(button().hasAttribute("data-pressed")).toBe(false);
});

it("a click still jumps once", () => {
  act(() => { button().click(); });
  expect(jumped).toHaveBeenCalledTimes(1);
});
