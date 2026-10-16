/**
 * Memory and hot cue lookups, as pure functions over a track's cue list.
 *
 * The deck's MEMORY cluster is three buttons and a key each — `B` calls the
 * memory cue before the playhead, `N` the one after, `X` deletes the one it
 * is standing on, `M` stores the cue point as one — all from rekordbox's own
 * Export key map. What "before", "after" and "standing on" mean is decided
 * here, where it can be tested without a deck. The hot cue pads are simpler:
 * a slot is a letter, and a letter is set or it is not.
 */
import type { Cue, RowCue } from "@/ipc/types";

/**
 * How close to a memory cue the playhead has to be to count as on it, in
 * milliseconds.
 *
 * Wide enough that a playhead that has just been called to a cue is still on
 * it after the engine's rounding, and narrow enough that two cues a beat
 * apart are never confused. The same 20 ms the CUE button uses to decide
 * whether it is on the cue point. [ASSUME] rekordbox's own tolerance is not
 * documented; nothing here depends on matching it exactly.
 */
export const MEMORY_TOLERANCE_MS = 20;

/** The memory cues of a list, in position order. Hot cues are not called. */
function memoryCues(cues: readonly Cue[]): Cue[] {
  return cues.filter((cue) => cue.memory).sort((a, b) => a.positionMs - b.positionMs);
}

/** The nth memory cue from the start of the track, one-based, or `null`. */
export function memoryCueNumber(cues: readonly Cue[], n: number): Cue | null {
  return memoryCues(cues)[n - 1] ?? null;
}

/**
 * The first memory cue after the playhead, or `null` when there is none.
 *
 * Strictly after, by more than the tolerance: called to a cue and pressed
 * again, ▶ moves to the next one rather than staying put.
 */
export function nextMemoryCue(cues: readonly Cue[], positionMs: number): Cue | null {
  for (const cue of memoryCues(cues)) {
    if (cue.positionMs > positionMs + MEMORY_TOLERANCE_MS) return cue;
  }
  return null;
}

/** The last memory cue before the playhead, or `null`. The mirror of `next`. */
export function previousMemoryCue(cues: readonly Cue[], positionMs: number): Cue | null {
  let found: Cue | null = null;
  for (const cue of memoryCues(cues)) {
    if (cue.positionMs < positionMs - MEMORY_TOLERANCE_MS) found = cue;
    else break;
  }
  return found;
}

/**
 * The memory cue the playhead is standing on, or `null`.
 *
 * What ✕ deletes. Only a cue within the tolerance counts: a delete that took
 * the nearest cue however far away would be a surprise with a track's worth
 * of cues at stake, and rekordbox's own X does nothing away from a cue.
 */
export function memoryCueAt(cues: readonly Cue[], positionMs: number): Cue | null {
  let best: Cue | null = null;
  for (const cue of cues) {
    if (!cue.memory) continue;
    const distance = Math.abs(cue.positionMs - positionMs);
    if (distance > MEMORY_TOLERANCE_MS) continue;
    if (!best || distance < Math.abs(best.positionMs - positionMs)) best = cue;
  }
  return best;
}

/**
 * The memory loop running from `inMs` to `outMs`, within the same tolerance
 * at both ends, or `null`. A plain memory cue at `inMs` is not one: what
 * MEMORY checks before it saves a loop.
 */
export function memoryLoopAt(cues: readonly Cue[], inMs: number, outMs: number): Cue | null {
  return cues.find((cue) => cue.memory && cue.outMs > cue.positionMs
    && Math.abs(cue.positionMs - inMs) <= MEMORY_TOLERANCE_MS
    && Math.abs(cue.outMs - outMs) <= MEMORY_TOLERANCE_MS) ?? null;
}

/**
 * The hot cue in a slot, or `null` when the pad is empty.
 *
 * A slot holds one cue. The writer does not stop a second row landing in the
 * same `Kind`, and the pads never ask it to — an occupied pad calls its cue
 * rather than setting over it — but a library is not ours alone, so if a slot
 * ever does hold two the earlier one is the pad's, as it is the earlier one
 * the list would have shown first.
 */
export function hotCue(cues: readonly Cue[], letter: string): Cue | null {
  let found: Cue | null = null;
  for (const cue of cues) {
    if (cue.memory || cue.letter !== letter) continue;
    if (!found || cue.positionMs < found.positionMs) found = cue;
  }
  return found;
}

/**
 * The letters of a track's hot cues, in letter order: `"ABCD"`.
 *
 * The same string the backend puts in a browser row's `cues`, built the same
 * way — letter order, each once — so a row patched from a deck's cue list
 * reads exactly as one fetched afresh would.
 */
export function hotLetters(cues: readonly Cue[]): string {
  return rowCuesOf(cues).map(([letter]) => letter).join("");
}

/**
 * A track's hot cues as its browser row carries them: one per slot, in
 * letter order, with the colour the deck was handed. The same shape the
 * backend builds a row from, so a row patched with this reads as if it had
 * been fetched after the edit.
 */
export function rowCuesOf(cues: readonly Cue[]): RowCue[] {
  const seen = new Set<string>();
  const out: RowCue[] = [];
  for (const cue of cues) {
    if (cue.memory || cue.letter === "" || seen.has(cue.letter)) continue;
    seen.add(cue.letter);
    out.push([cue.letter, cue.positionMs, cue.colour, cue.outMs > cue.positionMs]);
  }
  return out.sort((a, b) => a[0].localeCompare(b[0]));
}
