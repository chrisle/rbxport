import { useEffect, useRef } from "react";

import { useTranslation } from "@/i18n";
import type { MissingExportFile } from "@/ipc/types";
import styles from "./MissingFilesDialog.module.css";

/**
 * Names the selected tracks whose audio is not on disk before a sync starts,
 * and asks whether to sync the rest without them.
 */
export function MissingFilesDialog({ files, onChoose }: {
  files: readonly MissingExportFile[];
  /** True to sync without the missing tracks, false to stop. */
  onChoose: (proceed: boolean) => void;
}) {
  const t = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="missing-files-title"
      aria-describedby="missing-files-summary"
      onCancel={event => { event.preventDefault(); onChoose(false); }}
      onKeyDown={event => event.stopPropagation()}>
      <h2 id="missing-files-title" className={styles.title}>{t("Missing audio files")}</h2>
      <p id="missing-files-summary" className={styles.summary}>
        {files.length === 1
          ? t("1 selected track has a missing audio file.")
          : t("{count} selected tracks have missing audio files.", { count: files.length })}
        {" "}{t("Usable copies already on each USB will be kept. Tracks without a usable copy will be skipped.")}
      </p>
      <ul className={styles.list} aria-label={t("Missing audio files")}>
        {files.map((file, index) => <li key={`${index}:${file.path}`}>
          <strong>{file.title}</strong>
          <span className={styles.path}>{file.path}</span>
        </li>)}
      </ul>
      <div className={styles.buttons}>
        <button type="button" autoFocus onClick={() => onChoose(false)}>{t("Cancel")}</button>
        <button type="button" className={styles.primary} onClick={() => onChoose(true)}>{t("Keep available copies and sync")}</button>
      </div>
    </dialog>
  );
}
