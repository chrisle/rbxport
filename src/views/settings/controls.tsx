/**
 * The controls every pane of the Preferences window is built from: a
 * section with a title, a sub-heading, a toggle, a radio group, a checkbox,
 * a dropdown, a slider, a button. Each is drawn to the capture's geometry
 * through the tokens, so a pane says what it holds and nothing about how.
 */
import { useId, type ReactNode } from "react";

import styles from "./Preferences.module.css";

export function Section({ title, children, label, id }: {
  title: string;
  /** The accessible name when the title is not the whole story. */
  label?: string;
  /** Lets Preferences open its pane scrolled to this section. */
  id?: string;
  children?: ReactNode;
}) {
  return (
    <section id={id} className={styles.section} aria-label={label ?? title}>
      <h3 className={styles.title}>{title}</h3>
      {children}
    </section>
  );
}

export function Sub({ children, dim }: { children: ReactNode; dim?: boolean }) {
  return <h4 className={styles.sub} data-dim={dim || undefined}>{children}</h4>;
}

export function Separator() {
  return <div className={styles.separator} role="separator" />;
}

export function Note({ children, failed }: { children: ReactNode; failed?: boolean }) {
  return <p className={styles.note} data-failed={failed || undefined}>{children}</p>;
}

export function Toggle({ label, checked, onChange, nested, disabled }: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  nested?: boolean;
  disabled?: boolean;
}) {
  return (
    <label className={styles.row} data-nested={nested || undefined}>
      <input
        type="checkbox"
        role="switch"
        className={styles.toggle}
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span>{label}</span>
    </label>
  );
}

export function Checkbox({ label, checked, onChange, nested, disabled }: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  nested?: boolean;
  disabled?: boolean;
}) {
  return (
    <label className={styles.row} data-nested={nested || undefined}>
      <input
        type="checkbox"
        className={styles.checkbox}
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span>{label}</span>
    </label>
  );
}

export interface Choice<T extends string> {
  value: T;
  label: string;
}

/** Radios, one per choice, sharing a name so the arrows move between them. */
export function Radios<T extends string>({ label, value, choices, onChange, nested, dim }: {
  /** The group's accessible name. */
  label: string;
  value: T;
  choices: readonly Choice<T>[];
  onChange: (value: T) => void;
  nested?: boolean;
  /** Drawn greyed: shown for completeness, not usable. */
  dim?: boolean;
}) {
  const name = useId();
  return (
    <div role="radiogroup" aria-label={label}>
      {choices.map((choice) => (
        <label
          key={choice.value}
          className={styles.row}
          data-nested={nested || undefined}
          data-dim={dim || undefined}
        >
          <input
            type="radio"
            className={styles.radio}
            name={name}
            value={choice.value}
            checked={value === choice.value}
            disabled={dim}
            onChange={() => onChange(choice.value)}
          />
          <span>{choice.label}</span>
        </label>
      ))}
    </div>
  );
}

export function Select<T extends string>({
  label, value, choices, onChange, nested, disabled, plain, caption, preserveChoiceLabels,
}: {
  label: string;
  value: T;
  choices: readonly Choice<T>[];
  onChange: (value: T) => void;
  nested?: boolean;
  disabled?: boolean;
  /** Regular weight: the Analysis pane's dropdowns are not bold. */
  plain?: boolean;
  /** A caption to the left, as "Analysis Mode" sits beside its dropdown. */
  caption?: string;
  /** Keep choices in their own language, as in the language picker. */
  preserveChoiceLabels?: boolean;
}) {
  return (
    <div className={styles.selectRow} data-nested={nested || undefined}>
      {caption ? <span className={styles.selectLabel}>{caption}</span> : null}
      <select
        className={styles.select}
        data-plain={plain || undefined}
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as T)}
      >
        {choices.map((choice) => (
          <option key={choice.value} value={choice.value} data-i18n-ignore={preserveChoiceLabels || undefined}>
            {choice.label}
          </option>
        ))}
      </select>
    </div>
  );
}

/** A slider with `steps` stops and Small / Large under its ends. */
/**
 * A slider. By default `steps` positions from 0, with rekordbox's Small and
 * Large under the ends; a `range` makes it a value in units instead, and
 * `ends` say what its ends mean.
 */
export function Slider({ label, value, steps, range, ends, onChange, disabled }: {
  label: string;
  value: number;
  steps?: number;
  range?: { min: number; max: number; step: number };
  ends?: [string, string];
  onChange: (value: number) => void;
  disabled?: boolean;
}) {
  const [min, max, step] = range ? [range.min, range.max, range.step] : [0, (steps ?? 2) - 1, 1];
  const [low, high] = ends ?? ["Small", "Large"];
  return (
    <div className={styles.slider} data-disabled={disabled || undefined}>
      <input
        type="range"
        aria-label={label}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <div className={styles.sliderEnds} aria-hidden>
        <span>{low}</span>
        <span>{high}</span>
      </div>
    </div>
  );
}

export function Button({ children, onClick, disabled, className }: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  className?: string | undefined;
}) {
  return (
    <button
      type="button"
      className={className ? `${styles.button} ${className}` : styles.button}
      onClick={onClick}
      disabled={disabled}
    >
      {children}
    </button>
  );
}
