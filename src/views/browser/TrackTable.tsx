/**
 * The track browser.
 *
 * Column model and widths come from rekordbox's own `TableHeader-<Context>`
 * layout (design/measure/column-map.json); geometry comes from the measured
 * tokens. Rows are virtualized and keyed by row id so a re-sort moves DOM nodes
 * instead of rewriting every cell.
 */
import { dragTracksToDesktop, nativeTrackDragging } from "@/ipc/client";
import { SearchField } from "@/components/SearchField";
import { reportStartupPaint } from "@/lib/startup";
import { useEventCallback } from "@/store/useEventCallback";
import { TRACK_SEARCH_OPTIONS, type TrackSearchField } from "@/lib/search";

import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { DeckId, RowDto, SortColumn, TrackField, ViewSpec } from "@/ipc/types";
import { useTrackView, type PendingEdits, type Seed } from "@/store/useTrackView";
import { PAGE_SIZE } from "@/lib/rowCache";
import { SEEDED_ROWS } from "@/lib/session";
import { formatBitrate, formatBpm, formatBytes, formatDuration, formatShortDate } from "@/lib/format";
import {
  applyClick, clickSettles, emptySelection, inListOrder, pressModifier, pressSelects, selectAll, selectedTracks,
  type SelectionState,
} from "@/lib/selection";
import { ContextMenu } from "@/components/ContextMenu";
import {
  deleteKeyAction, deviceTrackMenu, MISSING_TRACK_MENU, MISSING_TRACK_TITLE, trackMenuFor, type MenuTarget,
} from "@/lib/contextMenus";
import { hasLooseId } from "@/lib/explorer";
import { previewFromClick, WaveformPreview } from "./WaveformPreview";
import styles from "./TrackTable.module.css";
import { FilterIcon, SortDownIcon, SortUpIcon } from "@/components/icons";
import { Artwork } from "@/components/Artwork";
import { RatingStar } from "@/components/RatingStar";
import { RecordIcon } from "@/components/icons";
import { EXTRA_COLUMNS, reorderTarget, type ColumnKey, type ColumnSpec } from "@/lib/columns";
import { COLOR_NAMES } from "@/lib/trackFilter";
import { browseListVars, browseScale, formatKey } from "@/lib/preferences";
import { trafficLightLit, type TrafficLightReach } from "@/lib/camelot";
import { TickIcon } from "@/components/icons";
import type { TrafficLightSource } from "@/lib/session";
import { usePreferences, useTooltip } from "@/store/usePreferences";
import type { KeyDisplay } from "@/ipc/types";
import { ColumnMenu } from "./ColumnMenu";
import { setRowDragImage } from "./dragGhost";
import { detectPlatform, dispatch, isTyping } from "@/lib/shortcuts";

const ROW_H = 25; // --s-row-height
/**
 * A removal the list asks for: resolves true once the tracks are gone, false
 * when the person declined or the write was refused.
 */
type RemoveTracks = (ids: readonly string[]) => Promise<boolean>;
/** One frozen empty list, so a row without cues does not re-render for a new one. */
const NO_CUES: RowDto["hotCues"] = [];
/** macOS, where a Control-click is the context menu's press, not a toggle. */
const MAC = detectPlatform().mac;
/**
 * Rows to fetch beyond the rendered window in each direction, so a fast scroll
 * lands on pages that are already cached instead of on blank rows. Two pages
 * each way: the cache holds ~100 pages and `planFetches` caps how many requests
 * go out per tick, so this cannot flood on a flick, and `missingPages` skips the
 * pages behind that are still cached — the fetches skew to the way of travel.
 */
const PREFETCH_MARGIN = PAGE_SIZE * 2;
/** A few bar widths, picked per cell so loading rows do not read as a rigid grid. */
const SKELETON_WIDTHS = ["42%", "66%", "54%"];
/// --s-col-header-h. The column header sits inside the scroller so it moves
/// with the rows horizontally, which costs it this much of the vertical scroll.
const COL_HEADER_H = 24;
/// --s-preview-band-h: the strip the row's waveform and its cue badges share.
/// The canvas needs the number for its backing store; the cell's CSS places it.
const PREVIEW_BAND_H = 15;

export type Column = ColumnSpec;

/**
 * The grid track list, with a trailing `1fr` that absorbs whatever the window
 * has spare so the table fills the width instead of stopping at the sum of its
 * columns. Below that sum the row's `min-width` takes over and the scroller
 * does its job.
 */
function gridOf(columns: readonly ColumnSpec[]): string {
  return `${columns.map((c) => `${c.width}px`).join(" ")} 1fr`;
}

function totalWidthOf(columns: readonly ColumnSpec[]): number {
  return columns.reduce((a, c) => a + c.width, 0);
}

/** The header row's headings, leaving out the floating copy of a dragged one. */
function headingsOf(head: HTMLElement): Element[] {
  return [...head.children].filter((cell) => cell.getAttribute("role") === "columnheader");
}

export function cellText(row: RowDto, key: Column["key"]): string {
  const extra = row.extra ?? {};
  const value = extra[key];
  switch (key) {
    case "trackNo": return String(row.trackNo);
    case "title": return row.title;
    case "artist": return row.artist;
    case "album": return row.album;
    case "genre": return row.genre;
    case "label": return row.label;
    case "comment": return row.comment;
    case "key": return row.key;
    case "bpm": return formatBpm(row.bpmX100);
    case "duration": return formatDuration(row.durationSec);
    case "dateAdded": return formatShortDate(row.dateAdded);
    case "releaseDate": return formatShortDate(row.releaseDate);
    case "rating": return "";
    case "fileName": return row.fileName ?? "";
    case "hotCue": return row.hotCues.map(([letter]) => letter).join(", ");
    case "size": return typeof value === "number" && value > 0 ? formatBytes(value) : "";
    case "dateCreated": return typeof value === "string" ? formatShortDate(value) : "";
    case "fileType": {
      const types: Record<number, string> = { 1: "MP3", 4: "M4A", 5: "FLAC", 6: "M4A", 11: "WAV", 12: "AIFF" };
      return typeof value === "number" ? types[value] ?? (value ? String(value) : "") : "";
    }
    case "color": return typeof value === "number" && value > 0 ? COLOR_NAMES[value - 1] ?? "" : "";
    case "publishTrackInfo": return value === true ? "On" : value === false ? "Off" : "";
    case "cloud": return value === true ? "Cloud" : "";
    case "sampleRate": return typeof value === "number" && value > 0 ? `${value / 1000} kHz` : "";
    case "bitrate": return typeof value === "number" ? formatBitrate(value) : "";
    case "bitDepth": return typeof value === "number" && value > 0 ? `${value} bit` : "";
    case "year": case "discNo": case "djPlayCount": case "trackNumber":
      return typeof value === "number" && value > 0 ? String(value) : "";
    case "albumArtist": case "composer": case "lyricist": case "mixName": case "remixer":
    case "originalArtist": case "location": case "message": case "myTag":
      return typeof value === "string" ? value : "";
    default: return "";
  }
}

/**
 * The columns that can be typed over in the list, and the field each writes.
 *
 * Plain text that the writer takes as given, and only columns whose cells are
 * not the gesture for something else.
 *
 * The title edits on a second single click; its double click still loads
 * the track into the deck.
 *
 * `key` is left out too — it is checked against the keys the library already
 * holds, so a free-typed one would be refused after the fact, and the
 * information panel offers the list instead. The dates are formatted on the
 * way out and would have to be parsed back on the way in. `bpm` is typed
 * over as a number, which the backend reads back and retimes the beat grid
 * to, so the CDJ and the column agree.
 */
const EDITABLE_FIELDS: Partial<Record<ColumnKey, TrackField>> = {
  title: "title",
  artist: "artist",
  album: "album",
  genre: "genre",
  label: "label",
  bpm: "bpm",
};

const Stars = memo(function Stars({
  rating, onRate,
}: {
  rating: number;
  onRate?: (stars: number) => void;
}) {
  if (onRate) {
    return (
      <span className={styles.stars} role="radiogroup" aria-label="Rating">
        {[1, 2, 3, 4, 5].map((star) => (
          <button
            key={star}
            type="button"
            className={styles.star}
            role="radio"
            aria-checked={rating === star}
            aria-label={`${star} of 5`}
            // Clicking the star already set clears the rating, which is how
            // rekordbox behaves and the only way to get back to none.
            onMouseDown={(e) => e.stopPropagation()}
            // Two quick clicks on a star are two ratings, not a request to
            // play the track.
            onDoubleClick={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              onRate(rating === star ? 0 : star);
            }}
          >
            <RatingStar lit={star <= rating} className={styles.starIcon} />
          </button>
        ))}
      </span>
    );
  }
  return (
    <span className={styles.stars} role="img" aria-label={`${rating} of 5`}>
      {[1, 2, 3, 4, 5].map((star) => <RatingStar key={star} lit={star <= rating} className={styles.starIcon} />)}
    </span>
  );
});

/**
 * A cell that turns into a text box on the configured editing gesture.
 *
 * Committing on blur as well as Enter matters: clicking away is how people
 * leave a field, and losing the edit then is the behaviour everyone hates.
 * Escape abandons it, which is the escape hatch that makes committing on blur
 * safe.
 */
const EditableCell = memo(function EditableCell({
  value, label, col, onCommit, onClick, tip, doubleClickLoads = false, onEditBlocked,
}: {
  value: string;
  label: string;
  /** The column this cell belongs to, which its width and alignment key off. */
  col: string;
  onCommit: (next: string) => void;
  onEditBlocked?: (() => void) | undefined;
  /**
   * Edit Library › Double-click to edit is off: a click on this cell of a
   * row that is already selected opens it, as in rekordbox. On, and only a
   * double click does.
   */
  onClick: boolean;
  /** Titles retain the row's double-click gesture for loading a track. */
  doubleClickLoads?: boolean;
  tip: string | undefined;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value);
  // Whether the row was selected when the button went down. The press
  // selects the row and React re-renders before the click arrives, so by
  // then `onClick` says "selected" for a row the same gesture selected —
  // and a click that selects must not also open the editor.
  const pressedOnSelected = useRef(false);
  const editTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const cancelPendingEdit = useCallback(() => {
    if (editTimer.current !== null) clearTimeout(editTimer.current);
    editTimer.current = null;
    document.removeEventListener("pointerdown", cancelPendingEdit, true);
    document.removeEventListener("keydown", cancelPendingEdit, true);
    document.removeEventListener("dragstart", cancelPendingEdit, true);
    window.removeEventListener("blur", cancelPendingEdit);
  }, []);
  // A pending edit must not follow a changed selection, lock, or recycled row.
  useEffect(() => cancelPendingEdit, [cancelPendingEdit, onClick, onEditBlocked, value]);

  if (!editing) {
    const begin = () => {
      cancelPendingEdit();
      if (onEditBlocked) {
        onEditBlocked();
        return;
      }
      // Only a title needs a grace period: its double click belongs to the
      // row and loads the track. Every other field owns its double click, so
      // deferring the editor only creates a window in which another pointer
      // press can cancel a valid second-click edit.
      if (!doubleClickLoads) {
        setDraft(value);
        setEditing(true);
        return;
      }
      document.addEventListener("pointerdown", cancelPendingEdit, true);
      document.addEventListener("keydown", cancelPendingEdit, true);
      document.addEventListener("dragstart", cancelPendingEdit, true);
      window.addEventListener("blur", cancelPendingEdit);
      editTimer.current = setTimeout(() => {
        cancelPendingEdit();
        setDraft(value);
        setEditing(true);
      }, 300);
    };
    return (
      <div
        className={styles.cell}
        data-col={col}
        role="gridcell"
        onMouseDown={(e) => {
          pressedOnSelected.current = onClick && e.button === 0 && !e.shiftKey && !e.metaKey && !e.ctrlKey;
        }}
        onClick={onClick ? (e) => {
          if (doubleClickLoads && e.detail > 1) return;
          if (pressedOnSelected.current) begin();
        } : undefined}
        onDoubleClick={(e) => {
          cancelPendingEdit();
          // Only a title cell hands its double click to the row, to load the
          // track — every other editable cell opens on it regardless of
          // click-to-edit, including the double click that both selects an
          // unselected row and opens the cell in the same gesture (the first
          // click's selection flips `onClick` true before the second click
          // lands, which used to make this handler defer to the row here).
          if (doubleClickLoads) return;
          e.stopPropagation();
          begin();
        }}
        title={tip}
      >
        {value}
      </div>
    );
  }
  return (
    <div
      className={styles.cell}
      data-col={col}
      role="gridcell"
      // A second click on a cell that has just opened is not a request to
      // play the track either.
      onDoubleClick={(e) => {
        if (doubleClickLoads) {
          setEditing(false);
        } else {
          e.stopPropagation();
        }
      }}
    >
      <input
        className={styles.editor}
        value={draft}
        readOnly={onEditBlocked !== undefined}
        aria-label={label}
        autoFocus
        onMouseDown={(e) => e.stopPropagation()}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          setEditing(false);
          if (draft !== value) {
            if (onEditBlocked) onEditBlocked();
            else onCommit(draft);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.currentTarget.blur();
          } else if (e.key === "Escape") {
            // Abandon: set the draft back first so the blur commits nothing.
            setDraft(value);
            setEditing(false);
          }
          e.stopPropagation();
        }}
      />
    </div>
  );
});

const TrackRow = memo(function TrackRow({
  row, top, selected, onSelect, onOpen, onDragStart, onDragEnd, index, columns, onRate,
  onComment, onEditField, onEditBlocked, onMenu, keyDisplay, previewCues, clickToEdit, tooltips, trafficKey, trafficReach,
  reorderable, isLocalDrag, dropEdge, onReorderOver, onReorderDrop, startupCache,
}: {
  row: RowDto | undefined;
  top: number;
  selected: boolean;
  index: number;
  columns: readonly ColumnSpec[];
  /** Preferences: `Ebm` or `2A`. */
  keyDisplay: KeyDisplay;
  /** Preferences: the hot cue badges over the row's waveform. */
  previewCues: boolean;
  /** Preferences: a comment opens on a click rather than a double click. */
  clickToEdit: boolean;
  tooltips: boolean;
  /** The Traffic Light: the key rows light against, and how far around it. Null lights nothing. */
  trafficKey: string | null;
  trafficReach: TrafficLightReach;
  onSelect: (index: number, id: string, e: React.MouseEvent) => void;
  /** Load the track into the player. A double-click, as in rekordbox. */
  onOpen: (index: number) => void;
  onDragStart: (row: RowDto) => boolean;
  /** Set the track's rating. Absent in a build that cannot write. */
  onRate: ((id: string, stars: number) => void) | undefined;
  /** Set the track's comment. */
  onComment: ((id: string, comment: string) => void) | undefined;
  /** Write a metadata field typed over in the row. */
  onEditField: ((id: string, field: TrackField, value: string) => void) | undefined;
  onEditBlocked?: (() => void) | undefined;
  onMenu: (index: number, row: RowDto, at: { x: number; y: number }) => void;
  onDragEnd: () => void;
  /** The list can be reordered by hand, so a drop here means something. */
  reorderable: boolean;
  /** A drag from the other browser copies into this playlist, never reorders it. */
  isLocalDrag: () => boolean;
  /** Which edge the line is drawn on, or null for a row that is not the target. */
  dropEdge: "above" | "below" | null;
  onReorderOver: (index: number, below: boolean) => void;
  onReorderDrop: () => void;
  startupCache: boolean;
}) {
  const nativePress = useRef<{ x: number; y: number } | null>(null);
  const suppressClick = useRef(false);

  if (!row) {
    // A skeleton, not a blank: while the page is in flight a dim bar stands in
    // for each text cell, so a fast scroll reads as content loading rather than
    // torn, empty rows. The box keeps the exact height and the column dividers,
    // so nothing shifts sideways on arrival; the row carries no row/gridcell
    // roles, so it stays invisible to anything selecting real rows. Static, not
    // pulsing: hundreds of these can be on screen mid-flick, where extra
    // animated layers would fight the scroll we are trying to smooth.
    return (
      <div
        className={styles.row}
        data-even={index % 2 === 1 || undefined}
        style={{ transform: `translate3d(0, ${top}px, 0)` }}
        aria-hidden
      >
        {columns.map((col, at) => {
          if (col.key === "artwork") return <div key={col.key} className={styles.artwork} />;
          if (col.key === "preview") return <div key={col.key} className={styles.preview} />;
          if (col.key === "attr") return <div key={col.key} className={styles.attr} />;
          const cls = col.align === "right" ? `${styles.cell} ${styles.right}` : styles.cell;
          return (
            <div key={col.key} className={cls}>
              {col.key === "rating" ? null : (
                <span
                  className={styles.skeleton}
                  style={{ width: SKELETON_WIDTHS[(index + at) % SKELETON_WIDTHS.length] }}
                />
              )}
            </div>
          );
        })}
      </div>
    );
  }
  return (
    <div
      className={styles.row}
      data-selected={selected || undefined}
      data-even={index % 2 === 1 || undefined}
      style={{ transform: `translate3d(0, ${top}px, 0)` }}
      onMouseDown={(e) => {
        if (pressSelects(e, selected, MAC)) onSelect(index, row.id, e);
      }}
      onPointerDown={(e) => {
        suppressClick.current = false;
        if (nativeTrackDragging() && e.button === 0 &&
            !(e.target as HTMLElement).closest("input, button, [contenteditable=true]")) {
          nativePress.current = { x: e.clientX, y: e.clientY };
        }
      }}
      onPointerMove={(e) => {
        const press = nativePress.current;
        if (!press) return;
        if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return;
        nativePress.current = null;
        suppressClick.current = true;
        e.preventDefault();
        onDragStart(row);
      }}
      onPointerUp={() => { nativePress.current = null; }}
      onPointerCancel={() => { nativePress.current = null; }}
      // The plain click a press on a selected row held back, now that the
      // release has shown it was not a drag.
      onClick={(e) => {
        if (suppressClick.current) return;
        if (clickSettles(e, selected, MAC)) onSelect(index, row.id, e);
        // A click on the waveform also previews the track from there; the
        // row is selected as well, as rekordbox's is.
        if (row.analysed) previewFromClick(e, row.id, row.durationSec, previewCues ? row.hotCues : NO_CUES);
      }}
      onDoubleClick={() => onOpen(index)}
      onContextMenu={(e) => {
        e.preventDefault();
        onMenu(index, row, { x: e.clientX, y: e.clientY });
      }}
      draggable={!nativeTrackDragging()}
      onDragStart={(e) => {
        if (onDragStart(row)) {
          // Replace the HTML source with a native file source. Destinations
          // inside the app still receive normal DOM drag/drop events.
          e.preventDefault();
          return;
        }
        // Both, because the same drag has two meanings: copied into a playlist
        // or a deck, moved within the list it came from. A `dropEffect` the
        // `effectAllowed` does not cover is an invalid pair, and the browser
        // silently refuses the drop — which is what swallowed the reorder.
        e.dataTransfer.effectAllowed = "copyMove";
        // Firefox will not start a drag without payload.
        e.dataTransfer.setData("text/plain", row.id);
        // A faded copy of the row travels with the hand, every time: the
        // browser's own snapshot of a virtualised row does not — see
        // `dragGhost.ts`.
        setRowDragImage(e.currentTarget, e.dataTransfer, { x: e.clientX, y: e.clientY });
      }}
      // A drag that is let go over nothing still ends. Without this the tree
      // kept offering its playlists as targets afterwards.
      onDragEnd={() => onDragEnd()}
      onDragOver={(e) => {
        if (!reorderable || !isLocalDrag()) return;
        // Taking the event is what lets the drop happen at all; the browser
        // refuses one over an element that did not ask for it.
        e.preventDefault();
        e.dataTransfer.dropEffect = nativeTrackDragging() ? "copy" : "move";
        const box = e.currentTarget.getBoundingClientRect();
        onReorderOver(index, e.clientY > box.top + box.height / 2);
      }}
      onDrop={(e) => {
        // External files must bubble to the playlist's import target.
        if (!reorderable || !isLocalDrag() || dropEdge === null) return;
        e.preventDefault();
        e.stopPropagation();
        onReorderDrop();
      }}
      data-drop={dropEdge ?? undefined}
      role="row"
      aria-selected={selected}
    >
      {columns.map((col) => {
        if (col.key === "attr") {
          return (
            <div key={col.key} className={styles.attr} data-col={col.key} role="gridcell">
              {row.analysed ? (
                <span className={styles.analysed} title={tooltips ? "Analyzed" : undefined} />
              ) : null}
              {row.missing === true ? (
                // rekordbox's orange [!], where CUE would be: no missing row
                // in either capture shows CUE beside it [OBS issue #201].
                <span className={styles.missing} role="img" aria-label="File is Missing"
                  title={tooltips ? "File is Missing" : undefined} />
              ) : (
                <span className={styles.cue}>{row.hotCues.length > 0 ? "CUE" : ""}</span>
              )}
            </div>
          );
        }
        if (col.key === "artwork") {
          return (
            <div key={col.key} className={styles.artwork} data-col={col.key} role="gridcell">
              {/*
                The record sits underneath as the fallback, the way rekordbox
                draws a cell without artwork: a little under half the
                reference library has none, and it also covers the gap while
                the image decodes. `loading="lazy"` keeps a fast scroll from
                queueing a fetch for every row it passes.
              */}
              <RecordIcon className={styles.record} aria-hidden />
              {row.hasArtwork ? (
                <Artwork trackId={row.id} className={styles.artworkImage} lazy />
              ) : null}
            </div>
          );
        }
        if (col.key === "comment" && onComment) {
          return (
            <EditableCell
              key={col.key}
              value={row.comment}
              onEditBlocked={onEditBlocked}
              label="Comment"
              col={col.key}
              onCommit={(next) => onComment(row.id, next)}
              // A click edits only a selected row's comment; on any other
              // row the click is a selection, as it has to be.
              onClick={clickToEdit && selected}
              tip={
                tooltips
                  ? clickToEdit
                    ? "Comment — click to edit"
                    : "Comment — double-click to edit"
                  : undefined
              }
            />
          );
        }
        if (onEditField) {
          const field = EDITABLE_FIELDS[col.key];
          if (field) {
            return (
              <EditableCell
                key={col.key}
                value={cellText(row, col.key)}
                onEditBlocked={onEditBlocked}
                label={col.label}
                col={col.key}
                onCommit={(next) => onEditField(row.id, field, next)}
                onClick={(clickToEdit || col.key === "title") && selected}
                doubleClickLoads={col.key === "title"}
                tip={
                  tooltips
                    ? `${col.label} — ${clickToEdit || col.key === "title" ? "click" : "double-click"} to edit`
                    : undefined
                }
              />
            );
          }
        }
        if (col.key === "preview") {
          return (
            <div key={col.key} className={styles.preview} data-col={col.key} role="gridcell">
              {row.analysed ? (
                <WaveformPreview
                  trackId={row.id}
                  width={col.width - 6}
                  height={PREVIEW_BAND_H}
                  hotCues={previewCues ? row.hotCues : NO_CUES}
                  memoryCues={previewCues ? row.memoryCues : undefined}
                  durationSec={row.durationSec}
                  startupCache={startupCache}
                />
              ) : null}
            </div>
          );
        }
        if (col.key === "rating") {
          return (
            <div key={col.key} className={styles.cell} data-col={col.key} role="gridcell">
              <Stars rating={row.rating} onRate={(stars) => onEditBlocked ? onEditBlocked() : onRate?.(row.id, stars)} />
            </div>
          );
        }
        if (col.key === "color") {
          // rekordbox's ListViewer::paintColourColumn: the colour's ball, then
          // its name; a track with no colour leaves the cell empty.
          const name = cellText(row, col.key);
          return (
            <div key={col.key} className={`${styles.cell} ${styles.colorCell}`} data-col={col.key} role="gridcell">
              {name ? <span className={styles.colorBall} data-color={name} aria-hidden /> : null}
              {name}
            </div>
          );
        }
        if (col.key === "key") {
          // The Traffic Light: a key that goes with the loaded track's is lit.
          const lit = trafficKey !== null && trafficLightLit(row.key, trafficKey, trafficReach);
          return (
            <div
              key={col.key}
              className={styles.cell}
              data-col={col.key}
              data-lit={lit || undefined}
              role="gridcell"
            >
              {formatKey(row.key, keyDisplay)}
            </div>
          );
        }
        return (
          <div
            key={col.key}
            className={col.align === "right" ? `${styles.cell} ${styles.right}` : styles.cell}
            data-col={col.key}
            role="gridcell"
          >
            {cellText(row, col.key)}
          </div>
        );
      })}
    </div>
  );
});

/**
 * A drag out of the track list.
 *
 * `ids` is what a playlist would take — the selection, or the one row grabbed
 * from outside it. `row` is the row under the hand, which is what a deck
 * takes: a deck holds one track, and dropping four onto it has no meaning.
 */
export interface TrackDrag {
  /** Every track travelling: the selection, or the one row grabbed outside it. */
  ids: readonly string[];
  /**
   * The one a deck takes: the topmost of them in the list, as rekordbox
   * loads the first of a dropped selection rather than the row the hand
   * was on [OBS].
   */
  row: RowDto;
}

export interface TrackTableProps {
  spec: ViewSpec;
  onSortChange: (column: SortColumn) => void;
  onSelectionChange?: (count: number) => void;
  title: string;
  /** The live search text. Rust does the filtering; this is only the box. */
  query: string;
  searchField?: TrackSearchField;
  onSearchFieldChange?: (field: TrackSearchField) => void;
  onQueryChange: (query: string) => void;
  /** Visible columns, in order, at their current widths. */
  columns: readonly ColumnSpec[];
  onColumnMove: (key: ColumnKey, to: number) => void;
  onColumnResize: (key: ColumnKey, width: number) => void;
  onColumnToggle: (key: ColumnKey) => void;
  onColumnAutoSize: (key: ColumnKey) => void;
  onColumnAutoSizeAll: () => void;
  /** The row the player should show, as the selection moves. */
  onFocusedRow?: (row: RowDto | null) => void;
  /**
   * The single selected row, or `null` when the selection is not one row.
   *
   * Selecting a track does not load it — arrowing down a playlist would load
   * every track on the way past — but an empty deck can be clicked to take
   * whatever is selected, and this is what it takes.
   */
  onSelectedRow?: (row: RowDto | null) => void;
  /** What is being dragged, so a drop target knows what it would get. */
  onDragTracks?: (drag: TrackDrag | null) => void;
  /** Accept tracks dragged from the other browser into this playlist. */
  dragging?: boolean;
  onDropTracks?: ((playlistId: string) => void) | undefined;
  /**
   * Files dragged in from outside the app (Finder, Explorer) and dropped
   * anywhere in the list. Absent unless the open view is a playlist tracks
   * can actually be added to.
   */
  onDropFiles?: ((files: File[]) => void) | undefined;
  onDragError?: ((message: string) => void) | undefined;
  /** Edit a track's rating or comment. Absent where writes are impossible. */
  onRate?: (id: string, stars: number) => void;
  onComment?: (id: string, comment: string) => void;
  /**
   * Write the playlist's new order, dragged by hand.
   *
   * Absent unless the rows can be reordered at all: only a playlist has an
   * order of its own to change, and only while it is being shown in that
   * order rather than sorted by a column.
   */
  onReorder?: ((order: readonly string[]) => void) | undefined;
  /**
   * Write a metadata field typed over in the list.
   *
   * Absent where the library cannot be written, which is what leaves the
   * cells as plain text rather than offering an edit that would be refused.
   */
  onEditField?: ((id: string, field: TrackField, value: string) => void) | undefined;
  onEditBlocked?: (() => void) | undefined;
  /** Bumped when the library changes, so cached pages are dropped. */
  libraryGeneration?: number;
  /** Edits shown before the backend has caught up. */
  pendingEdits?: PendingEdits;
  /**
   * Rows on their way into this list from a running operation, drawn as
   * skeletons after the last real row until they arrive.
   */
  pendingRows?: number;
  /** The rows behind the selection, for queueing analysis. */
  onSelectedTracks?: (tracks: { id: string; title: string }[]) => void;
  /** Analyse whatever is selected. */
  onAnalyse?: () => void;
  /** Right-click actions the table cannot do itself. */
  onShowInformation?: (row: RowDto) => void;
  onShowInFinder?: (row: RowDto) => void;
  onRemoveFromPlaylist?: RemoveTracks;
  onRemoveFromHistory?: RemoveTracks;
  onResetPlayCount?: (ids: readonly string[]) => void;
  /** Convert Memory Cues to Hot Cues, on the row under the pointer. */
  onConvertMemoryCues?: (row: RowDto) => void;
  onRemoveFromCollection?: RemoveTracks;
  /** Auto Relocate, from a missing track's menu: the selected tracks. */
  onAutoRelocate?: (ids: readonly string[]) => void;
  /**
   * Relocate, from a missing track's menu: every selected track, in list
   * order, as rekordbox's `popupEventRelocateTrack` takes them.
   */
  onRelocate?: (ids: readonly string[]) => void;
  /** Import To Collection: the Explorer's files, by their `file:` ids. */
  onImportToCollection?: (ids: readonly string[]) => void;
  /** Analysis Lock › Lock and Unlock. */
  onAnalysisLock?: (ids: readonly string[], on: boolean) => void;
  /** Add To Playlist › one of `playlists`. */
  onAddToPlaylist?: (playlist: string, ids: readonly string[]) => void;
  onAddToTagList?: (ids: readonly string[]) => void;
  onRemoveFromTagList?: RemoveTracks;
  /** Reload Tag: the files' tags read again. */
  onReloadTag?: (ids: readonly string[]) => void;
  /** Export Track › one of `devices`. */
  onExportTrack?: (device: string, ids: readonly string[]) => void;
  /** What Add To Playlist and Export Track offer. */
  playlists?: readonly MenuTarget[];
  /**
   * The menu over a stick's own tracks, when the view is one of its
   * libraries: Add To Playlist naming that library's playlists, and in one
   * of them Remove from Playlist.
   */
  deviceMenu?: {
    playlists: readonly MenuTarget[];
    inPlaylist: boolean;
    /** The stick is being synced, exported or ejected: its edits are greyed. */
    busy: boolean;
    onAdd: (playlist: string, ids: readonly string[]) => void;
    onRemove: (ids: readonly string[]) => void;
  } | undefined;
  devices?: readonly MenuTarget[];
  /** rekordbox is running, so every write is refused rather than raced. */
  readOnly?: boolean;
  /**
   * How many players the layout is drawing, so the menu offers those and no
   * others: a player that is not on screen has nowhere to put a track.
   */
  players?: number;
  /** Load a track into a deck, from the menu. */
  onLoadTrack?: (deck: DeckId, row: RowDto) => void;
  /** The last run's rows, drawn until the backend answers. */
  seed?: Seed | undefined;
  /**
   * The first rows of the current view, once they exist.
   *
   * The app keeps them only to write the next start's opening screen — it
   * still never holds the library, just the handful of rows that were on it.
   */
  onFirstRows?: (rows: RowDto[], count: number) => void;
  /** Lets the keyboard shortcut put the caret here from anywhere. */
  searchRef?: React.RefObject<HTMLInputElement | null>;
  /**
   * The Track Filter, opt-in: the header's filter button and the bar it drops
   * down. The main browser passes these; the sub-browser has neither.
   */
  filterOpen?: boolean;
  onToggleFilter?: () => void;
  /** The bar itself, drawn between the header and the column header. */
  filterBar?: React.ReactNode;
  /**
   * The Traffic Light: which deck the browser reads, the MASTER menu above
   * the list, and the key of the track on it. The main browser passes these;
   * the sub-browser has no menu and lights nothing.
   */
  trafficLight?: TrafficLightSource;
  onTrafficLight?: (source: TrafficLightSource) => void;
  trafficKey?: string | null;
}

/** The MASTER menu's rows, in rekordbox's wording [OBS]. */
const TRAFFIC_SOURCES: readonly { id: TrafficLightSource; label: string; short: string }[] = [
  { id: "master", label: "MASTER DECK - Traffic Light", short: "MASTER" },
  { id: "a", label: "PLAYER A - Traffic Light", short: "PLAYER A" },
  { id: "b", label: "PLAYER B - Traffic Light", short: "PLAYER B" },
];

export const TrackTable = memo(function TrackTable({
  spec, onSortChange, onSelectionChange, title, query, onQueryChange, searchRef, searchField = "all", onSearchFieldChange,
  columns, onColumnMove, onColumnResize, onColumnToggle, onColumnAutoSize,
  onColumnAutoSizeAll, onFocusedRow, onDragTracks, dragging = false, onDropTracks, onDropFiles, onDragError, onRate, onComment, onReorder, onEditField, onEditBlocked, seed, onFirstRows,
  libraryGeneration, pendingEdits, pendingRows = 0, onSelectedTracks, onAnalyse,
  onShowInformation, onShowInFinder, onRemoveFromPlaylist, onRemoveFromHistory, onResetPlayCount,
  onRemoveFromCollection, onAutoRelocate, onRelocate, onConvertMemoryCues, readOnly = false,
  onImportToCollection, onAnalysisLock, onAddToPlaylist, onAddToTagList, onRemoveFromTagList, onExportTrack, onReloadTag,
  playlists = [], devices = [], deviceMenu,
  players = 0, onLoadTrack, onSelectedRow, filterOpen = false, onToggleFilter, filterBar,
  trafficLight, onTrafficLight, trafficKey = null,
}: TrackTableProps) {
  const extraColumns = useMemo(() => EXTRA_COLUMNS.filter((key) => columns.some((column) => column.key === key)), [columns]);
  const view = useTrackView(spec, libraryGeneration, pendingEdits, seed, extraColumns);
  const preferences = usePreferences();
  const { keyDisplay, previewCueMarkers, tooltips } = preferences.view;
  const tip = useTooltip();
  // The MASTER menu, open or not. Closed by anything outside it, as every
  // other menu here is.
  const [trafficMenu, setTrafficMenu] = useState(false);
  const trafficBox = useRef<HTMLDivElement>(null);
  // A file dragged in from outside the app, hovering the list.
  const [fileOver, setFileOver] = useState(false);
  useEffect(() => {
    if (!trafficMenu) return;
    const onDown = (event: MouseEvent) => {
      if (!trafficBox.current?.contains(event.target as Node)) setTrafficMenu(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setTrafficMenu(false);
    };
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [trafficMenu]);
  const clickToEdit = !preferences.advanced.doubleClickToEdit;
  // Browse › FontSize and Line Space scale the measured tokens; the
  // virtualizer has to be told the same height the CSS draws.
  const rowH = Math.round(ROW_H * browseScale(preferences.view.browseLineSpace));

  // Hand the top of the view up once it is real, for the next start's opening
  // screen. Only the first page, and only when it is filled.
  const reported = useRef("");
  useEffect(() => {
    if (!onFirstRows || view.loading || view.count === 0) return;
    const first = view.rowAt(0);
    if (!first) return;
    reportStartupPaint("first-rows-painted");
    const stamp = `${view.token}:${view.count}:${first.id}`;
    if (reported.current === stamp) return;
    reported.current = stamp;
    const rows: RowDto[] = [];
    for (let i = 0; i < Math.min(view.count, SEEDED_ROWS); i++) {
      const row = view.rowAt(i);
      if (!row) break;
      rows.push(row);
    }
    onFirstRows(rows, view.count);
  }, [onFirstRows, view]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const rowsRef = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<SelectionState>(emptySelection);
  const [dragKey, setDragKey] = useState<ColumnKey | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; target: ColumnKey } | null>(null);
  // Live during a header-edge drag. A ref, not state: this updates per
  // mousemove and re-rendering the table on each would be a frame's work.
  const resizing = useRef<{ key: ColumnKey; x: number; width: number } | null>(null);
  // A heading drag in progress, and whether it passed the threshold. `grab`
  // is where in the heading it was taken, so the floating copy stays under
  // the pointer at the same place.
  const reorder = useRef<{ key: ColumnKey; x: number; grab: number; moved: boolean } | null>(null);
  // Set when a drag finishes, so the click that follows does not also sort.
  const draggedRef = useRef(false);
  const headRef = useRef<HTMLDivElement>(null);
  // The floating copy of the heading being dragged. Moved by its own style
  // on every mousemove rather than through state, so following the pointer
  // costs no render.
  const ghostRef = useRef<HTMLDivElement>(null);
  const ghostLeft = useRef(0);
  const columnsRef = useRef(columns);
  columnsRef.current = columns;

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const drag = resizing.current;
      if (drag) {
        onColumnResize(drag.key, drag.width + (e.clientX - drag.x));
        return;
      }
      const move = reorder.current;
      const head = headRef.current;
      if (!move || !head) return;
      // A few pixels of slop, so a slightly imprecise click still sorts.
      if (!move.moved && Math.abs(e.clientX - move.x) < 5) return;
      if (!move.moved) {
        move.moved = true;
        setDragKey(move.key);
      }
      // rekordbox's drag: the heading floats with the pointer and the columns
      // make room for it as it goes, so where it will land is always on show.
      const current = columnsRef.current;
      const at = current.findIndex((col) => col.key === move.key);
      if (at === -1) return;
      const origin = head.getBoundingClientRect().left;
      const left = e.clientX - move.grab;
      const width = current[at]?.width ?? 0;
      ghostLeft.current = Math.round(left - origin);
      if (ghostRef.current) ghostRef.current.style.transform = `translateX(${ghostLeft.current}px)`;
      const spans = headingsOf(head).map((cell) => {
        const box = cell.getBoundingClientRect();
        return { left: box.left, right: box.right };
      });
      const first = current.filter((col) => col.fixed).length;
      const to = reorderTarget(spans, at, left, left + width, first);
      if (to !== at) onColumnMove(move.key, to);
    };
    const onUp = () => {
      resizing.current = null;
      const move = reorder.current;
      reorder.current = null;
      setDragKey(null);
      // Already where it belongs: the columns moved while it was held.
      if (move?.moved) draggedRef.current = true;
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [onColumnResize, onColumnMove]);

  const virtualizer = useVirtualizer({
    count: view.count + pendingRows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowH,
    // Mount a half-screen of rows past the viewport each way. Beyond drawing
    // them ready, it starts their waveforms loading before they scroll into
    // view, so a steady scroll meets rows that already have one.
    overscan: 16,
    // The sticky column header is in the scroller's flow, so the list starts
    // this far down it. Without this the virtualizer's idea of which rows are
    // visible is a header's worth out.
    scrollMargin: COL_HEADER_H,
  });

  // A new row height throws away what the virtualizer measured at the old one.
  useEffect(() => {
    virtualizer.measure();
  }, [virtualizer, rowH]);

  const items = virtualizer.getVirtualItems();

  // The rows being drawn, as indices. These are what the fetch below keys on.
  //
  // It used to key on `items.length` — how *many* rows are on screen — which
  // does not change while scrolling. So moving the window asked for nothing,
  // and every row it landed on stayed a placeholder: a list at its full height
  // with a working scrollbar and no content in it.
  //
  // The page chain hid it. Each page that landed re-rendered the table and
  // re-ran the effect, which caught the window up by accident, so it only bit
  // when the scroll outran the fetches or moved after they had all settled.
  // Measured in the app: a drag ended on row 19,329 with the last request made
  // for rows 17,065-17,105, and nothing asked again for eight seconds.
  const firstIndex = items.length > 0 ? (items[0]?.index ?? 0) : -1;
  const lastIndex = items.length > 0 ? (items[items.length - 1]?.index ?? 0) : -1;

  // Ask for the pages covering what is on screen, plus a margin either side so a
  // fast scroll lands on cached rows. Cheap and idempotent.
  useEffect(() => {
    if (firstIndex < 0) return;
    // The virtualizer's own window, overscan included, rather than the same
    // arithmetic done twice: it already accounts for `scrollMargin`, so this
    // cannot drift a header's worth out of step with what is rendered. The
    // margin runs past both ends; `ensureRange` clamps it to [0, count].
    view.ensureRange(firstIndex - PREFETCH_MARGIN, lastIndex + 1 + PREFETCH_MARGIN);
  }, [firstIndex, lastIndex, view, view.count, view.token]);

  // Up/Down move a single highlight through the list, and PageUp/PageDown and
  // Home/End jump it, wherever the focus is — the keyboard counterpart of
  // clicking a row. Enter (loading Player 1) and the arrows the deck owns are
  // handled elsewhere; this only moves the browser's cursor.
  const platform = useMemo(detectPlatform, []);
  const moveCursor = useCallback(
    (to: number) => {
      const count = view.count;
      if (count === 0) return;
      const index = Math.max(0, Math.min(to, count - 1));
      virtualizer.scrollToIndex(index);
      const row = view.rowAt(index);
      if (row) {
        setSelection({ ids: new Set([row.id]), anchorIndex: index });
        return;
      }
      // Off screen and not yet fetched: the backend resolves the id at that
      // index, since the selection is by id and survives a re-sort.
      void view.idsInRange(index, index).then((ids) => {
        const id = ids[0];
        setSelection(
          id === undefined
            ? (s) => ({ ...s, anchorIndex: index })
            : { ids: new Set([id]), anchorIndex: index },
        );
      });
    },
    [view, virtualizer],
  );
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const action = dispatch(event, platform, target, preferences.keyboard.overrides);
      if (action === null || view.count === 0) return;
      // A focused knob or fader owns the up/down keys — turning one must not
      // also walk the track list. `dispatch` already yields the text fields.
      const role = target?.getAttribute?.("role");
      if (role === "slider" || role === "spinbutton") return;
      // Ctrl/⌘+A selects the whole list. The ids come from the backend, which
      // resolves them for every row of the view — including the ones that were
      // never scrolled into range and so never fetched. preventDefault stops
      // the webview's own "select all text" from highlighting the interface
      // instead, which is what a plain Ctrl+A did before this.
      if (action === "selectAll") {
        event.preventDefault();
        void view.idsInRange(0, view.count).then((ids) => {
          setSelection((s) => selectAll(s, ids));
        });
        return;
      }
      // The first press with nothing highlighted picks the top visible row;
      // after that the keys step from where the highlight is.
      const start = firstIndex < 0 ? 0 : firstIndex;
      const cursor = selection.anchorIndex ?? start;
      const page = Math.max(1, Math.floor((scrollRef.current?.clientHeight ?? 0) / rowH) - 1);
      let to: number;
      switch (action) {
        case "moveDown":
          to = selection.anchorIndex === null ? start : cursor + 1;
          break;
        case "moveUp":
          to = selection.anchorIndex === null ? start : cursor - 1;
          break;
        case "pageDown":
          to = cursor + page;
          break;
        case "pageUp":
          to = cursor - page;
          break;
        case "toTop":
          to = 0;
          break;
        case "toBottom":
          to = view.count - 1;
          break;
        default:
          return;
      }
      event.preventDefault();
      moveCursor(to);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [platform, selection.anchorIndex, firstIndex, rowH, view, view.count, moveCursor, preferences.keyboard.overrides]);

  const startDragOut = useCallback(
    (row: RowDto) => {
      // Whatever is selected, plus the row grabbed if it was not part of it —
      // dragging an unselected row should move that row, not the selection
      // somewhere else on screen.
      if (!selection.ids.has(row.id)) {
        onDragTracks?.({ ids: [row.id], row });
        return;
      }
      // A playlist takes all of them; a deck takes the first in list order,
      // which is what rekordbox loads from a dropped selection — not the row
      // under the hand. Found by walking the list from the top: the rows the
      // selection was made from are in the cache, and the walk ends at the
      // first hit. Once per drag, not per frame.
      let first = row;
      for (let i = 0; i < view.count; i += 1) {
        const at = view.rowAt(i);
        if (at && selection.ids.has(at.id)) {
          first = at;
          break;
        }
      }
      onDragTracks?.({ ids: [...selection.ids], row: first });
    },
    [selection.ids, onDragTracks, view],
  );

  // The ids travelling with the hand, and where they would land. Held here
  // rather than read back from the drag event: `dataTransfer` will not give
  // its payload up during `dragover`, only on the drop.
  const carrying = useRef<readonly string[] | null>(null);
  const [dropAt, setDropAt] = useState<{ index: number; below: boolean } | null>(null);
  // A library refresh can replace the source row before the browser sends
  // dragend. The shell clears its drag on a successful drop, so clear this
  // table's local payload at the same point as well.
  useEffect(() => {
    if (!dragging) carrying.current = null;
  }, [dragging]);

  const nativeDrag = useRef(false);
  const dragGeneration = useRef(0);
  const finishDraggingTracks = useEventCallback(() => {
    carrying.current = null;
    nativeDrag.current = false;
    setDropAt(null);
    onDragTracks?.(null);
  });

  const startDraggingTracks = useEventCallback(
    (row: RowDto) => {
      const generation = ++dragGeneration.current;
      carrying.current = selection.ids.has(row.id) ? [...selection.ids] : [row.id];
      startDragOut(row);
      if (!nativeTrackDragging()) return false;
      nativeDrag.current = true;
      void dragTracksToDesktop(carrying.current)
        .catch((error: unknown) => onDragError?.(error instanceof Error ? error.message : String(error)))
        // AppKit can finish before WKWebView dispatches its DOM drop. Keep
        // the track payload through that dispatch, including on cancellation.
        .finally(() => { window.setTimeout(() => {
          if (dragGeneration.current === generation) finishDraggingTracks();
        }, 100); });
      return true;
    },
  );

  const endDraggingTracks = useEventCallback(() => {
    if (!nativeDrag.current) finishDraggingTracks();
  });

  const isBelowTracks = (target: EventTarget, y: number) =>
    scrollRef.current?.contains(target as Node) && rowsRef.current !== null &&
    y >= rowsRef.current.getBoundingClientRect().bottom;

  /** Where the carried rows would go, as the pointer moves over a row. */
  const reorderOver = useEventCallback(
    (index: number, below: boolean) => {
      if (!onReorder || carrying.current === null) return;
      setDropAt((at) => (at?.index === index && at.below === below ? at : { index, below }));
    },
  );

  /**
   * Lands the carried rows at the line drawn, as the whole playlist's order.
   *
   * `reorderPlaylist` is given every id, not the moved ones: the backend
   * rewrites `TrackNo` from what it is handed, and a partial list would leave
   * the rest of the playlist to be appended in its old order.
   */
  const reorderDrop = useEventCallback((at: { index: number; below: boolean } | null = dropAt) => {
    const moved = carrying.current;
    setDropAt(null);
    if (!onReorder || moved === null || at === null || moved.length === 0) return;
    void (async () => {
      const all = await view.idsInRange(0, view.count);
      const moving = new Set(moved);
      // The boundary the line was drawn at, counted in ids that are staying:
      // the carried rows are lifted out first, so the rows above the line are
      // what the insertion point is measured against.
      const boundary = at.index + (at.below ? 1 : 0);
      const insertAt = all.slice(0, boundary).filter((id) => !moving.has(id)).length;
      const staying = all.filter((id) => !moving.has(id));
      // In list order, not selection order, so a multi-row drag keeps its shape.
      const lifted = all.filter((id) => moving.has(id));
      if (lifted.length === 0) return;
      onReorder([...staying.slice(0, insertAt), ...lifted, ...staying.slice(insertAt)]);
    })();
  });

  const handleSelect = useEventCallback(
    (index: number, id: string, e: React.MouseEvent) => {
      const modifier = pressModifier(e, MAC);
      if (modifier === "range" && selection.anchorIndex !== null) {
        const anchor = selection.anchorIndex;
        void view.idsInRange(anchor, index).then((ids) => {
          setSelection((s) => applyClick(s, { id, index }, "range", ids));
        });
        return;
      }
      setSelection((s) => applyClick(s, { id, index }, modifier));
    },
  );

  /**
   * Loads a row into the player.
   *
   * A double-click, not a click. Selecting a track and playing it are
   * different intentions — arrowing through a playlist to see what is in it
   * should not load forty tracks on the way past.
   */
  const handleOpen = useEventCallback(
    (index: number) => {
      onFocusedRow?.(view.rowAt(index) ?? null);
    },
  );

  useEffect(() => {
    onSelectionChange?.(selection.ids.size);
  }, [selection.ids, onSelectionChange]);

  const reportedRow = useRef<string | null>(null);
  useEffect(() => {
    if (!onSelectedRow) return;
    // One row, or nothing: a deck holds one track, so a selection of four has
    // no answer to which of them clicking a deck would load.
    const only = selection.ids.size === 1 ? [...selection.ids][0] : undefined;
    if (only === reportedRow.current) return;
    reportedRow.current = only ?? null;
    if (only === undefined) {
      onSelectedRow(null);
      return;
    }
    for (let i = 0; i < view.count; i++) {
      const row = view.rowAt(i);
      if (row?.id === only) {
        onSelectedRow(row);
        return;
      }
    }
    // Selected but not fetched — off screen, which a click cannot reach.
    onSelectedRow(null);
  }, [selection.ids, view, view.token, onSelectedRow]);

  // The rows behind the selection, resolved from what is cached. A selection
  // spanning unfetched rows contributes only what is on hand, which is what
  // the user can see anyway.
  /** The track menu: where it is, and which row it was opened on. */
  const [trackMenu, setTrackMenu] = useState<
    { x: number; y: number; row: RowDto } | null
  >(null);

  const openTrackMenu = useCallback(
    (index: number, row: RowDto, at: { x: number; y: number }) => {
      // Right-clicking a row the selection does not hold selects it first, as
      // every list does: otherwise the menu acts on something else.
      setSelection((current) =>
        current.ids.has(row.id) ? current : { ids: new Set([row.id]), anchorIndex: index },
      );
      setTrackMenu({ ...at, row });
    },
    [],
  );

  // Whether the Delete key speaks to this list: rekordbox's list hears it
  // only while it has the focus. The last press landed in this list, and
  // nowhere else since — not the tree, the other list, a deck, a waveform or
  // a button. A menu or a dialog opened from here gives the focus back.
  const rootRef = useRef<HTMLDivElement>(null);
  const engaged = useRef(false);
  useEffect(() => {
    const onDown = (event: MouseEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      if (target === null) return;
      if (rootRef.current?.contains(target)) engaged.current = true;
      else if (!target.closest('[role="menu"], [role="dialog"]')) engaged.current = false;
    };
    window.addEventListener("mousedown", onDown, true);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
    };
  }, []);

  // Delete and ⌫ remove the whole selection the way this list's own menu
  // entry does, asking first as it does: from the collection, a playlist, a
  // history or the Tag List. rekordbox's list does the same with either key
  // (#136). One press is one removal: a held key's repeats, and presses
  // while one removal is still asking or writing, do nothing.
  const removing = useRef(false);
  const removeSelection = useEventCallback((event: KeyboardEvent) => {
    if (event.key !== "Delete" && event.key !== "Backspace") return;
    if (event.defaultPrevented || !engaged.current || trackMenu !== null) return;
    // The key goes where the focus is: the page itself (a click on a row
    // focuses nothing) or something inside this list, never a control.
    const target = event.target instanceof HTMLElement ? event.target : null;
    const inList = target !== null && rootRef.current?.contains(target) === true;
    if (target !== null && !inList && target !== document.body && target !== document.documentElement) return;
    if (isTyping(target) || target?.closest('[role="tree"], [role="dialog"], [role="menu"], button, [role="slider"], [role="spinbutton"]')) return;
    // A key the person bound to something else in the Keyboard pane is theirs.
    if (dispatch(event, platform, target, preferences.keyboard.overrides) !== null) return;
    const action = deleteKeyAction(spec.source.kind);
    if (action === null || selection.ids.size === 0 || hasLooseId(selection.ids)) return;
    event.preventDefault();
    if (event.repeat || removing.current) return;
    if (readOnly) {
      onEditBlocked?.();
      return;
    }
    const remove: RemoveTracks | undefined = {
      removeFromCollection: onRemoveFromCollection,
      removeFromPlaylist: onRemoveFromPlaylist,
      removeFromHistory: onRemoveFromHistory,
      removeFromTagList: onRemoveFromTagList,
    }[action];
    if (remove === undefined) return;
    const ids = [...selection.ids];
    removing.current = true;
    void remove(ids)
      .then((removed) => {
        // The removed rows are gone; a second press must not name them again.
        if (!removed) return;
        const gone = new Set(ids);
        setSelection((current) => ({
          ids: new Set([...current.ids].filter((id) => !gone.has(id))),
          anchorIndex: current.anchorIndex,
        }));
      })
      .finally(() => {
        removing.current = false;
      });
  });
  useEffect(() => {
    window.addEventListener("keydown", removeSelection);
    return () => {
      window.removeEventListener("keydown", removeSelection);
    };
  }, [removeSelection]);

  const reportedSelection = useRef("");
  // The list order of a selection the row cache could not order, asked of
  // the backend once per selection and view; `order` is null while asked.
  const listOrder = useRef<{ ids: ReadonlySet<string>; token: string; order: string[] | null } | null>(null);
  useEffect(() => {
    if (!onSelectedTracks) return;
    // Titles come from whatever pages are cached; the ids are the whole
    // selection, cached or not. The scan runs top to bottom, so the titles
    // map holds the cached selected rows in list order.
    const titles = new Map<string, string>();
    for (let i = 0; i < view.count && titles.size < selection.ids.size; i++) {
      const row = view.rowAt(i);
      if (row && selection.ids.has(row.id)) titles.set(row.id, row.title);
    }
    const report = (ids: Iterable<string>) => {
      const tracks = selectedTracks(ids, titles);
      // Only when it has actually changed. This hands a new array upwards, and
      // the app holds it in state: sending an equal one re-renders the window,
      // which renders this table, which runs this effect again.
      const stamp = tracks.map((t) => t.id).join(",");
      if (stamp === reportedSelection.current) return;
      reportedSelection.current = stamp;
      onSelectedTracks(tracks);
    };
    // An answer still on its way for another selection or view is dropped.
    const known = listOrder.current;
    if (known && (known.ids !== selection.ids || known.token !== view.token)) listOrder.current = null;
    // Reported in list order, as rekordbox orders its selection (see
    // `inListOrder`). With every selected row cached the scan above has the
    // order already.
    if (titles.size === selection.ids.size) {
      report(titles.keys());
      return;
    }
    // Some rows are not cached. Report the selection as it stands, so a
    // command run straight after acts on all of it, then again in list order
    // once the backend has sent the view's ids. A range or select-all is in
    // list order already, which makes the second report a no-op. Pages
    // arriving re-run this effect; they must not ask again.
    if (listOrder.current) {
      report(listOrder.current.order ?? selection.ids);
      return;
    }
    report(selection.ids);
    if (selection.ids.size < 2) return;
    const asked = { ids: selection.ids, token: view.token, order: null as string[] | null };
    listOrder.current = asked;
    void view.idsInRange(0, view.count).then((listed) => {
      if (listOrder.current !== asked) return;
      asked.order = inListOrder(asked.ids, listed);
      report(asked.order);
    }).catch(() => {
      // The order only decides which track's colour the information panel
      // shows; the selection already reported stands.
    });
  }, [selection.ids, view, onSelectedTracks]);

  // An arrow drawn to rekordbox's geometry rather than the text arrows that
  // stood in for it: those render in the body font and sit off the baseline.
  const sortIndicator = useCallback(
    (col: Column) =>
      col.sortable && col.key === spec.sort ? (
        spec.descending ? (
          <SortDownIcon className={styles.sortArrow} />
        ) : (
          <SortUpIcon className={styles.sortArrow} />
        )
      ) : null,
    [spec.sort, spec.descending],
  );

  const dragged = dragKey === null ? undefined : columns.find((col) => col.key === dragKey);
  // The copy mounts on the move that starts the drag; put it under the
  // pointer before it paints rather than at the row's left edge.
  useLayoutEffect(() => {
    if (dragKey !== null && ghostRef.current) ghostRef.current.style.transform = `translateX(${ghostLeft.current}px)`;
  }, [dragKey]);

  const header = useMemo(
    () =>
      columns.map((col) => (
        <div
          key={col.key}
          className={col.align === "right" ? `${styles.headCell} ${styles.right}` : styles.headCell}
          data-sorted={col.key === spec.sort || undefined}
          data-dragging={dragKey === col.key || undefined}
          onMouseDown={(e) => {
            // Reordering is a pointer drag with a threshold, not HTML5
            // drag-and-drop: marking the heading `draggable` makes the browser
            // treat a plain click as the start of a drag and swallow it, which
            // stopped the heading sorting at all.
            if (e.button !== 0 || col.fixed) return;
            const box = e.currentTarget.getBoundingClientRect();
            reorder.current = { key: col.key, x: e.clientX, grab: e.clientX - box.left, moved: false };
          }}
          onClick={
            col.sortable
              ? () => {
                  // A drag ends in a click too; that one must not also sort.
                  if (draggedRef.current) {
                    draggedRef.current = false;
                    return;
                  }
                  onSortChange(col.key as SortColumn);
                }
              : undefined
          }
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, target: col.key });
          }}
          role="columnheader"
        >
          {col.label}
          {sortIndicator(col)}
          {/*
            The resize grip. Its own mousedown stops the header's click, so
            dragging an edge never also re-sorts the table.
          */}
          <span
            className={styles.grip}
            onMouseDown={(e) => {
              e.stopPropagation();
              e.preventDefault();
              resizing.current = { key: col.key, x: e.clientX, width: col.width };
            }}
            onClick={(e) => e.stopPropagation()}
            role="separator"
            aria-orientation="vertical"
            aria-label={`Resize ${col.label}`}
          />
        </div>
      )),
    [columns, spec.sort, onSortChange, sortIndicator, dragKey],
  );

  return (
    <div
      ref={rootRef}
      className={styles.browser}
      style={{
        ["--cols" as string]: gridOf(columns),
        ["--table-w" as string]: `${totalWidthOf(columns)}px`,
        // Browse › FontSize, Bold and Line Space, scoped to the list: the
        // tokens are the measured sizes, and these are the slider's multiples
        // of them.
        ...browseListVars(preferences.view, ROW_H),
      }}
      data-file-over={fileOver || undefined}
      onDragOver={(e) => {
        if (carrying.current) {
          if (onReorder && view.count > 0 && isBelowTracks(e.target, e.clientY)) {
            e.preventDefault();
            e.dataTransfer.dropEffect = nativeTrackDragging() ? "copy" : "move";
            reorderOver(view.count - 1, true);
          }
          return;
        }
        if (dragging && onDropTracks && spec.source.kind === "playlist") {
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
          setFileOver(true);
          return;
        }
        if (!onDropFiles || !e.dataTransfer.types.includes("Files")) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setFileOver(true);
      }}
      onDragLeave={(e) => {
        if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
        setFileOver(false);
      }}
      onDrop={(e) => {
        setFileOver(false);
        if (carrying.current) {
          e.preventDefault();
          if (onReorder && view.count > 0 && isBelowTracks(e.target, e.clientY)) {
            reorderDrop({ index: view.count - 1, below: true });
          }
          return;
        }
        if (dragging && onDropTracks && spec.source.kind === "playlist") {
          e.preventDefault();
          onDropTracks(spec.source.id);
          return;
        }
        if (!e.dataTransfer.types.includes("Files") && e.dataTransfer.files.length === 0) return;
        // File drops never invoke WebKit's default media navigation.
        e.preventDefault();
        if (!onDropFiles || e.dataTransfer.files.length === 0) return;
        onDropFiles(Array.from(e.dataTransfer.files));
      }}
    >
      <div className={styles.browserHead}>
        <span className={styles.title} data-testid="browser-title">
          {/*
            The count belongs to the list, so it appears only once the list
            has settled. While a view is opening, `view.count` is still the
            previous view's, and pairing it with the new title showed a
            playlist's name beside the whole collection's count.
          */}
          {view.loading ? title : `${title} (${view.count} Tracks)`}
        </span>
        {trafficLight && onTrafficLight ? (
          <div ref={trafficBox} className={styles.trafficBox}>
            <button
              type="button"
              className={styles.trafficMaster}
              aria-label="Traffic Light deck"
              aria-haspopup="menu"
              aria-expanded={trafficMenu}
              // Transcribed from rekordbox: "Select the target deck for
              // Traffic Light feature."
              title={tip("Select the target deck for Traffic Light feature.")}
              onClick={() => setTrafficMenu((open) => !open)}
            >
              {TRAFFIC_SOURCES.find((s) => s.id === trafficLight)?.short ?? "MASTER"}
            </button>
            <button
              type="button"
              className={styles.trafficChevron}
              aria-label="Choose the Traffic Light deck"
              aria-haspopup="menu"
              aria-expanded={trafficMenu}
              onClick={() => setTrafficMenu((open) => !open)}
            >
              <span aria-hidden />
            </button>
            {trafficMenu ? (
              <div className={styles.trafficMenu} role="menu" aria-label="Traffic Light deck">
                {TRAFFIC_SOURCES.filter((source) => source.id === "master" || (source.id === "a" ? players >= 1 : players >= 2)).map((source) => (
                  <button
                    key={source.id}
                    type="button"
                    role="menuitemradio"
                    aria-checked={source.id === trafficLight}
                    className={styles.trafficItem}
                    onClick={() => {
                      onTrafficLight(source.id);
                      setTrafficMenu(false);
                    }}
                  >
                    <span className={styles.trafficTick} aria-hidden>
                      {source.id === trafficLight ? <TickIcon className={styles.trafficTickGlyph} /> : null}
                    </span>
                    {source.label}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}
        {onToggleFilter ? (
          <button
            type="button"
            className={styles.filterToggle}
            data-on={filterOpen || undefined}
            aria-pressed={filterOpen}
            aria-label="Display/Hide Track Filter"
            title={tip("Display/Hide Track Filter")}
            data-testid="filter-toggle"
            onClick={onToggleFilter}
          >
            <FilterIcon className={styles.filterGlyph} />
          </button>
        ) : null}
        <SearchField className={styles.search} value={query} onChange={onQueryChange}
          scope={searchField} onScopeChange={onSearchFieldChange ?? (() => undefined)} options={TRACK_SEARCH_OPTIONS}
          label="Search within this track list" scopeLabel="Track search scope" inputRef={searchRef} />
      </div>

      {filterOpen ? filterBar : null}

      <div className={styles.scroll} ref={scrollRef} data-testid="track-scroll" role="grid" aria-rowcount={view.count}>
        {/*
          Inside the scroller, not above it. As a sibling it stayed put while
          the rows moved sideways, so every column sheared away from its own
          heading; sticky keeps it pinned vertically while it scrolls
          horizontally with them.
        */}
        <div className={styles.colHead} role="row" ref={headRef}>
          {header}
          {dragged ? (
            <div
              ref={ghostRef}
              className={dragged.align === "right" ? `${styles.headCell} ${styles.right} ${styles.headGhost}` : `${styles.headCell} ${styles.headGhost}`}
              style={{ width: `${dragged.width}px` }}
              data-testid="column-drag-ghost"
              aria-hidden
            >
              {dragged.label}
            </div>
          ) : null}
        </div>

        <div
          key={view.token}
          className={styles.inner}
          ref={rowsRef}
          style={{ height: `${virtualizer.getTotalSize()}px` }}
        >
          {items.map((item) => {
            // Past the last real row are the ones still arriving: skeletons.
            const row = item.index < view.count ? view.rowAt(item.index) : undefined;
            return (
              <TrackRow
                key={row?.id ?? `slot-${item.key}`}
                row={row}
                index={item.index}
                columns={columns}
                onDragStart={startDraggingTracks}
                onDragEnd={endDraggingTracks}
                onRate={onRate}
                onComment={onComment}
                onEditField={onEditField}
                onEditBlocked={onEditBlocked}
                keyDisplay={keyDisplay}
                previewCues={previewCueMarkers}
                clickToEdit={clickToEdit}
                tooltips={tooltips}
                trafficKey={trafficKey}
                trafficReach={preferences.view.trafficLight}
                top={item.start - COL_HEADER_H}
                selected={row ? selection.ids.has(row.id) : false}
                onSelect={handleSelect}
                onOpen={handleOpen}
                onMenu={openTrackMenu}
                reorderable={Boolean(onReorder)}
                isLocalDrag={() => carrying.current !== null}
                dropEdge={
                  dropAt?.index === item.index ? (dropAt.below ? "below" : "above") : null
                }
                onReorderOver={reorderOver}
                onReorderDrop={reorderDrop}
                startupCache={seed !== undefined}
              />
            );
          })}
        </div>
      </div>

      {trackMenu && deviceMenu && spec.source.kind === "device" ? (
        <ContextMenu
          x={trackMenu.x}
          y={trackMenu.y}
          rows={deviceTrackMenu(deviceMenu.playlists, deviceMenu.inPlaylist)}
          label="Track"
          context={{ inPlaylist: deviceMenu.inPlaylist, hasFile: true, readOnly: readOnly || deviceMenu.busy }}
          onChoose={(action) => {
            const ids = [...selection.ids];
            if (action.startsWith("deviceAddToPlaylist:")) deviceMenu.onAdd(action.slice("deviceAddToPlaylist:".length), ids);
            else if (action === "removeFromPlaylist") deviceMenu.onRemove(ids);
          }}
          onClose={() => setTrackMenu(null)}
        />
      ) : trackMenu ? (
        <ContextMenu
          x={trackMenu.x}
          y={trackMenu.y}
          rows={trackMenu.row.missing === true ? MISSING_TRACK_MENU : trackMenuFor(players, playlists, devices, {
            tagList: spec.source.kind === "tagList",
            explorer: spec.source.kind === "folder",
          })}
          title={trackMenu.row.missing === true ? MISSING_TRACK_TITLE : undefined}
          label="Track"
          context={{
            inPlaylist: spec.source.kind === "playlist",
            inHistory: spec.source.kind === "history",
            hasFile: true,
            loose: hasLooseId(selection.ids),
            readOnly,
          }}
          onChoose={(action) => {
            const ids = [...selection.ids];
            if (action.startsWith("addToPlaylist:")) {
              onAddToPlaylist?.(action.slice("addToPlaylist:".length), ids);
              return;
            }
            if (action.startsWith("exportTrack:")) {
              onExportTrack?.(action.slice("exportTrack:".length), ids);
              return;
            }
            switch (action) {
              case "importToCollection":
                onImportToCollection?.(ids);
                break;
              case "analysisLock":
                onAnalysisLock?.(ids, true);
                break;
              case "analysisUnlock":
                onAnalysisLock?.(ids, false);
                break;
              case "addToTagList":
                onAddToTagList?.(ids);
                break;
              case "removeFromTagList":
                void onRemoveFromTagList?.(ids);
                break;
              case "reloadTag":
                onReloadTag?.(ids);
                break;
              case "analyse":
                onAnalyse?.();
                break;
              case "showInformation":
                onShowInformation?.(trackMenu.row);
                break;
              case "showInFinder":
                onShowInFinder?.(trackMenu.row);
                break;
              case "removeFromPlaylist":
                void onRemoveFromPlaylist?.(ids);
                break;
              case "removeFromHistory":
                void onRemoveFromHistory?.(ids);
                break;
              case "resetPlayCount":
                onResetPlayCount?.(ids);
                break;
              case "convertMemoryCues":
                onConvertMemoryCues?.(trackMenu.row);
                break;
              case "removeFromCollection":
                void onRemoveFromCollection?.(ids);
                break;
              case "autoRelocate":
                onAutoRelocate?.(ids);
                break;
              case "relocate": {
                const chosen = selection.ids;
                void view.idsInRange(0, view.count)
                  .then((listed) => onRelocate?.(inListOrder(chosen, listed)))
                  .catch(() => onRelocate?.(ids));
                break;
              }
              case "loadPlayer1":
                onLoadTrack?.("a", trackMenu.row);
                break;
              case "loadPlayer2":
                onLoadTrack?.("b", trackMenu.row);
                break;
              default:
                break;
            }
          }}
          onClose={() => setTrackMenu(null)}
        />
      ) : null}

      {menu ? (
        <ColumnMenu
          x={menu.x}
          y={menu.y}
          target={menu.target}
          visible={columns.map((c) => c.key)}
          onToggle={onColumnToggle}
          onAutoSize={onColumnAutoSize}
          onAutoSizeAll={onColumnAutoSizeAll}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
});
