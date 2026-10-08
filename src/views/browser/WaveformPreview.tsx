/**
 * One row's waveform.
 *
 * Fetching and rendering happen once per track and the result is cached as a
 * bitmap, so scrolling back over a row is a single `drawImage` rather than an
 * IPC round trip and a redraw.
 */
import { memo, useEffect, useRef, type MouseEvent as ReactMouseEvent } from "react";
import { getBackend } from "@/ipc/client";
import type { RowCue } from "@/ipc/types";
import {
  drawPreviewMemoryCues, drawPreviewCues, previewClickMs, renderPreview, waveformKindOf, WaveformCache,
  type RenderedWaveform, type WavePalette,
} from "@/canvas";
import { useTranslation } from "@/i18n";
import { usePreferences } from "@/store/usePreferences";
import { previewPositionMs, startPreview, stopPreview, usePreview } from "@/store/usePreview";
import styles from "./TrackTable.module.css";

/** Shared across every row: bounded, and released when entries fall out. */
const cache = new WaveformCache(500);

export function clearWaveformPreviewCache(): void {
  cache.clear();
}
// A re-analysed track's renderings are stale; its rows draw afresh when they
// next settle. One subscription for the module rather than one per row.
void getBackend().then((backend) => {
  backend.onAnalysisChanged((trackId) => cache.forget(trackId));
});

/**
 * In-flight renders, shared rather than skipped.
 *
 * An earlier version kept a Set and returned early when a key was already in
 * flight. Under StrictMode the effect runs twice: the first pass cancels itself
 * on cleanup and the second sees the in-flight marker and returns, so nothing
 * ever painted. Sharing the promise means every caller still gets the result.
 */
const inFlight = new Map<string, Promise<RenderedWaveform | null>>();

/**
 * How long a row must stay on screen before its waveform is asked for.
 *
 * A flick through a big playlist mounts and unmounts thousands of rows, and
 * every one of them used to cost a `track_waveform` round trip that read an
 * analysis file off disk — for a row nobody saw. Those run on the same
 * blocking pool as `fetch_rows`, so the rows being scrolled *to* queued behind
 * the waveforms of rows already gone, and the list came up blank until it
 * drained. About two frames: enough to still skip a hard flick (which turns a
 * page faster than this), short enough that a steady scroll starts loading a
 * row's waveform almost as soon as it appears.
 */
const SETTLE_MS = 35;

/**
 * How many waveform requests may be on the command channel at once.
 *
 * A screenful is about twenty rows and they all settle together. The cap keeps
 * them from putting twenty file reads in front of the next `fetch_rows`, which
 * is the call that has to land for the list to draw. It can afford to be wider
 * than it was: the row data is now fetched a window ahead (`PREFETCH_MARGIN`),
 * so a scroll rarely waits on `fetch_rows` at all, and the per-row scan that
 * made each waveform expensive is gone (`row_of_id`, a map lookup). Sixteen
 * fills a screen fast without the list text falling behind — measured on a
 * fresh scroll of the reference library, the text kept up.
 */
const MAX_CONCURRENT = 16;

let active = 0;
const waiting: Array<() => void> = [];

async function acquire(): Promise<void> {
  if (active < MAX_CONCURRENT) {
    active += 1;
    return;
  }
  await new Promise<void>((resolve) => {
    waiting.push(resolve);
  });
  active += 1;
}

function release(): void {
  active -= 1;
  waiting.shift()?.();
}

async function load(
  trackId: string,
  key: string,
  width: number,
  height: number,
  dpr: number,
  palette: WavePalette,
): Promise<RenderedWaveform | null> {
  const existing = inFlight.get(key);
  if (existing) return existing;

  const pending = (async () => {
    await acquire();
    try {
      const backend = await getBackend();
      const data = await backend.trackWaveform(trackId, waveformKindOf(palette, false));
      if (data.length === 0) return null;
      const rendered = await renderPreview(data, width, height, dpr, palette);
      if (rendered) cache.set(key, rendered);
      return rendered;
    } catch {
      // A track without analysis simply stays blank.
      return null;
    } finally {
      release();
      inFlight.delete(key);
    }
  })();

  inFlight.set(key, pending);
  return pending;
}

export interface WaveformPreviewProps {
  trackId: string;
  width: number;
  height: number;
  /**
   * The track's hot cues, drawn as lettered badges over the waveform.
   *
   * Painted onto the canvas after the cached bitmap rather than into it: the
   * bitmap is keyed by track and size and lives until evicted, and a cue
   * edited in the app would otherwise keep its old badge until then.
   */
  hotCues: readonly RowCue[];
  memoryCues?: readonly number[] | undefined;
  durationSec: number;
  /** Separates cold-start placeholder media from the live library rendering. */
  startupCache?: boolean;
}

export const WaveformPreview = memo(function WaveformPreview({
  trackId,
  width,
  height,
  hotCues,
  memoryCues,
  durationSec,
  startupCache = false,
}: WaveformPreviewProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  // View › Color › Waveform color: the row follows the deck's palette, and a
  // bitmap rendered in one palette is not the row in another.
  const { waveformColor: palette, hotCueColor } = usePreferences().view;

  useEffect(() => {
    let cancelled = false;
    const dpr = window.devicePixelRatio || 1;
    const key = `${startupCache ? "startup" : "live"}:${palette}:${WaveformCache.key(trackId, width, dpr)}`;

    const paint = (entry: { bitmap: CanvasImageSource }) => {
      const canvas = ref.current;
      if (!canvas || cancelled) return;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(entry.bitmap, 0, 0, canvas.width, canvas.height);
      // CDJ: every badge the fallback green, whatever the cue was given.
      const badges = hotCueColor === "cdj" ? hotCues.map(([letter, at]) => [letter, at, null] as const) : hotCues;
      drawPreviewMemoryCues(ctx, memoryCues ?? [], durationSec * 1000, canvas.width, dpr);
      drawPreviewCues(ctx, badges, durationSec * 1000, canvas.width, dpr);
    };

    const cached = cache.get(key);
    if (cached) {
      paint(cached);
      return;
    }

    // Nothing is asked for until the row has settled. A row a flick goes past
    // is unmounted before this fires, and the request is never made.
    const timer = window.setTimeout(() => {
      void load(trackId, key, width, height, dpr, palette).then((rendered) => {
        if (rendered) paint(rendered);
      });
    }, SETTLE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [trackId, width, height, hotCues, memoryCues, durationSec, palette, hotCueColor, startupCache]);

  const dpr = typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
  return (
    <span className={styles.previewWave} style={{ width: `${width}px`, height: `${height}px` }}>
      <canvas
        ref={ref}
        width={Math.round(width * dpr)}
        height={Math.round(height * dpr)}
        style={{ width: `${width}px`, height: `${height}px` }}
        data-preview-wave
        aria-hidden
      />
      <PreviewPlayhead trackId={trackId} width={width} />
    </span>
  );
});

/**
 * The preview's playhead and its stop button, over the row that holds it.
 *
 * Every row has one and all but one draw nothing. The line moves every frame
 * by its own transform; the component itself re-renders only when the preview
 * starts, stops or answers where it is.
 */
function PreviewPlayhead({ trackId, width }: { trackId: string; width: number }) {
  const t = useTranslation();
  const preview = usePreview();
  const line = useRef<HTMLSpanElement>(null);
  const mine = preview.playing && preview.trackId === trackId;

  useEffect(() => {
    if (!mine) return;
    let frame = 0;
    const draw = () => {
      const el = line.current;
      if (el && preview.durationMs > 0) {
        const at = Math.min(previewPositionMs(preview) / preview.durationMs, 1);
        el.style.transform = `translateX(${Math.round(at * width)}px)`;
      }
      frame = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(frame);
  }, [mine, preview, width]);

  if (!mine) return null;
  return (
    <>
      <span ref={line} className={styles.previewHead} aria-hidden />
      <button
        type="button"
        className={styles.previewStop}
        title={t("Stop")}
        aria-label={t("Stop")}
        onMouseDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.stopPropagation();
          void stopPreview();
        }}
      />
    </>
  );
}

/**
 * A click on a row's waveform: previews the track from there, without loading
 * it onto a deck — rekordbox's `ListViewer::cellClickedWithLeftButton` into
 * `PreviewComponent::clickWave`. Only a plain left click does: not the second
 * of a double click (that loads the deck), not one with Shift or Command
 * (Control on Windows and Linux), which extend the selection. A click on the
 * stop button, or anywhere but the waveform, is not one.
 *
 * Returns whether it started a preview.
 */
export function previewFromClick(
  e: ReactMouseEvent,
  trackId: string,
  durationSec: number,
  hotCues: readonly RowCue[],
): boolean {
  if (e.button !== 0 || e.detail > 1 || e.shiftKey || e.metaKey || e.ctrlKey) return false;
  const target = e.target as Element | null;
  if (!target || target.closest("button")) return false;
  const canvas = target.closest("[data-col=preview]")?.querySelector("canvas[data-preview-wave]");
  if (!canvas) return false;
  const box = canvas.getBoundingClientRect();
  const durationMs = durationSec * 1000;
  if (box.width <= 0 || durationMs <= 0) return false;
  const at = previewClickMs(e.clientX - box.left, e.clientY - box.top, box.width, durationMs, hotCues);
  void startPreview(trackId, at, durationMs);
  return true;
}
