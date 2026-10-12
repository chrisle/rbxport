/**
 * How big a canvas's backing store should be for the space it occupies.
 *
 * A canvas stretched by CSS from a fixed backing store is blurry on a wide
 * window and wasteful on a narrow one, so the store follows the element. Two
 * things stop that being expensive:
 *
 * - the width is quantised, so dragging the window edge reallocates the buffer
 *   a handful of times rather than once per pixel;
 * - the device pixel ratio is capped, because past 2× the extra samples are
 *   not visible in a 14-pixel-high waveform and the buffer grows with the
 *   square of the ratio.
 */

/** Reallocate in steps this wide rather than on every pixel of a drag. */
export const STEP = 16;

/** Beyond this the extra resolution is not visible in a waveform strip. */
export const MAX_RATIO = 2;

/**
 * Guards against a runaway layout asking for a buffer megabytes wide.
 *
 * The hardware's own limit, not a budget: 4096 looked generous until the
 * detail waveform, which draws `OVERDRAW` spans at up to 2x, asked for
 * 6,028 device pixels on the reference 1800pt window and got 4,096 stretched
 * across them. A canvas scaled up by 1.47 turns the one-pixel bars into a
 * smear, which reads as the waveform losing its colour. Every engine we ship
 * on takes 16,384 in one dimension.
 */
export const MAX_WIDTH = 16_384;

export interface BackingSize {
  width: number;
  height: number;
}

export function backingSize(
  cssWidth: number,
  cssHeight: number,
  ratio: number = typeof window === "undefined" ? 1 : window.devicePixelRatio,
): BackingSize {
  // `Math.max(NaN, 1)` is NaN, and a canvas given a NaN width silently keeps
  // its old buffer — so anything not a real number becomes the floor here.
  const scale = Math.min(Math.max(finite(ratio, 1), 1), MAX_RATIO);
  const stepped = Math.ceil(Math.max(finite(cssWidth, 1), 1) / STEP) * STEP;
  return {
    width: Math.min(MAX_WIDTH, Math.max(STEP, Math.round(stepped * scale))),
    height: Math.max(1, Math.round(Math.max(finite(cssHeight, 1), 1) * scale)),
  };
}

/**
 * A backing store exactly the device pixels the element covers, unquantised.
 *
 * For a canvas that is redrawn on every size change anyway, where the step
 * saves nothing and costs sharpness: a store 16 CSS pixels wider than its
 * element is squeezed onto it, so every one-pixel bar is resampled across two
 * columns. On the scrolling detail waveform that resampling lands differently
 * after each redraw, which reads as the waveform flickering (#346).
 */
export function exactBackingSize(
  cssWidth: number,
  cssHeight: number,
  ratio: number = typeof window === "undefined" ? 1 : window.devicePixelRatio,
): BackingSize {
  const scale = Math.min(Math.max(finite(ratio, 1), 1), MAX_RATIO);
  return {
    width: Math.min(MAX_WIDTH, Math.max(1, Math.round(Math.max(finite(cssWidth, 1), 1) * scale))),
    height: Math.max(1, Math.round(Math.max(finite(cssHeight, 1), 1) * scale)),
  };
}

function finite(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}
