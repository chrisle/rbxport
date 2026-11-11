/**
 * A press held on a waveform or a button: down, any moves, then up.
 *
 * Pointer events, so a touch or a pen presses too, with the pointer captured
 * so the drag keeps going outside the element. And `mousedown` as well,
 * because WebKit sends no `pointerdown` for the first press after an HTML5
 * drag-and-drop (WebKit bug 202287) while `mousedown` still arrives. rbx
 * drags tracks with HTML5 drag-and-drop on Linux, so a track dragged onto a
 * deck left the next press on its waveform doing nothing.
 *
 * A `mousedown` that follows a `pointerdown` is that press's compatibility
 * event and is ignored, so the press never runs twice. One with no
 * `pointerdown` before it starts the press itself, and the window stands in
 * for pointer capture: its `mousemove` and `mouseup` carry the press until
 * the button comes up.
 */
import { useEffect, useMemo, useRef } from "react";

import { useEventCallback } from "@/store/useEventCallback";

/** Where a press is, and the element it started on. */
export interface Press {
  clientX: number;
  clientY: number;
  currentTarget: HTMLElement;
}

export interface PressHandlers {
  /** The press starts. `false` declines it: no moves and no release follow. */
  down: (press: Press) => boolean | void;
  move?: (press: Press) => void;
  /** The press ends; `cancelled` when the browser took the pointer away. */
  up?: (press: Press, cancelled: boolean) => void;
}

export interface PressProps {
  onPointerDown: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerMove: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerUp: (event: React.PointerEvent<HTMLElement>) => void;
  onPointerCancel: (event: React.PointerEvent<HTMLElement>) => void;
  onMouseDown: (event: React.MouseEvent<HTMLElement>) => void;
}

type Held =
  | { kind: "pointer"; id: number; target: HTMLElement }
  | { kind: "mouse"; target: HTMLElement; stop: () => void };

/**
 * `capture: false` leaves the pointer uncaptured, for a button whose
 * `pointerleave` must still let go of it.
 */
export function usePress(handlers: PressHandlers, { capture = true } = {}): PressProps {
  const down = useEventCallback(handlers.down);
  const move = useEventCallback((press: Press) => handlers.move?.(press));
  const up = useEventCallback((press: Press, cancelled: boolean) => handlers.up?.(press, cancelled));
  const held = useRef<Held | null>(null);
  // Set by a `pointerdown`, spent by the `mousedown` that follows it: after
  // it for a mouse, after its `pointerup` for a touch.
  const pointerPressed = useRef(false);

  useEffect(() => () => {
    if (held.current?.kind === "mouse") held.current.stop();
    held.current = null;
  }, []);

  const press = (event: { clientX: number; clientY: number }, target: HTMLElement): Press => ({
    clientX: event.clientX,
    clientY: event.clientY,
    currentTarget: target,
  });

  const onPointerDown = useEventCallback((event: React.PointerEvent<HTMLElement>) => {
    pointerPressed.current = true;
    if (held.current) return;
    const target = event.currentTarget;
    if (down(press(event, target)) === false) return;
    if (capture) target.setPointerCapture(event.pointerId);
    held.current = { kind: "pointer", id: event.pointerId, target };
  });

  const onPointerMove = useEventCallback((event: React.PointerEvent<HTMLElement>) => {
    const now = held.current;
    if (now?.kind !== "pointer" || now.id !== event.pointerId) return;
    if (capture && !now.target.hasPointerCapture(event.pointerId)) return;
    move(press(event, now.target));
  });

  const release = (event: React.PointerEvent<HTMLElement>) => {
    // A cancelled pointer sends no compatibility mouse events after it.
    if (event.type === "pointercancel") pointerPressed.current = false;
    const now = held.current;
    if (now?.kind !== "pointer" || now.id !== event.pointerId) return;
    held.current = null;
    if (now.target.hasPointerCapture(event.pointerId)) {
      now.target.releasePointerCapture(event.pointerId);
    }
    up(press(event, now.target), event.type === "pointercancel");
  };
  const onPointerUp = useEventCallback(release);
  const onPointerCancel = useEventCallback(release);

  const onMouseDown = useEventCallback((event: React.MouseEvent<HTMLElement>) => {
    if (pointerPressed.current) {
      pointerPressed.current = false;
      return;
    }
    if (event.button !== 0 || held.current) return;
    const target = event.currentTarget;
    if (down(press(event, target)) === false) return;
    let last = press(event, target);
    const end = (cancelled: boolean) => {
      stop();
      held.current = null;
      up(last, cancelled);
    };
    const moved = (native: MouseEvent) => {
      last = press(native, target);
      move(last);
    };
    const lifted = (native: MouseEvent) => {
      if (native.button !== 0) return;
      last = press(native, target);
      end(false);
    };
    // A release outside the window never arrives: losing the window ends it.
    const lost = () => end(true);
    const stop = () => {
      window.removeEventListener("mousemove", moved, true);
      window.removeEventListener("mouseup", lifted, true);
      window.removeEventListener("blur", lost);
    };
    window.addEventListener("mousemove", moved, true);
    window.addEventListener("mouseup", lifted, true);
    window.addEventListener("blur", lost);
    held.current = { kind: "mouse", target, stop };
  });

  // One object for the life of the component, so a memoised child that takes
  // it is not redrawn for it.
  return useMemo(
    () => ({ onPointerDown, onPointerMove, onPointerUp, onPointerCancel, onMouseDown }),
    [onPointerDown, onPointerMove, onPointerUp, onPointerCancel, onMouseDown],
  );
}
