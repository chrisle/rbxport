/**
 * One row's waveform.
 *
 * Fetching and rendering happen once per track and the result is cached as a
 * bitmap, so scrolling back over a row is a single `drawImage` rather than an
 * IPC round trip and a redraw.
 */
import { HoverTarget } from "@/components/HoverTarget";
import { memo, useEffect, useRef, type MouseEvent as ReactMouseEvent } from "react";
import { getBackend } from "@/ipc/client";
import type { RowCue } from "@/ipc/types";
import {
  drawPreviewMemoryCues, drawPreviewCues, previewClickMs, renderPreview, waveformKindOf, WaveformCache,
  type RenderedWaveform, type WavePalette,
} from "@/canvas";
import { useTranslation } from "@/i18n";
import { hotCueColour } from "@/lib/hotCueColours";
import { usePreferences } from "@/store/usePreferences";
import { previewPositionMs, startPreview, stopPreview, usePreview } from "@/store/usePreview";
import styles from "./TrackTable.module.css";

/** Shared across every row: bounded, and released when entries fall out. */
const cache = new WaveformCache(500);

/**
 * In-flight renders, shared rather than skipped.
 *
 * An earlier version kept a Set and returned early when a key was already in
 * flight. Under StrictMode the effect runs twice: the first pass cancels itself
 * on cleanup and the second sees the in-flight marker and returns, so nothing
 * ever painted. Sharing the promise means every caller still gets the result.
 */
interface PreviewRequest {
  trackId: string;
  retired: boolean;
  promise: Promise<RenderedWaveform | null>;
}
const inFlight = new Map<string, PreviewRequest>();
const visibleRows = new Map<string, Set<() => void>>();

function retireRequests(trackId?: string): void {
  for (const [key, request] of inFlight) {
    if (trackId !== undefined && request.trackId !== trackId) continue;
    request.retired = true;
    inFlight.delete(key);
  }
}

export function clearWaveformPreviewCache(): void {
  cache.clear();
  retireRequests();
  visibleRows.forEach(listeners => listeners.forEach(refresh => refresh()));
}

// One native subscription for the module. A completed analysis wakes only its
// visible rows; requests begun before it cannot refill the bitmap cache.
void getBackend().then((backend) => {
  backend.onAnalysisChanged((trackId) => {
    cache.forget(trackId);
    retireRequests(trackId);
    visibleRows.get(trackId)?.forEach(refresh => refresh());
  });
});

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
  half: boolean,
): Promise<RenderedWaveform | null> {
  const existing = inFlight.get(key);
  if (existing) return existing.promise;

  const request: PreviewRequest = { trackId, retired: false, promise: read() };
  async function read(): Promise<RenderedWaveform | null> {
    await acquire();
    try {
      if (request.retired) return null;
      const backend = await getBackend();
      if (request.retired) return null;
      const data = await backend.trackWaveform(trackId, waveformKindOf(palette, false));
      if (request.retired || data.length === 0) return null;
      const rendered = await renderPreview(data, width, height, dpr, palette, half);
      if (request.retired) {
        if (rendered && "close" in rendered.bitmap) rendered.bitmap.close();
        return null;
      }
      if (rendered) cache.set(key, trackId, rendered);
      return rendered;
    } catch {
      // A track without analysis simply stays blank.
      return null;
    } finally {
      release();
      if (inFlight.get(key) === request) inFlight.delete(key);
    }
  }

  inFlight.set(key, request);
  return request.promise;
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
  /** Each memory cue's `djmdCue.Color`, in `memoryCues` order; 255 for none. */
  memoryCueColours?: readonly number[] | undefined;
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
  memoryCueColours,
  durationSec,
  startupCache = false,
}: WaveformPreviewProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const painted = useRef<string | null>(null);
  // View › Color › Waveform color: the row follows the deck's palette, and a
  // bitmap rendered in one palette is not the row in another. View ›
  // Full/Preview Waveform switches the row between half and full, as it does
  // in rekordbox.
  const { waveformColor: palette, hotCueColor, overviewWaveform } = usePreferences().view;
  const half = overviewWaveform !== "full";

  useEffect(() => {
    let cancelled = false;
    const dpr = window.devicePixelRatio || 1;
    const key = `${startupCache ? "startup" : "live"}:${palette}:${half ? "half" : "full"}:${WaveformCache.key(trackId, width, dpr)}`;

    const identity = `${key}:${height}`;
    const clear = () => {
      const canvas = ref.current;
      if (!canvas || cancelled || !painted.current) return;
      canvas.getContext("2d")?.clearRect(0, 0, canvas.width, canvas.height);
      painted.current = null;
    };
    const paint = (entry: { bitmap: CanvasImageSource }) => {
      const canvas = ref.current;
      if (!canvas || cancelled) return;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(entry.bitmap, 0, 0, canvas.width, canvas.height);
      painted.current = identity;
      // A cue without a colour of its own takes its slot's HOT CUE color
      // default, as rekordbox's row preview does.
      const badges = hotCues.map(([letter, at, colour, loop]) =>
        [letter, at, hotCueColour(colour, letter, loop === true, hotCueColor)] as const);
      drawPreviewMemoryCues(ctx, memoryCues ?? [], durationSec * 1000, canvas.width, dpr, memoryCueColours);
      drawPreviewCues(ctx, badges, durationSec * 1000, canvas.width, dpr);
    };

    let timer: number | undefined;
    let revision = 0;
    const refresh = () => {
      const current = ++revision;
      window.clearTimeout(timer);
      const cached = cache.get(key);
      if (cached) {
        paint(cached);
        return;
      }
      // A row a flick goes past unmounts before making a request.
      timer = window.setTimeout(() => {
        void load(trackId, key, width, height, dpr, palette, half).then((rendered) => {
          if (current !== revision) return;
          if (rendered) paint(rendered);
          else clear();
        });
      }, SETTLE_MS);
    };
    const listeners = visibleRows.get(trackId) ?? new Set<() => void>();
    listeners.add(refresh);
    visibleRows.set(trackId, listeners);
    // The canvas survives palette/track changes. Its previous pixels must
    // not stand in for a different format while that request is unavailable.
    if (painted.current !== identity) clear();
    refresh();

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      listeners.delete(refresh);
      if (listeners.size === 0) visibleRows.delete(trackId);
    };
  }, [trackId, width, height, hotCues, memoryCues, memoryCueColours, durationSec, palette, half, hotCueColor, startupCache]);

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
      <HoverTarget identity={`${trackId}:${width}:${height}`} enabled={durationSec > 0}
        position={(x, y, w) => previewClickMs(x, y, w, durationSec * 1000, hotCues) / (durationSec * 1000)} />
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
