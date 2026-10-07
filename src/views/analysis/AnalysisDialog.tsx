import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import type { AnalysisSettings } from "@/ipc/types";
import type { AnalysisMode } from "@/lib/preferences";
import styles from "./AnalysisDialog.module.css";

export type AnalysisChoice = AnalysisSettings & { mode: AnalysisMode };

export function AnalysisDialog({ count, initialMode, onConfirm, onCancel }: {
  count: number;
  initialMode: AnalysisMode;
  onConfirm: (settings: AnalysisChoice) => void;
  onCancel: () => void;
}) {
  const t = useTranslation();
  const [settings, setSettings] = useState<AnalysisChoice>(() => ({
    mode: initialMode, waveform: true, bpmGrid: true, key: true, highPrecision: true, minBpm: 70, maxBpm: 180,
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
  const canAnalyse = settings.waveform || settings.bpmGrid || settings.key;

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="analysis-title"
      onCancel={event => { event.preventDefault(); onCancel(); }}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => { event.preventDefault(); if (canAnalyse) onConfirm(settings); }}>
        <h2 id="analysis-title" className={styles.title}>{t("Analysis Setting")}</h2>
        <p className={styles.selection}>{count} {t(count === 1 ? "track selected" : "tracks selected")}</p>
        <div className={styles.options}>
          <label className={styles.check}>
            <input type="checkbox" checked={settings.waveform} onChange={event => update({ waveform: event.target.checked })} />
            {t("Waveform")}
          </label>
          <label className={styles.check}>
            <input type="checkbox" checked={settings.bpmGrid} onChange={event => update({ bpmGrid: event.target.checked })} />
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
            <label className={styles.field}>
              <span>{t("BPM Range")}</span>
              <select value={`${settings.minBpm}-${settings.maxBpm}`} onChange={event => {
                const [minBpm = 70, maxBpm = 180] = event.target.value.split("-").map(Number);
                update({ minBpm, maxBpm });
              }}>
                {[[70, 180], [98, 195], [118, 236], [58, 115]].map(([min, max]) => (
                  <option key={min} value={`${min}-${max}`}>{min}–{max}</option>
                ))}
              </select>
            </label>
          </fieldset>
          <label className={styles.check}>
            <input type="checkbox" checked={settings.key} onChange={event => update({ key: event.target.checked })} />
            {t("KEY")}
          </label>
        </div>
        <p className={styles.note}>{t("Selected analysis results will be overwritten.")}<br />{t("Locked tracks will not be analyzed.")}</p>
        <div className={styles.buttons}>
          <button type="submit" disabled={!canAnalyse}>{t("OK")}</button>
          <button type="button" onClick={onCancel}>{t("Cancel")}</button>
        </div>
      </form>
    </dialog>
  );
}
