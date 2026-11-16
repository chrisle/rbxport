/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { usePress, type Press } from "./usePress";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let down: ReturnType<typeof vi.fn<(press: Press) => boolean | void>>;
let move: ReturnType<typeof vi.fn<(press: Press) => void>>;
let up: ReturnType<typeof vi.fn<(press: Press, cancelled: boolean) => void>>;
// jsdom has no pointer capture: a set of captured ids stands in for it.
let captured: Set<number>;

function Probe({ capture = true }: { capture?: boolean }) {
  const press = usePress({ down, move, up }, { capture });
  return <div data-testid="overview" {...press} />;
}

function render(capture = true) {
  act(() => root.render(<Probe capture={capture} />));
  return host.querySelector<HTMLDivElement>("[data-testid=overview]")!;
}

// jsdom has no PointerEvent either; a MouseEvent of the pointer type with a
// pointer id is what React's handlers read.
function pointer(target: EventTarget, type: string, clientX: number, pointerId = 1) {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX, button: 0 });
  Object.defineProperty(event, "pointerId", { value: pointerId });
  act(() => {
    target.dispatchEvent(event);
  });
}

function mouse(target: EventTarget, type: string, clientX: number, button = 0) {
  act(() => {
    target.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, clientX, button }));
  });
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  down = vi.fn<(press: Press) => boolean | void>();
  move = vi.fn<(press: Press) => void>();
  up = vi.fn<(press: Press, cancelled: boolean) => void>();
  captured = new Set();
  Element.prototype.setPointerCapture = function (id: number) { captured.add(id); };
  Element.prototype.releasePointerCapture = function (id: number) { captured.delete(id); };
  Element.prototype.hasPointerCapture = function (id: number) { return captured.has(id); };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("usePress", () => {
  it("presses from a mousedown with no pointerdown, as WebKit sends after a drag-and-drop", () => {
    const overview = render();
    mouse(overview, "mousedown", 10);
    expect(down).toHaveBeenCalledTimes(1);
    expect(down.mock.calls[0]![0]).toMatchObject({ clientX: 10, currentTarget: overview });

    // The window carries the press, as capture would, off the element too.
    mouse(document.body, "mousemove", 40);
    expect(move).toHaveBeenCalledTimes(1);
    expect(move.mock.calls[0]![0]).toMatchObject({ clientX: 40, currentTarget: overview });

    mouse(document.body, "mouseup", 50);
    expect(up).toHaveBeenCalledTimes(1);
    expect(up.mock.calls[0]![0]).toMatchObject({ clientX: 50, currentTarget: overview });
    expect(up.mock.calls[0]![1]).toBe(false);

    // Over: later moves are nobody's.
    mouse(document.body, "mousemove", 60);
    expect(move).toHaveBeenCalledTimes(1);
  });

  it("runs a pointer press once, whatever compatibility mouse events follow it", () => {
    const overview = render();
    pointer(overview, "pointerdown", 10);
    mouse(overview, "mousedown", 10);
    pointer(overview, "pointermove", 20);
    mouse(overview, "mousemove", 20);
    pointer(overview, "pointerup", 30);
    mouse(overview, "mouseup", 30);

    expect(down).toHaveBeenCalledTimes(1);
    expect(move).toHaveBeenCalledTimes(1);
    expect(up).toHaveBeenCalledTimes(1);
    expect(up.mock.calls[0]![0]).toMatchObject({ clientX: 30 });
    expect(up.mock.calls[0]![1]).toBe(false);
    expect(captured.size).toBe(0);
  });

  it("ignores the mousedown a touch sends after its pointerup", () => {
    const overview = render();
    pointer(overview, "pointerdown", 10);
    pointer(overview, "pointerup", 10);
    mouse(overview, "mousedown", 10);
    mouse(overview, "mouseup", 10);
    expect(down).toHaveBeenCalledTimes(1);
    expect(up).toHaveBeenCalledTimes(1);
  });

  it("presses from the mouse again after a pointer press", () => {
    const overview = render();
    pointer(overview, "pointerdown", 10);
    mouse(overview, "mousedown", 10);
    pointer(overview, "pointerup", 10);
    mouse(overview, "mouseup", 10);
    mouse(overview, "mousedown", 20);
    mouse(overview, "mouseup", 20);
    expect(down).toHaveBeenCalledTimes(2);
    expect(up).toHaveBeenCalledTimes(2);
  });

  it("reports a cancelled pointer, and a lost window, as cancelled", () => {
    const overview = render();
    pointer(overview, "pointerdown", 10);
    pointer(overview, "pointercancel", 10);
    expect(up.mock.calls[0]![1]).toBe(true);

    mouse(overview, "mousedown", 10);
    act(() => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(up).toHaveBeenCalledTimes(2);
    expect(up.mock.calls[1]![1]).toBe(true);
  });

  it("ignores other mouse buttons", () => {
    const overview = render();
    mouse(overview, "mousedown", 10, 2);
    mouse(overview, "mouseup", 10, 2);
    expect(down).not.toHaveBeenCalled();
    expect(up).not.toHaveBeenCalled();
  });

  it("follows a declined press with nothing", () => {
    down.mockReturnValue(false);
    const overview = render();
    mouse(overview, "mousedown", 10);
    mouse(document.body, "mousemove", 20);
    mouse(document.body, "mouseup", 20);
    pointer(overview, "pointerdown", 10);
    pointer(overview, "pointerup", 10);
    expect(move).not.toHaveBeenCalled();
    expect(up).not.toHaveBeenCalled();
    expect(captured.size).toBe(0);
  });

  it("leaves the pointer uncaptured when asked", () => {
    const overview = render(false);
    pointer(overview, "pointerdown", 10);
    expect(captured.size).toBe(0);
    pointer(overview, "pointerup", 10);
    expect(up).toHaveBeenCalledTimes(1);
  });
});
