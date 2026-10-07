import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import type { DriveLibrary, LibraryProblem } from "@/ipc/types";
import styles from "./NewLibraryDialog.module.css";

/** The startup problems the window asks about rather than reports. */
export type LibraryQuestion = Exclude<LibraryProblem, { kind: "failed" }>;

type Busy = "create" | "choose" | "open" | "retry" | null;

/**
 * The question asked when there is no library to open.
 *
 * With nothing configured (`missing`): open a library found on a connected
 * drive, pick a `master.db`, or create a new one. With a configured library
 * that is not there (`unavailable`, most often a drive that is not
 * connected), what rekordbox itself offers: connect the drive and try again,
 * or use the default location instead — never a new library on the missing
 * drive. Libraries on connected drives are listed first, found where
 * rekordbox's own Database management looks, and the list follows drives
 * being connected while the question is open. Nothing else in the window
 * works without a library, so Escape does nothing.
 */
export function NewLibraryDialog({
  problem, onDiscover, onDrivesChanged, onOpen, onChoose, onCreate, onRetry, onQuit,
}: {
  problem: LibraryQuestion;
  onDiscover: () => Promise<DriveLibrary[]>;
  /** Calls the listener when a drive is connected or removed; returns the unsubscribe. */
  onDrivesChanged: (listener: () => void) => () => void;
  onOpen: (masterDb: string) => Promise<void>;
  onChoose: (title: string, filterName: string) => Promise<boolean>;
  onCreate: () => Promise<void>;
  onRetry: () => Promise<void>;
  onQuit: () => void;
}) {
  const t = useTranslation();
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState("");
  const [drives, setDrives] = useState<DriveLibrary[]>([]);
  const dialog = useRef<HTMLDialogElement>(null);
  const retrying = useRef(false);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);

  useEffect(() => {
    let live = true;
    const look = () => {
      onDiscover().then(
        (found) => { if (live) setDrives(found); },
        () => { if (live) setDrives([]); },
      );
    };
    look();
    const stop = onDrivesChanged(look);
    return () => {
      live = false;
      stop();
    };
    // Looked for on opening and whenever a drive comes or goes; a new
    // callback identity from the parent is not a reason to look again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A retry that still finds nothing reports the problem afresh.
  useEffect(() => {
    if (!retrying.current) return;
    retrying.current = false;
    setBusy(null);
    setError(t("The library is still not there. Check that its drive is connected."));
  }, [problem, t]);

  /** Runs one action with every button off until it settles. */
  const run = (which: Exclude<Busy, null>, action: () => Promise<unknown>, fallback: string) => {
    setBusy(which);
    setError("");
    action()
      .then((done) => { if (done === false) setBusy(null); })
      .catch((e: unknown) => {
        setBusy(null);
        setError(t(errorText(e, fallback)));
      });
  };

  const create = () => run("create", onCreate, t("Could not create the database."));
  const choose = () => run(
    "choose",
    () => onChoose(t("Choose an existing rekordbox library"), t("rekordbox database")),
    t("Could not open the database."),
  );
  const open = (masterDb: string) => run("open", () => onOpen(masterDb), t("Could not open the database."));
  const retry = () => {
    retrying.current = true;
    run("retry", onRetry, t("Could not open the database."));
  };

  const unavailable = problem.kind === "unavailable";
  const text = !unavailable
    ? t("RBXport could not find a rekordbox library on this computer. Open one on a connected drive, choose a master.db, or create a new library.")
    : problem.configuredBy === "rekordbox"
      ? t("rekordbox is set to use a library that cannot be found. Connect the drive it is stored on and click Try Again, or use the library in the default location instead.")
      : t("The library chosen in RBXport cannot be found. Connect the drive it is stored on and click Try Again, or use the library in the default location instead.");

  return (
    <dialog ref={dialog} className={styles.dialog} aria-labelledby="new-library-title"
      aria-describedby="new-library-text"
      onCancel={event => event.preventDefault()}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => {
        event.preventDefault();
        if (busy !== null) return;
        if (unavailable) retry(); else create();
      }}>
        <h2 id="new-library-title" className={styles.title}>
          {unavailable ? t("Cannot Find Library") : t("No rekordbox Library")}
        </h2>
        <p id="new-library-text" className={styles.text}>{text}</p>
        <p className={styles.path} title={problem.masterDb}>{problem.masterDb}</p>
        {drives.length > 0 ? (
          <section className={styles.drives} aria-labelledby="new-library-drives">
            <h3 id="new-library-drives" className={styles.drivesTitle}>{t("Libraries on connected drives")}</h3>
            <ul className={styles.driveList}>
              {drives.map((drive) => (
                <li key={drive.masterDb} className={styles.drive}>
                  <span className={styles.driveName}>{drive.name}</span>
                  <span className={styles.drivePath} title={drive.masterDb}>{drive.masterDb}</span>
                  <button type="button" disabled={busy !== null} onClick={() => open(drive.masterDb)}
                    aria-label={t("Open the library on {drive}", { drive: drive.name })}>
                    {t("Open")}
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ) : null}
        <p className={styles.hint}>
          {t("RBXport remembers a library you choose here. rekordbox sets its own library in Preferences > Advanced > Database management.")}
        </p>
        {error ? <p className={styles.error} role="alert">{error}</p> : null}
        <div className={styles.buttons}>
          {unavailable ? (
            <>
              <button type="submit" disabled={busy !== null} autoFocus>
                {busy === "retry" ? t("Checking…") : t("Try Again")}
              </button>
              <button type="button" onClick={create} disabled={busy !== null} title={problem.defaultMasterDb}>
                {busy === "create"
                  ? (problem.defaultExists ? t("Opening…") : t("Creating…"))
                  : (problem.defaultExists ? t("Use Default Library") : t("Create in Default Location"))}
              </button>
            </>
          ) : null}
          <button type="button" onClick={choose} disabled={busy !== null} autoFocus={!unavailable}>
            {busy === "choose" || busy === "open" ? t("Opening…") : t("Choose master.db…")}
          </button>
          {unavailable ? null : (
            <button type="submit" disabled={busy !== null}>{busy === "create" ? t("Creating…") : t("Create New")}</button>
          )}
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
