import { useId } from "react";
import { useTranslation } from "@/i18n";
import { usePreferencesContext } from "@/store/usePreferences";
import { Section } from "./controls";
import layout from "./PaneLayout.module.css";
import controls from "./Preferences.module.css";
import styles from "./UsbExportPane.module.css";

export function UsbExportPane() {
  const t = useTranslation();
  const { preferences, update } = usePreferencesContext();
  const id = useId();
  return <Section title="USB Export">
    <p className={`${layout.help} ${styles.intro}`}>USB import and export options.</p>
    <label className={`${layout.summary} ${styles.option}`}>
      <span className={styles.details}>
        <strong id={`${id}-database-folders`}>Setup PIONEER folder on USB drives</strong>
        <span id={`${id}-database-folders-help`} className={styles.description}>Create the PIONEER folders automatically when a new USB drive is synced for the first time.</span>
      </span>
      <input type="checkbox" role="switch" className={controls.toggle}
        aria-labelledby={`${id}-database-folders`} aria-describedby={`${id}-database-folders-help`}
        checked={preferences.djSystem.createDatabaseFolders}
        onChange={event => update("djSystem", { createDatabaseFolders: event.target.checked })} />
    </label>
    <label className={`${layout.summary} ${styles.option}`}>
      <span className={styles.details}>
        <strong id={`${id}-settings`}>Automatically import CDJ/mixer settings when syncing</strong>
        <span id={`${id}-settings-help`} className={styles.description}>Copy valid CDJ and mixer settings from selected USB devices into rbxport when you click SYNC in Sync Manager.</span>
        <span className={styles.default}>Default: Off</span>
      </span>
      <input type="checkbox" role="switch" className={controls.toggle}
        aria-labelledby={`${id}-settings`} aria-describedby={`${id}-settings-help`}
        checked={preferences.usbExport.importSettings}
        onChange={event => update("usbExport", { importSettings: event.target.checked })} />
    </label>
    <label className={`${layout.summary} ${styles.option}`}>
      <span className={styles.details}>
        <strong id={`${id}-history`}>Automatically import play history when syncing</strong>
        <span id={`${id}-history-help`} className={styles.description}>Add new play-history entries from selected USB devices to your library when you click SYNC in Sync Manager.</span>
        <span className={styles.default}>Default: On</span>
      </span>
      <input type="checkbox" role="switch" className={controls.toggle}
        aria-labelledby={`${id}-history`} aria-describedby={`${id}-history-help`}
        checked={preferences.usbExport.importHistory}
        onChange={event => update("usbExport", { importHistory: event.target.checked })} />
    </label>
    <label className={`${layout.summary} ${styles.option}`}>
      <span className={styles.details}>
        <strong id={`${id}-import-cues`}>Import cues and beat grids</strong>
        <span id={`${id}-import-cues-help`} className={styles.description}>Ticked for Import in Sync Manager when the window opens.</span>
        <span className={styles.default}>Default: On</span>
      </span>
      <input type="checkbox" role="switch" className={controls.toggle}
        aria-labelledby={`${id}-import-cues`} aria-describedby={`${id}-import-cues-help`}
        checked={preferences.usbExport.importButtonCues}
        onChange={event => update("usbExport", { importButtonCues: event.target.checked })} />
    </label>
    <label className={`${layout.summary} ${styles.option}`}>
      <span className={styles.details}>
        <strong id={`${id}-import-history`}>Import play history</strong>
        <span id={`${id}-import-history-help`} className={styles.description}>Ticked for Import in Sync Manager when the window opens.</span>
        <span className={styles.default}>Default: On</span>
      </span>
      <input type="checkbox" role="switch" className={controls.toggle}
        aria-labelledby={`${id}-import-history`} aria-describedby={`${id}-import-history-help`}
        checked={preferences.usbExport.importButtonHistory}
        onChange={event => update("usbExport", { importButtonHistory: event.target.checked })} />
    </label>
    <label className={`${layout.summary} ${styles.option}`}>
      <span className={styles.details}>
        <strong id={`${id}-import-settings`}>Import CDJ/mixer settings</strong>
        <span id={`${id}-import-settings-help`} className={styles.description}>Ticked for Import in Sync Manager when the window opens.</span>
        <span className={styles.default}>Default: Off</span>
      </span>
      <input type="checkbox" role="switch" className={controls.toggle}
        aria-labelledby={`${id}-import-settings`} aria-describedby={`${id}-import-settings-help`}
        checked={preferences.usbExport.importButtonSettings}
        onChange={event => update("usbExport", { importButtonSettings: event.target.checked })} />
    </label>
    <label className={`${layout.summary} ${styles.option}`}>
      <span className={styles.details}>
        <strong id={`${id}-cleanup`}>Delete music not in any playlist</strong>
        <span id={`${id}-cleanup-help`} className={styles.description}>Free space on your USB stick by removing songs that aren't in any playlist.</span>
        <span className={styles.default}>Default: Off</span>
      </span>
      <input type="checkbox" role="switch" className={controls.toggle}
        aria-labelledby={`${id}-cleanup`} aria-describedby={`${id}-cleanup-help`}
        checked={preferences.usbExport.deleteUnlistedMusic}
        onChange={event => update("usbExport", { deleteUnlistedMusic: event.target.checked })} />
    </label>
    <div className={`${layout.summary} ${styles.conversion}`}>
      <label className={styles.conversionToggle}>
        <span className={styles.details}>
          <strong id={`${id}-compatibility`}>{t("Maximum CDJ compatibility")}</strong>
          <span id={`${id}-compatibility-help`} className={styles.description}>{t("Save formats like FLAC and M4A as WAV, AIFF or MP3 on your USB stick for older CDJ models. Originals stay untouched.")}</span>
        </span>
        <input type="checkbox" role="switch" className={controls.toggle}
          aria-labelledby={`${id}-compatibility`} aria-describedby={`${id}-compatibility-help`}
          checked={preferences.usbExport.maximumCompatibility}
          onChange={event => update("usbExport", { maximumCompatibility: event.target.checked })} />
      </label>
      <div className={styles.format}>
        <label htmlFor={`${id}-format`}>{t("Convert to")}</label>
        <select id={`${id}-format`} disabled={!preferences.usbExport.maximumCompatibility}
          value={preferences.usbExport.conversionFormat}
          onChange={event => update("usbExport", { conversionFormat: event.target.value === "mp3" || event.target.value === "aiff" ? event.target.value : "wav" })}>
          <option value="wav">{t("WAV — larger files")}</option>
          <option value="aiff">{t("AIFF — larger files")}</option>
          <option value="mp3">{t("MP3 — 320 kbps")}</option>
        </select>
      </div>
      <p className={styles.description}>{t("WAV/AIFF: 16-bit / 44.1 kHz. MP3: smaller, lossy files.")}</p>
    </div>
  </Section>;
}
