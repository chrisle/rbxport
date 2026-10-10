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
import { useCallback, useEffect, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { MissingTrack, RelocateSearch } from "@/ipc/types";
import { listIds, missingAmong, relocateSteps, relocateTracks } from "@/lib/relocate";
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

/** Runs the work straight away, for when no job queue is given. */
const now = <T,>(work: () => Promise<T>): Promise<T> => work();

export function MissingFileManager({ readOnly, search, searching = now, onWrote, onFailed, onClose }: {
  /** rekordbox holds the library, or Library Protection is on: nothing is written. */
  readOnly: boolean;
  /** Preferences › Advanced › Database › Auto Relocate Search Folders. */
  search: RelocateSearch;
  /**
   * Runs Auto Relocate's search as the app's "Searching..." job, so the
   * status bar shows it and analysis waits for it, as it does from the
   * track menu.
   */
  searching?: <T>(work: () => Promise<T>) => Promise<T>;
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

  /**
   * The selected rows in the list's order, each checked to be still missing
   * as Relocate reaches it. With every row selected the whole list's ids are
   * taken before the first chooser (`listIds`).
   */
  const selectedTracks = useCallback(async function* (): AsyncGenerator<MissingTrack> {
    const backend = await getBackend();
    let ids: string[];
    if (selection.all) ids = await listIds(async (offset, limit) => (await backend.missingTracks(offset, limit, false)).tracks, PAGE);
    else {
      ids = [...selection.ids];
      const order = new Map<string, number>();
      for (const [page, rows] of pages) rows.forEach((row, at) => order.set(row.id, page * PAGE + at));
      ids.sort((a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0));
    }
    yield* missingAmong(ids, (some) => backend.relocationTargets(some));
  }, [selection, pages]);

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
        setNote(null);
        onFailed(e instanceof Error ? e.message : String(e));
      } finally {
        setBusy(false);
      }
    })();
  };

  // [OBS rekordbox 7.2.19 static] rekordbox searches under a "Searching..."
  // progress window (`UpdateMissingFiles` @0x1012a760c); this says so here
  // until the answer comes.
  const autoRelocate = () => run(async () => {
    const backend = await getBackend();
    setNote(t("Searching..."));
    const report = await searching(() => backend.autoRelocate(search, targets));
    const said = report.unresolved > 0
      ? t("{relocated} relocated, {unresolved} not found in the search folders.", { ...report })
      : t("{relocated} relocated.", { ...report });
    setNote(said);
    return report.relocated > 0 ? said : null;
  });

  // Relocate over the selected rows, rekordbox's way (src/lib/relocate.ts).
  const relocate = () => run(async () => {
    const backend = await getBackend();
    const moved = await relocateTracks(selectedTracks(), relocateSteps(backend, t));
    return moved > 0 ? "" : null;
  });

  // [OBS rekordbox 7.2.19 static] Delete, the button
  // (`MissingFileComponent::buttonClicked` → `deleteSelectedFiles(true)`
  // @0x1012a5e74) and the Delete key (`MissingFileTable::deleteKeyPressed`
  // @0x1012a5bb4), asks OK/Cancel under the title "Remove": "Are you sure you
  // want to remove the selected tracks?". The key's Command + Delete, which
  // skips the question, and the line saying so are not here, as they are
  // not in the track list (#136).
  const remove = () => run(async () => {
    const backend = await getBackend();
    const sure = await backend.confirm(
      t("Are you sure you want to remove the selected tracks?"),
      { yes: t("OK"), no: t("Cancel"), title: t("Remove") },
    );
    if (!sure) return null;
    const removed = await backend.removeMissingTracks(targets);
    return removed === 1
      ? t("Removed {count} track.", { count: removed })
      : t("Removed {count} tracks.", { count: removed });
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
      <div
        className={styles.table}
        role="grid"
        aria-label="Missing files"
        aria-rowcount={total ?? 0}
        tabIndex={0}
        onKeyDown={(event) => {
          // rekordbox's list takes Delete and Backspace alike
          // (`CustomListBox::keyPressed` → `MissingFileTable::deleteKeyPressed`).
          if (event.key !== "Delete" && event.key !== "Backspace") return;
          event.preventDefault();
          if (event.repeat || readOnly || busy || chosen === 0) return;
          remove();
        }}
      >
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
        <button type="button" disabled={readOnly || busy || chosen === 0} onClick={relocate}>Relocate</button>
        <button type="button" disabled={readOnly || busy || chosen === 0} onClick={remove}>Delete</button>
        <span className={styles.spacer} />
        <button type="button" onClick={onClose}>OK</button>
      </div>
    </dialog>
  );
}
