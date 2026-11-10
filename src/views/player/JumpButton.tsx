import { useEffect, useRef, useState } from "react";

import styles from "./Player.module.css";

/** A click is over in a few milliseconds; the lit state is held this long so the eye catches it. */
export const PRESS_HOLD_MS = 120;

/**
 * A beat-jump arrow. It lights (`data-pressed`) from the moment it is pressed
 * and stays lit for `PRESS_HOLD_MS` after release, so a quick click is seen to
 * have registered rather than looking like nothing happened.
 */
export function JumpButton({ label, glyph, disabled, onJump }: {
  label: string;
  glyph: string;
  disabled: boolean;
  onJump: () => void;
}) {
  const [pressed, setPressed] = useState(false);
  const release = useRef<number | null>(null);
  const clear = () => {
    if (release.current !== null) window.clearTimeout(release.current);
    release.current = null;
  };
  useEffect(() => clear, []);

  const press = () => {
    clear();
    setPressed(true);
  };
  const lift = () => {
    clear();
    release.current = window.setTimeout(() => setPressed(false), PRESS_HOLD_MS);
  };

  return (
    <button
      type="button"
      className={styles.square}
      aria-label={label}
      disabled={disabled}
      data-pressed={pressed ? "" : undefined}
      onPointerDown={press}
      onPointerUp={lift}
      onPointerLeave={lift}
      onPointerCancel={lift}
      onClick={onJump}
    >
      {glyph}
    </button>
  );
}
