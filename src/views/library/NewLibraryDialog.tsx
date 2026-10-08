import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import type { LibraryProblem } from "@/ipc/types";
import styles from "./NewLibraryDialog.module.css";

/** The startup problems the window asks about rather than reports. */
export type LibraryQuestion = Exclude<LibraryProblem, { kind: "failed" }>;

/**
 * The question asked when there is no library to open. Nothing else in the
 * window works without a library, so Escape does nothing.
 *
 * `missing`: no library anywhere; make one, or quit.
 *
 * `unavailable`: rekordbox's library is set to a drive that is not there.
 * This is rekordbox's own `MasterDbMissingWindow`, word for word: "Cannot
 * find Master Database…", Yes or No. Yes confirms "Location of Master
 * Database will be changed to the default drive…" (OK or Cancel) and then
 * uses the default drive; No ends the launch. Nothing is ever made on the
 * missing drive [OBS rekordbox 7.2.11, static analysis].
 */
export function NewLibraryDialog({ problem, onCreate, onUseDefault, onConfirm, onQuit }: {
  problem: LibraryQuestion;
  onCreate: () => Promise<void>;
  onUseDefault: () => Promise<void>;
  /** rekordbox's OK/Cancel confirmation; resolves true for OK. */
  onConfirm: (message: string, labels: { yes: string; no: string }) => Promise<boolean>;
  onQuit: () => void;
}) {
  const t = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  // A new question (the default drive turned out to be empty) starts afresh.
  useEffect(() => {
    setBusy(false);
    setError("");
  }, [problem]);

  const run = (action: () => Promise<void>, fallback: string) => {
    setBusy(true);
    setError("");
    action().catch((e: unknown) => {
      setBusy(false);
      setError(errorText(e, fallback));
    });
  };

  if (problem.kind === "unavailable") {
    const switchToDefault = () => run(async () => {
      const sure = await onConfirm(
        [
          t("Location of Master Database will be changed to the default drive."),
          t("The location can be changed at [Advanced] tab of [Preferences] window."),
        ].join("\n"),
        { yes: t("OK"), no: t("Cancel") },
      );
      if (!sure) {
        setBusy(false);
        return;
      }
      await onUseDefault();
    }, t("Failed to switch Master Database."));
    return (
      <dialog ref={dialog} className={styles.dialog} aria-labelledby="master-db-missing-text"
        onCancel={event => event.preventDefault()}
        onKeyDown={event => event.stopPropagation()}>
        <form onSubmit={event => { event.preventDefault(); if (!busy) switchToDefault(); }}>
          <p id="master-db-missing-text" className={styles.message}>
            <span>{t("Cannot find Master Database.")}</span>
            <span>{t("Launch RBXport after connecting a drive where Master Database is stored.")}</span>
            <span>{t("Do you want to open Master Database in the default drive?")}</span>
          </p>
          {error ? <p className={styles.error} role="alert">{error}</p> : null}
          <div className={styles.buttons}>
            <button type="submit" disabled={busy} autoFocus>{t("Yes")}</button>
            <button type="button" onClick={onQuit} disabled={busy}>{t("No")}</button>
          </div>
        </form>
      </dialog>
    );
  }

  const create = () => run(onCreate, t("Could not create the database."));
  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="new-library-title"
      aria-describedby="new-library-text"
      onCancel={event => event.preventDefault()}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => { event.preventDefault(); if (!busy) create(); }}>
        <h2 id="new-library-title" className={styles.title}>No rekordbox Library</h2>
        <p id="new-library-text" className={styles.text}>
          rekordbox isn&apos;t installed and there is no rekordbox database.
          Would you like to create a new database?
        </p>
        <p className={styles.path} title={problem.masterDb}>{problem.masterDb}</p>
        {error ? <p className={styles.error} role="alert">{error}</p> : null}
        <div className={styles.buttons}>
          <button type="submit" disabled={busy} autoFocus>{busy ? "Creating…" : "Create"}</button>
          <button type="button" onClick={onQuit} disabled={busy}>Quit</button>
        </div>
      </form>
    </dialog>
  );
}

/** The backend's message when it sent one, which says what went wrong. */
function errorText(e: unknown, fallback: string): string {
  if (typeof e === "object" && e !== null && "message" in e && typeof e.message === "string") return e.message;
  if (typeof e === "string") return e;
  return fallback;
}
