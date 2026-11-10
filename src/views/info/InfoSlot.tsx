import { useCallback, useRef, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { clampWidth, type SplitterBounds } from "@/lib/splitter";
import styles from "./InfoSlot.module.css";

/** One arrow-key step, and the larger one with Shift held. */
const KEY_STEP = 16;
const KEY_STEP_LARGE = 64;

interface InfoSlotProps {
  /** The width drawn now, or null while the panel is at its measured default. */
  width: number | null;
  bounds: SplitterBounds;
  /** Called with the width the drag asks for, already clamped to `bounds`. */
  onWidthChange: (width: number) => void;
  children: ReactNode;
}

/**
 * The Information panel's place in the browser row, with a drag handle on its
 * left edge: the same convention as the tree and the sub-browser, so the
 * panel can be narrowed when it takes too much of the window (#860).
 */
export function InfoSlot({ width, bounds, onWidthChange, children }: InfoSlotProps) {
  const from = useRef<{ x: number; width: number } | null>(null);
  const slot = useRef<HTMLDivElement>(null);

  const current = useCallback(
    () => width ?? slot.current?.getBoundingClientRect().width ?? bounds.min,
    [width, bounds.min],
  );

  const onPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      from.current = { x: event.clientX, width: current() };
      // Capture, so the drag survives the pointer leaving the handle.
      event.currentTarget.setPointerCapture?.(event.pointerId);
      event.preventDefault();
    },
    [current],
  );
  const onPointerMove = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      const start = from.current;
      if (!start) return;
      // The handle is on the left edge, so dragging left widens the panel.
      onWidthChange(clampWidth(start.width - (event.clientX - start.x), bounds));
    },
    [bounds, onWidthChange],
  );
  const onPointerEnd = useCallback((event: PointerEvent<HTMLDivElement>) => {
    from.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }, []);
  const onKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      const step = event.shiftKey ? KEY_STEP_LARGE : KEY_STEP;
      onWidthChange(clampWidth(current() + (event.key === "ArrowLeft" ? step : -step), bounds));
      event.preventDefault();
    },
    [bounds, current, onWidthChange],
  );

  return (
    <div ref={slot} className={styles.slot}>
      <div
        className={styles.edge}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerEnd}
        onPointerCancel={onPointerEnd}
        onKeyDown={onKeyDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the information panel"
        aria-valuenow={width === null ? undefined : width}
        aria-valuemin={bounds.min}
        tabIndex={0}
      />
      {children}
    </div>
  );
}
