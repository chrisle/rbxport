import { useId } from "react";
import { useTranslation } from "@/i18n";
import { usePreferencesContext } from "@/store/usePreferences";
import { Section } from "./controls";
import layout from "./PaneLayout.module.css";
import controls from "./Preferences.module.css";
import styles from "./UsbExportPane.module.css";

/** The ids of the pane's sections, which Preferences can open it scrolled to. */
export const USB_EXPORT_SECTIONS = { sync: "usb-export-sync", import: "usb-export-import" } as const;

export function UsbExportPane() {
  const t = useTranslation();
  const { preferences, update } = usePreferencesContext();
  const id = useId();
  return <>
    <Section title={t("Sync")} id={USB_EXPORT_SECTIONS.sync}>
      <label className={`${layout.summary} ${styles.option}`}>
        <span className={styles.details}>
          <strong id={`${id}-database-folders`}>Setup PIONEER folder on USB drives</strong>
          <span id={`${id}-database-folders-help`} className={styles.description}>Create the PIONEER folders automatically when a new USB drive is synced for the first time.</span>
          <span className={styles.default}>{t("Default: On")}</span>
        </span>
        <input type="checkbox" role="switch" className={controls.toggle}
          aria-labelledby={`${id}-database-folders`} aria-describedby={`${id}-database-folders-help`}
          checked={preferences.djSystem.createDatabaseFolders}
          onChange={event => update("djSystem", { createDatabaseFolders: event.target.checked })} />
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
            <span id={`${id}-compatibility-help`} className={styles.description}>{t("Save formats like FLAC and M4A as WAV, AIFF or MP3 on your USB stick for older CDJ models. Also converts low-sample-rate MP3s (16, 22.05 or 24 kHz) that some players play too fast. Originals stay untouched.")}</span>
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
      <label className={`${layout.summary} ${styles.option}`}>
        <span className={styles.details}>
          <strong id={`${id}-verify-all`}>{t("Verify all files (slower)")}</strong>
          <span id={`${id}-verify-all-help`} className={styles.description}>{t("After each sync, compare every file on the USB device with its original instead of checking only what the sync changed.")}</span>
          <span className={styles.default}>{t("Default: Off")}</span>
        </span>
        <input type="checkbox" role="switch" className={controls.toggle}
          aria-labelledby={`${id}-verify-all`} aria-describedby={`${id}-verify-all-help`}
          checked={preferences.usbExport.verifyAllFiles}
          onChange={event => update("usbExport", { verifyAllFiles: event.target.checked })} />
      </label>
    </Section>
    <Section title={t("Import")} id={USB_EXPORT_SECTIONS.import}>
      {/* What Import brings back. SYNC never imports: device changes reach
          the library only when Import is clicked, as rekordbox's reach its
          collection only through Update Collection. */}
      <p className={layout.help}>{t("Import in Sync Manager brings back what is turned on here. SYNC only writes to USB devices and never imports.")}</p>
      <label className={`${layout.summary} ${styles.option}`}>
        <span className={styles.details}>
          <strong id={`${id}-import-cues`}>{t("Import cues and beat grids")}</strong>
          <span id={`${id}-import-cues-help`} className={styles.description}>{t("Replace cues and beat grids for matching tracks in your library with the USB device's.")}</span>
          <span className={styles.default}>{t("Default: On")}</span>
        </span>
        <input type="checkbox" role="switch" className={controls.toggle}
          aria-labelledby={`${id}-import-cues`} aria-describedby={`${id}-import-cues-help`}
          checked={preferences.usbExport.importButtonCues}
          onChange={event => update("usbExport", { importButtonCues: event.target.checked })} />
      </label>
      <label className={`${layout.summary} ${styles.option}`}>
        <span className={styles.details}>
          <strong id={`${id}-import-history`}>{t("Import play history")}</strong>
          <span id={`${id}-import-history-help`} className={styles.description}>{t("Add new play-history entries from the USB device to your library.")}</span>
          <span className={styles.default}>{t("Default: On")}</span>
        </span>
        <input type="checkbox" role="switch" className={controls.toggle}
          aria-labelledby={`${id}-import-history`} aria-describedby={`${id}-import-history-help`}
          checked={preferences.usbExport.importButtonHistory}
          onChange={event => update("usbExport", { importButtonHistory: event.target.checked })} />
      </label>
      <label className={`${layout.summary} ${styles.option}`}>
        <span className={styles.details}>
          <strong id={`${id}-import-settings`}>{t("Import CDJ/mixer settings")}</strong>
          <span id={`${id}-import-settings-help`} className={styles.description}>{t("Keep the USB device's CDJ and mixer settings in rbxport. Sync gives them to USB devices that have no settings of their own.")}</span>
          <span className={styles.default}>{t("Default: Off")}</span>
        </span>
        <input type="checkbox" role="switch" className={controls.toggle}
          aria-labelledby={`${id}-import-settings`} aria-describedby={`${id}-import-settings-help`}
          checked={preferences.usbExport.importButtonSettings}
          onChange={event => update("usbExport", { importButtonSettings: event.target.checked })} />
      </label>
      <label className={`${layout.summary} ${styles.option}`}>
        <span className={styles.details}>
          <strong id={`${id}-import-ratings`}>{t("Import track ratings")}</strong>
          <span id={`${id}-import-ratings-help`} className={styles.description}>{t("Bring back ratings changed on the USB device since its last sync. Device ratings win when a track was also rated in your library.")}</span>
          <span className={styles.default}>{t("Default: On")}</span>
        </span>
        <input type="checkbox" role="switch" className={controls.toggle}
          aria-labelledby={`${id}-import-ratings`} aria-describedby={`${id}-import-ratings-help`}
          checked={preferences.usbExport.importButtonRatings}
          onChange={event => update("usbExport", { importButtonRatings: event.target.checked })} />
      </label>
    </Section>
  </>;
}
