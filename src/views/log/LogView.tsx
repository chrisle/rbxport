import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { getBackend } from "@/ipc/client";
import type { LogLevel, LogLine } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import { appendLogLines, formatLogLine, visibleLogLines } from "@/lib/logLines";
import { startWindowDrag } from "@/lib/windowDrag";
import { useShowWindowWhenReady } from "@/lib/windowReady";
import styles from "./LogView.module.css";

/** How often the open window asks for lines logged since it last asked. */
const POLL_MS = 1000;

/** The level filter's choices, most severe first; the labels are found by the locale script. */
const LEVEL_CHOICES: readonly { level: LogLevel; label: string }[] = [
  { level: "error", label: "Errors" },
  { level: "warn", label: "Warnings and errors" },
  { level: "info", label: "Info and above" },
  { level: "debug", label: "Debug and above" },
  { level: "trace", label: "Everything" },
];

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * What the app has logged since it started, to read back what the status bar
 * said for a moment. Its own window in the desktop app; drawn over the main
 * window in a browser.
 */
export function LogView({ windowed = false, onClose }: { windowed?: boolean; onClose: () => void }) {
  const t = useTranslation();
  const [lines, setLines] = useState<LogLine[]>([]);
  const [oldest, setOldest] = useState(1);
  const [loaded, setLoaded] = useState(false);
  const [minimum, setMinimum] = useState<LogLevel>("trace");
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState<{ text: string; failed: boolean } | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const last = useRef(0);
  useShowWindowWhenReady(loaded);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const backend = await getBackend();
        const page = await backend.logLines(last.current);
        if (!live) return;
        last.current = page.lines.at(-1)?.seq ?? last.current;
        setOldest(page.oldest);
        setLines(held => appendLogLines(held, page.lines));
      } catch (error) {
        if (live) setNotice({ text: message(error), failed: true });
      } finally {
        if (live) {
          setLoaded(true);
          timer = setTimeout(() => { void poll(); }, POLL_MS);
        }
      }
    };
    void poll();
    return () => { live = false; clearTimeout(timer); };
  }, []);

  const visible = useMemo(() => visibleLogLines(lines, minimum, query), [lines, minimum, query]);

  // New lines keep the newest in view while the list is scrolled to its end.
  useLayoutEffect(() => {
    const element = list.current;
    if (element && following.current) element.scrollTop = element.scrollHeight;
  }, [visible]);

  useEffect(() => {
    if (!notice || notice.failed) return;
    const timer = setTimeout(() => setNotice(null), 3000);
    return () => clearTimeout(timer);
  }, [notice]);

  const text = () => visible.map(formatLogLine).join("\n") + (visible.length > 0 ? "\n" : "");
  const copy = () => {
    void navigator.clipboard.writeText(text())
      .then(() => setNotice({ text: t(visible.length === 1 ? "Copied {count} line." : "Copied {count} lines.", { count: visible.length }), failed: false }))
      .catch((error: unknown) => setNotice({ text: message(error), failed: true }));
  };
  const save = () => {
    void getBackend().then(backend => backend.saveLogText(text()))
      .then(path => { if (path !== null) setNotice({ text: t(visible.length === 1 ? "Saved {count} line." : "Saved {count} lines.", { count: visible.length }), failed: false }); })
      .catch((error: unknown) => setNotice({ text: message(error), failed: true }));
  };
  const openFolder = () => {
    void getBackend().then(backend => backend.openLogFolder())
      .catch((error: unknown) => setNotice({ text: message(error), failed: true }));
  };

  const body = (
    <section className={styles.window} role="dialog" aria-label={t("Log")} aria-modal={windowed ? undefined : true}>
      <header className={styles.title} onMouseDown={windowed ? startWindowDrag : undefined}>
        {t("Log")}
        {windowed ? null : <button type="button" aria-label={t("Close")} onClick={onClose}>×</button>}
      </header>
      <div className={styles.controls}>
        <label className={styles.level}>
          <span>{t("Show")}</span>
          <select value={minimum} onChange={event => setMinimum(event.target.value as LogLevel)}>
            {LEVEL_CHOICES.map(({ level, label }) => <option key={level} value={level}>{t(label)}</option>)}
          </select>
        </label>
        <input className={styles.filter} type="search" aria-label={t("Filter the log")} placeholder={t("Filter")}
          value={query} onChange={event => setQuery(event.target.value)} />
        <span className={styles.count}>{t("{shown} of {total} lines", { shown: visible.length, total: lines.length })}</span>
        <button type="button" onClick={copy} disabled={visible.length === 0}>{t("Copy")}</button>
        <button type="button" onClick={save} disabled={visible.length === 0}>{t("Save…")}</button>
        <button type="button" onClick={openFolder}>{t("Open log folder")}</button>
      </div>
      <div ref={list} className={styles.lines} role="log" aria-label={t("Log lines")} tabIndex={0}
        onScroll={event => {
          const element = event.currentTarget;
          following.current = element.scrollHeight - element.scrollTop - element.clientHeight < 4;
        }}>
        {oldest > 1
          ? <p className={styles.note}>{t("Older lines are only in the log file.")}</p> : null}
        {loaded && visible.length === 0
          ? <p className={styles.note}>{lines.length === 0 ? t("Nothing has been logged yet.") : t("No lines match.")}</p> : null}
        {/* The lines are what the app logged, in English; never translated. */}
        <div data-i18n-ignore>
          {visible.map(line => (
            <div key={line.seq} className={styles.line} data-level={line.level}>{formatLogLine(line)}</div>
          ))}
        </div>
      </div>
      {notice ? <p className={notice.failed ? styles.error : styles.status} role="status">{notice.text}</p> : null}
    </section>
  );
  return windowed ? body : <div className={styles.backdrop}>{body}</div>;
}

export function LogWindow() {
  return <LogView windowed onClose={() => { void getBackend().then(backend => backend.closeWindow()); }} />;
}
