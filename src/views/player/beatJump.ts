import { jumpStepSeconds, type BeatGrid, type JumpSize } from "@/lib/player";

interface JumpPlayback {
  moveByGridBeats: (gridTimesMs: number[], beats: number) => void;
  moveBy: (seconds: number) => void;
}

/** A no-loop jump. Grid moves must use the audio head when the command arrives. */
export function jumpHead(
  playback: JumpPlayback, grid: BeatGrid, size: JumpSize, bpmX100: number,
  displayedSeconds: number, direction: number, phaseLocked: boolean,
  seekInPhase: (seconds: number) => void,
): void {
  const beats = size.beats * direction;
  if (size.beats > 0 && grid.times.length >= 2) {
    // A whole-beat grid move retains the head's fractional beat phase. The
    // deck's phase check corrects any remaining difference from the master.
    playback.moveByGridBeats(Array.from(grid.times), beats);
    return;
  }
  const by = jumpStepSeconds(size, bpmX100) * direction;
  if (by === 0) return;
  if (phaseLocked) seekInPhase(displayedSeconds + by);
  else playback.moveBy(by);
}
