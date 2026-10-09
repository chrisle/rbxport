/**
 * The Update Manager window.
 *
 * rekordbox's is a window of its own with a title bar and a row of buttons
 * at the foot; the strings here are its own, from `german.lang`: "Update
 * Manager", "The current version", "The latest version", "Downloading",
 * "The latest version has been downloaded.", "An error occurred. Please try
 * later."
 *
 * Nothing here asks whether to download: an update found is taken, and the
 * window shows the download happening when somebody asked to look. Between
 * the versions and the buttons sits what changed: every release-note section
 * between the version running and the one on offer, so somebody two
 * releases behind reads both. The download's progress is a bar with the
 * bytes beside it; the install that follows has no progress to give, so its
 * bar just moves. Once the update is in place the window says the next
 * launch runs it, and offers to restart now instead. The window can be
 * closed at any point; the work goes on behind it.
 */
import { useEffect, useRef } from "react";

import { useTranslation } from "@/i18n";
import { formatBytes, parseChangelog, spans, type ChangelogBlock } from "@/lib/changelog";
import type { UpdaterState } from "@/store/useUpdater";
import styles from "./UpdateManager.module.css";

export interface UpdateManagerProps {
  state: UpdaterState;
  onCheck: () => void;
  /** Download again after a download that failed. */
  onRetry: () => void;
  /** Restart into the downloaded update now. */
  onRestart: () => void;
  onClose: () => void;
}

/** The section's blocks, drawn. */
function Changes({ markdown }: { markdown: string }) {
  return (
    <>
      {parseChangelog(markdown).map((block, i) => (
        <Block key={i} block={block} />
      ))}
    </>
  );
}

function Inline({ text }: { text: string }) {
  return (
    <>
      {spans(text).map((span, i) =>
        span.code ? <code key={i}>{span.text}</code> : <span key={i}>{span.text}</span>,
      )}
    </>
  );
}

function Block({ block }: { block: ChangelogBlock }) {
  switch (block.kind) {
    case "release":
      return (
        <h3 className={styles.release}>
          Version {block.version}
          {block.date ? <span className={styles.date}>{block.date}</span> : null}
        </h3>
      );
    case "heading":
      return <h4 className={styles.heading}>{block.text}</h4>;
    case "paragraph":
      return (
        <p className={styles.paragraph}>
          <Inline text={block.text} />
        </p>
      );
    case "list":
      return (
        <ul className={styles.list}>
          {block.items.map((item, i) => (
            <li key={i}>
              <Inline text={item} />
            </li>
          ))}
        </ul>
      );
  }
}

function Progress({ downloaded, total }: { downloaded: number; total: number | null }) {
  const fraction = total !== null && total > 0 ? Math.min(downloaded / total, 1) : null;
  return (
    <div className={styles.progressRow}>
      <div
        className={styles.progress}
        role="progressbar"
        aria-label="Downloading"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
        data-indeterminate={fraction === null || undefined}
      >
        <span className={styles.progressFill} style={fraction === null ? undefined : { width: `${fraction * 100}%` }} />
      </div>
      <span className={styles.progressText}>
        {total !== null
          ? `${formatBytes(downloaded)} of ${formatBytes(total)}`
          : formatBytes(downloaded)}
      </span>
    </div>
  );
}

export function UpdateManager({ state, onCheck, onRetry, onRestart, onClose }: UpdateManagerProps) {
  const panel = useRef<HTMLDivElement>(null);
  const t = useTranslation();

  const check = state.phase === "available" || state.phase === "downloading" || state.phase === "installing" ||
    state.phase === "ready" || (state.phase === "failed" && state.check)
    ? state.check
    : null;

  // Escape closes as the close button does. A download or install runs on
  // behind a closed window: it is the app's work, not the window's.
  useEffect(() => {
    panel.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className={styles.backdrop} onMouseDown={onClose} role="presentation">
      <div
        ref={panel}
        className={styles.window}
        onMouseDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-label="Update Manager"
        tabIndex={-1}
      >
        <header className={styles.titlebar}>
          <button type="button" className={styles.close} onClick={onClose} aria-label="Close">
            ✕
          </button>
          Update Manager
        </header>

        <div className={styles.body}>
          {state.phase === "checking" ? (
            <p className={styles.status}>Checking for updates…</p>
          ) : state.phase === "store" ? (
            // The Store installs this copy's updates; the app's own installer
            // would put a second copy beside it (#189).
            <p className={styles.status}>{t("This copy of rbxport is from the Microsoft Store. Get updates from the Microsoft Store.")}</p>
          ) : state.phase === "upToDate" ? (
            <p className={styles.status}>
              rbxport {state.currentVersion} is the latest version.
            </p>
          ) : check ? (
            <>
              <dl className={styles.versions}>
                <dt>The current version</dt>
                <dd>{check.currentVersion}</dd>
                <dt>The latest version</dt>
                <dd>{check.version}</dd>
              </dl>
              {state.phase === "available" ? (
                <p className={styles.status}>{t("New update available v{version}.", { version: check.version ?? "?" })}</p>
              ) : state.phase === "downloading" ? (
                <>
                  <p className={styles.status}>Downloading…</p>
                  <Progress downloaded={state.progress?.downloaded ?? 0} total={state.progress?.total ?? null} />
                </>
              ) : state.phase === "installing" ? (
                <>
                  <p className={styles.status}>The latest version has been downloaded. Installing…</p>
                  <Progress downloaded={0} total={null} />
                </>
              ) : state.phase === "ready" ? (
                <p className={styles.status}>
                  The latest version has been downloaded. It will be used the next time rbxport opens.
                </p>
              ) : (
                <p className={`${styles.status} ${styles.failed}`} role="alert">
                  An error occurred. Please try later.
                  {state.phase === "failed" && state.message ? (
                    <span className={styles.detail}>{state.message}</span>
                  ) : null}
                </p>
              )}
              <section className={styles.changes} aria-label="What's new">
                {check.changes.length > 0 ? (
                  check.changes.map((change) => <Changes key={change.version} markdown={change.body} />)
                ) : (
                  <p className={styles.paragraph}>No release notes for this version.</p>
                )}
              </section>
            </>
          ) : (
            <p className={`${styles.status} ${styles.failed}`} role="alert">
              An error occurred. Please try later.
              {state.phase === "failed" && state.message ? (
                <span className={styles.detail}>{state.message}</span>
              ) : null}
            </p>
          )}
        </div>

        <footer className={styles.buttons}>
          {state.phase === "ready" ? (
            <>
              <button type="button" className={styles.button} onClick={onClose}>Later</button>
              <button type="button" className={`${styles.button} ${styles.primary}`} onClick={onRestart}>
                Restart Now
              </button>
            </>
          ) : state.phase === "failed" ? (
            <>
              <button type="button" className={styles.button} onClick={onClose}>Close</button>
              <button
                type="button"
                className={`${styles.button} ${styles.primary}`}
                onClick={state.check ? onRetry : onCheck}
              >
                {state.check ? "Try Again" : "Check Again"}
              </button>
            </>
          ) : state.phase === "downloading" || state.phase === "installing" ? (
            <button type="button" className={styles.button} onClick={onClose}>Close</button>
          ) : (
            <button type="button" className={`${styles.button} ${styles.primary}`} onClick={onClose}>
              OK
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}
