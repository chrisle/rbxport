/**
 * Matching one deck to another: the tempo, and then the bar.
 *
 * Two numbers come out of here and nothing else — a tempo ratio and a distance
 * to move the follower's playhead — because that is all sync is once the grids
 * are known. Both are exact rather than servo-corrected: the engine owns the
 * sample clock, so a deck told to play at 1.0234× plays at exactly that, and a
 * playhead told to move 12.4 ms moves exactly that far. A phase-locked loop
 * belongs where the clock drifts, and this one does not.
 *
 * 4/4 throughout, as rekordbox's grid is.
 */
import { BEATS_PER_BAR, type BeatGrid } from "./player";

/** What a deck brings to the calculation. */
export interface Deck {
  /** Hundredths of a BPM, as the library stores it: the file's own tempo. */
  bpmX100: number;
  /**
   * The multiple of the file's speed the deck is playing at; 1 when absent.
   * A leader that has been nudged is followed at the tempo it is playing,
   * not the one printed on its file.
   */
  tempo?: number;
  /** Whether the deck is running; a leader that is not has no next beat to wait for. */
  playing?: boolean;
  /** Where the playhead is, in seconds. */
  position: number;
  /** The analysed grid, or an empty one. */
  grid: BeatGrid;
}

/** What to do to the follower to bring it into line. */
export interface Sync {
  /** The follower's new speed, as a multiple of its file's own. */
  tempo: number;
  /** Seconds to move the follower's playhead by, positive or negative. */
  nudge: number;
}

/**
 * The furthest sync will pull a deck, as a multiple of its own speed.
 *
 * A CDJ's tempo range at its widest is ±100%, and the stretcher's is the same,
 * so this is the stretcher's limit and not an opinion of its own. Beyond it
 * the two tracks are not a tempo apart, they are a different tempo.
 */
export const MIN_TEMPO = 0.5;
export const MAX_TEMPO = 2.0;

/**
 * The tempo that makes the follower play at the leader's BPM.
 *
 * Returns 1 when either side has no BPM: a track nobody has analysed syncs to
 * nothing, and pretending otherwise would silently play it at some ratio of
 * two numbers that mean nothing.
 */
export function tempoFor(leader: Deck, follower: Deck, options?: SyncOptions): number {
  if (leader.bpmX100 <= 0 || follower.bpmX100 <= 0) return 1;
  let ratio = (leader.bpmX100 * (leader.tempo ?? 1)) / follower.bpmX100;
  if (!Number.isFinite(ratio)) return 1;
  // "Allow BEAT/BPM SYNC with double/half BPM": a 140 next to a 70 is a
  // match at twice the leader's tempo, not a track slowed to half speed.
  // Whichever of the ratio, its double and its half is nearest to the
  // follower's own speed is the one taken.
  if (options?.doubleHalf ?? true) {
    for (const candidate of [ratio * 2, ratio / 2]) {
      if (Math.abs(Math.log(candidate)) < Math.abs(Math.log(ratio))) ratio = candidate;
    }
  }
  return Math.min(Math.max(ratio, MIN_TEMPO), MAX_TEMPO);
}

/** The Preferences window's BEAT/BPM SYNC choices. */
export interface SyncOptions {
  /** BPM SYNC matches the tempo alone and leaves the playhead where it is. */
  type?: "beat" | "bpm";
  /** Whether a track at twice or half the leader's BPM counts as matching. Default on. */
  doubleHalf?: boolean;
}

/**
 * Where the bar containing `seconds` began, and how long a bar lasts there.
 *
 * From the grid rather than from the BPM: a track that was recorded to a click
 * and a track that was played by hand have the same average BPM and a
 * different bar every eight bars, and the grid is the one that knows which.
 * Falls back to the BPM when the grid does not reach — the last bar of a track
 * whose analysis stopped early, most often.
 */
export function barAt(deck: Deck, seconds: number): { start: number; length: number } | null {
  const ms = seconds * 1000;
  const { times, numbers } = deck.grid;
  const beatSeconds = deck.bpmX100 > 0 ? 6000 / deck.bpmX100 : 0;
  if (times.length < 2) {
    if (beatSeconds <= 0) return null;
    const length = beatSeconds * BEATS_PER_BAR;
    return { start: Math.floor(seconds / length) * length, length };
  }

  // The last beat at or before the position, by binary search: a grid holds a
  // beat every half second and a track runs for minutes.
  let low = 0;
  let high = times.length - 1;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if ((times[middle] ?? 0) <= ms) low = middle;
    else high = middle - 1;
  }

  // Back to the downbeat of the bar this beat is in. `numbers` counts 1 to 4,
  // so beat 3 is two beats past its own downbeat.
  const within = (numbers[low] ?? 1) - 1;
  const downbeat = Math.max(0, low - within);
  const start = (times[downbeat] ?? 0) / 1000;
  const next = times[downbeat + BEATS_PER_BAR];
  const length =
    next === undefined ? beatSeconds * BEATS_PER_BAR || 0 : next / 1000 - start;
  return length > 0 ? { start, length } : null;
}

/**
 * How far to move the follower so its bar starts where the leader's does.
 *
 * The shortest way, which is at most half a bar: a deck that has to jump three
 * beats forward jumps one back instead, because the ear hears the beat and not
 * which bar it belongs to. The leader's bar length is what both are measured
 * in, since the follower is about to be playing at the leader's tempo.
 */
export function nudgeFor(leader: Deck, follower: Deck): number {
  const lead = barAt(leader, leader.position);
  const follow = barAt(follower, follower.position);
  if (!lead || !follow || lead.length <= 0) return 0;

  // How far each is into its own bar, as a fraction of it. The follower's is
  // measured in its own bar, because that is the one it is playing.
  const into = (bar: { start: number; length: number }, at: number) =>
    bar.length > 0 ? (at - bar.start) / bar.length : 0;
  const gap = into(lead, leader.position) - into(follow, follower.position);
  // Wrapped to the nearest half bar either way.
  const wrapped = gap - Math.round(gap);
  return wrapped * follow.length;
}

/**
 * How far to move the follower so its beat falls where the leader's does.
 *
 * The beat rather than the bar: this is what quantized play on a synced deck
 * lines up. A CDJ with SYNC and QUANTIZE on starts a track on the master's
 * beat, whichever beat of the bar that is — the bar is the DJ's to choose, by
 * where the cue was set. At most half a beat either way, as `nudgeFor` is at
 * most half a bar.
 *
 * `periodBeats` measures the phase in a part of a beat. A loop of a half
 * beat repeats twice in each beat of the master, so its phase is in a half
 * beat, and the nudge is at most a quarter beat.
 */
export function beatNudgeFor(leader: Deck, follower: Deck, periodBeats = 1): number {
  const lead = barAt(leader, leader.position);
  const follow = barAt(follower, follower.position);
  if (!lead || !follow || lead.length <= 0 || follow.length <= 0 || !(periodBeats > 0)) return 0;
  const phaseOf = (bar: { start: number; length: number }, at: number) => {
    const period = (bar.length / BEATS_PER_BAR) * periodBeats;
    const into = (at - bar.start) / period;
    return { fraction: into - Math.floor(into), period };
  };
  const leadPhase = phaseOf(lead, leader.position);
  const followPhase = phaseOf(follow, follower.position);
  const gap = leadPhase.fraction - followPhase.fraction;
  const wrapped = gap - Math.round(gap);
  return wrapped * followPhase.period;
}

/**
 * The period that a loop keeps in phase with the master, in beats, or null
 * when the loop cannot stay in phase.
 *
 * A loop of whole beats starts again on a beat, so its period is one beat. A
 * loop of 1/2, 1/4 or 1/8 beat starts again on each part of a beat, so its
 * period is its length. Any other length (1.5 beats, for example) moves the
 * deck off the beat on each repeat.
 */
export function loopPeriodBeats(loopSeconds: number, beatSeconds: number): number | null {
  if (!(loopSeconds > 0) || !(beatSeconds > 0)) return null;
  const beats = loopSeconds / beatSeconds;
  const near = (a: number, b: number) => Math.abs(a - b) <= LOOP_TOLERANCE * Math.max(1, b);
  if (beats >= 1 - LOOP_TOLERANCE) return near(beats, Math.round(beats)) ? 1 : null;
  for (let part = 2; part <= 32; part *= 2) {
    if (near(beats, 1 / part)) return 1 / part;
  }
  return null;
}

/** How far a loop length can be from a whole or part beat, as a fraction of a beat. */
const LOOP_TOLERANCE = 0.02;

/** A loop that plays, in seconds. */
export interface PlayingLoop {
  inSeconds: number;
  outSeconds: number;
}

/**
 * Where a move of the follower to `at` lands, in phase with the leader.
 *
 * The landing moves by half a period at most, so the bar the hand chose
 * stays the bar that plays. Inside a loop the period is the loop's (see
 * `loopPeriodBeats`), and a landing past an end goes back into the loop by
 * whole loops. A loop of a length that cannot stay in phase leaves `at` as
 * it is: a move on each repeat would be worse than the loop itself.
 */
export function inPhaseAt(leader: Deck, follower: Deck, at: number, loop: PlayingLoop | null): number {
  const length = loop ? loop.outSeconds - loop.inSeconds : 0;
  const inside = (time: number) =>
    loop !== null && length > 0 && time >= loop.inSeconds && time < loop.outSeconds;
  let period: number | null = 1;
  if (loop && inside(at)) {
    const bar = barAt(follower, loop.inSeconds);
    period = bar ? loopPeriodBeats(length, bar.length / BEATS_PER_BAR) : null;
  }
  if (period === null) return at;
  let to = at + beatNudgeFor(leader, { ...follower, position: at }, period);
  if (loop && inside(at) && !inside(to)) {
    to = loop.inSeconds + ((((to - loop.inSeconds) % length) + length) % length);
  }
  return to;
}

/**
 * How long until the leader's next beat, in real seconds — its file's time
 * to the beat divided by the tempo it is playing at — or null when it has
 * no grid and no BPM to say. A leader already on a beat, within a few
 * milliseconds, answers a whole beat: the press came too late for that one.
 */
export function beatWait(leader: Deck): number | null {
  const bar = barAt(leader, leader.position);
  if (!bar || bar.length <= 0) return null;
  const beat = bar.length / BEATS_PER_BAR;
  const into = (leader.position - bar.start) % beat;
  let left = beat - into;
  if (left < ON_THE_BEAT) left += beat;
  return left / (leader.tempo ?? 1);
}

/** Closer than this to a beat is on it, in seconds: about a callback. */
const ON_THE_BEAT = 0.005;

/**
 * Both halves: what the follower's tempo becomes and how far it moves.
 *
 * A deck with no grid and no BPM comes back as "leave it alone" rather than as
 * a guess — sync acting on a track nobody has analysed is sync putting it in
 * the wrong place with confidence.
 */
export function syncTo(leader: Deck, follower: Deck, options?: SyncOptions): Sync {
  return {
    tempo: tempoFor(leader, follower, options),
    nudge: options?.type === "bpm" ? 0 : nudgeFor(leader, follower),
  };
}
