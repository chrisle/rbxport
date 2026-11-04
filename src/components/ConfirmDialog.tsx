import { useCallback, useEffect, useRef, useState } from "react";
import { setConfirmHandler, type ConfirmLabels } from "@/ipc/confirmHost";
import { useTranslation } from "@/i18n";
import styles from "./ConfirmDialog.module.css";

interface Request {
  message: string;
  labels: ConfirmLabels | undefined;
  settle: (answer: boolean) => void;
}

/**
 * Asks the yes/no questions the backend's `confirm` is given, in the app's own
 * window instead of the operating system's. Mounted once; questions asked
 * while one is open wait their turn.
 *
 * A native `<dialog>` opened with `showModal()`, so the prompt joins the
 * browser's top layer: it paints above any other open app dialog (Missing
 * File Manager and the rest, which are native dialogs themselves) instead of
 * behind it, regardless of z-index (#839).
 */
export function ConfirmHost() {
  const t = useTranslation();
  const [queue, setQueue] = useState<readonly Request[]>([]);
  const dialog = useRef<HTMLDialogElement>(null);
  // Tracked ourselves rather than read from the dialog's own `open`: two
  // questions asked back to back can move `current` straight from one
  // request to the next in a single render, and showModal() throws if the
  // dialog calling it is already open.
  const open = useRef(false);
  const no = useRef<HTMLButtonElement>(null);
  const current = queue[0];

  useEffect(() => setConfirmHandler((message, labels) => new Promise<boolean>((resolve) => {
    setQueue((waiting) => [...waiting, { message, labels, settle: resolve }]);
  })), []);

  const answer = useCallback((yes: boolean) => {
    current?.settle(yes);
    setQueue((waiting) => waiting.slice(1));
  }, [current]);

  useEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (current && !open.current) {
      element.showModal();
      open.current = true;
    } else if (!current && open.current) {
      element.close();
      open.current = false;
    }
  }, [current]);

  // No is the focused button: a question about removing things should not be
  // answered yes by a stray Enter.
  useEffect(() => { if (current) no.current?.focus(); }, [current]);

  useEffect(() => {
    if (!current) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      answer(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [current, answer]);

  const title = current?.labels?.title ?? "rbxport";
  return (
    <dialog ref={dialog} className={styles.dialog} role="alertdialog" aria-label={title}
      aria-describedby="confirm-message"
      // The native Escape-to-cancel default would close the dialog without
      // settling the pending promise; the window listener above does both.
      onCancel={(event) => event.preventDefault()}>
      {current ? (
        <>
          <div className={styles.title}>{title}</div>
          <p id="confirm-message" className={styles.message}>{current.message}</p>
          <div className={styles.buttons}>
            <button ref={no} type="button" className={styles.button} onClick={() => answer(false)}>
              {current.labels?.no ?? t("No")}
            </button>
            <button type="button" className={`${styles.button} ${styles.primary}`} onClick={() => answer(true)}>
              {current.labels?.yes ?? t("Yes")}
            </button>
          </div>
        </>
      ) : null}
    </dialog>
  );
}
