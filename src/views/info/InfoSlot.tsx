import { useRef, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { useTranslation } from "@/i18n";
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
  const t = useTranslation();
  const from = useRef<{ x: number; width: number } | null>(null);
  const slot = useRef<HTMLDivElement>(null);

  const current = () => width ?? slot.current?.getBoundingClientRect().width ?? bounds.min;

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    from.current = { x: event.clientX, width: current() };
    // Capture, so the drag survives the pointer leaving the handle.
    event.currentTarget.setPointerCapture?.(event.pointerId);
    event.preventDefault();
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const start = from.current;
    if (!start) return;
    // The handle is on the left edge, so dragging left widens the panel.
    onWidthChange(clampWidth(start.width - (event.clientX - start.x), bounds));
  };
  const onPointerEnd = (event: PointerEvent<HTMLDivElement>) => {
    from.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    const step = event.shiftKey ? KEY_STEP_LARGE : KEY_STEP;
    onWidthChange(clampWidth(current() + (event.key === "ArrowLeft" ? step : -step), bounds));
    event.preventDefault();
  };

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
        aria-label={t("Resize the information panel")}
        aria-valuenow={width === null ? undefined : width}
        aria-valuemin={bounds.min}
        aria-valuemax={clampWidth(Number.MAX_SAFE_INTEGER, bounds)}
        tabIndex={0}
      />
      {children}
    </div>
  );
}
