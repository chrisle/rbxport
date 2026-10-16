/**
 * The master knob's scale.
 *
 * The knob reads 0 to 10, and 10 is a decibel under full: the headroom
 * every output is given. Past 10 is a notch, and past the notch is 11 at
 * +2 dB, drawn a notch beyond the 10 at the end of the travel. Between 0
 * and 10 the taper is logarithmic, forty decibels across the last decade,
 * so the bottom of the travel is quiet rather than silent-until-nine.
 */

/** The reading at the top of the ordinary travel. */
export const KNOB_TOP = 10;
/** The reading past the notch. */
export const KNOB_FULL = 11;
/** What the top of the travel is, in decibels. */
export const TOP_DB = -1;
/** The boost at 11, in decibels. */
export const FULL_DB = 2;
/** The engine gain at 11. */
export const FULL_GAIN = 10 ** (FULL_DB / 20);

/** Decibels for a reading; −Infinity at 0. */
export function knobToDb(reading: number): number {
  if (!(reading > 0)) return Number.NEGATIVE_INFINITY;
  if (reading >= KNOB_FULL) return FULL_DB;
  const inTravel = Math.min(reading, KNOB_TOP);
  return TOP_DB + 40 * Math.log10(inTravel / KNOB_TOP);
}

/** The engine's linear gain for a reading, 0 to +2 dB. */
export function knobToGain(reading: number): number {
  const db = knobToDb(reading);
  return Number.isFinite(db) ? Math.min(10 ** (db / 20), FULL_GAIN) : 0;
}

/** The reading the engine's gain stands for: 11 at full, else on the taper. */
export function gainToKnob(gain: number): number {
  if (!(gain > 0)) return 0;
  const db = 20 * Math.log10(Math.min(gain, FULL_GAIN));
  // Anything past the top of the travel is the notch's own setting.
  if (db > TOP_DB + 0.05) return KNOB_FULL;
  return Math.min(KNOB_TOP * 10 ** ((db - TOP_DB) / 40), KNOB_TOP);
}

/** The readout while the knob turns: whole numbers, `10`, then `11`. */
export function knobLabel(reading: number): string {
  return String(Math.round(Math.min(Math.max(reading, 0), KNOB_FULL)));
}

/**
 * Volume / Volume Down (rekordbox's 3003 and 3004, command + F12 and F11):
 * the master gain one knob reading up or down, from the reading the knob
 * shows, kept within 0 and 11.
 */
export function stepMasterGain(gain: number, direction: 1 | -1): number {
  const reading = Math.round(gainToKnob(gain));
  const next = direction === 1 ? Math.min(reading + 1, KNOB_FULL) : Math.max(reading - 1, 0);
  return knobToGain(next);
}

/**
 * Mute (rekordbox's 3005, command + F10): the master goes to silence and
 * remembers where it was, and a second Mute puts it back. `mutedFrom` is
 * the level it was muted from, or null when it is not muted. A silent
 * master that was not muted stays as it is.
 */
export function toggleMute(gain: number, mutedFrom: number | null): { gain: number; mutedFrom: number | null } {
  if (mutedFrom !== null) return { gain: mutedFrom, mutedFrom: null };
  if (gain > 0) return { gain: 0, mutedFrom: gain };
  return { gain, mutedFrom: null };
}
