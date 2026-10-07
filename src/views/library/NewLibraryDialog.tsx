import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import styles from "./NewLibraryDialog.module.css";

/**
 * The question a machine with no configured rekordbox library gets: make one,
 * choose an existing database, or quit. Nothing else in the window works
 * without a library, so Escape does nothing.
 */
export function NewLibraryDialog({ masterDb, onCreate, onChoose, onQuit }: {
  masterDb: string;
  onCreate: () => Promise<void>;
  onChoose: (title: string, filterName: string) => Promise<boolean>;
  onQuit: () => void;
}) {
  const t = useTranslation();
  const [busy, setBusy] = useState<"create" | "choose" | null>(null);
  const [error, setError] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  const create = () => {
    setBusy("create");
    setError("");
    onCreate().catch((e: unknown) => {
      setBusy(null);
      setError(errorText(e, t("Could not create the database.")));
    });
  };

  const choose = () => {
    setBusy("choose");
    setError("");
    onChoose(t("Choose an existing rekordbox library"), t("rekordbox database"))
      .then((selected) => { if (!selected) setBusy(null); })
      .catch((e: unknown) => {
        setBusy(null);
        setError(errorText(e, t("Could not open the database.")));
      });
  };

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="new-library-title"
      aria-describedby="new-library-text"
      onCancel={event => event.preventDefault()}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => { event.preventDefault(); if (busy === null) create(); }}>
        <h2 id="new-library-title" className={styles.title}>{t("No rekordbox Library")}</h2>
        <p id="new-library-text" className={styles.text}>
          {t("RBXport could not find a configured rekordbox database. Create a new one or choose an existing master.db.")}
        </p>
        <p className={styles.path} title={masterDb}>{masterDb}</p>
        {error ? <p className={styles.error} role="alert">{error}</p> : null}
        <div className={styles.buttons}>
          <button type="button" onClick={choose} disabled={busy !== null} autoFocus>
            {busy === "choose" ? t("Opening…") : t("Choose Existing…")}
          </button>
          <button type="submit" disabled={busy !== null}>{busy === "create" ? t("Creating…") : t("Create New")}</button>
          <button type="button" onClick={onQuit} disabled={busy !== null}>{t("Quit")}</button>
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
