import { useEffect, useRef } from "react";

import { useTranslation } from "@/i18n";
import styles from "./ConfirmDialog.module.css";
import skipStyles from "./ImportSkipsDialog.module.css";

/**
 * Every file an import left out and why, one line each. The status line names
 * only the first, so a drop of several files that could not be read would
 * otherwise explain one of them and leave the rest unaccounted for (#800).
 * Each entry is the backend's "file name: reason". Escape or Close dismisses.
 */
export function ImportSkipsDialog({ skipped, onClose }: {
  skipped: readonly string[];
  onClose: () => void;
}) {
  const t = useTranslation();
  const close = useRef<HTMLButtonElement>(null);

  useEffect(() => close.current?.focus(), []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const title = t("Some files were not imported");
  return (
    <div className={styles.backdrop} role="presentation">
      <div className={styles.window} role="alertdialog" aria-modal="true" aria-label={title}
        aria-describedby="import-skips-message">
        <div className={styles.title}>{title}</div>
        <p id="import-skips-message" className={styles.message}>
          {t("{count} files were not imported:", { count: skipped.length })}
        </p>
        <ul className={skipStyles.list} data-testid="import-skips">
          {skipped.map((line, index) => <li key={`${String(index)}:${line}`}>{line}</li>)}
        </ul>
        <div className={styles.buttons}>
          <button ref={close} type="button" className={`${styles.button} ${styles.primary}`} onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}
