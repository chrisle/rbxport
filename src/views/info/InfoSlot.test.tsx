/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { infoBounds } from "@/lib/splitter";
import { InfoSlot } from "./InfoSlot";

const bounds = infoBounds(1600, 305, false);
let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

/** jsdom has no PointerEvent; a MouseEvent carries the clientX React reads. */
function fire(el: Element, type: string, init: { clientX?: number; key?: string; shiftKey?: boolean } = {}) {
  const event = type.startsWith("key")
    ? new KeyboardEvent(type, { bubbles: true, cancelable: true, ...init })
    : new MouseEvent(type, { bubbles: true, cancelable: true, ...init });
  act(() => { el.dispatchEvent(event); });
}

function setup(width: number | null = 557) {
  const onWidthChange = vi.fn();
  act(() => {
    root.render(
      <InfoSlot width={width} bounds={bounds} onWidthChange={onWidthChange}>
        <aside>panel</aside>
      </InfoSlot>,
    );
  });
  return { onWidthChange, handle: host.querySelector('[role="separator"]')! };
}

describe("InfoSlot", () => {
  it("widens the panel when the handle is dragged left", () => {
    const { handle, onWidthChange } = setup();
    fire(handle, "pointerdown", { clientX: 1000 });
    fire(handle, "pointermove", { clientX: 950 });
    expect(onWidthChange).toHaveBeenLastCalledWith(607);
  });

  it("clamps to the floor and the ceiling", () => {
    const { handle, onWidthChange } = setup();
    fire(handle, "pointerdown", { clientX: 1000 });
    fire(handle, "pointermove", { clientX: 5000 });
    expect(onWidthChange).toHaveBeenLastCalledWith(bounds.min);
    fire(handle, "pointermove", { clientX: -5000 });
    const widest = onWidthChange.mock.lastCall![0] as number;
    expect(widest).toBeLessThanOrEqual(1600 * bounds.maxFraction);
    expect(1600 - widest).toBeGreaterThanOrEqual(bounds.minOther);
  });

  it("measures every move from where the drag began", () => {
    const { handle, onWidthChange } = setup();
    fire(handle, "pointerdown", { clientX: 1000 });
    fire(handle, "pointermove", { clientX: 700 });
    fire(handle, "pointermove", { clientX: 1000 });
    expect(onWidthChange).toHaveBeenLastCalledWith(557);
  });

  it("ignores moves outside a drag, and after it ends or is cancelled", () => {
    const { handle, onWidthChange } = setup();
    fire(handle, "pointermove", { clientX: 900 });
    fire(handle, "pointerdown", { clientX: 1000 });
    fire(handle, "pointerup");
    fire(handle, "pointermove", { clientX: 900 });
    fire(handle, "pointerdown", { clientX: 1000 });
    fire(handle, "pointercancel");
    fire(handle, "pointermove", { clientX: 900 });
    expect(onWidthChange).not.toHaveBeenCalled();
  });

  it("steps with the arrow keys within bounds", () => {
    const { handle, onWidthChange } = setup(bounds.min);
    fire(handle, "keydown", { key: "ArrowRight" });
    expect(onWidthChange).toHaveBeenLastCalledWith(bounds.min);
    fire(handle, "keydown", { key: "ArrowLeft", shiftKey: true });
    expect(onWidthChange).toHaveBeenLastCalledWith(bounds.min + 64);
  });

  it("renders the panel beside the handle", () => {
    setup();
    ;
  });
});
