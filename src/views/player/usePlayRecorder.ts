import { useEffect, useRef } from "react";

import { getBackend } from "@/ipc/client";

/** What decides whether, and when, the deck records a play. */
export interface PlayRecorderInput {
  /** The loaded track's id, or `null` for an empty deck. */
  trackId: string | null;
  /** Whether the track is sounding. */
  playing: boolean;
  /** Preferences › Advanced › History: plays go on the history. */
  enabled: boolean;
  /** Preferences › Advanced › Browse › Playback time setting, in seconds. */
  seconds: number;
  onError?: ((message: string | null) => void) | undefined;
}

/**
 * Records a play after `seconds` of the track sounding, once per load.
 * Seconds played before a pause count; loading another track starts again.
 * This is rekordbox's PERFORMANCE mode timer
 * (`PlayHistoryManager::startCountDown` @0x101b9c58c starts
 * `getCurrentPlayedTime() * 1000` ms; 60 s by default) [OBS static, #540].
 */
export function usePlayRecorder({ trackId, playing, enabled, seconds, onError }: PlayRecorderInput): void {
  const playedSeconds = useRef(0);
  const recordedFor = useRef<string | null>(null);
  useEffect(() => {
    playedSeconds.current = 0;
    recordedFor.current = null;
  }, [trackId]);
  useEffect(() => {
    if (!playing || trackId === null || !enabled) return undefined;
    const id = trackId;
    const timer = globalThis.setInterval(() => {
      playedSeconds.current += 1;
      if (playedSeconds.current < seconds || recordedFor.current === id) return;
      recordedFor.current = id;
      void getBackend()
        .then((backend) => backend.edits.recordPlay(id))
        .catch((e: unknown) => onError?.(e instanceof Error ? e.message : "The play could not be recorded."));
    }, 1000);
    return () => globalThis.clearInterval(timer);
  }, [playing, trackId, enabled, seconds, onError]);
}
