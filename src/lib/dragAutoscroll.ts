/**
 * Scrolling the track list while rows are dragged to reorder a playlist, so a
 * row can be dropped above or below what is in view.
 *
 * [OBS static, rekordbox 7.2.19 macOS arm64] `browse::ListViewer::itemDragMove`
 * (0x1003dfe4c) runs only when `canSortWithDragAndDrop` (0x1003e0260) holds:
 * the drag started in this same list, the library is not protected, the list
 * is a playlist sorted by its # column and no search text is set. It then
 * calls `beginDragAutoRepeat(100)` and
 * `CustomViewport::autoScroll(width / 2, y, 30, 50)` (0x1003e0208..0x1003e0228),
 * with `y` measured from the top of the rows (below the column header).
 * `CustomViewport::autoScroll` (0x100d235ac) is JUCE's `Viewport::autoScroll`:
 * inside 30px of the top it moves `30 - y` pixels, inside 30px of the bottom
 * `y - (height - 30)` pixels, at most 50 per call, clamped to the content.
 * The x passed is the middle of the list, so it never scrolls sideways.
 *
 * [OBS JUCE 8 source, juce_MouseInputSourceList.h] `beginDragAutoRepeat`
 * makes a timer send a forced mouse move every 100ms while the button is
 * held, so the list keeps scrolling while the pointer rests in the zone, on
 * top of a step for every real pointer move.
 *
 * [OBS static, same function] each pass of `itemDragMove`, the repeat's
 * included, first finds the insertion index under the pointer
 * (`CustomListBox::getInsertionIndexForPosition`, 0x1003dfe84) and moves the
 * insertion marker there (`setBounds`, 0x1003e01c0), so the marker follows the
 * rows scrolling under a resting pointer. `scrolled` is that hook here.
 */
export const AUTOSCROLL_BORDER_PX = 30;
export const AUTOSCROLL_MAX_STEP_PX = 50;
export const AUTOSCROLL_REPEAT_MS = 100;

/**
 * How far one drag move scrolls the rows: negative up, positive down, 0 when
 * the pointer is clear of both edges. `top` and `bottom` bound the visible
 * rows (not the column header). The top edge wins when the two zones overlap,
 * as in JUCE.
 */
export function autoscrollStep(pointerY: number, top: number, bottom: number): number {
  const y = Math.round(pointerY - top);
  const height = Math.round(bottom - top);
  if (y < AUTOSCROLL_BORDER_PX) return -Math.min(AUTOSCROLL_BORDER_PX - y, AUTOSCROLL_MAX_STEP_PX);
  if (y >= height - AUTOSCROLL_BORDER_PX) {
    return Math.min(y - (height - AUTOSCROLL_BORDER_PX), AUTOSCROLL_MAX_STEP_PX);
  }
  return 0;
}

export interface DragAutoscroll {
  /** A `dragover` at this pointer position. */
  move(clientX: number, clientY: number): void;
  /** The drag left the list, dropped or ended. */
  stop(): void;
}

/**
 * Drives `autoscrollStep` the way rekordbox's drag does: one step for each
 * real pointer move, and one every `AUTOSCROLL_REPEAT_MS` while the pointer
 * rests in a zone. Browsers repeat `dragover` on their own schedule while the
 * pointer rests, so a `dragover` at an unchanged position is left to the timer.
 */
export function createDragAutoscroll(
  rows: () => { top: number; bottom: number } | null,
  /** Returns false when the list could not move (already at that end). */
  scrollBy: (dy: number) => boolean,
  /**
   * After a step that moved the list, with the pointer position: the rows
   * under the pointer changed, so whatever marks the drop place must be
   * worked out again there.
   */
  scrolled?: (clientX: number, clientY: number) => void,
): DragAutoscroll {
  let at: { x: number; y: number } | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  /** One step at the last position; true when it scrolled. */
  const step = () => {
    const box = rows();
    if (!at || !box) return false;
    const dy = autoscrollStep(at.y, box.top, box.bottom);
    if (dy === 0 || !scrollBy(dy)) return false;
    scrolled?.(at.x, at.y);
    return true;
  };
  // The repeat is armed only while a step moves the list, so a drag resting
  // mid-list or against the end of the list runs no timer.
  const repeat = () => {
    timer = step() ? setTimeout(repeat, AUTOSCROLL_REPEAT_MS) : null;
  };

  return {
    move(x, y) {
      const moved = !at || at.x !== x || at.y !== y;
      at = { x, y };
      if (moved && step() && timer === null) timer = setTimeout(repeat, AUTOSCROLL_REPEAT_MS);
    },
    stop() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      at = null;
    },
  };
}
