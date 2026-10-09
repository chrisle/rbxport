/**
 * The Missing File Manager: File › Display All Missing Files.
 *
 * Laid out as rekordbox 7.2.14 lays out its own window [OBS Winrig
 * chris-win11 2026-10-08, issue #201]: a list of every track whose file is
 * gone, under Track Title, artist, album and location; the count as "N
 * Track" over the buttons; Auto Relocate, Relocate and Delete on the left
 * and OK on the right. Every row is selected when it opens [OBS], so the
 * buttons act on the whole list until a row is clicked.
 *
 * The list can run to tens of thousands of tracks, so it is drawn a
 * screenful at a time and fetched a page at a time from the backend's last
 * scan. Opening the manager, and every change made from it, scans again.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { MissingTrack } from "@/ipc/types";
import { useTranslation } from "@/i18n";
import styles from "./MissingFileManager.module.css";

/** Rows per request: under the backend's 128-row cap. */
const PAGE = 100;
/** A row's height in CSS px, rekordbox's 25pt pitch. */
const ROW_H = 25;
/** Rows drawn beyond the visible ones, above and below. */
const OVERSCAN = 8;

/** Which rows the buttons act on: every one, or the ones clicked. */
type Selection = { all: true } | { all: false; ids: ReadonlySet<string>; anchor: number };

const EVERY: Selection = { all: true };

export function MissingFileManager({ readOnly, folders, onWrote, onFailed, onClose }: {
  /** rekordbox holds the library, or Library Protection is on: nothing is written. */
  readOnly: boolean;
  /** Preferences › Advanced › Database › Auto Relocate Search Folders. */
  folders: readonly string[];
  /** A change was saved: the status line says so and the tree is re-read. */
  onWrote: (said: string) => void;
  onFailed: (said: string) => void;
  onClose: () => void;
}) {
  const t = useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [pages, setPages] = useState<ReadonlyMap<number, readonly MissingTrack[]>>(new Map());
  const [selection, setSelection] = useState<Selection>(EVERY);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [scroll, setScroll] = useState({ top: 0, height: 0 });
  /** Bumped by every rescan, so a page that lands after one is dropped. */
  const scanId = useRef(0);
  const requested = useRef(new Set<number>());

  useEffect(() => {
    const element = dialog.current;
    const previous = document.activeElement;
    element?.showModal();
    return () => {
      element?.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  const rescan = useCallback(async () => {
    const id = ++scanId.current;
    requested.current = new Set([0]);
    const backend = await getBackend();
    const first = await backend.missingTracks(0, PAGE, true);
    if (id !== scanId.current) return;
    setTotal(first.total);
    setPages(new Map([[0, first.tracks]]));
    setSelection(EVERY);
    list.current?.scrollTo({ top: 0 });
  }, []);

  useEffect(() => {
    void rescan().catch((e: unknown) => onFailed(e instanceof Error ? e.message : String(e)));
  }, [rescan, onFailed]);

  // The pages the visible rows fall in, fetched once each per scan.
  const first = Math.max(0, Math.floor(scroll.top / ROW_H) - OVERSCAN);
  const last = Math.min(total ?? 0, Math.ceil((scroll.top + scroll.height) / ROW_H) + OVERSCAN);
  useEffect(() => {
    if (total === null) return;
    const id = scanId.current;
    for (let page = Math.floor(first / PAGE); page * PAGE < last; page++) {
      if (requested.current.has(page)) continue;
      requested.current.add(page);
      void (async () => {
        const backend = await getBackend();
        const got = await backend.missingTracks(page * PAGE, PAGE, false);
        if (id !== scanId.current) return;
        setPages((current) => new Map(current).set(page, got.tracks));
      })();
    }
  }, [first, last, total]);

  useEffect(() => {
    const element = list.current;
    if (!element) return;
    const measure = () => setScroll({ top: element.scrollTop, height: element.clientHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const rowAt = useCallback(
    (index: number): MissingTrack | undefined => pages.get(Math.floor(index / PAGE))?.[index % PAGE],
    [pages],
  );

  const select = (index: number, track: MissingTrack, event: React.MouseEvent) => {
    setSelection((current) => {
      if (event.shiftKey && !current.all) {
        const ids = new Set<string>();
        const [from, to] = current.anchor < index ? [current.anchor, index] : [index, current.anchor];
        for (let at = from; at <= to; at++) {
          const row = rowAt(at);
          if (row) ids.add(row.id);
        }
        return { all: false, ids, anchor: current.anchor };
      }
      if ((event.metaKey || event.ctrlKey) && !current.all) {
        const ids = new Set(current.ids);
        if (ids.has(track.id)) ids.delete(track.id);
        else ids.add(track.id);
        return { all: false, ids, anchor: index };
      }
      return { all: false, ids: new Set([track.id]), anchor: index };
    });
  };

  const chosen = selection.all ? total ?? 0 : selection.ids.size;
  /** What the backend is asked to act on: null is every missing track. */
  const targets = selection.all ? null : [...selection.ids];

  /** The first selected row in the list's order, for Relocate. */
  const firstChosen = useMemo((): MissingTrack | undefined => {
    if (selection.all) return rowAt(0);
    for (const page of [...pages.keys()].sort((a, b) => a - b)) {
      const found = pages.get(page)?.find((row) => selection.ids.has(row.id));
      if (found) return found;
    }
    return undefined;
  }, [selection, pages, rowAt]);

  const run = (action: () => Promise<string | null>) => {
    setBusy(true);
    setNote(null);
    void (async () => {
      try {
        const said = await action();
        if (said !== null) {
          onWrote(said);
          await rescan();
        }
      } catch (e) {
        onFailed(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    })();
  };

  const autoRelocate = () => run(async () => {
    const backend = await getBackend();
    const report = await backend.autoRelocate([...folders], targets);
    const said = report.unresolved > 0
      ? t("{relocated} relocated, {unresolved} not found in the search folders.", { ...report })
      : t("{relocated} relocated.", { ...report });
    setNote(said);
    return report.relocated > 0 ? said : null;
  });

  const relocate = () => run(async () => {
    if (!firstChosen) return null;
    const backend = await getBackend();
    const path = await backend.relocateTrack(firstChosen.id);
    return path === null ? null : t("Relocated {title}.", { title: firstChosen.title });
  });

  const remove = () => run(async () => {
    const backend = await getBackend();
    const count = `${chosen} track${chosen === 1 ? "" : "s"}`;
    const sure = await backend.confirm(
      t("Remove {count} from the collection? This can’t be undone. The files stay where they are.", { count }),
    );
    if (!sure) return null;
    const removed = await backend.removeMissingTracks(targets);
    const gone = `${removed} track${removed === 1 ? "" : "s"}`;
    return t("Removed {count} from the collection.", { count: gone });
  });

  const isSelected = (track: MissingTrack | undefined) =>
    track !== undefined && (selection.all || selection.ids.has(track.id));

  const rows = [];
  for (let index = first; index < last; index++) {
    const track = rowAt(index);
    rows.push(
      <div
        key={index}
        className={styles.row}
        role="row"
        aria-selected={isSelected(track)}
        data-selected={isSelected(track) || undefined}
        data-even={index % 2 === 1 || undefined}
        style={{ top: index * ROW_H }}
        onClick={(event) => { if (track) select(index, track, event); }}
      >
        <span className={styles.cell} role="gridcell">{track?.title ?? ""}</span>
        <span className={styles.cell} role="gridcell">{track?.artist ?? ""}</span>
        <span className={styles.cell} role="gridcell">{track?.album ?? ""}</span>
        <span className={styles.cell} role="gridcell">{track?.path ?? ""}</span>
      </div>,
    );
  }

  return (
    <dialog
      ref={dialog}
      className={styles.dialog}
      aria-labelledby="missing-file-manager-title"
      onCancel={(event) => { event.preventDefault(); onClose(); }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      <h2 id="missing-file-manager-title" className={styles.title}>Missing File Manager</h2>
      <div className={styles.table} role="grid" aria-label="Missing files" aria-rowcount={total ?? 0}>
        <div className={styles.header} role="row">
          <span className={styles.cell} role="columnheader">Track Title</span>
          <span className={styles.cell} role="columnheader">artist</span>
          <span className={styles.cell} role="columnheader">album</span>
          <span className={styles.cell} role="columnheader">location</span>
        </div>
        <div
          ref={list}
          className={styles.list}
          onScroll={(event) => setScroll({ top: event.currentTarget.scrollTop, height: event.currentTarget.clientHeight })}
        >
          <div className={styles.rows} style={{ height: (total ?? 0) * ROW_H }}>{rows}</div>
        </div>
      </div>
      <div className={styles.count} aria-live="polite">
        {/* rekordbox's own "36444 Track", the word apart so it is translated alone. */}
        {total === null ? "Checking…" : <><span>{total}</span> <span>Track</span></>}
      </div>
      {note !== null ? <p className={styles.note}>{note}</p> : null}
      <div className={styles.buttons}>
        <button type="button" disabled={readOnly || busy || chosen === 0} onClick={autoRelocate}>Auto Relocate</button>
        <button type="button" disabled={readOnly || busy || firstChosen === undefined} onClick={relocate}>Relocate</button>
        <button type="button" disabled={readOnly || busy || chosen === 0} onClick={remove}>Delete</button>
        <span className={styles.spacer} />
        <button type="button" onClick={onClose}>OK</button>
      </div>
    </dialog>
  );
}
