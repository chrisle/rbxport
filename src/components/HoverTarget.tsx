/** Decorative destination feedback, separate from playback and cached waveforms.
 * Native listeners compose with the host's press/capture handlers. */
import { useLayoutEffect, useRef } from "react";
import styles from "./HoverTarget.module.css";
const linear = (x: number, _y: number, width: number) => x / width;
export function HoverTarget({ identity, enabled, position = linear }: {
  identity: string;
  enabled: boolean;
  position?: (x: number, y: number, width: number) => number;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const line = ref.current;
    const host = line?.parentElement;
    if (!line || !host || !enabled) return;
    const exits = ["pointerleave", "pointerdown", "mousedown", "pointercancel", "lostpointercapture", "dragstart"];
    const outside = ["blur", "keydown", "scroll"];
    let active = false;
    const hide = () => {
      line.hidden = true;
      if (active) for (const name of outside) window.removeEventListener(name, hide, true);
      active = false;
    };
    const move = (event: PointerEvent) => {
      if (event.pointerType === "touch" || event.buttons || event.shiftKey || event.metaKey || event.ctrlKey
        || (event.target instanceof Element && event.target.closest("button"))) { hide(); return; }
      const box = host.getBoundingClientRect();
      if (box.width <= 0) { hide(); return; }
      const at = position(event.clientX - box.left, event.clientY - box.top, box.width);
      if (!Number.isFinite(at)) { hide(); return; }
      line.style.left = `${Math.max(0, Math.min(box.width - 1, at * box.width))}px`;
      line.hidden = false;
      if (!active) for (const name of outside) window.addEventListener(name, hide, true);
      active = true;
    };
    host.addEventListener("pointermove", move);
    for (const name of exits) host.addEventListener(name, hide);
    return () => {
      hide();
      host.removeEventListener("pointermove", move);
      for (const name of exits) host.removeEventListener(name, hide);
    };
  }, [identity, enabled, position]);
  return <span ref={ref} hidden className={styles.target} data-hover-head aria-hidden />;
}
