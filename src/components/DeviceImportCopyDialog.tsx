import { useEffect, useRef } from "react";

import { useTranslation } from "@/i18n";
import type { DevicePlaylistImportPlan } from "@/ipc/types";
import { formatBytes } from "@/lib/format";
import styles from "./ConfirmDialog.module.css";

/**
 * rekordbox's question before Import Playlist copies a device playlist's
 * missing tracks to the computer [static, rekordbox 7.2.19
 * `importFromDeviceCopySettingWindow` @0x1001d6af0: title "Import Playlist
 * from Device", the message, "Music files to be copied:" with "xxxx files
 * yyyy to the folder : " (count, then `descriptionOfSizeInBytes`), "Music
 * file copy destination:", "Free space of xxxx is yyyy", OK and Cancel].
 * As rekordbox 7.2.14 draws it [OBS chris-win11, #198]: "2 files 256 bytes
 * to the folder :", the size without brackets, the free space beside
 * "Music file copy destination:", the folder once, below both, OK left of
 * Cancel. Change… picks another folder for the copies
 * (`buttonClicked` @0x1001d802c: `FileChooser::browseForDirectory`, then
 * the free space again), through `onChange`, which gives a new plan. When
 * the files do not fit, the free space is red (0xFFC00000) and OK is
 * disabled, on opening and after Change… (the window's constructor
 * @0x1001d7544 and `buttonClicked` @0x1001d82cc compare the size with the
 * free space). OK copies and imports; Cancel and Escape import nothing.
 */
export function DeviceImportCopyDialog({ plan, onAnswer, onChange }: {
  plan: DevicePlaylistImportPlan;
  onAnswer: (copy: boolean) => void;
  onChange?: () => void;
}) {
  const t = useTranslation();
  const ok = useRef<HTMLButtonElement>(null);

  useEffect(() => ok.current?.focus(), []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onAnswer(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onAnswer]);

  const title = t("Import Playlist from Device");
  const tooBig = plan.freeBytes !== null && plan.freeBytes < plan.bytes;
  const size = formatBytes(plan.bytes);
  const files = plan.files === 1
    ? t("{count} file {size} to the folder : ", { count: plan.files, size })
    : t("{count} files {size} to the folder : ", { count: plan.files, size });
  return (
    <div className={styles.backdrop} role="presentation">
      <div className={styles.window} role="alertdialog" aria-modal="true" aria-label={title}
        aria-describedby="device-import-copy-message">
        <div className={styles.title}>{title}</div>
        <div id="device-import-copy-message" className={styles.message}>
          <p>{t("The playlist you are trying to import contains music files that are not present in rekordbox on your computer.")}</p>
          <p>{t("These music files will be copied to this computer.")}</p>
          <p><strong>{t("Music files to be copied:")}</strong><br />{files}</p>
          <p><strong>{t("Music file copy destination:")}</strong>
            {plan.freeBytes !== null
              ? <><br /><span className={tooBig ? styles.short : undefined} data-short={tooBig || undefined}>
                {t("Free space of {drive} is {size}", { drive: plan.destination, size: formatBytes(plan.freeBytes) })}
              </span></>
              : null}
          </p>
          <p>{plan.destination}</p>
          {onChange
            ? <p><button type="button" className={styles.button} onClick={onChange}>{t("Change…")}</button></p>
            : null}
        </div>
        <div className={styles.buttons}>
          <button ref={ok} type="button" className={`${styles.button} ${styles.primary}`} disabled={tooBig}
            onClick={() => onAnswer(true)}>{t("OK")}</button>
          <button type="button" className={styles.button} onClick={() => onAnswer(false)}>{t("Cancel")}</button>
        </div>
      </div>
    </div>
  );
}
