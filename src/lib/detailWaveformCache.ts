/** Shared detail bytes live beyond a deck's current track and layout. */
import { getBackend } from "@/ipc/client";
import type { Backend, WaveformKind } from "@/ipc/types";

/** Raw waveform bytes per track. Small — a few hundred bytes each. */
const bytesByTrack = new Map<string, Uint8Array>();
/** In-flight fetches, shared so StrictMode's double effect does not double-fetch. */
const inFlight = new Map<string, Promise<Uint8Array>>();
/**
 * Bumped per track each time its bytes are dropped, so a fetch that set off
 * before then neither fills the cache nor is shared with a newer request:
 * while an analysis draws the track, its bytes change several times a second.
 */
const editions = new Map<string, number>();

export async function loadDetailWaveform(
  trackId: string, kind: WaveformKind, retryPendingMiss?: () => boolean,
): Promise<Uint8Array> {
  const key = `${trackId}:${kind}`;
  const cached = bytesByTrack.get(key);
  if (cached) return cached;
  const edition = editions.get(trackId) ?? 0;
  const current = () => (editions.get(trackId) ?? 0) === edition;
  const existing = inFlight.get(key);
  if (existing) {
    const bytes = await existing;
    // A newer explicit Load can join a read made before analysis returned.
    // Retry that miss once, only while its committed owner/edition survives.
    return bytes.length === 0 && current() && retryPendingMiss?.()
      ? loadDetailWaveform(trackId, kind)
      : bytes;
  }
  const pending = (async () => {
    try {
      const backend = await getBackend();
      const data = await backend.trackWaveform(trackId, kind);
      // A missing companion/tag is an empty success. Do not keep that miss
      // across a later mount after the analysis files become available.
      if (current() && data.length > 0) bytesByTrack.set(key, data);
      return data;
    } catch {
      // A track without analysis simply stays blank.
      return new Uint8Array();
    } finally {
      if (current()) inFlight.delete(key);
    }
  })();
  inFlight.set(key, pending);
  return pending;
}

/** Drops a track's bytes: its analysis was rewritten, or drew further. */
export function forgetDetailWaveform(trackId: string): void {
  editions.set(trackId, (editions.get(trackId) ?? 0) + 1);
  for (const cache of [bytesByTrack, inFlight]) {
    for (const key of [...cache.keys()]) {
      if (key.startsWith(`${trackId}:`)) cache.delete(key);
    }
  }
}

const visible = new Map<string, Set<() => void>>();

/** Visible decks refresh only after the shared completion cache is invalidated. */
export function subscribeDetailWaveform(trackId: string, refresh: () => void): () => void {
  let listeners = visible.get(trackId);
  if (!listeners) visible.set(trackId, listeners = new Set());
  listeners.add(refresh);
  return () => {
    listeners.delete(refresh);
    if (listeners.size === 0) visible.delete(trackId);
  };
}

/** Owned by the app shell, including while no deck is mounted. */
export function watchDetailWaveforms(backend: Pick<Backend, "onAnalysisChanged">): () => void {
  let live = true;
  const stop = backend.onAnalysisChanged(trackId => {
    if (!live) return;
    forgetDetailWaveform(trackId);
    visible.get(trackId)?.forEach(refresh => refresh());
  });
  return () => { live = false; stop(); };
}
