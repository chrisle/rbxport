/**
 * The track filter bar's state, and what it asks Rust for.
 *
 * The bar holds picks per column and a tick box that turns each column on.
 * Only ticked columns reach the backend, as `ViewSpec.filter`; the picks in
 * an unticked column are kept so ticking it again restores them, which is
 * what rekordbox does with its own.
 *
 * Nothing here touches a row: the value lists come from `filterValues`, and
 * the narrowing happens in `rbl-index`.
 */
import type { TrackFilter } from "@/ipc/types";

/**
 * The eight colour comments, in `ColorID` order (1 to 8).
 *
 * `[REF]` `djmdColor` on the reference library, read-only: `Commnt` Pink, Red,
 * Orange, Yellow, Green, Aqua, Blue, Purple under ids 1 to 8, in `SortKey`
 * order — the order the capture lists them in.
 */
export const COLOR_NAMES: readonly string[] =
  ["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"];

/** The `MASTER PLAYER ±` list: ±0% to ±6%, as the capture shows. */
export const TOLERANCES: readonly number[] = [0, 1, 2, 3, 4, 5, 6];

/** The rating rows: no stars through five. */
export const RATINGS: readonly number[] = [0, 1, 2, 3, 4, 5];

/**
 * How many tag columns the bar draws.
 *
 * rekordbox draws one per My Tag category and the reference library has four;
 * this pads the bar out to the capture when a library has fewer.
 */
export const TAG_COLUMNS = 4;

/** One column's picks. An empty `values` is `All`. */
export interface PickedColumn<T> {
  enabled: boolean;
  values: T[];
}

/** A tag column's tick box, its picked tags (empty is `All`) and its AND / OR switch. */
export interface TagColumn extends PickedColumn<string> {
  /** `true` is OR — a track carrying any picked tag; `false` is AND. */
  any: boolean;
}

export interface FilterState {
  bpm: PickedColumn<number> & { tolerancePct: number };
  key: PickedColumn<string>;
  rating: PickedColumn<number>;
  color: PickedColumn<string>;
  tags: TagColumn[];
}

export const EMPTY_FILTER: FilterState = {
  bpm: { enabled: false, values: [], tolerancePct: 0 },
  key: { enabled: false, values: [] },
  rating: { enabled: false, values: [] },
  color: { enabled: false, values: [] },
  tags: Array.from({ length: TAG_COLUMNS }, () => ({ enabled: false, values: [], any: false })),
};

/** The nearest whole BPM, as the bar's list groups them. */
export function wholeBpm(bpmX100: number): number {
  return Math.round(bpmX100 / 100);
}

/**
 * What a click in a list does to its picks.
 *
 * A plain click picks that one value; with the toggle modifier it is added to
 * or taken out of the picks, so several can be picked. `null` is the `All`
 * row, which clears the picks.
 */
export function pick<T>(values: readonly T[], value: T | null, toggle: boolean): T[] {
  if (value === null) return [];
  if (!toggle) return [value];
  return values.includes(value) ? values.filter((v) => v !== value) : [...values, value];
}

/** Whether any column is ticked, which is when the bar narrows the list. */
export function isNarrowing(state: FilterState): boolean {
  return state.bpm.enabled || state.key.enabled || state.rating.enabled || state.color.enabled || state.tags.some((t) => t.enabled);
}

/**
 * The picks as the backend wants them, or `undefined` when nothing is ticked
 * so the view's key is the same as it was before the bar was opened.
 *
 * `masterBpmX100` is the master player's BPM if a deck is loaded; the ± list
 * is inert without one and the backend treats a ticked BPM column at `All`
 * as no constraint then.
 */
export function toSpecFilter(
  state: FilterState,
  masterBpmX100: number | null,
): TrackFilter | undefined {
  if (!isNarrowing(state)) return undefined;
  const out: TrackFilter = {};
  if (state.bpm.enabled) {
    out.bpm = {
      values: state.bpm.values,
      tolerancePct: state.bpm.tolerancePct,
      masterBpmX100: masterBpmX100 !== null && masterBpmX100 > 0 ? masterBpmX100 : null,
    };
  }
  // An `All` in a set column is no constraint from that column, so it is left
  // off the wire rather than sent as an empty set, which would match nothing.
  if (state.key.enabled && state.key.values.length > 0) out.keys = state.key.values;
  if (state.rating.enabled && state.rating.values.length > 0) out.ratings = state.rating.values;
  if (state.color.enabled && state.color.values.length > 0) out.colors = state.color.values;
  // A tag column at `All` is no constraint either.
  const tags = state.tags.flatMap((tag, category) =>
    tag.enabled && tag.values.length > 0 ? [{ category, any: tag.any, tags: tag.values }] : [],
  );
  if (tags.length > 0) out.tags = tags;
  return Object.keys(out).length > 0 ? out : undefined;
}
