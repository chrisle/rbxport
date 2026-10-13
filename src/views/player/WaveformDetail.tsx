/**
 * The player's waveform: a window around the playhead.
 *
 * A `span` of 1 is the whole track, which is what the overview strip above the
 * detail wants — same component, same bytes, one fetch.
 *
 * Deliberately not the row preview's component. That one renders once per
 * track and caches a bitmap, which is right for a row that never changes and
 * wrong here — the window moves with playback, so a cached bitmap would be
 * stale on the next frame. This draws straight from the bytes instead, which
 * are a few hundred of them and cost nothing to redraw.
 */
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { drawPcmWave, drawWave, strideOf, waveformKindOf, type HalfWaveform, type WavePalette } from "@/canvas";
import { getBackend } from "@/ipc/client";
import type { WaveformKind } from "@/ipc/types";
import { exactBackingSize } from "@/lib/canvasSize";
import { detailWaveOriginMs, detailWaveWindow, waveSlice } from "@/lib/player";
import { usePreferences } from "@/store/usePreferences";

/** Raw waveform bytes per track. Small — a few hundred bytes each. */
const bytesByTrack = new Map<string, Uint8Array>();
/** In-flight fetches, shared so StrictMode's double effect does not double-fetch. */
const inFlight = new Map<string, Promise<Uint8Array>>();

/** Waveform bytes are not interchangeable: PCM is eight bytes per point and
 * PWV7 is three. Keep their format beside the response during a zoom switch. */
interface LoadedWaveform {
  trackId: string;
  bytes: Uint8Array;
}
/** Tag bytes with the palette whose layout they are in, which is not the
 * chosen palette when the overview fell back to the `.DAT`'s preview. */
interface LoadedTag extends LoadedWaveform {
  palette: WavePalette;
}
interface PcmBuffer extends LoadedWaveform {
  revision: number;
  fromMs: number;
  toMs: number;
}

async function load(trackId: string, kind: WaveformKind): Promise<Uint8Array> {
  const key = `${trackId}:${kind}`;
  const cached = bytesByTrack.get(key);
  if (cached) return cached;
  const existing = inFlight.get(key);
  if (existing) return existing;

  const pending = (async () => {
    try {
      const backend = await getBackend();
      const data = await backend.trackWaveform(trackId, kind);
      bytesByTrack.set(key, data);
      return data;
    } catch {
      // A track without analysis simply stays blank.
      return new Uint8Array();
    } finally {
      inFlight.delete(key);
    }
  })();
  inFlight.set(key, pending);
  return pending;
}

/**
 * The bytes to draw for `palette`, and the palette they are in. A deck's
 * overview whose palette tag is missing (a 3Band `PWV6` on a track with no
 * `.2EX`, or an RGB `PWV4`) shows the `.DAT`'s 400-column `PWAV` in blue, as
 * rekordbox's deck does: `UiPlayer::updateTrackData` and
 * `UiPlayer::repaintWidgetPlayer` (7.2.19) use the 3-band or colour overview
 * only when it was loaded, and otherwise the `PWAV` one (#372). The detail
 * has no such fallback here.
 */
async function loadTag(trackId: string, palette: WavePalette, detail: boolean): Promise<LoadedTag> {
  const bytes = await load(trackId, waveformKindOf(palette, detail));
  if (bytes.length > 0 || detail || palette === "blue") return { trackId, bytes, palette };
  return { trackId, bytes: await load(trackId, waveformKindOf("blue", false)), palette: "blue" };
}

export interface WaveformDetailProps {
  trackId: string;
  /** How far through the track playback is, 0 to 1. */
  progress: number;
  /** How much of the track the window spans, as a fraction. 1 is all of it. */
  span?: number;
  /** The space the canvas occupies, in CSS pixels. */
  width: number;
  height: number;
  /** Half height from the baseline: stacked for the overview, overlaid for the 2 PLAYER detail. */
  half?: HalfWaveform;
  /** Read the full-resolution `PWV7` rather than the 1,200-column `PWV6`. */
  detail?: boolean;
  /** Decoded audio duration, used to put fixed-rate detail columns on their millisecond clock. */
  durationMs?: number;
  /** First timestamped beat, for removing a leading encoder delay from PWV7's clock. */
  firstBeatMs?: number | undefined;
  /** Rows to leave clear at the top and bottom, in CSS pixels. */
  inset?: { top: number; bottom: number };
  /** A source-audio window to draw as a stereo PCM envelope instead of PWV7. */
  pcmWindow?: { fromMs: number; toMs: number; drawFromMs: number; drawToMs: number } | undefined;
}

/** Drops a track's bytes: its analysis was rewritten. */
function forget(trackId: string): void {
  for (const key of [...bytesByTrack.keys()]) {
    if (key.startsWith(`${trackId}:`)) bytesByTrack.delete(key);
  }
}

export const WaveformDetail = memo(function WaveformDetail({
  trackId, progress, span = 0.08, width, height, half = false, detail = false,
  durationMs = 0, firstBeatMs, inset = { top: 0, bottom: 0 }, pcmWindow: requestedPcmWindow,
}: WaveformDetailProps) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [data, setData] = useState<LoadedTag | null>(null);
  const [pcmData, setPcmData] = useState<PcmBuffer | null>(null);
  const pendingPcm = useRef<Omit<PcmBuffer, "bytes"> | null>(null);
  const pcmMode = requestedPcmWindow !== undefined;
  // Bumped when the track is re-analysed, so the bytes are fetched again.
  const [revision, setRevision] = useState(0);
  // View › Color › Waveform color: each palette reads its own tags.
  const palette = usePreferences().view.waveformColor;
  const pcmFromMs = requestedPcmWindow?.fromMs;
  const pcmToMs = requestedPcmWindow?.toMs;
  const pcmDrawFromMs = requestedPcmWindow?.drawFromMs;
  const pcmDrawToMs = requestedPcmWindow?.drawToMs;
  // Player constructs this object while rendering. Keep it stable while its
  // bounds stay the same, otherwise an unrelated deck render would re-decode.
  const pcmWindow = useMemo(() => (
    pcmFromMs === undefined || pcmToMs === undefined || pcmDrawFromMs === undefined || pcmDrawToMs === undefined
      ? undefined
      : { fromMs: pcmFromMs, toMs: pcmToMs, drawFromMs: pcmDrawFromMs, drawToMs: pcmDrawToMs }
  ), [pcmFromMs, pcmToMs, pcmDrawFromMs, pcmDrawToMs]);

  useEffect(() => {
    if (pcmMode) return;
    let live = true;
    setData(null);
    void loadTag(trackId, palette, detail).then((loaded) => {
      if (live) setData(loaded);
    });
    return () => { live = false; };
  }, [trackId, detail, palette, revision, pcmMode]);

  useEffect(() => () => { pendingPcm.current = null; }, [trackId, revision, pcmMode]);

  useEffect(() => {
    if (!pcmWindow) return;
    // The player requests two seconds of guard audio on either side. Reuse
    // that buffer, and decode its replacement with 750 ms still in reserve.
    // A render/palette change must never clear a usable PCM envelope.
    const from = Math.max(pcmWindow.fromMs, pcmWindow.drawFromMs - 750);
    const to = Math.min(pcmWindow.toMs, pcmWindow.drawToMs + 750);
    if (pcmData?.trackId === trackId && pcmData.revision === revision
      && pcmData.fromMs <= from && pcmData.toMs >= to) {
      // A seek back into the cached window supersedes its pending successor.
      pendingPcm.current = null;
      return;
    }
    const pending = pendingPcm.current;
    if (pending?.trackId === trackId && pending.revision === revision
      && pending.fromMs <= Math.max(pcmWindow.fromMs, pcmWindow.drawFromMs)
      && pending.toMs >= Math.min(pcmWindow.toMs, pcmWindow.drawToMs)) return;
    const request = { trackId, revision, fromMs: pcmWindow.fromMs, toMs: pcmWindow.toMs };
    pendingPcm.current = request;
    void getBackend().then(backend => backend.trackPcmWaveform(trackId, request.fromMs, request.toMs, 7_500))
      .then(bytes => {
        if (pendingPcm.current !== request) return;
        pendingPcm.current = null;
        setPcmData({ ...request, bytes });
      }).catch(() => {
        // Retain the last good buffer on a failed read. The next window
        // update can retry; stale seeks/track responses cannot replace it.
        if (pendingPcm.current === request) pendingPcm.current = null;
      });
  }, [trackId, revision, pcmWindow, pcmData]);

  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      stop = backend.onAnalysisChanged((changed) => {
        if (changed !== trackId) return;
        forget(trackId);
        setRevision((r) => r + 1);
      });
    })();
    return () => {
      live = false;
      stop?.();
    };
  }, [trackId]);

  // A layout effect, so the redraw lands before the frame that shows it. The
  // strip that holds this is slid by a transform written in the parent's own
  // layout effect, and children run first: the two stay in step only because
  // the canvas is already new by the time the slide is written.
  useLayoutEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    // Exactly the device pixels the strip covers: this redraws on every size
    // change anyway, and a quantised store squeezed onto the strip resamples
    // every bar differently after each redraw — a flicker while playing (#346).
    const { width: w, height: h } = exactBackingSize(width, height);
    // Assigning either clears the canvas, so only when it actually changed.
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    if (pcmWindow) {
      const loaded = pcmData?.trackId === trackId && pcmData.revision === revision ? pcmData : null;
      const fromMs = loaded?.fromMs ?? pcmWindow.fromMs;
      const toMs = loaded?.toMs ?? pcmWindow.toMs;
      const duration = Math.max(toMs - fromMs, 1);
      // Always map samples with their own decoded bounds, not the bounds of
      // an in-flight replacement: otherwise transients jump during read-ahead.
      drawPcmWave(ctx, loaded?.bytes ?? new Uint8Array(), w, h, palette, {
        from: (pcmWindow.drawFromMs - fromMs) / duration,
        to: (pcmWindow.drawToMs - fromMs) / duration,
      });
      return;
    }
    if (!data || data.trackId !== trackId) {
      ctx.clearRect(0, 0, w, h);
      return;
    }

    // Centred on the playhead and *not* pinned: the head stays in the middle
    // and the track moves under it, so at either end the window hangs off the
    // edge. The overhang is drawn as nothing rather than as a stretched copy
    // of the first bar. Both tags cover the whole track, so a window into one
    // is a slice of its columns rather than a second fetch.
    // The bytes' own palette: the chosen one, or blue for the overview's
    // `PWAV` fallback, whose layout differs.
    const stride = strideOf(data.palette, detail);
    const originMs = detailWaveOriginMs(firstBeatMs, data.bytes, stride);
    const timed = detailWaveWindow(progress, span, durationMs, Math.floor(data.bytes.length / stride), originMs);
    const { first, last, x0, width: span_ } = waveSlice(timed.progress, timed.span, data.bytes.length, w, stride);
    // The inset is given in CSS pixels; the canvas is in device pixels.
    const scale = h / Math.max(height, 1);
    ctx.clearRect(0, 0, w, h);
    if (last <= first || span_ < 1) return;
    ctx.save();
    ctx.translate(x0, 0);
    drawWave(ctx, data.bytes.subarray(first, last), span_, h, data.palette, detail, half, {
      top: inset.top * scale,
      bottom: inset.bottom * scale,
    });
    ctx.restore();
  }, [data, pcmData, trackId, revision, progress, span, durationMs, firstBeatMs, width, height, half, detail, palette, inset.top, inset.bottom, pcmWindow]);

  return <canvas ref={ref} style={{ width: "100%", height: "100%", display: "block" }} />;
});
