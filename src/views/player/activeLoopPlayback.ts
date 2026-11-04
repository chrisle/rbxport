import type { Cue } from "@/ipc/types";

/** Only djmdCue.Kind 4 in the MEMORY list may arm deck playback. */
export function activeMemoryLoop(cues: readonly Cue[]): Cue | null {
  return cues.find((cue) => cue.memory && cue.activeLoop === true && cue.outMs > cue.positionMs) ?? null;
}
