import { useEffect, useRef, useState } from "react";
import type { AnalysisSettings } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import { BPM_RANGES, bpmRangeLimits, type AnalysisMode, type BpmRange } from "@/lib/preferences";
import { lastAnalysisChoice, rememberAnalysisChoice } from "@/lib/analysisChoice";
import styles from "./AnalysisDialog.module.css";

export type AnalysisChoice = AnalysisSettings & { mode: AnalysisMode };

/**
 * rekordbox's Analysis Setting window.
 *
 * Opened two ways, as in rekordbox. From Analyze Track it names how many
 * tracks are selected and warns that their results are overwritten. At launch
 * with Auto Analysis on it says "Auto Analysis is starting." instead, with
 * BPM / Grid ticked and greyed, since every track it covers has never been
 * analysed [OBS: rekordbox 7.2.14, chris-win11].
 */
export function AnalysisDialog({ count, auto = false, initialMode, initialBpmRange, initialFirstBeatCue, onConfirm, onCancel }: {
  count: number;
  /** The launch prompt for never-analysed Collection tracks. */
  auto?: boolean;
  initialMode: AnalysisMode;
  /** The Preferences BPM Range; changing it here applies to this batch only. */
  initialBpmRange: BpmRange;
  /** The Preferences switch, which this window has no box for. */
  initialFirstBeatCue: boolean;
  onConfirm: (settings: AnalysisChoice) => void;
  onCancel: () => void;
}) {
  // The launch prompt always starts from its defaults; Analyze Track starts
  // from what it was last confirmed with. The first-beat cue is always the
  // Preferences switch as it stands now (#343).
  const [settings, setSettings] = useState<AnalysisChoice>(() => ({
    mode: initialMode, bpmGrid: true, key: true, highPrecision: true,
    ...(auto ? {} : lastAnalysisChoice()),
    ...bpmRangeLimits(initialBpmRange),
    firstBeatCue: initialFirstBeatCue,
  }));
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    const previous = document.activeElement;
    element?.showModal();
    return () => {
      element?.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);
  const t = useTranslation();
  const update = (patch: Partial<AnalysisChoice>) => setSettings(current => ({ ...current, ...patch }));
  const canAnalyse = settings.bpmGrid || settings.key;

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="analysis-title"
      onCancel={event => { event.preventDefault(); onCancel(); }}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => { event.preventDefault(); if (!canAnalyse) return; if (!auto) rememberAnalysisChoice(settings); onConfirm(settings); }}>
        <h2 id="analysis-title" className={styles.title}>{t("Analysis Setting")}</h2>
        {auto ? null : (
          <p className={styles.selection}>{count} {count === 1 ? t("track selected") : t("tracks selected")}</p>
        )}
        <div className={styles.options}>
          <label className={styles.check}>
            <input type="checkbox" checked={settings.bpmGrid} disabled={auto}
              onChange={event => update({ bpmGrid: event.target.checked })} />
            {t("BPM / Grid")}
          </label>
          <fieldset className={styles.gridOptions} disabled={!settings.bpmGrid}>
            <label className={styles.check}>
              <input type="checkbox" checked={settings.highPrecision} onChange={event => update({ highPrecision: event.target.checked })} />
              {t("High precision analysis")}
            </label>
            <label className={styles.field}>
              <span>{t("Analysis Mode")}</span>
              <select value={settings.mode} onChange={event => update({ mode: event.target.value as AnalysisMode })}>
                <option value="rekordbox">{t("Normal")}</option>
                <option value="rbxport">{t("RBXport (Electronic Music)")}</option>
              </select>
            </label>
            <p className={styles.modeBlurb}>
              {settings.mode === "rbxport"
                ? t("Allows tempo changes and builds a variable beat grid. Intended for electronic music.")
                : t("Analyses the track as one steady BPM with a single, constant beat grid.")}
            </p>
            <label className={styles.field}>
              <span>{t("BPM Range")}</span>
              <select value={`${settings.minBpm}-${settings.maxBpm}`}
                onChange={event => update(bpmRangeLimits(event.target.value as BpmRange))}>
                {BPM_RANGES.map(range => <option key={range} value={range}>{range}</option>)}
              </select>
            </label>
          </fieldset>
          <label className={styles.check}>
            <input type="checkbox" checked={settings.key} onChange={event => update({ key: event.target.checked })} />
            {t("KEY")}
          </label>
        </div>
        {auto ? (
          <p className={styles.note}>{t("Auto Analysis is starting.")}</p>
        ) : (
          <p className={styles.note}>
            {t("Selected analysis results will be overwritten.")}<br />{t("Locked tracks will not be analyzed.")}
          </p>
        )}
        <div className={styles.buttons}>
          <button type="submit" disabled={!canAnalyse}>{t("OK")}</button>
          <button type="button" onClick={onCancel}>{t("Cancel")}</button>
        </div>
      </form>
    </dialog>
  );
}
