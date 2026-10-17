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
 *
 * The window resizes from any edge or corner, between 400 x 300 and
 * 1024 x 768 (`MissingFileDialog` @0x10190deac: `setResizable(true,
 * false)`, `setResizeLimits(400, 300, 1024, 768)`; on chris-win11 the limits
 * hold the whole window, title bar included [OBS issue #541]). Each column
 * resizes from its heading's right edge, 20 to 1000 px, and a click on a
 * heading sorts by it, again to turn it round (`MissingFileTable` ctor
 * @0x1012a5404: every column added with flags 0x1f). The columns keep their
 * widths when the window changes: the list scrolls sideways instead.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { SortDownIcon, SortUpIcon } from "@/components/icons";
import { getBackend } from "@/ipc/client";
import type { MissingColumn, MissingTrack, RelocateSearch } from "@/ipc/types";
import { autoRelocated, listIds, missingAmong, relocateSteps, relocateTracks } from "@/lib/relocate";
import { useTranslation } from "@/i18n";
import styles from "./MissingFileManager.module.css";

/** Rows per request: under the backend's 128-row cap. */
const PAGE = 100;
/** A row's height in CSS px, rekordbox's 25pt pitch. */
const ROW_H = 25;
/** Rows drawn beyond the visible ones, above and below. */
const OVERSCAN = 8;

/**
 * The columns, as `MissingFileTable`'s ctor adds them [OBS rekordbox 7.2.19
 * static @0x1012a5404]: `addColumn(name, id, width, 20, 1000, 0x1f)`. The
 * lower-case headings are rekordbox's own, shown that way on Windows.
 */
const COLUMNS: readonly { key: MissingColumn; label: string; width: number }[] = [
  { key: "title", label: "Track Title", width: 120 },
  { key: "artist", label: "artist", width: 120 },
  { key: "album", label: "album", width: 120 },
  { key: "location", label: "location", width: 240 },
];
const COLUMN_MIN = 20;
const COLUMN_MAX = 1000;
/** `setResizeLimits(400, 300, 1024, 768)`. */
const WINDOW_MIN = { width: 400, height: 300 };
const WINDOW_MAX = { width: 1024, height: 768 };
/** The window's edges and corners, each a resize handle. */
const EDGES = ["n", "s", "e", "w", "ne", "nw", "se", "sw"] as const;
type Edge = (typeof EDGES)[number];
type Box = { left: number; top: number; width: number; height: number };

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), Math.max(min, max));

/**
 * The window dragged by `edge` from `start` by (dx, dy): the edges not held
 * stay put, and the size keeps within rekordbox's limits and the app's
 * window.
 */
export function resizedBox(start: Box, edge: Edge, dx: number, dy: number, view: { width: number; height: number }): Box {
  const maxWidth = Math.min(WINDOW_MAX.width, view.width);
  const maxHeight = Math.min(WINDOW_MAX.height, view.height);
  let { left, top, width, height } = start;
  if (edge.includes("e")) width = clamp(start.width + dx, WINDOW_MIN.width, Math.min(maxWidth, view.width - start.left));
  if (edge.includes("w")) {
    width = clamp(start.width - dx, WINDOW_MIN.width, Math.min(maxWidth, start.left + start.width));
    left = start.left + start.width - width;
  }
  if (edge.includes("s")) height = clamp(start.height + dy, WINDOW_MIN.height, Math.min(maxHeight, view.height - start.top));
  if (edge.startsWith("n")) {
    height = clamp(start.height - dy, WINDOW_MIN.height, Math.min(maxHeight, start.top + start.height));
    top = start.top + start.height - height;
  }
  return { left, top, width, height };
}

/** Which rows the buttons act on: every one, or the ones clicked. */
type Selection = { all: true } | { all: false; ids: ReadonlySet<string>; anchor: number };

const EVERY: Selection = { all: true };

/** Runs the work straight away, for when no job queue is given. */
const now = <T,>(work: () => Promise<T>): Promise<T> => work();

export function MissingFileManager({ readOnly, search, searching = now, onWrote, onFailed, onClose }: {
  /** rekordbox holds the library, or Library Protection is on: nothing is written. */
  readOnly: boolean;
  /** The stored Auto Relocate Search Folders. */
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
  const [widths, setWidths] = useState<readonly number[]>(() => COLUMNS.map((column) => column.width));
  /** The sorted column; none until a heading is clicked, as in rekordbox. */
  const [sort, setSort] = useState<{ column: MissingColumn; forwards: boolean } | null>(null);
  /** Where the window is once it has been resized; centred until then. */
  const [box, setBox] = useState<Box | null>(null);
  /** Sorts run one after another, each over the list the last one left. */
  const sorting = useRef<Promise<void>>(Promise.resolve());
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

  /**
   * Checks every file again. After a change made from the manager the list
   * keeps its sorted order (`keepOrder`), as rekordbox's only loses the
   * rows the change took away.
   */
  const rescan = useCallback(async (keepOrder: boolean) => {
    const id = ++scanId.current;
    requested.current = new Set([0]);
    const backend = await getBackend();
    const first = await backend.missingTracks(0, PAGE, true, keepOrder);
    if (id !== scanId.current) return;
    setTotal(first.total);
    setPages(new Map([[0, first.tracks]]));
    setSelection(EVERY);
    list.current?.scrollTo({ top: 0 });
  }, []);

  useEffect(() => {
    void rescan(false).catch((e: unknown) => onFailed(e instanceof Error ? e.message : String(e)));
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
  /**
   * What the backend is asked to act on: null is every missing track, or
   * the selected ones in the list's order, as rekordbox's `getSelectedRows`
   * gives them.
   */
  const targets = selection.all ? null : inListOrder(selection.ids, pages);

  /**
   * The selected rows in the list's order, each checked to be still missing
   * as Relocate reaches it. With every row selected the whole list's ids are
   * taken before the first chooser (`listIds`).
   */
  const selectedTracks = useCallback(async function* (): AsyncGenerator<MissingTrack> {
    const backend = await getBackend();
    let ids: string[];
    if (selection.all) ids = await listIds(async (offset, limit) => (await backend.missingTracks(offset, limit, false)).tracks, PAGE);
    else ids = inListOrder(selection.ids, pages);
    yield* missingAmong(ids, (some) => backend.relocationTargets(some));
  }, [selection, pages]);

  /**
   * A click on a heading. [OBS rekordbox 7.2.14, chris-win11, issue #541]
   * The first click on a column sorts it forwards (an up arrow), the next
   * turns it round; the list stays scrolled where it was, and the selection
   * stays on the same rows of the list, not the same tracks (JUCE's
   * `ListBox` selects by row).
   */
  const sortBy = (column: MissingColumn) => {
    if (busy || total === null) return;
    const forwards = sort?.column === column ? !sort.forwards : true;
    setSort({ column, forwards });
    const chosenRows: number[] | null = selection.all ? null : [];
    if (chosenRows !== null && !selection.all) {
      for (const [page, rows] of pages) {
        rows.forEach((row, at) => { if (selection.ids.has(row.id)) chosenRows.push(page * PAGE + at); });
      }
    }
    const anchor = selection.all ? 0 : selection.anchor;
    sorting.current = sorting.current.then(async () => {
      const backend = await getBackend();
      await backend.sortMissingTracks(column, forwards);
      const id = ++scanId.current;
      const wanted = new Set<number>();
      for (let page = Math.floor(first / PAGE); page * PAGE < Math.max(last, 1); page++) wanted.add(page);
      for (const row of chosenRows ?? []) wanted.add(Math.floor(row / PAGE));
      const fetched = new Map<number, readonly MissingTrack[]>();
      for (const page of wanted) fetched.set(page, (await backend.missingTracks(page * PAGE, PAGE, false)).tracks);
      if (id !== scanId.current) return;
      requested.current = new Set(wanted);
      setPages(fetched);
      if (chosenRows !== null) {
        const ids = new Set<string>();
        for (const row of chosenRows) {
          const track = fetched.get(Math.floor(row / PAGE))?.[row % PAGE];
          if (track) ids.add(track.id);
        }
        setSelection({ all: false, ids, anchor });
      }
    }).catch((e: unknown) => onFailed(e instanceof Error ? e.message : String(e)));
  };

  /** Drags a heading's right edge: that column alone changes width. */
  const resizeColumn = (index: number, event: React.PointerEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const from = event.clientX;
    const start = widths[index] ?? COLUMN_MIN;
    const move = (e: PointerEvent) => {
      const width = clamp(start + e.clientX - from, COLUMN_MIN, COLUMN_MAX);
      setWidths((current) => current.map((w, at) => (at === index ? width : w)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  /** Drags an edge or corner of the window. */
  const resizeWindow = (edge: Edge, event: React.PointerEvent) => {
    const element = dialog.current;
    if (!element) return;
    event.preventDefault();
    const rect = element.getBoundingClientRect();
    const start: Box = { left: rect.left, top: rect.top, width: rect.width, height: rect.height };
    const from = { x: event.clientX, y: event.clientY };
    const move = (e: PointerEvent) => {
      const view = { width: document.documentElement.clientWidth, height: document.documentElement.clientHeight };
      setBox(resizedBox(start, edge, e.clientX - from.x, e.clientY - from.y, view));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const columnsTemplate = widths.map((width) => `${width}px`).join(" ");
  const tableWidth = widths.reduce((sum, width) => sum + width, 0);

  const run = (action: () => Promise<string | null>) => {
    setBusy(true);
    setNote(null);
    void (async () => {
      try {
        const said = await action();
        if (said !== null) {
          onWrote(said);
          await rescan(true);
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
    const said = await autoRelocated(report, backend, t);
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
        style={{ top: index * ROW_H, gridTemplateColumns: columnsTemplate, width: tableWidth }}
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
      style={box ? { inset: "auto", margin: 0, left: box.left, top: box.top, width: box.width, height: box.height } : undefined}
      aria-labelledby="missing-file-manager-title"
      onCancel={(event) => { event.preventDefault(); onClose(); }}
      onKeyDown={(event) => event.stopPropagation()}
    >
      {EDGES.map((edge) => (
        <span key={edge} className={styles.edge} data-edge={edge} aria-hidden onPointerDown={(event) => resizeWindow(edge, event)} />
      ))}
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
        <div
          ref={list}
          className={styles.list}
          onScroll={(event) => setScroll({ top: event.currentTarget.scrollTop, height: event.currentTarget.clientHeight })}
        >
          <div className={styles.header} role="row" style={{ gridTemplateColumns: columnsTemplate, width: tableWidth }}>
            {COLUMNS.map((column, index) => {
              const sorted = sort?.column === column.key ? sort : null;
              return (
                <span
                  key={column.key}
                  className={`${styles.cell} ${styles.heading}`}
                  role="columnheader"
                  aria-sort={sorted ? (sorted.forwards ? "ascending" : "descending") : "none"}
                  data-sorted={sorted ? true : undefined}
                  onClick={() => sortBy(column.key)}
                >
                  <span className={styles.label}>{column.label}</span>
                  {sorted ? (sorted.forwards
                    ? <SortUpIcon className={styles.sortArrow} />
                    : <SortDownIcon className={styles.sortArrow} />) : null}
                  <span
                    className={styles.grip}
                    role="separator"
                    aria-orientation="vertical"
                    aria-label={t("Resize {column}", { column: column.label })}
                    onPointerDown={(event) => resizeColumn(index, event)}
                    onClick={(event) => event.stopPropagation()}
                  />
                </span>
              );
            })}
          </div>
          <div className={styles.rows} style={{ height: (total ?? 0) * ROW_H, width: tableWidth }}>{rows}</div>
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

/** `ids` in the order the list shows them, as far as its pages are loaded. */
function inListOrder(ids: ReadonlySet<string>, pages: ReadonlyMap<number, readonly MissingTrack[]>): string[] {
  const order = new Map<string, number>();
  for (const [page, rows] of pages) rows.forEach((row, at) => order.set(row.id, page * PAGE + at));
  return [...ids].sort((a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0));
}
