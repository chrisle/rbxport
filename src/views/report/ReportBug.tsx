import { useCallback, useEffect, useRef, useState } from "react";
import { getBackend } from "@/ipc/client";
import { submitBugReport } from "@/lib/bugReport";
import { errorMessage } from "@/lib/errorMessage";
import { startWindowDrag } from "@/lib/windowDrag";
import styles from "./ReportBug.module.css";
import { useShowWindowWhenReady } from "@/lib/windowReady";

/**
 * The report Worker's Turnstile page, framed out of sight.
 *
 * Turnstile cannot run on the app's own pages: under `tauri://localhost` its
 * challenge fails with 600010. On an https page it passes, including inside a
 * frame whose parent is the app, so the widget lives on the Worker
 * (`report-worker/src/index.ts`, `/verify`) and hands each token to the origin
 * named here — this one.
 */
const VERIFY_ORIGIN = "https://report.rbxport.com";

/** How long without a word from the page before verification counts as failed. */
const VERIFY_SILENCE_MS = 20_000;

type VerifyMessage =
  | { source: "rbxport-verify"; type: "token"; token: string }
  | { source: "rbxport-verify"; type: "expired" }
  | { source: "rbxport-verify"; type: "error"; code: string };

function isVerifyMessage(data: unknown): data is VerifyMessage {
  if (!data || typeof data !== "object") return false;
  const message = data as { source?: unknown; type?: unknown; token?: unknown };
  if (message.source !== "rbxport-verify") return false;
  return (message.type === "token" && typeof message.token === "string") || message.type === "expired" || message.type === "error";
}

function Turnstile({ onToken, onError, resetCount }: { onToken: (token: string) => void; onError: (code: string) => void; resetCount: number }) {
  const frame = useRef<HTMLIFrameElement>(null);
  useEffect(() => {
    // A page that never loads (offline, blocked) sends nothing at all, and a
    // frame reports no load error; say so rather than leave Send disabled.
    let heard = false;
    const silence = window.setTimeout(() => { if (!heard) onError("no response"); }, VERIFY_SILENCE_MS);
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== VERIFY_ORIGIN || event.source !== frame.current?.contentWindow || !isVerifyMessage(event.data)) return;
      heard = true;
      if (event.data.type === "token") onToken(event.data.token);
      else if (event.data.type === "expired") onToken("");
      else onError(event.data.code);
    };
    window.addEventListener("message", onMessage);
    return () => {
      window.clearTimeout(silence);
      window.removeEventListener("message", onMessage);
    };
  }, [onError, onToken]);
  // A token is spent by one submission; ask the page for the next.
  useEffect(() => {
    if (resetCount > 0) frame.current?.contentWindow?.postMessage({ source: "rbxport-verify", type: "reset" }, VERIFY_ORIGIN);
  }, [resetCount]);
  return (
    <iframe
      ref={frame}
      className={styles.turnstile}
      src={`${VERIFY_ORIGIN}/verify?origin=${encodeURIComponent(window.location.origin)}`}
      title="Human verification"
    />
  );
}

export function ReportBug({ onClose, windowed = false }: { onClose: () => void; windowed?: boolean }) {
  const [email, setEmail] = useState("");
  const [description, setDescription] = useState("");
  const [include, setInclude] = useState(true);
  const [attachment, setAttachment] = useState<string | null>(null);
  const [opening, setOpening] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [turnstileToken, setTurnstileToken] = useState("");
  const [receipt, setReceipt] = useState<{ key: string; attachmentAdded: boolean } | null>(null);
  const [resetCount, setResetCount] = useState(0);
  const receiveTurnstileToken = useCallback((token: string) => { setTurnstileToken(token); if (token) setError(""); }, []);
  // Turnstile's own code, so a report of this message says which failure it was.
  const reportTurnstileError = useCallback((code: string) => setError(`Human verification could not load (${code}). Check your connection and try again.`), []);
  useEffect(() => {
    if (!include) { setAttachment(null); return; }
    let live = true;
    void getBackend().then(backend => backend.reportAttachment()).then(text => {
      if (live) { setAttachment(text); setError(""); }
    }).catch((e: unknown) => { if (live) setError(errorMessage(e)); });
    return () => { live = false; };
  }, [include]);
  const body = (
    <section className={styles.dialog} role="dialog" aria-label="Report bug" aria-modal={windowed ? undefined : true}>
      <header className={styles.title} onMouseDown={windowed ? startWindowDrag : undefined}>
        Report bug
        {windowed ? null : <button type="button" aria-label="Close report" onClick={onClose}>×</button>}
      </header>
      <form className={styles.form} onSubmit={event => {
        event.preventDefault();
        if (busy || (include && attachment === null)) return;
        setBusy(true); setError(""); setReceipt(null);
        void submitBugReport({ email, description, attachment: include ? attachment ?? "" : "", turnstileToken })
          .then(setReceipt).catch((e: unknown) => setError(errorMessage(e)))
          .finally(() => { setBusy(false); setTurnstileToken(""); setResetCount(count => count + 1); });
      }}>
        <div className={styles.fields}>
          <label className={styles.email}>
            <span>Your email <span className={styles.optional}>(optional)</span></span>
            <input type="email" autoComplete="email" placeholder="you@example.com" maxLength={320} value={email} onChange={e => setEmail(e.target.value)} />
          </label>
          <label className={styles.description}>What happened?
            <textarea required maxLength={30000} rows={7} placeholder="What were you doing, what went wrong, and what did you expect?" value={description} onChange={e => setDescription(e.target.value)} />
          </label>
          <div className={styles.attachments}>
            <div className={styles.attachmentControls}>
              <label className={styles.toggle}><input type="checkbox" checked={include} onChange={e => setInclude(e.target.checked)} />Attach log and system information</label>
              <button type="button" className={styles.previewToggle} disabled={!include || attachment === null || opening} onClick={() => {
                if (attachment === null) return;
                setOpening(true);
                setError("");
                void getBackend().then(backend => backend.openReportAttachment(attachment))
                  .catch((e: unknown) => setError(errorMessage(e)))
                  .finally(() => setOpening(false));
              }}>
                {opening ? "Opening…" : "Show log"}
              </button>
            </div>
            <p className={styles.hint}>The log may include library paths and track titles.</p>
          </div>
          <Turnstile onToken={receiveTurnstileToken} onError={reportTurnstileError} resetCount={resetCount} />
          {error ? <p className={styles.error} role="alert">{error}</p> : null}
          {receipt ? <p role="status">Report {receipt.key} submitted.{receipt.attachmentAdded ? "" : " The log attachment could not be added."}</p> : null}
        </div>
        <footer>
          <span className={styles.hint}>Reports are sent to TRIODE. I read every report, but please don’t expect a personal reply.</span>
          <button type="button" onClick={onClose}>Close</button>
          <button className={styles.save} type="submit" disabled={busy || !turnstileToken || !description.trim() || (include && attachment === null)}>{busy ? "Sending…" : "Send report"}</button>
        </footer>
      </form>
    </section>
  );
  return windowed ? body : <div className={styles.backdrop}>{body}</div>;
}

export function ReportWindow() {
  useShowWindowWhenReady();
  return <ReportBug windowed onClose={() => { void getBackend().then(backend => backend.closeWindow()); }} />;
}
