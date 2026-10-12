/**
 * How far to scroll a list on one `dragover` while a row is dragged near its
 * top or bottom edge, so a row can be dropped beyond what is in view.
 *
 * [UNKNOWN] rekordbox's own edge zone and speed have not been measured (no
 * documentation in ../alphatheta-docs; needs a Winrig drag capture). These
 * values are a placeholder, not rekordbox parity.
 */
export const AUTOSCROLL_EDGE_PX = 40;
export const AUTOSCROLL_MAX_STEP_PX = 24;

/** Negative scrolls up, positive down, 0 when the pointer is clear of both edges. */
export function autoscrollStep(
  pointerY: number,
  top: number,
  bottom: number,
  edge = AUTOSCROLL_EDGE_PX,
  maxStep = AUTOSCROLL_MAX_STEP_PX,
): number {
  const size = Math.min(edge, (bottom - top) / 2);
  if (size <= 0) return 0;
  if (pointerY < top + size) {
    const depth = Math.min(1, (top + size - pointerY) / size);
    return -Math.max(1, Math.round(depth * maxStep));
  }
  if (pointerY > bottom - size) {
    const depth = Math.min(1, (pointerY - (bottom - size)) / size);
    return Math.max(1, Math.round(depth * maxStep));
  }
  return 0;
}
