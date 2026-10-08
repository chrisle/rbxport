import { useEffect, useRef, useState } from "react";
import type { AnalysisSettings } from "@/ipc/types";
import type { AnalysisMode } from "@/lib/preferences";
import styles from "./AnalysisDialog.module.css";

export type AnalysisChoice = AnalysisSettings & { mode: AnalysisMode };

export function AnalysisDialog({ count, initialMode, initialFirstBeatCue, onConfirm, onCancel }: {
  count: number;
  initialMode: AnalysisMode;
  /** The Preferences default; changing the box here applies to this batch only. */
  initialFirstBeatCue: boolean;
  onConfirm: (settings: AnalysisChoice) => void;
  onCancel: () => void;
}) {
  const [settings, setSettings] = useState<AnalysisChoice>(() => ({
    mode: initialMode, bpmGrid: true, key: true, highPrecision: true, minBpm: 70, maxBpm: 180,
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
  const update = (patch: Partial<AnalysisChoice>) => setSettings(current => ({ ...current, ...patch }));
  const canAnalyse = settings.bpmGrid || settings.key;

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="analysis-title"
      onCancel={event => { event.preventDefault(); onCancel(); }}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => { event.preventDefault(); if (canAnalyse) onConfirm(settings); }}>
        <h2 id="analysis-title" className={styles.title}>Analysis Setting</h2>
        <p className={styles.selection}>{count} {count === 1 ? "track selected" : "tracks selected"}</p>
        <div className={styles.options}>
          <label className={styles.check}>
            <input type="checkbox" checked={settings.bpmGrid} onChange={event => update({ bpmGrid: event.target.checked })} />
            BPM / Grid
          </label>
          <fieldset className={styles.gridOptions} disabled={!settings.bpmGrid}>
            <label className={styles.check}>
              <input type="checkbox" checked={settings.highPrecision} onChange={event => update({ highPrecision: event.target.checked })} />
              High precision analysis
            </label>
            <label className={styles.field}>
              <span>Analysis Mode</span>
              <select value={settings.mode} onChange={event => update({ mode: event.target.value as AnalysisMode })}>
                <option value="rekordbox">Normal</option>
                <option value="rbxport">RBXport (Electronic Music)</option>
              </select>
            </label>
            <label className={styles.field}>
              <span>BPM Range</span>
              <select value={`${settings.minBpm}-${settings.maxBpm}`} onChange={event => {
                const [minBpm = 70, maxBpm = 180] = event.target.value.split("-").map(Number);
                update({ minBpm, maxBpm });
              }}>
                {[[70, 180], [98, 195], [118, 236], [58, 115]].map(([min, max]) => (
                  <option key={min} value={`${min}-${max}`}>{min}–{max}</option>
                ))}
              </select>
            </label>
            <label className={styles.check}>
              <input type="checkbox" checked={settings.firstBeatCue} onChange={event => update({ firstBeatCue: event.target.checked })} />
              Add memory cue at first beat
            </label>
          </fieldset>
          <label className={styles.check}>
            <input type="checkbox" checked={settings.key} onChange={event => update({ key: event.target.checked })} />
            KEY
          </label>
        </div>
        <p className={styles.note}>Selected analysis results will be overwritten.<br />Locked tracks will not be analyzed.</p>
        <div className={styles.buttons}>
          <button type="submit" disabled={!canAnalyse}>OK</button>
          <button type="button" onClick={onCancel}>Cancel</button>
        </div>
      </form>
    </dialog>
  );
}
