/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { HoverTarget } from "./HoverTarget";
import { usePress } from "@/views/player/usePress";

const box = { left: 10, top: 20, width: 100, height: 30, right: 110, bottom: 50, x: 10, y: 20, toJSON: () => ({}) };
function pointer(host: HTMLElement, type: string, extra: MouseEventInit = {}, touch = false) {
  const event = new MouseEvent(type, { bubbles: true, clientX: 60, clientY: 30, ...extra });
  Object.defineProperties(event, { pointerType: { value: touch ? "touch" : "mouse" }, pointerId: { value: 1 } });
  act(() => { host.dispatchEvent(event); });
}
it("updates only the decorative line and clears mouse, press, touch and source transitions", () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div"); const root = createRoot(host);
  let renders = 0;
  function Probe({ id, enabled = true }: { id: string; enabled?: boolean }) {
    renders++; return <div><HoverTarget identity={id} enabled={enabled} /><button>Stop</button></div>;
  }
  try {
    act(() => root.render(<Probe id="a" />));
    const surface = host.firstElementChild as HTMLElement; surface.getBoundingClientRect = () => box;
    const line = surface.querySelector<HTMLElement>("[data-hover-head]")!;
    pointer(surface, "pointermove"); expect(line.hidden).toBe(false); expect(line.style.left).toBe("50px");
    pointer(surface, "pointermove", { clientX: 200 }); expect(line.style.left).toBe("99px");
    pointer(surface, "pointermove", { clientX: -20 }); expect(line.style.left).toBe("0px");
    expect(renders).toBe(1);
    for (const type of ["pointerleave", "pointerdown", "pointercancel", "lostpointercapture", "mousedown", "dragstart"]) {
      pointer(surface, "pointermove"); pointer(surface, type); expect(line.hidden, type).toBe(true);
    }
    pointer(surface, "pointerdown"); pointer(surface, "pointerenter");
    expect(line.hidden).toBe(true); // A removed Stop/changed row may expose the canvas without a real move.
    for (const type of ["blur", "scroll", "keydown"]) {
      pointer(surface, "pointermove"); act(() => { window.dispatchEvent(new Event(type)); }); expect(line.hidden, type).toBe(true);
    }
    pointer(surface, "pointermove", { buttons: 1 }); expect(line.hidden).toBe(true);
    pointer(surface, "pointermove", { shiftKey: true }); expect(line.hidden).toBe(true);
    pointer(surface, "pointermove", {}, true); expect(line.hidden).toBe(true);
    pointer(surface.querySelector("button")!, "pointermove"); expect(line.hidden).toBe(true);
    pointer(surface, "pointermove");
    act(() => root.render(<Probe id="b" />)); expect(line.hidden).toBe(true);
    pointer(surface, "pointermove"); act(() => root.render(<Probe id="b" enabled={false} />));
    expect(line.hidden).toBe(true); pointer(surface, "pointermove"); expect(line.hidden).toBe(true);
  } finally { act(() => root.unmount()); }
});
it("composes with captured presses and the existing compatibility mouse fallback", () => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div"); const root = createRoot(host);
  const down = vi.fn(), move = vi.fn(), up = vi.fn();
  function Probe() { const press = usePress({ down, move, up }); return <div {...press}><HoverTarget identity="a" enabled /></div>; }
  try {
    act(() => root.render(<Probe />));
    const surface = host.firstElementChild as HTMLElement; surface.getBoundingClientRect = () => box;
    let captured = false;
    surface.setPointerCapture = () => { captured = true; };
    surface.hasPointerCapture = () => captured;
    surface.releasePointerCapture = () => { captured = false; };
    pointer(surface, "pointermove"); pointer(surface, "pointerdown", { buttons: 1 });
    pointer(surface, "mousedown", { buttons: 1 }); // compatibility event must not double the press
    pointer(surface, "pointermove", { buttons: 1 }); pointer(surface, "pointerup");
    expect(down).toHaveBeenCalledTimes(1); expect(move).toHaveBeenCalledTimes(1); expect(up).toHaveBeenCalledTimes(1);
    expect(surface.querySelector<HTMLElement>("[data-hover-head]")!.hidden).toBe(true);
    pointer(surface, "mousedown", { buttons: 1 });
    act(() => { window.dispatchEvent(new MouseEvent("mousemove", { clientX: 90, buttons: 1 })); window.dispatchEvent(new MouseEvent("mouseup", { button: 0 })); });
    expect(down).toHaveBeenCalledTimes(2); expect(move).toHaveBeenCalledTimes(2); expect(up).toHaveBeenCalledTimes(2);
  } finally { act(() => root.unmount()); }
});
