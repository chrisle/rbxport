/**
 * The Track Filter: the bar that drops down between the browser header and
 * the column header when the header's filter button is on.
 *
 * Eight columns, as the capture has them — BPM with its MASTER PLAYER ± list,
 * KEY, RATING, COLOR and one column per My Tag category — each with a tick
 * box that turns it on, and RST at the far right. The picks live in
 * `FilterState`; what they do to the list is Rust's (`ViewSpec.filter`), and
 * the values on offer arrive from `filterValues`. Nothing here reads a row.
 *
 * A tag column's chips are picked like any other list, and its AND / OR switch
 * says whether a track must carry all the picked tags or any of them.
 */
import { memo, useMemo, type MouseEvent } from "react";
import type { FilterValues } from "@/ipc/types";
import {
  COLOR_NAMES, RATINGS, TAG_COLUMNS, TOLERANCES, EMPTY_FILTER, pick,
  type FilterState, type PickedColumn, type TagColumn,
} from "@/lib/trackFilter";
import { TickIcon } from "@/components/icons";
import { RatingStar } from "@/components/RatingStar";
import styles from "./TrackFilter.module.css";

export interface TrackFilterProps {
  state: FilterState;
  onChange: (next: FilterState) => void;
  /** What the lists offer, or `null` until the backend has answered. */
  values: FilterValues | null;
  /** The master player's BPM x100, or `null` when no deck is loaded. */
  masterBpmX100: number | null;
}

/** Cmd on a Mac, Ctrl elsewhere: the pick-several modifier. */
function toggling(event: MouseEvent): boolean {
  return event.metaKey || event.ctrlKey;
}

const Check = memo(function Check({
  checked, label, onChange,
}: {
  checked: boolean;
  label: string;
  onChange: () => void;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={label}
      className={styles.check}
      data-checked={checked || undefined}
      onClick={onChange}
    >
      {checked ? <TickIcon className={styles.tick} /> : null}
    </button>
  );
});

/**
 * One list of picks. `All` is the row with no value; the others come from
 * the backend's tally. A plain click picks one, Cmd/Ctrl-click picks several.
 */
function PickList<T extends string | number>({
  column, options, onPick, all, render, className, inert = false,
}: {
  column: PickedColumn<T>;
  options: readonly T[];
  onPick: (values: T[]) => void;
  /** Whether to lead with an `All` row. */
  all: boolean;
  render?: (value: T) => React.ReactNode;
  /** CSS Modules hands back `undefined` for a class that is not there. */
  className?: string | undefined;
  /** Drawn greyed and ignoring clicks. */
  inert?: boolean;
}) {
  const nothingPicked = column.values.length === 0;
  return (
    <div
      className={className ? `${styles.list} ${className}` : styles.list}
      role="listbox"
      aria-multiselectable
      aria-disabled={inert || undefined}
      data-inert={inert || undefined}
    >
      {all ? (
        <div
          className={styles.row}
          role="option"
          aria-selected={nothingPicked}
          data-picked={nothingPicked || undefined}
          onClick={() => {
            if (!inert) onPick([]);
          }}
        >
          All
        </div>
      ) : null}
      {options.map((value) => {
        const picked = column.values.includes(value);
        return (
          <div
            key={String(value)}
            className={styles.row}
            role="option"
            aria-selected={picked}
            data-picked={picked || undefined}
            onClick={(e) => {
              if (!inert) onPick(pick(column.values, value, toggling(e)));
            }}
          >
            {render ? render(value) : String(value)}
          </div>
        );
      })}
    </div>
  );
}

const Stars = memo(function Stars({ stars }: { stars: number }) {
  return (
    <span className={styles.stars} role="img" aria-label={`${stars} of 5`}>
      {[1, 2, 3, 4, 5].map((star) => <RatingStar key={star} lit={star <= stars} className={styles.starIcon} />)}
    </span>
  );
});

export function TrackFilter({ state, onChange, values, masterBpmX100 }: TrackFilterProps) {
  // Value lists, not rows: at most a few hundred entries, mapped once per
  // answer rather than per render.
  const bpms = useMemo(() => values?.bpms.map((c) => c.value) ?? [], [values]);
  const keys = useMemo(() => values?.keys.map((c) => c.value) ?? [], [values]);
  const categories = values?.tags ?? [];
  // A tolerance needs something to centre on: a picked BPM, or the master
  // player. With neither the list is greyed, as the capture's is when no deck
  // is loaded.
  const noCentre = state.bpm.values.length === 0 && (masterBpmX100 === null || masterBpmX100 <= 0);

  const setTag = (at: number, patch: Partial<TagColumn>) => {
    const tags = state.tags.map((tag, i) => (i === at ? { ...tag, ...patch } : tag));
    onChange({ ...state, tags });
  };

  return (
    <div className={styles.bar} data-testid="track-filter" role="region" aria-label="Track Filter">
      <div className={styles.columns}>
        {/* BPM and its ± list share one heading: the MASTER PLAYER strip
            runs from the BPM label to the ± list's right edge. */}
        <div className={styles.bpmGroup}>
          <div className={styles.head}>
            <Check
              checked={state.bpm.enabled}
              label="BPM"
              onChange={() => onChange({ ...state, bpm: { ...state.bpm, enabled: !state.bpm.enabled } })}
            />
            <span className={styles.label}>BPM</span>
            <div className={styles.strip} aria-hidden>
              <span className={styles.arrowLeft} />
              <span className={styles.stripLabel}>
                MASTER
                <br />
                PLAYER
              </span>
              <span className={styles.arrowRight} />
            </div>
          </div>
          <div className={styles.bpmLists}>
            <PickList
              column={state.bpm}
              options={bpms}
              all
              className={styles.bpmList}
              onPick={(picked) => onChange({ ...state, bpm: { ...state.bpm, values: picked } })}
            />
            <PickList
              column={{ enabled: state.bpm.enabled, values: [state.bpm.tolerancePct] }}
              options={TOLERANCES}
              all={false}
              inert={noCentre}
              className={styles.pctList}
              render={(pct) => `± ${pct}%`}
              onPick={(picked) =>
                onChange({ ...state, bpm: { ...state.bpm, tolerancePct: picked[0] ?? 0 } })
              }
            />
          </div>
        </div>

        <div className={styles.keyCol}>
          <div className={styles.head}>
            <Check
              checked={state.key.enabled}
              label="Key"
              onChange={() => onChange({ ...state, key: { ...state.key, enabled: !state.key.enabled } })}
            />
            <span className={styles.label}>KEY</span>
          </div>
          <PickList
            column={state.key}
            options={keys}
            all
            onPick={(picked) => onChange({ ...state, key: { ...state.key, values: picked } })}
          />
        </div>

        <div className={styles.ratingCol}>
          <div className={styles.head}>
            <Check
              checked={state.rating.enabled}
              label="Rating"
              onChange={() =>
                onChange({ ...state, rating: { ...state.rating, enabled: !state.rating.enabled } })
              }
            />
            <span className={styles.label}>RATING</span>
          </div>
          <PickList
            column={state.rating}
            options={RATINGS}
            all={false}
            className={styles.ratingList}
            render={(stars) => <Stars stars={stars} />}
            onPick={(picked) => onChange({ ...state, rating: { ...state.rating, values: picked } })}
          />
        </div>

        <div className={styles.colorCol}>
          <div className={styles.head}>
            <Check
              checked={state.color.enabled}
              label="Color"
              onChange={() =>
                onChange({ ...state, color: { ...state.color, enabled: !state.color.enabled } })
              }
            />
            <span className={styles.label}>COLOR</span>
          </div>
          <PickList
            column={state.color}
            options={COLOR_NAMES}
            all={false}
            className={styles.colorList}
            render={(name) => (
              <>
                <span className={styles.dot} data-color={name} />
                {name}
              </>
            )}
            onPick={(picked) => onChange({ ...state, color: { ...state.color, values: picked } })}
          />
        </div>

        {Array.from({ length: Math.max(TAG_COLUMNS, categories.length) }, (_, at) => {
          const category = categories[at];
          const tag = state.tags[at] ?? { enabled: false, values: [], any: false };
          return (
            <div className={styles.tagCol} key={at} data-testid="filter-tag-column">
              <div className={styles.head}>
                <Check
                  checked={tag.enabled}
                  label={category?.name ?? "My Tag"}
                  onChange={() => setTag(at, { enabled: !tag.enabled })}
                />
                <span className={styles.label}>{category?.name ?? ""}</span>
              </div>
              <div className={styles.andOr}>
                <button
                  type="button"
                  className={styles.and}
                  data-on={!tag.any || undefined}
                  onClick={() => setTag(at, { any: false })}
                >
                  AND
                </button>
                <button
                  type="button"
                  className={styles.or}
                  data-on={tag.any || undefined}
                  onClick={() => setTag(at, { any: true })}
                >
                  OR
                </button>
              </div>
              <div
                className={`${styles.list} ${styles.tagList}`}
                role="listbox"
                aria-multiselectable
                aria-disabled={category === undefined || undefined}
                data-inert={category === undefined || undefined}
              >
                {category?.tags.map((name) => {
                  const picked = tag.values.includes(name);
                  return (
                    <div
                      key={name}
                      className={styles.chip}
                      role="option"
                      aria-selected={picked}
                      data-picked={picked || undefined}
                      onClick={(e) => setTag(at, { values: pick(tag.values, name, toggling(e)) })}
                    >
                      {name}
                    </div>
                  );
                })}
              </div>
            </div>
          );
        })}
      </div>
      <button
        type="button"
        className={styles.rst}
        onClick={() => onChange(EMPTY_FILTER)}
        aria-label="Reset"
      >
        RST
      </button>
      <div className={styles.foot} aria-hidden />
    </div>
  );
}
