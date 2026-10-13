import { useCallback, useEffect, useRef, useState } from "react";

import { useTranslation } from "@/i18n";
import type { FolderClashAnswer, FolderClashChoice } from "@/lib/folderDrop";
import styles from "./ConfirmDialog.module.css";

/**
 * Preferences › Playlists' Ask me for a dropped folder whose name a list
 * beside it already has (#355): import it as a copy, replace the old list,
 * or keep the old list and skip the folder. The box applies the answer to
 * the rest of the drop. Escape keeps the old list, as Cancel does on
 * rekordbox's own question.
 */
export function FolderClashDialog({ name, onAnswer }: {
  name: string;
  onAnswer: (answer: FolderClashAnswer) => void;
}) {
  const t = useTranslation();
  const [all, setAll] = useState(false);
  const copy = useRef<HTMLButtonElement>(null);

  // Import as a copy is focused: it is the one answer that changes no list.
  useEffect(() => copy.current?.focus(), []);

  const answer = useCallback((choice: FolderClashChoice) => onAnswer({ choice, all }), [onAnswer, all]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onAnswer({ choice: "keep", all: false });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onAnswer]);

  const title = t("One or several lists with the same name already exist.");
  return (
    <div className={styles.backdrop} role="presentation">
      <div className={styles.window} role="alertdialog" aria-modal="true" aria-label={title}
        aria-describedby="folder-clash-message">
        <div className={styles.title}>{title}</div>
        <p id="folder-clash-message" className={styles.message}>
          {t("A list named {name} already exists here.", { name })}
        </p>
        <label className={styles.remember}>
          <input type="checkbox" checked={all} onChange={(event) => setAll(event.target.checked)} />
          <span>{t("Apply to the rest of all")}</span>
        </label>
        <div className={styles.buttons}>
          <button type="button" className={styles.button} onClick={() => answer("keep")}>{t("Keep existing")}</button>
          <button type="button" className={styles.button} onClick={() => answer("replace")}>{t("Replace existing")}</button>
          <button ref={copy} type="button" className={`${styles.button} ${styles.primary}`}
            onClick={() => answer("copy")}>{t("Import as a copy")}</button>
        </div>
      </div>
    </div>
  );
}
