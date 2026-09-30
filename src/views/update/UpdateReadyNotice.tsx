import { useEffect, useRef } from "react";
import { CircleCheck } from "lucide-react";

import { useTranslation } from "@/i18n";
import type { UpdaterState } from "@/store/useUpdater";
import styles from "./UpdateReadyNotice.module.css";

interface UpdateReadyNoticeProps {
  state: Extract<UpdaterState, { phase: "ready" }>;
  onRestart: () => void;
  onWhatsNew: () => void;
  onDismiss: () => void;
}

/** The brief restart prompt that lives inside the status bar. */
export function UpdateReadyNotice({ state, onRestart, onWhatsNew, onDismiss }: UpdateReadyNoticeProps) {
  const t = useTranslation();
  const dismiss = useRef(onDismiss);
  dismiss.current = onDismiss;

  useEffect(() => {
    const timer = window.setTimeout(() => dismiss.current(), 15_000);
    return () => window.clearTimeout(timer);
  }, []);

  return (
    <aside className={styles.notice} role="status" aria-live="polite" aria-label={t("Update ready. Restart to apply.") + ` (${state.ready.version})`}>
      <CircleCheck className={styles.icon} aria-hidden="true" />
      <p><strong>{t("Update ready. Restart to apply.")}</strong></p>
      <button type="button" className={styles.whatsNew} onClick={onWhatsNew}>{t("What's new")}</button>
      <button type="button" className={styles.restart} onClick={onRestart}>{t("Restart now")}</button>
    </aside>
  );
}
