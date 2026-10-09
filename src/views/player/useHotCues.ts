/**
 * The hot cue pads: set an empty one, call a set one, clear from the list.
 *
 * rekordbox's wording, from `german.lang` and the Export key map: `Set Hot
 * Cue A` (`1`, `2`, `3` for A to C; nothing is bound past C) and `Clear Hot
 * Cue A` (`command + 1`-`3`). A pad is set or it is not, and what a press
 * does follows from that: an empty pad takes the playhead, a set one calls
 * its cue. The HOT CUE list beside the deck has a ✕ per set row.
 *
 * Calling a set pad also starts a stopped deck. rekordbox 7's manual, EXPORT
 * mode "Calling and playing saved hot cue points" (p.102): "Select a hot cue
 * point. Playback starts from the selected hot cue point." The only
 * exception it names is the Gate Cue preference ("Gate playback during Pause
 * (Gate Cue)", p.248), which rbx does not offer, so a call from pause plays
 * and keeps playing [OBS manual].
 *
 * With Q on, a call on a playing deck waits for the beat. rekordbox 7.2.19
 * in EXPORT mode plays on to the next quantize step (at the cue's own place
 * within a step) and jumps there, so the rhythm runs on unbroken: see
 * `quantizedLaunchMs`. This holds with or without BEAT SYNC; a synced deck
 * is on the master's beat already, so the jump keeps it there. Export mode
 * never applies PERFORMANCE mode's "Jump before reaching the next beat"
 * preference, so the wait is the only behaviour it has [OBS static,
 * `PlayerWaveView::updateQuantizeSettings` @0x100c8f10c]. From pause the call
 * is at once, as is any call with Q off.
 *
 * A set pad is never set over. What rekordbox does with the old row when a
 * slot is filled twice — a soft delete and a new row, or an update in place
 * — has not been recorded [UNKNOWN], so the pad calls rather than replaces,
 * and the writer is never asked to fill an occupied slot.
 */
import { useCallback } from "react";

import type { Cue } from "@/ipc/types";
import { hotCue } from "@/lib/cues";
import { nearestBeatMs, quantizedLaunchMs, type BeatGrid } from "@/lib/player";
import { useCueWriter } from "./useCueWriter";

export interface HotCueDeck {
  /** The loaded track's id, or `null` when the deck is empty. */
  trackId: string | null;
  cues: readonly Cue[];
  /** The playhead, in seconds, read at the moment a pad goes down. */
  positionSeconds: () => number;
  seek: (seconds: number) => void;
  /**
   * Starts the deck from where `seek` put it, as PLAY does; nothing when it
   * is already playing. A called hot cue plays from its point in rekordbox.
   */
  play: () => void;
  /** Whether the deck is playing now. A paused deck has no beat to wait for. */
  playing?: (() => boolean) | undefined;
  /**
   * Jumps to `toSeconds` when the playhead reaches `atSeconds`, carrying on
   * from there as if the music had not been cut: a quantized hot cue call.
   * Without it a call is always made at once.
   */
  jumpAt?: ((atSeconds: number, toSeconds: number) => void) | undefined;
  /**
   * The grid to snap a new hot cue to, when Q is on, or `null`. The same
   * rule CUE follows: with Q on a cue lands on the nearest beat, which is
   * why a CDJ's cues sit on the grid whatever the finger did.
   */
  quantiseTo: BeatGrid | null;
  /** Rekordbox holds the database, so nothing here can write. */
  readOnly: boolean;
  onError?: ((message: string | null) => void) | undefined;
}

export interface HotCueActions {
  /** Whether an empty pad can be set: a track is loaded and can be written. */
  canEdit: boolean;
  /** The cue in a slot, or `null` for an empty pad. */
  at: (letter: string) => Cue | null;
  /**
   * A pad press: `Set Hot Cue <letter>` on an empty pad, a call on a set one.
   * A call plays from the cue; setting a cue leaves the transport alone.
   */
  press: (letter: string) => void;
  /** `Clear Hot Cue <letter>`: the ✕ on a list row, and `command + 1`-`3`. */
  clear: (letter: string) => void;
}

export function useHotCues(deck: HotCueDeck): HotCueActions {
  const { trackId, cues, positionSeconds, seek, play, playing, jumpAt, quantiseTo, readOnly, onError } = deck;
  const canEdit = trackId !== null && !readOnly;
  const write = useCueWriter(onError);

  const at = useCallback((letter: string) => hotCue(cues, letter), [cues]);

  const press = useCallback(
    (letter: string) => {
      const cue = hotCue(cues, letter);
      if (cue) {
        // Calling a hot cue is a jump that plays: from pause rekordbox starts
        // playback at the cue (manual p.102), and a playing deck carries on
        // from it. Unlike a memory cue it does not become the cue point, on a
        // CDJ or in rekordbox [REF].
        if (quantiseTo && jumpAt && playing?.()) {
          const at = quantizedLaunchMs(quantiseTo, positionSeconds() * 1000, cue.positionMs);
          if (at !== null) {
            jumpAt(at / 1000, cue.positionMs / 1000);
            return;
          }
        }
        seek(cue.positionMs / 1000);
        play();
        return;
      }
      if (!canEdit || trackId === null) return;
      const at = Math.max(positionSeconds(), 0) * 1000;
      const positionMs = Math.round(quantiseTo ? nearestBeatMs(quantiseTo, at) : at);
      write((edits) => edits.addCue(trackId, { hot: letter }, positionMs));
    },
    [cues, seek, play, playing, jumpAt, canEdit, trackId, positionSeconds, quantiseTo, write],
  );

  const clear = useCallback(
    (letter: string) => {
      const cue = hotCue(cues, letter);
      // An empty id is a cue the backend cannot address; the row shows it
      // without a ✕, and a key press finds nothing to do.
      if (!cue || !canEdit || cue.id === "") return;
      write((edits) => edits.deleteCue(cue.id));
    },
    [cues, canEdit, write],
  );

  return { canEdit, at, press, clear };
}
