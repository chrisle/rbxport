import { useEffect, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";

import { MEMORY_CUE_COLOURS } from "@/lib/memoryCueColours";

import styles from "./CueColorMenu.module.css";

// Rekordbox's compact 4 × 4 hot-cue picker, in its displayed order. Values
// are ColorTableIndex entries from the desktop palette.
const HOT = [49, 56, 60, 62, 1, 3, 9, 15, 18, 22, 26, 30, 32, 38, 41, 46] as const;
const HOT_CSS = [
  "#DE44CF", "#B432FF", "#AA72FF", "#6473FF", "#305AFF", "#508CFF", "#00E0FF", "#19A08C",
  "#10B176", "#28E214", "#A5E116", "#B4BE04", "#C3AF04", "#E0641B", "#E02823", "#F51E8C",
] as const;

/**
 * Closing a menu by clicking elsewhere must not also act on what was clicked,
 * as a cue row would by jumping to its cue. The click that follows this
 * mouse-down is cancelled before it reaches the page; a later mouse-down
 * ends the wait, so a press that never completes cannot eat a later click.
 */
function swallowNextClick(): void {
  const stop = (event: Event) => {
    event.stopPropagation();
    event.preventDefault();
    done();
  };
  const done = () => {
    window.removeEventListener("click", stop, true);
    window.removeEventListener("mousedown", done, true);
  };
  window.addEventListener("click", stop, true);
  window.addEventListener("mousedown", done, true);
}

export function CueColorMenu({ x, y, memory, onChoose, onComment, onClose }: {
  x: number; y: number; memory: boolean;
  /** Starts editing the cue's comment; absent when the cue cannot be edited. */
  onComment?: (() => void) | undefined;
  onChoose: (colour: number | null) => void; onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const outside = (event: MouseEvent) => {
      if (menu.current?.contains(event.target as Node)) return;
      onClose();
      swallowNextClick();
    };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("mousedown", outside, true);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("mousedown", outside, true); window.removeEventListener("keydown", key); };
  }, [onClose]);
  useLayoutEffect(() => {
    const element = menu.current;
    if (!element) return;
    const box = element.getBoundingClientRect();
    element.style.left = `${Math.max(8, Math.min(x, window.innerWidth - box.width - 8))}px`;
    element.style.top = `${Math.max(8, Math.min(y, window.innerHeight - box.height - 8))}px`;
  }, [x, y, memory]);
  const choose = (value: number | null) => { onChoose(value); onClose(); };
  const commentButton = (
    <button role="menuitem" className={styles.comments} disabled={!onComment}
      onClick={() => { onComment?.(); onClose(); }}>Add comments</button>
  );
  return createPortal(
    <div ref={menu} className={`${styles.menu} ${memory ? "" : styles.hotMenu}`} role="menu" aria-label={`${memory ? "Memory" : "Hot"} cue color`} style={{left: x, top: y}}>
      {memory ? (
        <>
          {commentButton}
          {MEMORY_CUE_COLOURS.map(([name, color], index) => (
            <button key={name} role="menuitem" className={styles.memory} onClick={() => choose(index)}>
              <span className={styles.dot} style={{background: color}} />{name}
            </button>
          ))}
        </>
      ) : (
        <>
          {commentButton}
          <div className={styles.grid} role="group" aria-label="Hot cue colors">
            {HOT.map((value, index) => <button key={value} aria-label={`Color ${index + 1}`} style={{background: HOT_CSS[index]}} onClick={() => choose(value)} />)}
          </div>
        </>
      )}
      <button role="menuitem" className={styles.reset} onClick={() => choose(null)}>{memory ? "No Color" : "Reset"}</button>
    </div>, document.body,
  );
}
