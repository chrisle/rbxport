/**
 * The preview player.
 *
 * Every size here is a token measured off a 2x capture of rekordbox 7.2.11
 * running: the player spans y 56..335pt, its transport column is 80pt wide and
 * its memory panel 209pt, and the five bands inside it (title, overview,
 * phrase, detail, pads) have their measured heights recorded as token sources.
 *
 * Playback is the Rust engine behind the `deck_*` commands — see
 * `usePlayback` and `crates/rbl-deck`. Outside Tauri there is no engine, so
 * the transport is drawn and disabled: a player waiting for a backend, rather
 * than an unfinished panel. Controls with nothing behind them yet are drawn
 * the same way, for the same reason.
 */
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { TimeReadouts } from "./TimeReadouts";
import { useEventCallback } from "@/store/useEventCallback";

import type { Cue, DeckId, Phrase, RowDto } from "@/ipc/types";
import { getBackend } from "@/ipc/client";
import { useElementSize } from "@/store/useElementSize";
import { Artwork } from "@/components/Artwork";
import {
  EjectIcon, GridAlignAllIcon, GridAlignHereIcon, GridCutIcon, GridDoubleIcon, GridHalveIcon,
  GridLockIcon, GridLockOpenIcon, GridMarkIcon, GridMetronomeIcon, GridNarrowIcon, GridRedoIcon,
  GridShiftBackIcon, GridShiftForwardIcon, GridUndoIcon, GridWidenIcon, RecordIcon,
} from "@/components/icons";
import { formatBpm } from "@/lib/format";
import {
  DETAIL_BARS,
  NO_BEATS,
  nearestBeatMs,
  subdivideGrid,
  ZOOM_STEPS,
  showsEveryBeat,
  beatsIn,
  cuesFor,
  detailSpan,
  dragSeconds,
  JUMP_SIZE_ID,
  jumpSizeById,
  jumpStepSeconds,
  zoomBy,
  createWheelZoomGate,
  needsRedraw,
  OVERDRAW,
  scrollOffset,
  pressCue,
  releaseCue,
  beatCountText,
  headPercent,
  isClick,
  phraseSpans,
  memoryTime,
  splitTime,
  windowAround,
  type CuePanel,
  type PadMode,
  beatLoopRange,
  clampLoopBeats,
  LOOP_BEATS_MAX,
  LOOP_BEATS_MIN,
  loopBeatsLabel,
  resizedLoopRange,
  wrapIntoLoop,
  tempoAtMs,
  tempoAnnotations,
  type BeatGrid as TrackBeatGrid,
} from "@/lib/player";
import { type DeckLoop, usePlayback } from "@/store/usePlayback";
import { usePreferences, usePreferencesContext, useTooltip } from "@/store/usePreferences";
import { ContextMenu } from "@/components/ContextMenu";
import { deckMenu, type DeckAction } from "@/lib/contextMenus";
import { KeyShift } from "./KeyShift";
import { TempoToggle } from "./TempoToggle";
import { TempoSlider } from "./TempoSlider";
import type { HotCueColor } from "@/lib/preferences";
import { formatKey, quantizeFraction } from "@/lib/preferences";
import {
  beatNudgeFor, beatWait, inPhaseAt, MAX_TEMPO, MIN_TEMPO, syncTo, tempoFor, type Deck as SyncDeck,
} from "@/lib/sync";
import {
  beatLoopLength, detectPlatform, dispatchBinding, hotCuePad, matchBinding, memoryCueNumber,
} from "@/lib/shortcuts";
import { WaveformDetail } from "./WaveformDetail";
import { SimplePlayer } from "./SimplePlayer";
import { JumpMenu } from "./JumpMenu";
import { VocalStrip } from "./VocalStrip";
import { useTrackCues } from "./useTrackCues";
import { useTrackDetails } from "./useTrackDetails";
import { useTrackGrid } from "./useTrackGrid";
import { useGridEditor } from "./useGridEditor";
import { nudgeGrid } from "@/lib/gridEdit";
import { listenEditHistory } from "@/lib/editHistory";
import { registerDeck } from "@/lib/scripting";
import { useHoldRepeat } from "./useHoldRepeat";
import { DeckInfo } from "./DeckInfo";
import { DualControls, DualHead } from "./DualDeck";
import { READ_ONLY_REASON, useMemoryCues } from "./useMemoryCues";
import { useHotCues } from "./useHotCues";
import { useCueWriter } from "./useCueWriter";
import { CueColorMenu } from "./CueColorMenu";
import styles from "./Player.module.css";

export interface PlayerProps {
  /** The row the browser has selected, or `null` when nothing is. */
  track: RowDto | null;
  /** Which engine deck this drives. The 2-player layout adds a second. */
  deck?: DeckId;
  /**
   * The simple player: one strip — PLAY, the sleeve, the readouts over the
   * overview, the rating — drawn by `SimplePlayer` from this deck's state, so
   * the layout switch changes what is on screen and not what is playing.
   */
  simple?: boolean;
  /**
   * Take the track out of the deck.
   *
   * The artwork is the eject button, as it is on a CDJ's screen: clicking the
   * sleeve is how you get a track out without loading another over it.
   */
  onEject?: () => void;
  /**
   * Something the deck could not do.
   *
   * Reported rather than drawn: the message used to print across the pad row
   * and the tree underneath it. It belongs in the status bar, with everything
   * else the app has to say.
   */
  onError?: (message: string | null) => void;
  /** Analyze Track from the deck's ≡ menu: the loaded track goes to the analyser. */
  onAnalyse?: (trackId: string, title: string) => void;
  /** Export Track from the ≡ menu, to one of `devices`. */
  onExportTrack?: (device: string, trackId: string) => void;
  devices?: readonly { id: string; name: string }[];
  /** Export Loop As WAV: the loop's stretch of the loaded track. */
  onExportLoop?: (trackId: string, title: string, inMs: number, outMs: number) => void;
  /**
   * A track dropped onto the deck.
   *
   * The whole deck takes the drop, not only the sleeve: rekordbox loads a
   * track dropped anywhere on a player, and a 79-pixel square is a small
   * target for a hand that is already carrying something. The sleeve is what
   * lights up, because that is where the track lands.
   *
   * What was dropped is not passed back: the shell started the drag and knows
   * what is in it, and a deck takes one track whoever is holding it.
   */
  onDropTrack?: () => void;
  /** A track is being dragged, so the deck can offer itself as a target. */
  dragging?: boolean;
  /**
   * Where the transport column is drawn, when it is not drawn here.
   *
   * The two-deck layouts share one transport column between the decks — deck A
   * down from the top, deck B up from the bottom, with the mixer strip beside
   * it — so the two halves are siblings in the shell's grid rather than each
   * inside its own deck.
   *
   * A portal rather than lifted state: everything the transport touches — the
   * playhead, the cue, the beat-jump size — belongs to this deck and is held
   * here, and moving all of that up to the shell to move a column would be a
   * great deal of state travelling for a layout change.
   */
  transportSlot?: HTMLElement | null;
  /** Deck B's transport reads bottom-up, mirroring deck A's. */
  flipped?: boolean;
  /**
   * The two-deck body: rekordbox's 2 PLAYER deck is a different arrangement
   * from its 1 PLAYER deck, not the same one at half height — see `DualDeck`.
   * Its own prop rather than inferred from `transportSlot`, which is null for
   * a render before the slot has mounted.
   */
  dual?: boolean;
  /**
   * The zoom, when the shell draws the cluster.
   *
   * The two-deck layout has one zoom cluster for the pair, over the line
   * where the two detail waveforms meet; pressing it zooms both decks.
   * Registered the way `publishSync` is, for the same reason.
   */
  publishZoom?: ((zoom: (by: number) => void) => void) | undefined;
  /**
   * The waveform zoom and the beat-jump size, when something outside is
   * driving them.
   *
   * DUAL CONTROL: with it on the shell holds one of each and hands the same
   * value to both decks, so zooming one zooms the other. Left out, the deck
   * keeps its own — a deck on its own has nothing to link to.
   */
  bars?: number;
  onBars?: (bars: number) => void;
  jumpSize?: string;
  onJumpSize?: (id: string) => void;
  /**
   * Sync, which needs both decks and so is arranged by the shell.
   *
   * `publishSync` registers a getter the *other* deck reads when its BEAT SYNC
   * is pressed, and `peerSync` is that other deck's. Getters rather than
   * state: a deck's position changes every frame and sync reads it once, at
   * the moment the button goes down.
   */
  publishSync?: ((get: () => SyncDeck | null) => void) | undefined;
  peerSync?: (() => SyncDeck | null) | undefined;
  /** Whether this deck is the one the other syncs to. */
  isMaster?: boolean;
  onMaster?: () => void;
  /**
   * BEAT SYNC held on: the deck follows the master's tempo for as long as
   * it is lit, as a CDJ's does, rather than matching once. The shell holds
   * the flag, since the master is the shell's to name.
   */
  synced?: boolean;
  onSyncToggle?: (() => void) | undefined;
  /**
   * The tempo the master is playing at, in hundredths of a BPM, or null
   * with no master track. A synced deck re-matches whenever it changes —
   * a nudge on the master, a reset, a new track.
   */
  leaderBpmX100?: number | null;
  /** What this deck is playing at, for the shell to hand to a synced deck. */
  onPlayingBpm?: ((bpmX100: number | null) => void) | undefined;
  /** This deck's key shift in semitones, for the shell's Traffic Light. */
  onKeyShift?: ((semitones: number) => void) | undefined;
  /**
   * A grid shift on this deck, in milliseconds, for the shell to hand to the
   * other deck: a deck synced to this one moves with it. `publishGridFollow`
   * registers what the shell calls on this deck when the other one shifts.
   */
  onGridNudge?: ((ms: number) => void) | undefined;
  publishGridFollow?: ((follow: (ms: number) => void) => void) | undefined;
  /**
   * Load whatever the browser has selected.
   *
   * The third load gesture, and the sleeve is where it lives: loaded, the
   * sleeve ejects, and empty it takes the selection. Absent — nothing
   * selected, or several things — the empty deck is inert.
   */
  onLoadSelected?: (() => void) | undefined;
  /**
   * The id of the highlighted browser row, so Enter can load it onto Player 1
   * and — when this deck is already playing — carry the sound straight into it.
   */
  selectedTrackId?: string | null;
  /**
   * rekordbox holds the database. The MEMORY cluster and the list's ✕ are
   * drawn and disabled, with the same reason the menus give.
   */
  readOnly?: boolean;
}

/**
 * A cue's colour, as the CSS variable the marker, pad and chip styles read.
 *
 * Only set when the cue has one; the stylesheet's `var(--cue-colour,
 * var(--c-cue-hot))` falls back to the token green otherwise, so the default
 * lives in one place.
 */
export function cueStyle(
  left: string | undefined,
  colour: string | null | undefined,
  /** View › Color › HOT CUE color: CDJ draws every cue in the fallback green. */
  hotCueColor: HotCueColor = "colorful",
): React.CSSProperties {
  const style: Record<string, string> = {};
  if (left !== undefined) style.left = left;
  if (colour && hotCueColor === "colorful") style["--cue-colour"] = colour;
  return style;
}

/**
 * Cue points on a waveform.
 *
 * A hot cue is a lettered badge, not a line: measured off `docs/screenshots`,
 * 11pt square, `#3CEB50`, black letter, its left edge on the cue. The overview
 * draws no line through the waveform at all — four hot cues, four badges, and
 * the waveform under them unbroken.
 *
 * A memory cue is a small red head at its position. The capture has one beside
 * each badge, which looked at first like part of the hot cue marker; the live
 * `djmdCue` says otherwise — the measured track carries a `Kind` 0 cue at the
 * same `InMsec` as each of its four hot cues, so the red belongs to those.
 *
 * The detail draws the same two things larger, measured off the user's crop of
 * a hot cue there: a 16pt red triangle pointing down from 8.25pt under the
 * band's top for the memory cue, and the 11pt badge centred on the cue 15pt
 * down for the hot cue. The white line belongs to the beat grid; an off-grid
 * hot cue has a badge at its saved position without an extra vertical line.
 *
 * A badge takes the colour rekordbox paints for the cue's `ColorTableIndex`,
 * which arrives with the cue from the nine indices measured off the captures.
 * An index outside those arrives without one and draws the token green —
 * index 21's colour, which 735,427 of the library's 850,000 hot cues carry —
 * rather than a guess at a neighbour's.
 *
 * Exported for the simple player's overview, which is the same strip.
 */
export const CueMarkers = memo(function CueMarkers({
  cues, totalMs, band = "overview", window, loop = null,
}: {
  cues: readonly Cue[];
  totalMs: number;
  /**
   * Which waveform this is drawn over.
   *
   * The overview hangs its badges from the top of the strip, left edge on the
   * cue. The detail centres each badge on its saved cue position. Memory cues
   * and grid beats retain their own timestamps, even when close to a hot cue.
   */
  band?: "overview" | "detail";
  /** The slice of the track being shown, for the zoomed detail waveform. */
  window?: { from: number; to: number };
  /** The deck's own loop, drawn as a band; lit while it plays. */
  loop?: DeckLoop | null;
}) {
  const tip = useTooltip();
  const hotCueColor = usePreferences().view.hotCueColor;
  if (totalMs <= 0) return null;
  const from = window?.from ?? 0;
  const to = window?.to ?? 1;
  const span = Math.max(to - from, 1e-6);
  // A band from one point to another, clipped to the window; null when none
  // of it is in view.
  const bandStyle = (inMs: number, outMs: number) => {
    const a = Math.max(inMs / totalMs, from);
    const b = Math.min(outMs / totalMs, to);
    if (b <= a) return null;
    return { left: `${((a - from) / span) * 100}%`, width: `${((b - a) / span) * 100}%` };
  };
  const deckLoop = loop ? bandStyle(loop.inSeconds * 1000, loop.outSeconds * 1000) : null;
  return (
    <>
      {/* Memory loops as bands under their heads, and the deck's loop over
          them, lit while it plays — the stored ones read as places to go,
          the live one as where the deck is going round. */}
      {cues.map((cue) => {
        if (!cue.memory || cue.outMs <= cue.positionMs) return null;
        const style = bandStyle(cue.positionMs, cue.outMs);
        return style ? <span key={`loop-${cue.id || cue.positionMs}`} className={styles.loopBand} style={style} aria-hidden /> : null;
      })}
      {deckLoop ? (
        <span className={styles.loopBand} data-active={loop?.active || undefined} style={deckLoop} aria-hidden />
      ) : null}
      {cues.map((cue) => {
        const at = cue.positionMs / totalMs;
        // A cue outside the window is not drawn at the edge — a marker pinned
        // to the edge reads as a cue that is there.
        if (at < from || at > to) return null;
        const left = `${((at - from) / span) * 100}%`;
        // A hot cue is its lettered badge on both waveforms; the stylesheet
        // places it by band. A memory cue's red head is the overview's small
        // one or the detail's 16pt triangle.
        const head = !cue.memory ? (
          <b className={styles.hotCueBadge}>{cue.letter}</b>
        ) : band === "detail" ? (
          <i className={styles.cueMarker} />
        ) : (
          <i className={styles.cueHead} />
        );
        return (
          <span
            key={cue.id || (cue.memory ? `m-${cue.positionMs}` : `h-${cue.letter}-${cue.positionMs}`)}
            className={cue.memory ? styles.memoryCue : styles.hotCue}
            data-band={band}
            data-cue={cue.memory ? "" : cue.letter}
            style={cueStyle(left, cue.colour, hotCueColor)}
            title={tip(cue.memory ? "Memory cue" : `Hot cue ${cue.letter}`)}
            aria-hidden
          >
            {head}
          </span>
        );
      })}
    </>
  );
});

/** A transition spans the actual ramp, clipped to the visible waveform. */
const TempoMarkers = memo(function TempoMarkers({ grid, totalMs, window, overview = false }: {
  grid: TrackBeatGrid;
  totalMs: number;
  window: { from: number; to: number };
  overview?: boolean;
}) {
  const showBpmChanges = usePreferences().view.showBpmChanges;
  const annotations = useMemo(() => showBpmChanges ? tempoAnnotations(grid) : [], [grid, showBpmChanges]);
  if (totalMs <= 0 || !showBpmChanges) return null;
  const from = window.from * totalMs;
  const to = window.to * totalMs;
  const span = Math.max(to - from, 1);
  return <>{annotations.map((mark) => {
    if (mark.toMs < from || mark.fromMs > to) return null;
    const ramp = mark.toMs > mark.fromMs;
    const left = (Math.max(from, mark.fromMs) - from) / span * 100;
    if (!ramp) return <span key={mark.fromMs}
      className={`${styles.cueTempo} ${overview ? styles.overviewTempo : styles.detailTempo}`}
      style={{ left: `${left}%` }} data-testid={overview ? "overview-tempo" : "cue-tempo"} aria-hidden
    >{formatBpm(mark.toBpmX100)}{overview ? "" : " BPM"}</span>;
    return <span key={mark.fromMs} className={`${styles.tempoRamp} ${overview ? styles.overviewRamp : ""}`}
      style={{ left: `${left}%`, width: `${(Math.min(to, mark.toMs) - Math.max(from, mark.fromMs)) / span * 100}%` }}
      data-testid="tempo-ramp" title={`${formatBpm(mark.fromBpmX100)} → ${formatBpm(mark.toBpmX100)} BPM`}
      aria-hidden>
      <span>{formatBpm(mark.fromBpmX100)}{overview ? "" : " BPM"}</span>
      <span className={styles.tempoRampArrow} />
      <span>{formatBpm(mark.toBpmX100)}{overview ? "" : " BPM"}</span>
    </span>;
  })}</>;
});

/** Overview and detail share the same grouped tempo annotations. */
export const OverviewTempoMarkers = memo(function OverviewTempoMarkers({ grid, totalMs }: {
  grid: TrackBeatGrid;
  totalMs: number;
}) {
  return <TempoMarkers grid={grid} totalMs={totalMs} window={{ from: 0, to: 1 }} overview />;
});

/** The detail beat grid, with heavier downbeats and labels at tempo changes. */
const BeatGrid = memo(function BeatGrid({
  beats, grid, totalMs, window, everyBeat = true, fromMs = null,
}: {
  beats: readonly { timeMs: number; downbeat: boolean }[];
  grid: TrackBeatGrid;
  totalMs: number;
  window: { from: number; to: number };
  /** False at the widest zoom, where only the bar lines are drawn. */
  everyBeat?: boolean;
  /** While editing from a boundary, rekordbox hides the earlier grid. */
  fromMs?: number | null;
}) {
  if (totalMs <= 0 || beats.length === 0) return null;
  const span = Math.max(window.to - window.from, 1e-6);
  return (
    <>
      {beats.map((beat) => {
        if (!everyBeat && !beat.downbeat) return null;
        if (fromMs !== null && beat.timeMs < fromMs) return null;
        const at = beat.timeMs / totalMs;
        if (at < window.from || at > window.to) return null;
        return (
          <span
            key={beat.timeMs}
            className={beat.downbeat ? styles.downbeat : styles.beat}
            style={{ left: `${((at - window.from) / span) * 100}%` }}
            data-testid="beat-grid-marker"
            data-beat-ms={beat.timeMs}
            aria-hidden
          >

          </span>
        );
      })}
      <TempoMarkers grid={grid} totalMs={totalMs} window={window} />
    </>
  );
});

/**
 * The edit boundary over the detail waveform: the beat from which the GRID
 * panel's edits apply while one is set. Drawn like a beat line, in the
 * downbeat's red, so it reads as part of the grid rather than a cue.
 */
const GridEditBoundary = memo(function GridEditBoundary({
  fromMs, totalMs, window,
}: {
  fromMs: number;
  totalMs: number;
  window: { from: number; to: number };
}) {
  const at = fromMs / totalMs;
  if (at < window.from || at > window.to) return null;
  const span = Math.max(window.to - window.from, 1e-6);
  return (
    <span
      className={styles.cutMark}
      style={{ left: `${((at - window.from) / span) * 100}%` }}
      title="Grid edits apply from here on"
      data-testid="grid-edit-start"
    />
  );
});

/**
 * The phrase bar: the track's structure, from the `PSSI` tag.
 *
 * Labelled where a block is wide enough to hold its label and left as bare
 * colour where it is not, which is what rekordbox does — a clipped "CHORU"
 * is worse than a coloured block whose shape already says what it is.
 */
const PhraseBar = memo(function PhraseBar({
  phrases, totalMs, beatMs, labels = true,
}: {
  phrases: readonly Phrase[];
  totalMs: number;
  /** Milliseconds a beat lasts, for phrases the beat grid did not reach. */
  beatMs: number;
  /** "Always show types of phrases": the name in each block, or colour alone. */
  labels?: boolean;
}) {
  const spans = phraseSpans(phrases, totalMs, beatMs);
  const tip = useTooltip();
  return (
    <div className={styles.phrase} aria-label="Phrase" data-testid="player-phrase">
      {spans.map((span) => (
        <span
          key={`${span.from}-${span.label}`}
          className={styles.phraseBlock}
          data-kind={span.kind}
          style={{ left: `${span.from * 100}%`, width: `${(span.to - span.from) * 100}%` }}
          title={tip(span.label)}
        >
          {labels ? span.label : ""}
        </span>
      ))}
    </div>
  );
});

/** Transport buttons, in the order and pairing rekordbox has them. */
const SKIPS = [
  { id: "previous", label: "Previous track", glyph: "❘◀" },
  { id: "next", label: "Next track", glyph: "▶❘" },
] as const;

const JUMPS = [
  { id: "jump-back", label: "Beat jump back", glyph: "‹" },
  { id: "jump-forward", label: "Beat jump forward", glyph: "›" },
] as const;

/**
 * What the waveform leaves clear at the top and bottom of its band: the
 * `--s-wave-inset-*` tokens, read once. The strip above carries the bar
 * count and the heads of the cue markers, and the beat markers reach 15px
 * past the waveform at both ends, which is why it is inset as far as it is.
 * The overview does not take it: its own grid row is already the 30pt the
 * capture paints.
 */
let waveInset: { top: number; bottom: number } | null = null;
function waveInsetOf(): { top: number; bottom: number } {
  if (waveInset) return waveInset;
  const read = (name: string, fallback: number) => {
    if (typeof document === "undefined") return fallback;
    const value = Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name));
    return Number.isFinite(value) ? value : fallback;
  };
  waveInset = { top: read("--s-wave-inset-top", 20), bottom: read("--s-wave-inset-bottom", 16) };
  return waveInset;
}

/** Hot cue slots, as the pad row lays them out. */
const PADS = ["A", "B", "C", "D", "E", "F", "G", "H"] as const;

/** Seconds of a track sounding before its play goes on the history. */
const PLAY_RECORD_SECONDS = 60;

/**
 * The key that sets each pad, for its tooltip: the Export preset binds `1`,
 * `2` and `3` to `Set Hot Cue A` to `C` and nothing to the rest.
 */
const HOT_CUE_KEYS: Partial<Record<(typeof PADS)[number], string>> = { A: "1", B: "2", C: "3" };

/** How often the phase lock checks a locked deck, in milliseconds. */
const PHASE_CHECK_MS = 100;
/**
 * How far off the master's beat a locked deck can be before the lock moves
 * it, in seconds. Two hits further apart than about 10 ms sound as two.
 */
const PHASE_TOLERANCE = 0.01;
/** How long the lock waits after its own move, before it checks again. */
const PHASE_SETTLE_MS = 250;

/**
 * What GRID puts in the pad row, read off `docs/screenshots`.
 *
 * GRID EDIT buttons grouped in pairs. Every button but TAP carries one of our icons, drawn to the
 * grid-edit row capture; each carries an `aria-label` saying what it does.
 *
 * The grid buttons are live: what each does is `useGridEditor`'s, and the
 * edit is written to the track's analysis file by the backend, which
 * updates the beat grid without changing the phrase display.
 */
const GRID_EDITS: readonly (readonly { id: string; label: string }[])[] = [
  [{ id: "mark", label: "Mark the downbeat here" }],
  [{ id: "tap", label: "Tap the tempo" }],
  [
    { id: "shift-back", label: "Shift the grid earlier" },
    { id: "shift-forward", label: "Shift the grid later" },
  ],
  [
    { id: "widen", label: "Slow the grid" },
    { id: "narrow", label: "Speed the grid up" },
  ],
  [
    { id: "double", label: "Double the tempo" },
    { id: "halve", label: "Halve the tempo" },
  ],
  [
    { id: "snap-start", label: "Adjust all beats" },
    { id: "snap-here", label: "Adjust beats from here" },
  ],
  [
    { id: "undo", label: "Undo the last grid edit" },
    { id: "redo", label: "Redo the last grid edit" },
  ],
  [
    { id: "cut-grid", label: "Toggle metronome" },
    { id: "metronome", label: "Metronome" },
    { id: "lock", label: "Lock the grid" },
  ],
];

type IconComponent = (props: { className?: string | undefined }) => React.ReactElement;

/** Each button's face; TAP is a word, and the lock changes with its state. */
const EDIT_ICONS: Record<string, IconComponent> = {
  mark: GridMarkIcon,
  "shift-back": GridShiftBackIcon,
  "shift-forward": GridShiftForwardIcon,
  widen: GridWidenIcon,
  narrow: GridNarrowIcon,
  double: GridDoubleIcon,
  halve: GridHalveIcon,
  "snap-start": GridAlignAllIcon,
  "snap-here": GridAlignHereIcon,
  undo: GridUndoIcon,
  redo: GridRedoIcon,
  "cut-grid": GridCutIcon,
  metronome: GridMetronomeIcon,
  lock: GridLockOpenIcon,
};

/** What a grid-edit button does, is enabled by, and says on hover. */
interface GridButton {
  disabled: boolean;
  pressed: boolean | undefined;
  title: string;
  handlers: React.ButtonHTMLAttributes<HTMLButtonElement>;
}

/**
 * Wires one GRID EDIT button to the editor.
 *
 * The shift and stretch buttons repeat while held, as rekordbox's do; the
 * rest fire once. Every editing button says why it is off — no grid, the
 * library read-only, or the lock — so a greyed button is never a mystery.
 */
function GridBpmField({ value, disabled, onCommit, tapping }: { value: string; tapping: boolean; disabled: boolean; onCommit: (value: string) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  return <input className={styles.bpmField} data-testid="grid-bpm" data-tapping={tapping || undefined} aria-label="Grid BPM" inputMode="decimal"
    disabled={disabled} value={draft ?? value} onFocus={() => setDraft(value)} onChange={event => setDraft(event.target.value)}
    onBlur={() => { if (draft !== null && draft !== value) onCommit(draft); setDraft(null); }}
    onKeyDown={event => {
      event.stopPropagation();
      if (event.key === "Enter") event.currentTarget.blur();
      if (event.key === "Escape") { setDraft(null); }
    }} />;
}

function gridButton(
  id: string,
  editor: ReturnType<typeof useGridEditor>,
  hold: ReturnType<typeof useHoldRepeat>,
  deck: { metronome: boolean; metronomeLevel: string; metronomeBusy: boolean; toggleMetronome: () => void; cycleMetronomeVolume: () => void; idle: boolean; readOnly: boolean },
): GridButton {
  const reason = !editor.hasGrid
    ? "This track has no beat grid to edit"
    : deck.readOnly
      ? READ_ONLY_REASON
      : "";
  const locked = editor.hasGrid && (editor.state?.locked ?? false);
  const once = (action: () => void, title: string): GridButton => ({
    disabled: !editor.canEdit || (editor.fromMs !== null && ["mark", "tap", "shift-back", "shift-forward"].includes(id)),
    pressed: undefined,
    title: editor.canEdit ? title : locked ? "The beat grid is locked" : reason || title,
    handlers: { onClick: action },
  });
  const held = (action: (held?: boolean) => void, title: string): GridButton => ({
    ...once(action, title),
    handlers: hold(action),
  });
  const withCut = (title: string) => (editor.fromMs !== null ? `${title}, from the selected beat on` : title);
  switch (id) {
    case "mark":
      return once(editor.mark, withCut("Make the beat nearest the playhead beat 1"));
    case "tap":
      return once(editor.tap, withCut("Tap the tempo; updates begin with the second tap"));
    case "shift-back":
      return held((repeat) => editor.shift(-1, repeat), withCut("Shift the grid 1 ms earlier"));
    case "shift-forward":
      return held((repeat) => editor.shift(1, repeat), withCut("Shift the grid 1 ms later"));
    case "widen":
      return held((repeat) => editor.stretch(-1, repeat), withCut("Widen the beats: move the target beat 1 ms later, the first beat held"));
    case "narrow":
      return held((repeat) => editor.stretch(1, repeat), withCut("Narrow the beats: move the target beat 1 ms earlier, the first beat held"));
    case "double":
      return once(editor.double, withCut("Double the tempo"));
    case "halve":
      return once(editor.halve, withCut("Halve the tempo"));
    case "snap-start":
      return { ...once(editor.adjustAll, "Adjust all beats"), pressed: editor.fromMs === null };
    case "snap-here":
      return { ...once(editor.adjustFrom, "Adjust beats from here"), pressed: editor.fromMs !== null };
    case "undo":
      return {
        disabled: !(editor.state?.canUndo ?? false) || deck.readOnly,
        pressed: undefined,
        title: editor.state?.canUndo ? "Undo the last grid edit" : "Nothing to undo",
        handlers: { onClick: editor.undo },
      };
    case "redo":
      return {
        disabled: !(editor.state?.canRedo ?? false) || deck.readOnly,
        pressed: undefined,
        title: editor.state?.canRedo ? "Redo the last undone grid edit" : "Nothing to redo",
        handlers: { onClick: editor.redo },
      };
    case "cut-grid":
      return {
        disabled: deck.idle || deck.metronomeBusy || !editor.hasGrid,
        pressed: deck.metronome,
        title: "Toggle metronome",
        handlers: { onClick: deck.toggleMetronome },
      };
    case "metronome":
      return {
        disabled: deck.idle || deck.metronomeBusy,
        pressed: deck.metronome,
        title: `Metronome volume: ${deck.metronomeLevel}. Click to cycle Low → Medium → High.`,
        handlers: { onClick: deck.cycleMetronomeVolume },
      };
    case "lock":
      return {
        disabled: !editor.hasGrid || deck.readOnly,
        pressed: locked,
        title: locked ? "Unlock the beat grid for editing" : "Lock the beat grid so nothing here changes it",
        handlers: { onClick: editor.toggleLock },
      };
    default:
      return { disabled: true, pressed: undefined, title: "", handlers: {} };
  }
}

/**
 * How many rows the MEMORY list draws whatever it holds.
 *
 * Ten, counted off the capture (`docs/screenshots` 9.08.22 PM): four cues and
 * six empty boxes below them, each with its dimmed ✕, filling the panel. A
 * track with no memory cues shows the same ten empty boxes rather than a
 * blank panel; a track with more scrolls.
 */
const MEMORY_ROWS = 10;

/** The panel tabs beside the deck. */
const PANELS = [
  { id: "memory", label: "MEMORY" },
  { id: "hotCue", label: "HOT CUE" },
  { id: "info", label: "INFO" },
] as const;

export const Player = memo(function Player({
  track, onEject, onError, onAnalyse, onDropTrack, onLoadSelected, selectedTrackId = null,
  onExportTrack, devices = [], onExportLoop,
  dragging = false, deck = "a",
  simple = false, transportSlot, flipped = false, dual = false, publishZoom,
  bars: linkedBars, onBars, jumpSize: linkedJump, onJumpSize,
  publishSync, peerSync, isMaster = false, onMaster, synced = false, onSyncToggle,
  leaderBpmX100 = null, onPlayingBpm, onKeyShift, onGridNudge, publishGridFollow, readOnly = false,
}: PlayerProps) {
  const playback = usePlayback(track?.id ?? null, deck, false);
  // The waveforms follow their containers, which change with the window and
  // with the tree splitter — a fixed-width canvas stretched by CSS is blurry
  // on a wide window and wasted resolution on a narrow one.
  const [measureOverview, overview] = useElementSize<HTMLDivElement>();
  const overviewElement = useRef<HTMLDivElement | null>(null);
  const overviewRef = useCallback((element: HTMLDivElement | null) => {
    overviewElement.current = element;
    return measureOverview(element);
  }, [measureOverview]);
  const [detailRef, detail] = useElementSize<HTMLDivElement>();
  // Written to by the frame loop below rather than rendered: see the effect.
  const overviewHead = useRef<HTMLSpanElement>(null);
  const detailHead = useRef<HTMLSpanElement>(null);
  const scrubFill = useRef<HTMLDivElement>(null);
  const barsLabel = useRef<HTMLSpanElement>(null);
  // The grid and the GRID panel's state, kept current by the backend: an
  // edit from any deck refetches both — see `useTrackGrid`.
  const { grid: savedGrid, state: gridState, setState: setGridState } = useTrackGrid(track);
  // A grid shift plays before its save ends: the shifted grid stands in for
  // the saved one until the save comes back. Kept with its track, so a
  // track change drops it.
  const [nudge, setNudge] = useState<{ track: string; grid: TrackBeatGrid } | null>(null);
  const nudgePreview = nudge !== null && nudge.track === track?.id ? nudge.grid : null;
  const grid = nudgePreview ?? savedGrid;
  // The tempo, from the grid where there is one: an edit that changes it
  // reaches here before the browser's row is re-read.
  const bpmX100 = gridState?.bpmX100 ?? track?.bpmX100 ?? 0;
  // The tempo under the playhead, for the GRID panel's field: a grid that
  // changes tempo partway through has to read as the tempo where the edits
  // would land, not as the one it started at. The state's tempo is the first
  // beat's, which is all there is before the grid arrives.
  const [gridBpmX100, setGridBpmX100] = useState(bpmX100);
  const { positionRef, subscribe, positionNow } = playback;
  useEffect(() => {
    let previous: number | undefined;
    const update = (seconds: number) => {
      const next = tempoAtMs(grid, seconds * 1000) || bpmX100;
      if (next !== previous) { previous = next; setGridBpmX100(next); }
    };
    update(positionRef.current);
    return subscribe(update);
  }, [grid, bpmX100, positionRef, subscribe]);
  const [phrases, setPhrases] = useState<Phrase[]>([]);
  const [ownBars, setOwnBars] = useState<number>(DETAIL_BARS);
  // Linked or its own, and the setter follows whichever it is: a controlled
  // zoom that kept updating a local copy would fight the link on every change.
  const bars = linkedBars ?? ownBars;
  // The zoom last asked of the shell, ahead of the render that shows it. A
  // fast scroll delivers several wheel events before the shell re-renders,
  // and each has to step from the one before, not from this render's value.
  const requestedBars = useRef(bars);
  useLayoutEffect(() => {
    requestedBars.current = bars;
  }, [bars]);
  const setBars = useCallback(
    (next: number | ((current: number) => number)) => {
      const resolve = (current: number) =>
        typeof next === "function" ? next(current) : next;
      if (onBars) {
        requestedBars.current = resolve(requestedBars.current);
        onBars(requestedBars.current);
      } else setOwnBars(resolve);
    },
    [onBars],
  );
  const [padMode, setPadMode] = useState<PadMode>("cue");
  const [panel, setPanel] = useState<CuePanel>("memory");
  const [cueColorMenu, setCueColorMenu] = useState<{x: number; y: number; cue: Cue} | null>(null);
  const writeCue = useCueWriter(onError);
  /**
   * Where CUE returns to. A track opens on its first memory cue, which is
   * where rekordbox and a CDJ both put the playhead, and CUE moves it from
   * there the way the deck does.
   */
  const [cuePoint, setCuePoint] = useState(0);
  // Kept current by the backend: an edit from any deck refetches. The cue
  // point is settled from the first fetch only — see `useTrackCues`.
  const cues = useTrackCues(track, (loaded) => {
    const first = cuesFor(loaded, "memory")[0];
    setCuePoint(first ? first.positionMs / 1000 : 0);
  });
  // The INFO tab's record, fetched only while that tab is showing — see
  // `useTrackDetails` for why not on every load.
  const details = useTrackDetails(track?.id ?? null, panel === "info");
  /**
   * Quantize — the Q button at the end of the pad row. On by default, as a CDJ
   * ships: a cue set by hand lands tens of milliseconds off the beat, and every
   * loop and mix taken from it inherits that.
   */
  const [quantize, setQuantize] = useState(true);
  const { preferences, update: updatePreferences } = usePreferencesContext();
  const [metronomeMenu, setMetronomeMenu] = useState<{x: number; y: number} | null>(null);
  const [metronome, setMetronome] = useState(false);
  const [metronomeBusy, setMetronomeBusy] = useState(false);
  const metronomePending = useRef(false);
  const metronomeLevel = preferences.audio.metronomeVolume === "small" ? "Low" : preferences.audio.metronomeVolume === "middle" ? "Medium" : "High";
  const toggleMetronome = useCallback(() => {
    if (metronomePending.current) return;
    const volume = preferences.audio.metronomeVolume;
    const on = !metronome;
    metronomePending.current = true;
    setMetronomeBusy(true);
    void getBackend().then(async (backend) => {
      if (on) await backend.setMetronome(preferences.audio.metronomeSound, volume);
      await backend.deckMetronome(deck, on);
      if (on) updatePreferences("audio", { metronomeVolume: volume });
      setMetronome(on);
    }).catch((e: unknown) => onError?.(e instanceof Error ? e.message : "The metronome could not be changed."))
      .finally(() => { metronomePending.current = false; setMetronomeBusy(false); });
  }, [metronome, preferences.audio.metronomeVolume, preferences.audio.metronomeSound, updatePreferences, deck, onError]);
  const cycleMetronomeVolume = useCallback(() => {
    const volume = preferences.audio.metronomeVolume === "small" ? "middle" : preferences.audio.metronomeVolume === "middle" ? "large" : "small";
    void getBackend().then(async backend => {
      await backend.setMetronome(preferences.audio.metronomeSound, volume);
      updatePreferences("audio", { metronomeVolume: volume });
    }).catch(() => onError?.("The metronome volume could not be changed."));
  }, [preferences.audio.metronomeVolume, preferences.audio.metronomeSound, updatePreferences, onError]);
  const { view: viewPrefs, advanced: advancedPrefs } = preferences;
  // The ≡ menu at the foot of the deck.
  const [deckMenuAt, setDeckMenuAt] = useState<{ x: number; y: number } | null>(null);
  const chooseFromDeckMenu = useCallback(
    (action: DeckAction) => {
      if (action.startsWith("exportTrack:")) {
        if (track) onExportTrack?.(action.slice("exportTrack:".length), track.id);
        return;
      }
      switch (action) {
        case "exportLoopWav":
          if (track && playback.loop) {
            onExportLoop?.(track.id, track.title, playback.loop.inSeconds * 1000, playback.loop.outSeconds * 1000);
          }
          break;
        case "waveformBlue": updatePreferences("view", { waveformColor: "blue" }); break;
        case "waveformRgb": updatePreferences("view", { waveformColor: "rgb" }); break;
        case "waveform3band": updatePreferences("view", { waveformColor: "3band" }); break;
        case "beatPosition": updatePreferences("view", { beatCount: "position" }); break;
        case "beatToMemoryBars": updatePreferences("view", { beatCount: "toMemoryBars" }); break;
        case "beatToMemoryBeats": updatePreferences("view", { beatCount: "toMemoryBeats" }); break;
        case "waveformClickOn": updatePreferences("view", { waveformClick: true }); break;
        case "waveformClickOff": updatePreferences("view", { waveformClick: false }); break;
        case "analyse": if (track) onAnalyse?.(track.id, track.title); break;
        default: break;
      }
    },
    [updatePreferences, track, onAnalyse, onExportTrack, onExportLoop, playback.loop],
  );
  const tip = useTooltip();
  // QUANTIZE BEAT VALUE in Preferences: the grid every quantized cue snaps
  // to, split as finely as the value asks.
  const quantizeGrid = useMemo(
    () => subdivideGrid(grid, 1 / quantizeFraction(advancedPrefs.quantizeBeat)),
    [grid, advancedPrefs.quantizeBeat],
  );
  /** How far a jump moves, chosen from the size menu. */
  const [ownJumpSizeId, setOwnJumpSizeId] = useState<string>(JUMP_SIZE_ID);
  const jumpSizeId = linkedJump ?? ownJumpSizeId;
  const setJumpSizeId = onJumpSize ?? setOwnJumpSizeId;
  /** Where the size menu is open, in client coordinates, or closed. */
  const [jumpMenu, setJumpMenu] = useState<{ x: number; y: number } | null>(null);
  const jumpButton = useRef<HTMLButtonElement>(null);
  /**
   * Whether the deck has the keyboard.
   *
   * Armed by clicking it and dropped by clicking anything else, which is how a
   * CDJ's deck behaves and what makes the arrow keys unambiguous: the same
   * keys move the browser's cursor when the browser has it.
   */
  const [armed, setArmed] = useState(false);
  const platform = useMemo(detectPlatform, []);

  // Handed up as it changes, so the status bar owns the only place the app
  // says something went wrong.
  const { error: deckError } = playback;
  useEffect(() => {
    onError?.(deckError);
  }, [deckError, onError]);
  /**
   * Where the scrolling layer is drawn from. Not the playhead: the layer is
   * drawn once across `OVERDRAW` spans and slid by a transform, and it is
   * redrawn only when the head has travelled far enough to see its edge.
   */
  const [anchor, setAnchor] = useState(0);
  /** The anchor the layer is actually showing, so the slide never leads it. */
  const drawn = useRef(0);
  const scroller = useRef<HTMLDivElement>(null);

  // Bumped when the track's analysis is rewritten — a phrase edit, or a
  // re-analysis — so the strip is read again.
  const [phraseRevision, setPhraseRevision] = useState(0);
  useEffect(() => {
    if (!track) {
      setPhrases([]);
      return;
    }
    let live = true;
    void (async () => {
      const backend = await getBackend();
      const found = await backend.trackPhrases(track.id);
      // The track may have changed while this was in flight.
      if (live) setPhrases(found);
    })();
    return () => {
      live = false;
    };
  }, [track, phraseRevision]);
  useEffect(() => {
    if (!track) return undefined;
    let live = true;
    let stop: (() => void) | undefined;
    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      stop = backend.onAnalysisChanged((changed) => {
        if (changed === track.id) setPhraseRevision((r) => r + 1);
      });
    })();
    return () => {
      live = false;
      stop?.();
    };
  }, [track]);

  // Falls back to the track's own length before the file's metadata has
  // loaded, so nothing jumps when it arrives.
  const total = playback.duration || track?.durationSec || 0;

  // Bars rather than a fraction: twelve bars is twelve bars whether the track
  // is three minutes or ninety.
  const span = detailSpan(bars, bpmX100, total);
  // Memoised, not rebuilt each render: `BeatGrid` and `CueMarkers` are
  // `memo()` components taking this object, and a fresh one every render means
  // neither ever hits its memo.
  const window = useMemo(() => windowAround(anchor, span * OVERDRAW), [anchor, span]);

  // The beats the detail window actually draws, found by binary search rather
  // than by filtering the whole grid on every tick.
  const beats = useMemo(
    () => (total > 0 ? beatsIn(grid, window.from * total * 1000, window.to * total * 1000) : []),
    [grid, window, total],
  );

  // The tempo, for the bar count the frame loop prints.
  const bpm = bpmX100 / 100;
  // The memory cues' positions, for the count-down modes of the beat count.
  const memorySeconds = useMemo(
    () => cues.filter((cue) => cue.memory).map((cue) => cue.positionMs / 1000),
    [cues],
  );
  const beatCount = viewPrefs.beatCount;

  /*
   * The playhead, written straight to its elements every frame.
   *
   * A transform through a ref moves it on the compositor without laying
   * anything out. Time labels subscribe separately to displayed tenths;
   * the deck only updates its canvas anchor when the buffered slice runs out.
   */
  useEffect(() => {
    const x = (headPercent() / 100) * detail.width;
    if (detailHead.current) detailHead.current.style.transform = `translateX(${x}px)`;
    if (barsLabel.current) barsLabel.current.style.transform = `translateX(${x}px)`;
    const apply = (seconds: number) => {
      const at = total > 0 ? Math.min(seconds / total, 1) : 0;
      if (overviewHead.current) {
        overviewHead.current.style.transform = `translateX(${Math.max(0, at) * overview.width}px)`;
      }
      if (scrubFill.current) scrubFill.current.style.transform = `scaleX(${Math.max(0, at)})`;
      // The detail head does not move at all: the layer under it does, by a
      // transform on the compositor rather than a redraw. Redrawing the canvas
      // from React state stepped it at the tick rate — ten times a second,
      // which is what made a scrolling waveform look like a slideshow.
      if (scroller.current) {
        const dx = scrollOffset(at, drawn.current, span, detail.width);
        scroller.current.style.transform = `translateX(${dx}px)`;
      }
      if (needsRedraw(at, drawn.current, span)) setAnchor(at);
      if (barsLabel.current) {
        const text = beatCountText(seconds, bpm, beatCount, memorySeconds, grid);
        if (barsLabel.current.textContent !== text) barsLabel.current.textContent = text;
      }
      const progress = overviewElement.current;
      const position = String(Math.round(seconds));
      if (progress && progress.getAttribute("aria-valuenow") !== position) progress.setAttribute("aria-valuenow", position);
    };
    // At once as well as on every frame: a paused player schedules no frames,
    // and the head would otherwise sit where the last track left it.
    apply(positionRef.current);
    return subscribe(apply);
  }, [total, bpm, span, overview.width, detail.width, positionRef, subscribe, beatCount, memorySeconds, grid, overviewRef, simple, dual]);

  /*
   * The layer's new anchor, taken only once it is on screen.
   *
   * A layout effect, and after the canvas's own: children run first, so by the
   * time this slides the layer back the redraw it is sliding for has already
   * happened. Updating the anchor in the frame loop instead moved the layer a
   * frame before its contents caught up, which showed as a jump.
   */
  useLayoutEffect(() => {
    drawn.current = anchor;
    if (scroller.current) {
      const at = total > 0 ? Math.min(positionRef.current / total, 1) : 0;
      scroller.current.style.transform = `translateX(${scrollOffset(at, anchor, span, detail.width)}px)`;
    }
  }, [anchor, span, total, detail.width, positionRef]);

  /**
   * The wheel zooms, over the waveform it is pointing at.
   *
   * Travel is accumulated and rate-limited (see `createWheelZoomGate`): a
   * trackpad streams many small events plus inertia, and a step per event
   * ran through the whole zoom range in one swipe.
   */
  const wheelGate = useRef(createWheelZoomGate());
  const wheelZoom = useCallback((event: React.WheelEvent<HTMLDivElement>) => {
    if (event.deltaY === 0) return;
    event.preventDefault();
    // Line and page modes (mice on some platforms) report lines, not pixels.
    const px = event.deltaY * (event.deltaMode === 1 ? 33 : event.deltaMode === 2 ? 400 : 1);
    const direction = wheelGate.current(px, event.timeStamp);
    if (direction !== 0) setBars((current) => zoomBy(current, direction));
  }, [setBars]);

  const zoom = useCallback((by: number) => {
    setBars((current) => {
      const at = ZOOM_STEPS.indexOf(current as (typeof ZOOM_STEPS)[number]);
      // An unrecognised value snaps back to the default rather than sticking.
      const from = at === -1 ? ZOOM_STEPS.indexOf(DETAIL_BARS) : at;
      const to = Math.min(Math.max(from + by, 0), ZOOM_STEPS.length - 1);
      return ZOOM_STEPS[to] ?? DETAIL_BARS;
    });
  }, [setBars]);

  /*
   * The deck takes the keyboard when it is clicked and gives it up when
   * anything else is. A document listener rather than `onBlur`: the browser
   * and the tree are not focusable containers, so there is nothing to blur to
   * — what matters is that the pointer went down somewhere that is not here.
   */
  const shell = useRef<HTMLElement>(null);
  useEffect(() => {
    const elsewhere = (event: PointerEvent) => {
      const box = shell.current;
      if (!box) return;
      setArmed(box.contains(event.target as Node));
    };
    document.addEventListener("pointerdown", elsewhere, true);
    return () => document.removeEventListener("pointerdown", elsewhere, true);
  }, []);

  // What the other deck reads when its BEAT SYNC is pressed. A ref holding a
  // closure over the current render, registered once: the shell keeps the
  // getter, not the values, so nothing here re-renders anything there.
  const syncState = useRef<() => SyncDeck | null>(() => null);
  syncState.current = () =>
    track
      ? {
          bpmX100,
          tempo: playback.tempo,
          playing: playback.playing,
          position: playback.positionNow(),
          grid,
        }
      : null;
  useEffect(() => {
    publishSync?.(() => syncState.current());
  }, [publishSync]);

  /** BEAT SYNC lit and Q on: the deck must stay on the master's beat. */
  const phaseLocked = synced && quantize;
  /**
   * Where a move on a locked, playing deck lands: on the master's beat, by
   * `inPhaseAt`. A master that is stopped has no beat to keep, so the move
   * stays as it is.
   */
  const inPhase = useEventCallback((at: number) => {
    if (!phaseLocked || !playback.playing) return at;
    const leader = peerSync?.();
    const follower = syncState.current();
    if (!leader?.playing || !follower) return at;
    return inPhaseAt(leader, follower, at, playback.loop?.active ? playback.loop : null);
  });
  const seekInPhase = useCallback((at: number) => playback.seek(inPhase(at)), [playback, inPhase]);

  /** Moves by the chosen size: a number of beats, or the fine nudge. */
  const jump = useCallback(
    (direction: number) => {
      const step = jumpStepSeconds(jumpSizeById(jumpSizeId), bpmX100);
      if (step === 0) return;
      seekInPhase(playback.positionRef.current + step * direction);
    },
    [jumpSizeId, bpmX100, playback, seekInPhase],
  );


  /*
   * CUE, as a CDJ does it: stop and rewind while playing, preview while held
   * on the cue point, set the cue point anywhere else. `pressCue` decides
   * which; this only carries it out and remembers whether a preview is running.
   */
  const previewing = useRef(false);

  const holdCue = useCallback(() => {
    if (playback.idle) return;
    const action = pressCue(
      playback.positionRef.current,
      cuePoint,
      playback.playing,
      quantize ? quantizeGrid : null,
    );
    previewing.current = action.playing;
    if (action.cuePoint !== cuePoint) setCuePoint(action.cuePoint);
    if (action.seekTo !== null) playback.seek(action.seekTo);
    if (action.playing !== playback.playing) playback.toggle();
  }, [playback, cuePoint, quantize, quantizeGrid]);

  const dropCue = useCallback(() => {
    const action = releaseCue(previewing.current, cuePoint);
    previewing.current = false;
    if (!action) return;
    playback.seek(action.seekTo ?? cuePoint);
    if (playback.playing) playback.toggle();
  }, [playback, cuePoint]);

  const seek = seekInPhase;
  const positionSeconds = useCallback(() => positionRef.current, [positionRef]);
  const memory = useMemoryCues({
    trackId: playback.idle ? null : track?.id ?? null,
    cues, positionSeconds, seek, setLoop: playback.setLoop, cuePoint, setCuePoint, readOnly, onError,
  });
  const hot = useHotCues({
    trackId: playback.idle ? null : track?.id ?? null,
    cues, positionSeconds, seek, quantiseTo: quantize ? quantizeGrid : null, readOnly, onError,
  });
  // The hooks own editability, so the disabled state and its explanation
  // must come from that same result. Keeping a second readOnly-only branch
  // let a cached WebKit render show an enabled-action tooltip on a control
  // that the hook had already disabled.
  const hotCueEditReason = hot.canEdit ? undefined : READ_ONLY_REASON;
  const memoryCueEditReason = memory.canEdit ? undefined : READ_ONLY_REASON;
  // The GRID EDIT cluster. The playhead it reads is the extrapolated one:
  // a beat is a few frames, and the frame loop's copy can be a frame behind.
  const positionMs = useCallback(() => positionNow() * 1000, [positionNow]);
  const isDynamicFrom = useCallback((from: number | null) => {
    const startTime = from === null ? 0 : nearestBeatMs(grid, from);
    const start = Math.max(0, grid.times.findIndex(time => time >= startTime));
    return grid.tempos.slice(start).some(tempo => tempo !== grid.tempos[start]);
  }, [grid]);
  /**
   * BEAT SYNC lit: the deck moves `ms` of its own track with a grid shift,
   * so it keeps its place against the master's beat. A move by the shift,
   * not a new match, so an offset the DJ chose stays.
   */
  const followShift = useCallback((ms: number) => {
    if (!synced || !playback.playing || advancedPrefs.syncType === "bpm") return;
    playback.moveBy(ms / 1000);
  }, [synced, playback, advancedPrefs.syncType]);
  // The master's grid moved `ms` of its track later: its beat comes that
  // much later in time, so this deck goes back by the same time, measured
  // at the two decks' tempos.
  const followMaster = useEventCallback((ms: number) => {
    followShift(-ms * playback.tempo / (peerSync?.()?.tempo ?? 1));
  });
  useEffect(() => {
    publishGridFollow?.(followMaster);
  }, [publishGridFollow, followMaster]);
  const totalMs = Math.round(total * 1000);
  const onNudge = useCallback((ms: number) => {
    const id = track?.id;
    if (id === undefined) return;
    setNudge(p => ({ track: id, grid: nudgeGrid(p?.track === id ? p.grid : savedGrid, ms, totalMs) }));
    followShift(ms);
    onGridNudge?.(ms);
  }, [track?.id, savedGrid, totalMs, followShift, onGridNudge]);
  // A save that fails leaves the saved grid as it was, so the deck goes back to it.
  const onGridError = useCallback((message: string | null) => {
    if (message !== null) setNudge(null);
    onError?.(message);
  }, [onError]);
  const gridEditor = useGridEditor({
    trackId: playback.idle ? null : track?.id ?? null,
    deck, state: gridState, setState: setGridState, positionMs, readOnly, onError: onGridError,
    durationMs: totalMs,
    isDynamicFrom,
    onNudge,
  });
  const { state: historyState, undo: undoGrid, redo: redoGrid } = gridEditor;
  useEffect(() => {
    if (!armed || readOnly) return;
    return listenEditHistory((action) => {
      if (action === "undo" && historyState?.canUndo) undoGrid();
      if (action === "redo" && historyState?.canRedo) redoGrid();
    },
    historyState?.canUndo ? historyState.undoLabel ?? "Beat Grid Edit" : null,
    historyState?.canRedo ? historyState.redoLabel ?? "Beat Grid Edit" : null);
  }, [armed, readOnly, historyState, undoGrid, redoGrid]);
  const hold = useHoldRepeat();

  // Loops. AU sets a beat loop of the chosen length from the head, snapped
  // to the grid when Q is on; MA takes IN and OUT by hand. RELOOP/EXIT and
  // the loop itself are the engine's, read back on every tick, so what the
  // buttons show is what sounds. Nothing is written to the library.
  const [loopMode, setLoopMode] = useState<"auto" | "manual">("auto");
  const [loopBeats, setLoopBeats] = useState(4);
  /** A LOOP IN pressed and waiting for its OUT, in seconds. */
  const [loopIn, setLoopIn] = useState<number | null>(null);
  const activeLoop = playback.loop?.active ?? false;
  // A locked deck loops on whole beats: a loop IN between two beats, or a
  // length such as 1.5 beats, takes the deck off the master's beat.
  const loopSnap = quantize ? (phaseLocked ? grid : quantizeGrid) : null;
  /** A loop of so many beats from the playhead, on the grid when Q is on. */
  const loopOfBeats = useCallback((beats: number) => {
    if (playback.idle) return;
    const range = beatLoopRange(grid, loopSnap, playback.positionNow() * 1000, beats);
    if (range) playback.setLoop(range[0] / 1000, range[1] / 1000);
  }, [playback, grid, loopSnap]);
  const autoLoop = useCallback(() => {
    if (playback.idle) return;
    if (activeLoop) {
      playback.setLoopActive(false);
      return;
    }
    loopOfBeats(loopBeats);
  }, [playback, activeLoop, loopOfBeats, loopBeats]);
  /** The head in seconds, on the loop snap grid when Q is on. */
  const loopPoint = useCallback(() => {
    const ms = playback.positionNow() * 1000;
    return (loopSnap && loopSnap.times.length > 0 ? nearestBeatMs(loopSnap, ms) : ms) / 1000;
  }, [playback, loopSnap]);
  const markLoopIn = useCallback(() => {
    if (playback.idle) return;
    setLoopIn(loopPoint());
  }, [playback, loopPoint]);
  const markLoopOut = useCallback(() => {
    if (playback.idle || loopIn === null) return;
    const out = loopPoint();
    if (out > loopIn) playback.setLoop(loopIn, out);
    setLoopIn(null);
  }, [playback, loopIn, loopPoint]);
  const reloopOrExit = useCallback(() => {
    if (!playback.loop) return;
    playback.setLoopActive(!playback.loop.active);
  }, [playback]);
  /**
   * Halve or double the loop, from ‹ › or the half and double keys. A loop
   * that is playing takes the new length at once, from its own in point: a
   * beat loop on the grid, a manual loop in time. A head past the new out
   * point keeps its place in the beat: it goes back by whole loops, not to
   * the in point. With no loop playing, only the next beat loop changes.
   */
  const resizeLoop = useCallback((factor: number) => {
    const loop = playback.loop;
    if (!loop?.active || playback.idle) {
      setLoopBeats((beats) => clampLoopBeats(beats * factor));
      return;
    }
    // Rounded, so a loop from an on-beat IN finds its beat on the grid.
    const resized = resizedLoopRange(grid, Math.round(loop.inSeconds * 1000), loop.outSeconds * 1000, loopBeats, factor);
    if (!resized) return;
    setLoopBeats(resized.beats);
    const [from, to] = [resized.range[0] / 1000, resized.range[1] / 1000];
    playback.setLoop(from, to);
    const head = playback.positionNow();
    const wrapped = wrapIntoLoop(head, from, to);
    if (wrapped !== head) playback.seek(wrapped);
  }, [playback, grid, loopBeats]);
  /** AU or MA. A change drops an IN that waits for its OUT. */
  const chooseLoopMode = useCallback((mode: "auto" | "manual") => {
    setLoopMode(mode);
    setLoopIn(null);
  }, []);
  /** OUT: the end of a waiting IN, or else RELOOP/EXIT. Both layouts. */
  const loopOut = useCallback(() => {
    if (loopIn !== null) markLoopOut();
    else reloopOrExit();
  }, [loopIn, markLoopOut, reloopOrExit]);
  // The two-deck control row: AU starts a loop of the chosen length from
  // the head and MA takes IN and OUT by hand, both on the grid when Q is on.
  // The handlers are the one-deck layout's, so the two behave the same.
  const dualLoop = useMemo(() => ({
    mode: loopMode,
    onMode: chooseLoopMode,
    beats: loopBeats,
    onShorter: () => resizeLoop(0.5),
    onLonger: () => resizeLoop(2),
    active: activeLoop,
    pendingIn: loopIn !== null,
    canLoop: !playback.idle && grid.times.length >= 2,
    idle: playback.idle,
    onToggle: autoLoop,
    onIn: loopMode === "auto" ? () => loopOfBeats(loopBeats) : markLoopIn,
    onOut: loopOut,
    hasLoop: playback.loop !== null,
  }), [loopMode, chooseLoopMode, loopBeats, resizeLoop, activeLoop, loopIn, playback.idle, playback.loop, grid.times.length,
    autoLoop, loopOfBeats, markLoopIn, loopOut]);
  // A play is recorded after a minute of the track sounding, once per load,
  // when Preferences › Advanced › History says so and the library can be
  // written. rekordbox's own threshold is not recorded; a minute is what
  // tells a track played from one auditioned [ASSUME].
  const playedSeconds = useRef(0);
  const recordedFor = useRef<string | null>(null);
  const recordHistory = advancedPrefs.recordHistory;
  useEffect(() => {
    playedSeconds.current = 0;
    recordedFor.current = null;
  }, [track?.id]);
  useEffect(() => {
    if (!playback.playing || !track || !recordHistory || readOnly) return undefined;
    const id = track.id;
    const timer = globalThis.setInterval(() => {
      playedSeconds.current += 1;
      if (playedSeconds.current < PLAY_RECORD_SECONDS || recordedFor.current === id) return;
      recordedFor.current = id;
      void getBackend()
        .then((backend) => backend.edits.recordPlay(id))
        .catch((e: unknown) => onError?.(e instanceof Error ? e.message : "The play could not be recorded."));
    }, 1000);
    return () => globalThis.clearInterval(timer);
  }, [playback.playing, track, recordHistory, readOnly, onError]);

  /** A list row: a cue is a jump, a memory loop is the loop itself. */
  const callCue = useCallback(
    (cue: Cue) => {
      if (cue.outMs > cue.positionMs) {
        playback.setLoop(cue.positionMs / 1000, cue.outMs / 1000);
      } else {
        seekInPhase(cue.positionMs / 1000);
      }
    },
    [playback, seekInPhase],
  );

  /** The phase lock waits until this time: see `checkPhase`. */
  const lockHold = useRef(0);
  /**
   * PLAY. With BEAT SYNC lit and Q on, a stopped deck starts on the beat, as
   * a CDJ with SYNC and QUANTIZE does: it is put on its own nearest beat and
   * held until the master's next one lands, so the two are on the beat
   * together from the first sound. The wait is the engine's, counted in
   * output frames. A master that is not running has no next beat to wait
   * for, so the deck is lined up with it and started at once. A deck already
   * playing, or one following nothing, simply toggles.
   */
  const togglePlay = useCallback(() => {
    // PLAY while a CUE preview is held: the preview becomes real playback, so
    // letting go of CUE no longer snaps back to the cue point. The deck is
    // already running; pausing it here would defeat the gesture.
    if (previewing.current && playback.playing) {
      previewing.current = false;
      return;
    }
    if (!playback.playing && synced && quantize) {
      const leader = peerSync?.();
      const follower = syncState.current();
      if (leader && follower) {
        const wait = leader.playing ? beatWait(leader) : null;
        if (wait !== null && grid.times.length > 0) {
          const onBeat = nearestBeatMs(grid, follower.position * 1000) / 1000;
          if (Math.abs(onBeat - follower.position) > 0.001) playback.seek(onBeat);
          playback.playAfter(wait * 1000);
          // The lock waits for the held start to end, so it does not move
          // the deck before the deck sounds.
          lockHold.current = performance.now() + wait * 1000 + PHASE_SETTLE_MS;
          return;
        }
        const nudge = beatNudgeFor(leader, follower);
        if (Math.abs(nudge) > 0.001) playback.seek(follower.position + nudge);
      }
    }
    playback.toggle();
  }, [playback, synced, quantize, peerSync, grid]);

  /*
   * The phase lock. A locked deck that plays is checked ten times a second,
   * and it goes back onto the master's beat when it is more than
   * PHASE_TOLERANCE off. This catches what no single press can: a loop that
   * starts again, a jump or a hot cue on the master, and the drift of a grid
   * whose tempo changes. A drag holds the lock off until the drag lands.
   */
  const checkPhase = useEventCallback(() => {
    const now = performance.now();
    if (now < lockHold.current || playback.isScrubbing()) return;
    const head = playback.positionNow();
    const to = inPhase(head);
    if (Math.abs(to - head) <= PHASE_TOLERANCE) return;
    // A move from the engine's own head, not a seek to `to`: a seek lands
    // late by the time the command takes, and the lock then moves again.
    playback.moveBy(to - head);
    // The ticks run a command behind the move: let the move arrive first.
    lockHold.current = now + PHASE_SETTLE_MS;
  });
  useEffect(() => {
    if (!phaseLocked || !playback.playing) return undefined;
    const timer = globalThis.setInterval(checkPhase, PHASE_CHECK_MS);
    return () => globalThis.clearInterval(timer);
  }, [phaseLocked, playback.playing, checkPhase]);

  // AppleScript's PLAY and pause, read at the moment a script asks, and the
  // same PLAY a click gives: see `src/lib/scripting.ts`.
  const scriptTrack = useEventCallback(() => track?.id ?? null);
  const scriptIdle = useEventCallback(() => playback.idle);
  const scriptPlaying = useEventCallback(() => playback.playing);
  const scriptError = useEventCallback(() => playback.error);
  const scriptPlay = useEventCallback(() => togglePlay());
  useEffect(
    () => registerDeck(deck, {
      track: scriptTrack, idle: scriptIdle, playing: scriptPlaying, error: scriptError, togglePlay: scriptPlay,
    }),
    [deck, scriptTrack, scriptIdle, scriptPlaying, scriptError, scriptPlay],
  );


  /**
   * The overview is a scrubber: the pointer goes where you put it, and holding
   * it down drags the playhead along the track.
   */
  const scrubOverview = (event: React.PointerEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    const box = element.getBoundingClientRect();
    if (box.width <= 0) return;
    element.setPointerCapture(event.pointerId);
    playback.scrubBegin();
    playback.scrubTo(((event.clientX - box.left) / box.width) * total);
  };

  const dragOverview = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
    const box = event.currentTarget.getBoundingClientRect();
    if (box.width <= 0) return;
    playback.scrubTo(((event.clientX - box.left) / box.width) * total);
  };

  /**
   * The detail is the record, not a scrubber: it moves *with* the pointer, so
   * dragging right pulls earlier music into view. Absolute seeking here would
   * jump the track by half a window on the first pixel of movement, because
   * the head sits in the middle whatever it is pointing at.
   */
  const grab = useRef<{ x: number; y: number; at: number } | null>(null);

  const startDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    grab.current = { x: event.clientX, y: event.clientY, at: playback.positionRef.current };
    playback.scrubBegin();
  };

  /**
   * A press let go where it landed: View › Display Type › Click on the
   * waveform for PLAY and CUE. A stopped deck plays; a playing one pauses
   * and takes the playhead as its cue point, the way a CUE press does. The
   * head does not move for a click — a drag is what moves it. Off, a click
   * does nothing.
   */
  const clickDetail = () => {
    if (!viewPrefs.waveformClick || playback.idle || total <= 0) return;
    if (playback.playing) {
      setCuePoint(playback.positionNow());
      playback.toggle();
      return;
    }
    // PLAY, so a synced deck starts on the master's beat as the button does.
    togglePlay();
  };

  const dragDetail = (event: React.PointerEvent<HTMLDivElement>) => {
    const held = grab.current;
    if (!held || !event.currentTarget.hasPointerCapture(event.pointerId)) return;
    const box = event.currentTarget.getBoundingClientRect();
    playback.scrubTo(held.at + dragSeconds(event.clientX - held.x, box.width, span, total));
  };

  const endDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const held = grab.current;
    grab.current = null;
    const click = held !== null && event.type === "pointerup" && isClick(event.clientX - held.x, event.clientY - held.y);
    // A click is PLAY or CUE, not a move, so it is left where it is. A drag
    // on a locked deck lands on the master's beat: see `inPhase`.
    playback.scrubEnd(!click && phaseLocked && playback.playing ? inPhase : undefined);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    if (click) clickDetail();
  };

  // A beat's length, for phrases whose time the grid did not resolve.
  const beatMs = bpmX100 > 0 ? 6_000_000 / bpmX100 : 0;

  useEffect(() => {
    publishZoom?.(zoom);
  }, [publishZoom, zoom]);

  // What this deck is playing at, for a synced deck to follow: the file's
  // tempo times the deck's. Reported when either changes and nothing else.
  const playingBpmX100 = track && bpmX100 > 0 ? Math.round(bpmX100 * playback.tempo) : null;
  useEffect(() => {
    onPlayingBpm?.(playingBpmX100);
  }, [onPlayingBpm, playingBpmX100]);

  // The key this deck sounds in moves with its key shift, and so does the
  // set of keys the Traffic Light lights against it.
  const keyShift = track ? playback.keyShift : 0;
  useEffect(() => {
    onKeyShift?.(keyShift);
  }, [onKeyShift, keyShift]);

  /** Match this deck to the other one: its tempo, then its bar. */
  const matchLeader = useCallback(() => {
    const leader = peerSync?.();
    const follower = syncState.current();
    if (!leader || !follower) return;
    // BEAT/BPM SYNC in Preferences: whether the bar is matched as well as
    // the tempo, and whether a double or half BPM counts as the same tempo.
    const { tempo, nudge } = syncTo(leader, follower, {
      type: advancedPrefs.syncType,
      doubleHalf: advancedPrefs.syncDoubleHalf,
    });
    playback.setTempo(tempo);
    // The nudge second: it is measured against where the follower is now, and
    // the tempo does not move the playhead.
    if (Math.abs(nudge) > 0.001) playback.seek(follower.position + nudge);
  }, [peerSync, playback, advancedPrefs.syncType, advancedPrefs.syncDoubleHalf]);

  /**
   * BEAT SYNC: lights and matches, or goes out. Lit, the deck keeps the
   * master's tempo — the effect below re-matches on every change to it — and
   * the bar is matched once, now, as a CDJ does on the press.
   */
  const beatSync = useCallback(() => {
    if (!onSyncToggle) {
      matchLeader();
      return;
    }
    if (!synced) matchLeader();
    onSyncToggle();
  }, [onSyncToggle, synced, matchLeader]);

  // Following: the tempo alone, with the bar left where the press put it.
  // The master's BPM arrives from the shell rather than being read through
  // the getter, so this runs exactly when that BPM changes and never on a
  // frame.
  const fileBpmX100 = bpmX100;
  useEffect(() => {
    if (!synced || leaderBpmX100 === null || leaderBpmX100 <= 0 || fileBpmX100 <= 0) return;
    const tempo = tempoFor(
      { bpmX100: leaderBpmX100, position: 0, grid: NO_BEATS },
      { bpmX100: fileBpmX100, position: 0, grid: NO_BEATS },
      { doubleHalf: advancedPrefs.syncDoubleHalf },
    );
    if (Math.abs(tempo - playback.tempo) > 1e-4) playback.setTempo(tempo);
  }, [synced, leaderBpmX100, fileBpmX100, advancedPrefs.syncDoubleHalf, playback]);

  // The metronome clicks on the grid the deck shows: a shift press at once,
  // the saved grid when the save comes back. The first grid is the load's,
  // which gives it to the engine itself.
  const metronomeGrid = useRef(grid);
  useEffect(() => {
    if (metronomeGrid.current === grid) return;
    metronomeGrid.current = grid;
    const pairs = Array.from(grid.times, (ms, i): [number, boolean] => [ms, grid.numbers[i] === 1]);
    void getBackend().then(backend => backend.setMetronomeGrid(deck, pairs)).catch(() => {});
  }, [grid, deck]);

  // The stand-in goes once nothing is left to save and the saved grid has
  // caught up: equal to it, or changed since the last save ended (an undo,
  // or an edit from the other deck). A refetch that does not come in a
  // second is not waited for.
  const savedWhenIdle = useRef<TrackBeatGrid | null>(null);
  useEffect(() => {
    if (gridEditor.nudging || nudgePreview === null) {
      savedWhenIdle.current = null;
      return;
    }
    const drop = () => {
      savedWhenIdle.current = null;
      setNudge(null);
    };
    savedWhenIdle.current ??= savedGrid;
    const same = savedGrid.times.length === nudgePreview.times.length
      && savedGrid.times.every((ms, i) => ms === nudgePreview.times[i]);
    if (same || savedGrid !== savedWhenIdle.current) {
      drop();
      return;
    }
    const late = setTimeout(drop, 1000);
    return () => clearTimeout(late);
  }, [gridEditor.nudging, nudgePreview, savedGrid]);

  const [tempoResetLocked, setTempoResetLocked] = useState(true);

  /** RST: the file's own speed, and no longer following anything. */
  const resetTempo = useCallback(() => {
    setTempoResetLocked(true);
    if (synced) onSyncToggle?.();
    playback.setTempo(1);
  }, [synced, onSyncToggle, playback]);

  /** The readout edits the deck's playing BPM, leaving the track's beat grid alone. */
  const setDisplayedBpm = useCallback((bpm: number) => {
    if (bpmX100 <= 0) return;
    if (synced) onSyncToggle?.();
    setTempoResetLocked(false);
    playback.setTempo(bpm * 100 / bpmX100);
  }, [bpmX100, synced, onSyncToggle, playback]);

  /*
   * The deck's keys, from rekordbox's own Export key map — see `shortcuts.ts`.
   *
   * Space, C, Q and F10-F12 belong to the deck whenever nothing is being typed
   * into, as they do in rekordbox. The arrows are the exception: they move the
   * browser's cursor as readily as the track, so they wait until the deck has
   * been clicked, which is what turns the playhead red.
   */
  const keyOverrides = preferences.keyboard.overrides;
  const onDeckKey = useEventCallback((event: KeyboardEvent) => {
      const hit = dispatchBinding(event, platform, event.target as HTMLElement | null, keyOverrides);
      if (hit?.action === undefined) return;
      // Player A's keys are Player A's and Player B's, with shift, Player
      // B's: a row for the other deck is not this one's business.
      if (hit.deck !== undefined && hit.deck !== deck) return;
      const action = hit.action;
      if (action === "jumpBack" || action === "jumpForward") {
        event.preventDefault();
        jump(action === "jumpForward" ? 1 : -1);
        return;
      }
      if (action === "loadPlayer1") {
        // Enter loads the highlighted track onto Player 1. A deck already
        // playing carries the sound into the new track; a stopped one cues it.
        if (deck !== "a" || !onLoadSelected || selectedTrackId === null) return;
        event.preventDefault();
        onLoadSelected();
        return;
      }
      if (action === "metronomeSound") {
        // The engine's click, not the deck's: the next of the three sounds.
        event.preventDefault();
        const sound = preferences.audio.metronomeSound;
        updatePreferences("audio", { metronomeSound: sound === 3 ? 1 : sound === 2 ? 3 : 2 });
        return;
      }
      if (playback.idle && action !== "showMemory" && action !== "showHotCues"
        && action !== "showInfo") {
        return;
      }
      switch (action) {
        case "playPause":
          // Space scrolls the page otherwise.
          event.preventDefault();
          togglePlay();
          break;
        case "cue":
          // Pressed, not tapped. CUE is a held control on the hardware and in
          // rekordbox: on the cue point it plays for as long as it is down and
          // snaps back when it comes up, which is how a preview works. Doing
          // both on the key down made the key the one control that could not
          // preview — and auto-repeat then ran the pair thirty times a second
          // for as long as the key was held. `keyup` below lets go.
          if (!event.repeat) holdCue();
          break;
        case "quantize":
          setQuantize((on) => !on);
          break;
        case "showMemory":
          event.preventDefault();
          setPanel("memory");
          break;
        case "showHotCues":
          event.preventDefault();
          setPanel("hotCue");
          break;
        case "showInfo":
          event.preventDefault();
          setPanel("info");
          break;
        case "memoryCue":
          if (!event.repeat) memory.store();
          break;
        case "previousMemoryCue":
          memory.callPrevious();
          break;
        case "nextMemoryCue":
          memory.callNext();
          break;
        case "deleteMemoryCue":
          if (!event.repeat) memory.deleteAtHead();
          break;
        case "loopIn":
          if (!event.repeat) markLoopIn();
          break;
        case "loopOut":
          if (!event.repeat) markLoopOut();
          break;
        case "reloop":
          if (!event.repeat) reloopOrExit();
          break;
        case "loopHalf":
          resizeLoop(0.5);
          break;
        case "loopDouble":
          event.preventDefault();
          resizeLoop(2);
          break;
        case "sync":
          event.preventDefault();
          if (!event.repeat) beatSync();
          break;
        case "masterTempo":
          event.preventDefault();
          if (!event.repeat) playback.setMasterTempo(!playback.masterTempo);
          break;
        case "tempoReset":
          event.preventDefault();
          resetTempo();
          break;
        case "bpmUp":
        case "bpmDown":
          // A tenth of a percent a press [ASSUME]: the fader's finest step.
          event.preventDefault();
          playback.setTempo(Math.min(Math.max(
            playback.tempo + (action === "bpmUp" ? 0.001 : -0.001), MIN_TEMPO), MAX_TEMPO));
          break;
        // The GRID panel's keys, from the same key map: `Adjust BPM/BeatGrid`
        // opens the panel, the arrows with the modifier shift the grid a
        // millisecond (a held key repeats, one write at a time), and
        // `Shift Beatgrid to the center` puts the nearest beat under the head.
        case "adjustGrid":
          event.preventDefault();
          setPadMode("grid");
          break;
        case "shiftGridLeft":
          event.preventDefault();
          gridEditor.shift(-1);
          break;
        case "shiftGridRight":
          event.preventDefault();
          gridEditor.shift(1);
          break;
        case "shiftGridToCenter":
          event.preventDefault();
          if (!event.repeat) gridEditor.align();
          break;
        default: {
          const beats = beatLoopLength(action);
          if (beats !== null) {
            // A beat loop key sets the length and starts the loop, as the
            // pad does; a held key is one press.
            if (event.repeat) break;
            setLoopBeats(beats);
            loopOfBeats(beats);
            break;
          }
          const number = memoryCueNumber(action);
          if (number !== null) {
            memory.callNumber(number);
            break;
          }
          // `1`-`3` are the first three pads and `command + 1`-`3` their
          // clears; a repeat on a held key is one press, as with M and X.
          const pad = hotCuePad(action);
          if (!pad || event.repeat) break;
          // Command with a digit is a tab switch in a browser; not here.
          event.preventDefault();
          if (pad.clear) hot.clear(pad.letter);
          else hot.press(pad.letter);
          break;
        }
      }
  });
  const onDeckKeyUp = useEventCallback((event: KeyboardEvent) => {
    const hit = matchBinding(event, platform, keyOverrides);
    if (hit?.action === "cue" && hit.deck === deck) dropCue();
  });
  const onDeckBlur = useEventCallback(() => dropCue());
  useEffect(() => {
    /**
     * Letting go of CUE.
     *
     * Mapped without the typing guard `dispatch` applies, on purpose: a key
     * released while the search box has the focus still has to end a preview
     * that is running, and `dropCue` does nothing when none is. The same
     * reasoning covers the window losing focus altogether — a preview that
     * outlives the key would play on with nothing able to stop it.
     */

    // `globalThis`, because `window` here is the slice of the track on screen.
    globalThis.addEventListener("keydown", onDeckKey);
    globalThis.addEventListener("keyup", onDeckKeyUp);
    globalThis.addEventListener("blur", onDeckBlur);
    return () => {
      globalThis.removeEventListener("keydown", onDeckKey);
      globalThis.removeEventListener("keyup", onDeckKeyUp);
      globalThis.removeEventListener("blur", onDeckBlur);
    };
  }, [onDeckKey, onDeckKeyUp, onDeckBlur]);

  const takesDrop = dragging && Boolean(onDropTrack);

  const dragOver = (event: React.DragEvent<HTMLElement>) => {
    if (!takesDrop) return;
    // Without the preventDefault the browser refuses the drop and the
    // cursor says so, whatever the handler below would have done.
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  };
  const drop = (event: React.DragEvent<HTMLElement>) => {
    if (!takesDrop) return;
    event.preventDefault();
    onDropTrack?.();
  };

  // The simple player is this deck drawn as one strip. Everything above —
  // the engine, the cues, the frame loop, the keys — is still this
  // component's, which is what keeps the track playing across the switch.
  if (simple) {
    return (
      <SimplePlayer
        track={track}
        deck={deck}
        shell={shell}
        armed={armed}
        droppable={takesDrop}
        onDragOver={dragOver}
        onDrop={drop}
        playing={playback.playing}
        idle={playback.idle}
        onToggle={togglePlay}
        positionSource={playback}
        total={total}
        bpmX100={Math.round(bpmX100 * playback.tempo)}
        baseBpmX100={bpmX100}
        onBpmChange={setDisplayedBpm}
        cues={cues}
        grid={grid}
        cuePoint={cuePoint}
        overviewRef={overviewRef}
        overview={overview}
        overviewHead={overviewHead}
        scrubFill={scrubFill}
        onScrubStart={scrubOverview}
        onScrubMove={dragOverview}
        onScrubEnd={endDrag}
        onEject={onEject}
        onLoadSelected={onLoadSelected}
      />
    );
  }

  // The track skips, which the two-deck column does not draw: rekordbox
  // drops them there, and half a deck's height has no room for them.
  const skips = (
        <div className={styles.pair}>
          {SKIPS.map((button) => (
            <button
              key={button.id}
              type="button"
              className={styles.square}
              aria-label={button.label}
              // No playlist cursor behind these yet, so they are drawn and
              // inert rather than absent.
              disabled
            >
              {button.glyph}
            </button>
          ))}
        </div>
  );

  const jumps = (
        <div className={styles.pair}>
          {JUMPS.map((button) => (
            <button
              key={button.id}
              type="button"
              className={styles.square}
              aria-label={button.label}
              disabled={playback.idle}
              onClick={() => jump(button.id === "jump-back" ? -1 : 1)}
            >
              {button.glyph}
            </button>
          ))}
        </div>
  );

  const beatSize = (
        <button
          ref={jumpButton}
          type="button"
          className={styles.beats}
          aria-label="Beat jump size"
          aria-haspopup="menu"
          aria-expanded={jumpMenu !== null}
          // Transcribed from rekordbox: "Select the beat/bar length jumping
          // from the current position."
          title={tip("Select the beat/bar length jumping from the current position.")}
          onClick={(event) => {
            const box = event.currentTarget.getBoundingClientRect();
            // Opened beside the button rather than under it: the deck sits at
            // the bottom of the window and a menu below would be off screen.
            setJumpMenu((open) => (open ? null : { x: box.right + 6, y: box.top }));
          }}
        >
          {jumpSizeById(jumpSizeId).label}
          <span className={styles.chevron} aria-hidden />
        </button>
  );

  const cueButton = (
        <button
          type="button"
          className={styles.cue}
          aria-label="Cue"
          // Held, not clicked: on the cue point the deck plays for as long as
          // the button is down and snaps back when it comes up.
          onPointerDown={holdCue}
          onPointerUp={dropCue}
          onPointerCancel={dropCue}
          onPointerLeave={dropCue}
          disabled={playback.idle}
        >
          CUE
        </button>
  );

  const playButton = (
        <button
          type="button"
          className={styles.play}
          data-on={playback.playing ? "" : undefined}
          aria-label={playback.playing ? "Pause" : "Play"}
          onClick={togglePlay}
          disabled={playback.idle}
        >
          {/* Both are here so hover swaps them in CSS: a state change for a
              pointer moving over a button is a re-render the frame loop does
              not need to share the frame with. */}
          <span className={styles.playGlyph} aria-hidden />
          <span className={styles.pauseGlyph} aria-hidden />
        </button>
  );

  // The transport column, as a value: it is drawn here in the one-deck
  // layouts and portalled into the column the two decks share in the others.
  //
  // The order differs between them, and follows the capture. One deck reads
  // jumps, size, CUE, PLAY down the column. Two decks put CUE and PLAY at the
  // outside of each half and the jump controls towards the middle, so the two
  // halves mirror as groups — within a group the order is the same either way,
  // which is what the capture shows.
  const transport = (
    <div
      className={styles.transport}
      data-flipped={flipped || undefined}
      data-shared={transportSlot ? "" : undefined}
      // Named, because in the two-deck layouts it is drawn outside the deck it
      // belongs to: without this the CUE and PLAY of a deck would be a pair of
      // unattached buttons to anything reading the page.
      role="group"
      aria-label={deck === "b" ? "Deck B transport" : "Deck A transport"}
    >
      {transportSlot ? null : skips}
      {transportSlot && !flipped ? (
        <>
          {cueButton}
          {playButton}
          {jumps}
          {beatSize}
        </>
      ) : (
        <>
          {jumps}
          {beatSize}
          {cueButton}
          {playButton}
        </>
      )}
    </div>
  );

  // The sleeve: the eject button loaded and the load button empty, and the
  // record where there is no artwork. Both bodies draw it, at their own size.
  const sleeve = (
          <button
            type="button"
            className={styles.artwork}
            // Loaded, the sleeve ejects — as it does on a CDJ's screen. Empty,
            // it takes whatever the browser has selected, which is the third
            // way a track reaches a deck alongside the drop and the menu.
            aria-label={track ? "Eject" : "Load the selected track"}
            title={tip(track ? "Eject" : "Load the selected track")}
            onClick={track ? onEject : onLoadSelected}
            disabled={track ? !onEject : !onLoadSelected}
          >
            {/* The record underneath, the way the track list draws a row
                without artwork: the same asset, so one record looks the same
                everywhere. */}
            <RecordIcon className={styles.disc} aria-hidden />
            {track?.hasArtwork ? (
              <Artwork trackId={track.id} className={styles.sleeve} />
            ) : null}
            {/* Shown on hover, over a scrim: what the sleeve does when clicked
                is not otherwise guessable from a sleeve. */}
            {track ? <EjectIcon className={styles.eject} /> : null}
          </button>
  );

  const overviewStack = (
          <div className={styles.overviewStack}>
            {/* Where the vocals are, from the analysis; Vocal (Full
                Waveform) in Preferences turns the strip off. */}
            {viewPrefs.vocalFull ? (
              <VocalStrip trackId={track && track.analysed ? track.id : null} />
            ) : null}
            {/*
              Clicking either waveform seeks, which is what they are for.

              Reported, not operated: the overview scrubs with the pointer and
              has no keys of its own — the arrows already belong to the deck. A
              tab stop here would only park the focus somewhere the keyboard can
              do nothing, and then paint a ring around the waveform the next
              time any key went down.
            */}
            <div
              ref={overviewRef}
              className={styles.overview}
              data-testid="player-overview"
              onPointerDown={scrubOverview}
              onPointerMove={dragOverview}
              onPointerUp={endDrag}
              onPointerCancel={endDrag}
              role="progressbar"
              aria-label="Position"
              aria-valuemin={-5}
              aria-valuemax={Math.round(total)}
              aria-valuenow={Math.round(playback.positionRef.current)}
            >
              {track && track.analysed ? (
                <WaveformDetail
                  trackId={track.id}
                  progress={0.5}
                  span={1}
                  width={overview.width}
                  height={overview.height}
                  // Full/Preview Waveform in Preferences: single-sided from
                  // the baseline, or mirrored about the middle.
                  half={viewPrefs.overviewWaveform === "half"}
                />
              ) : null}
              <CueMarkers cues={cues} totalMs={total * 1000} loop={playback.loop} />
              <OverviewTempoMarkers grid={grid} totalMs={total * 1000} />
              <span
                ref={overviewHead}
                className={styles.playhead}
                data-testid="player-head"
                aria-hidden
              />
            </div>
            {/* How far through the track the head is, under the overview. */}
            <div className={styles.scrub} aria-hidden>
              <div ref={scrubFill} className={styles.scrubFill} />
            </div>
          </div>
  );

  return (
    <section
      ref={shell}
      className={styles.player}
      aria-label={deck === "b" ? "Preview player B" : "Preview player"}
      data-armed={armed ? "" : undefined}
      data-empty={track ? undefined : ""}
      // The transport is drawn elsewhere, so the deck is two columns wide
      // rather than three.
      data-shared={transportSlot ? "" : undefined}
      // Deck B, which reads bottom-up so the two decks' waveforms meet at the
      // line between them.
      data-flipped={flipped || undefined}
      // The two-deck body, whose rows are DualDeck's.
      data-dual={dual || undefined}
      data-tempo-slider={viewPrefs.tempoSlider || undefined}
      data-droppable={takesDrop || undefined}
      onDragOver={dragOver}
      onDrop={drop}
    >
      {transportSlot ? createPortal(transport, transportSlot) : transport}

      <div className={styles.main}>
        {dual ? (
          <DualHead
            track={track}
            positionSource={playback}
            total={total}
            sleeve={sleeve}
            keyControl={<KeyShift musicalKey={track ? formatKey(track.key, viewPrefs.keyDisplay) : ""}
              shift={playback.keyShift} disabled={playback.idle || !playback.shiftsKey} onChange={playback.setKeyShift} />}
            bpmX100={Math.round(bpmX100 * playback.tempo)}
            baseBpmX100={bpmX100}
            onBpmChange={setDisplayedBpm}
            onBeatSync={beatSync}
            synced={synced}
            isMaster={isMaster}
            onMaster={onMaster}
          />
        ) : (
        <div className={styles.head}>
          <span className={styles.title} data-testid="player-title">
            {track ? track.title : ""}
          </span>
          {track ? (
            <>
              <span className={styles.artist}>{track.artist}</span>
              <TimeReadouts source={playback} total={total} classes={styles} />
              <div className={styles.keyControl}>
                <KeyShift musicalKey={formatKey(track.key, viewPrefs.keyDisplay)} shift={playback.keyShift}
                  disabled={playback.idle || !playback.shiftsKey} onChange={playback.setKeyShift} />
              </div>
              <TempoToggle className={styles.readout} bpmX100={Math.round(bpmX100 * playback.tempo)} baseBpmX100={bpmX100} onBpmChange={setDisplayedBpm} />
            </>
          ) : null}
          {/* Sync belongs to the two-deck layouts and to nothing else: one
              deck has nothing to sync to and nothing to be master of. */}
          {peerSync ? (
            <div className={styles.sync} role="group" aria-label="Sync">
              <button
                type="button"
                className={styles.chip}
                aria-label="Beat sync"
                aria-pressed={synced}
                data-on={synced ? "" : undefined}
                disabled={!track || isMaster}
                title={tip(
                  isMaster
                    ? "This deck is the master; sync the other one to it."
                    : synced
                      ? "Following the master's tempo; press to stop."
                      : "Match this deck to the master's tempo and bar, and keep its tempo.",
                )}
                onClick={beatSync}
              >
                BEAT SYNC
              </button>
              <button
                type="button"
                className={styles.chip}
                aria-label="Sync master"
                aria-pressed={isMaster}
                data-on={isMaster ? "" : undefined}
                onClick={onMaster}
              >
                MASTER
              </button>
            </div>
          ) : null}
        </div>
        )}

        {/* The one-deck overview sits beside the sleeve; the two-deck one
            runs the deck's width, its sleeve up in the title row. */}
        <div className={styles.overviewRow}>
          {dual ? null : sleeve}
          {overviewStack}
        </div>

        {viewPrefs.phraseFull ? (
          <PhraseBar
            phrases={phrases}
            totalMs={total * 1000}
            beatMs={beatMs}
            labels={viewPrefs.phraseLabels}
          />
        ) : (
          // Keep the phrase row's grid track: the rows are placed by
          // auto-flow, so omitting it shifts the detail into a fixed-height track.
          <div className={styles.phrase} data-testid="player-phrase-off" aria-hidden />
        )}

        {/* The control row comes before the detail in the two-deck body, as
            the capture has it: the detail is the last row, and takes what is
            left. */}
        {dual ? (
          <DualControls
            gridEditor={gridEditor}
            readOnly={readOnly}
            memory={memory}
            quantize={quantize}
            onQuantize={() => setQuantize((on) => !on)}
            loop={dualLoop}
          />
        ) : null}

        <div className={styles.detailRow}>
          {/* The two-deck layout's zoom is the shell's, one for the pair. */}
          {dual ? null : (
          <div className={styles.zoom}>
            <button type="button" aria-label="Zoom in" onClick={() => zoom(-1)}>+</button>
            <span className={styles.rst} aria-hidden>RST</span>
            <button type="button" aria-label="Zoom out" onClick={() => zoom(1)}>−</button>
          </div>
          )}
          <div
            ref={detailRef}
            className={styles.detail}
            data-testid="player-detail"
            data-pcm={bars <= 0.5 || undefined}
            onWheel={wheelZoom}
            onPointerDown={startDrag}
            onPointerMove={dragDetail}
            onPointerUp={endDrag}
            onPointerCancel={endDrag}
          >
            <div ref={scroller} className={styles.scroller}>
              {track && track.analysed ? (
                <WaveformDetail
                  // PCM and PWV7 have incompatible binary layouts. This is
                  // the detail waveform the wheel zooms, so make its format
                  // boundary a full unmount/remount; no canvas or async state
                  // can then cross from one byte layout into the other.
                  key={bars <= 0.5 ? "pcm" : "pwv7"}
                  trackId={track.id}
                  progress={anchor}
                  span={span * OVERDRAW}
                  durationMs={total * 1000}
                  firstBeatMs={grid.times[0]}
                  width={detail.width * OVERDRAW}
                  height={detail.height}
                  detail
                  pcmWindow={bars <= 0.5 ? {
                    // The canvas draws the same 2× viewport as PWV7, but the
                    // decoder carries a two-second guard either side. That is
                    // enough source audio around the playhead without writing
                    // a permanent peak file.
                    fromMs: Math.max(0, window.from * total * 1000 - 2000),
                    toMs: Math.min(total * 1000, window.to * total * 1000 + 2000),
                    drawFromMs: window.from * total * 1000,
                    drawToMs: window.to * total * 1000,
                  } : undefined}
                  // In the 2 PLAYER layout the two details are halves that
                  // meet at the line between the decks: deck A's rises from
                  // it and deck B's, whose canvas is flipped, hangs from it
                  // [OBS]. On its own the deck draws the centred waveform.
                  half={dual ? "overlaid" : false}
                  inset={waveInsetOf()}
                />
              ) : null}
              <BeatGrid
                grid={grid}
                beats={beats}
                totalMs={total * 1000}
                window={window}
                everyBeat={showsEveryBeat(bars)}
                fromMs={gridEditor.fromMs}
              />
              {/* The edit boundary, while one is set: where the grid edits start. */}
              {gridEditor.fromMs !== null && total > 0 ? (
                <GridEditBoundary fromMs={gridEditor.fromMs} totalMs={total * 1000} window={window} />
              ) : null}
              <CueMarkers cues={cues} totalMs={total * 1000} band="detail" window={window} loop={playback.loop} />
            </div>
            {/* Bars elapsed, printed to the left of the playhead. Its text and
                its position are both the frame loop's, so React renders it
                empty and never touches it again. */}
            {track && bpmX100 > 0 ? (
              <span ref={barsLabel} className={styles.bars} data-testid="player-bars" />
            ) : null}
            {/* Fixed in the middle; the layer above scrolls under it. */}
            <span
              ref={detailHead}
              className={styles.playhead}
              data-testid="player-detail-head"
              aria-hidden
            />
          </div>
        </div>

        {dual ? null : (
        <div className={styles.pads}>
          {/*
            Two stacked tabs, not a pair of pills. The selected one takes the
            row's own colour and the other is cut out in black, which is what
            the capture shows and the opposite of the usual convention.
          */}
          <div className={styles.modes} role="tablist" aria-label="Pad mode">
            {([["cue", "CUE/LOOP"], ["grid", "GRID"]] as const).map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={padMode === id}
                className={styles.mode}
                data-on={padMode === id || undefined}
                onClick={() => setPadMode(id)}
              >
                {label}
              </button>
            ))}
          </div>

          {padMode === "grid" ? (
            <div className={styles.gridRow}>
              <section className={styles.editGroup} aria-label="Beat grid">
                <span className={styles.sectionLabel}>GRID EDIT</span>
                <div className={styles.editButtons}>
                  {GRID_EDITS.map((group, at) => (
                    <div key={group[0]?.id ?? at} className={styles.editPair}>
                      {at === 1 ? (
                        // The tempo the grid has under the playhead — or,
                        // while tapping, the tempo the taps so far describe.
                        <GridBpmField tapping={gridEditor.tapBpmX100 !== null} value={formatBpm(gridEditor.tapBpmX100 ?? gridBpmX100)} disabled={!gridEditor.canEdit} onCommit={gridEditor.setBpm} />
                      ) : null}
                      {group.map((edit) => {
                        const button = gridButton(edit.id, gridEditor, hold, {
                          metronome, metronomeLevel, metronomeBusy, toggleMetronome, cycleMetronomeVolume, idle: playback.idle, readOnly,
                        });
                        // The padlock closes when the grid is locked.
                        const Icon = edit.id === "lock" && button.pressed ? GridLockIcon : EDIT_ICONS[edit.id];
                        return (
                          <button
                            key={edit.id}
                            type="button"
                            className={styles.editButton}
                            aria-label={edit.id === "metronome" ? `Metronome volume: ${metronomeLevel}` : edit.label}
                            data-metronome={edit.id === "metronome" ? metronomeLevel : undefined}
                            disabled={button.disabled}
                            aria-pressed={button.pressed}
                            title={tip(button.title)}
                            onContextMenu={edit.id === "cut-grid" ? event => { event.preventDefault(); setMetronomeMenu({x: event.clientX, y: event.clientY}); } : undefined}
                            {...button.handlers}
                          >
                            {Icon ? <Icon className={styles.editIcon} /> : "TAP"}
                          </button>
                        );
                      })}
                    </div>
                  ))}
                </div>
              </section>

            </div>
          ) : (
          <div className={styles.padCluster}>
            {/* A set pad calls its cue; an empty one sets `Hot Cue <letter>`
                at the playhead — `Set Hot Cue A` in german.lang, on `1`-`3`
                for the first three pads. An empty pad that cannot be set is
                disabled with the reason the MEMORY cluster gives. */}
            <div className={styles.hotCues} aria-label="Hot cues">
              {PADS.map((letter) => {
                const cue = hot.at(letter);
                const key = HOT_CUE_KEYS[letter];
                return (
                  <button
                    key={letter}
                    type="button"
                    className={styles.pad}
                    data-set={cue ? "" : undefined}
                    style={cueStyle(undefined, cue?.colour, viewPrefs.hotCueColor)}
                    aria-label={`Hot cue ${letter}`}
                    aria-pressed={cue !== null}
                    title={cue
                      ? undefined
                      : tip(hotCueEditReason ?? `Set Hot Cue ${letter}${key ? ` (${key})` : ""}`)}
                    disabled={!cue && !hot.canEdit}
                    onClick={() => hot.press(letter)}
                  >
                    <span className={styles.padInner}>{letter}</span>
                  </button>
                );
              })}
            </div>

            {/* MEMORY stores the cue point, ◀ ▶ call the memory cue either
                side of the playhead, ✕ deletes the one it is on — `Set Memory
                Cue`, `Call Previous/Next Memory Cue`, `Delete Memory Cue` in
                german.lang, on M, B, N and X in the Export key map. Calling
                needs no write and works read-only; the rest is disabled with
                the reason the menus give. */}
            <div className={styles.memory} aria-label="Memory cues">
              <button
                type="button"
                className={styles.memoryLabel}
                aria-label="Set memory cue"
                title={tip(memoryCueEditReason ?? "Set Memory Cue (M)")}
                disabled={!memory.canEdit}
                onClick={memory.store}
              >
                MEMORY
              </button>
              <button
                type="button"
                className={styles.step}
                aria-label="Previous memory cue"
                title={tip("Call Previous Memory Cue (B)")}
                disabled={playback.idle}
                onClick={memory.callPrevious}
              >
                ◀
              </button>
              <button
                type="button"
                className={styles.step}
                aria-label="Next memory cue"
                title={tip("Call Next Memory Cue (N)")}
                disabled={playback.idle}
                onClick={memory.callNext}
              >
                ▶
              </button>
              <button
                type="button"
                className={styles.step}
                aria-label="Delete memory cue"
                title={tip(memoryCueEditReason ?? "Delete Memory Cue (X)")}
                disabled={!memory.canEdit}
                onClick={memory.deleteAtHead}
              >
                ✕
              </button>
            </div>

            {/* AU/MA is "Change Auto Beat Loop/Manual Loop display" in
                german.lang, and the `‹ 2 ›` beside it "Switch the page of
                beat length": the auto beat loop's length in beats, which
                its number sets from the head. MA puts IN, OUT and
                RELOOP/EXIT in the same place. [ASSUME: the capture shows
                the controls, not a loop in use.] */}
            <div className={styles.auto} role="group" aria-label="Loop mode">
              <button
                type="button"
                className={styles.chip}
                data-on={loopMode === "auto" || undefined}
                aria-pressed={loopMode === "auto"}
                title={tip("Auto Beat Loop")}
                onClick={() => chooseLoopMode("auto")}
              >
                AU
              </button>
              <button
                type="button"
                className={styles.chip}
                data-on={loopMode === "manual" || undefined}
                aria-pressed={loopMode === "manual"}
                title={tip("Manual Loop")}
                onClick={() => chooseLoopMode("manual")}
              >
                MA
              </button>
            </div>

            {loopMode === "auto" ? (
              <div className={styles.page} role="group" aria-label="Beat loop">
                <button
                  type="button"
                  className={styles.step}
                  aria-label="Shorter loop"
                  disabled={loopBeats <= LOOP_BEATS_MIN}
                  onClick={() => resizeLoop(0.5)}
                >
                  ‹
                </button>
                <button
                  type="button"
                  className={styles.pageNumber}
                  data-on={activeLoop || undefined}
                  aria-pressed={activeLoop}
                  aria-label={activeLoop ? "Exit loop" : `${loopBeats} beat loop`}
                  title={tip(activeLoop ? "Exit the loop" : `${loopBeatsLabel(loopBeats)} Beat Loop`)}
                  disabled={playback.idle || grid.times.length < 2}
                  onClick={autoLoop}
                >
                  {loopBeatsLabel(loopBeats)}
                </button>
                <button
                  type="button"
                  className={styles.step}
                  aria-label="Longer loop"
                  disabled={loopBeats >= LOOP_BEATS_MAX}
                  onClick={() => resizeLoop(2)}
                >
                  ›
                </button>
              </div>
            ) : (
              <div className={styles.page} role="group" aria-label="Manual loop">
                <button
                  type="button"
                  className={styles.memoryLabel}
                  data-on={loopIn !== null || undefined}
                  aria-label="Loop in"
                  title={tip("Loop In")}
                  disabled={playback.idle}
                  onClick={markLoopIn}
                >
                  IN
                </button>
                <button
                  type="button"
                  className={styles.memoryLabel}
                  aria-label="Loop out"
                  title={tip(loopIn !== null ? "Loop Out" : "Reloop/Exit")}
                  disabled={loopIn === null && !playback.loop}
                  onClick={loopOut}
                >
                  OUT
                </button>
                <button
                  type="button"
                  className={styles.memoryLabel}
                  data-on={activeLoop || undefined}
                  aria-label={activeLoop ? "Exit loop" : "Reloop"}
                  title={tip("Reloop/Exit")}
                  disabled={!playback.loop}
                  onClick={reloopOrExit}
                >
                  {activeLoop ? "EXIT" : "RELOOP"}
                </button>
              </div>
            )}
          </div>
          )}

          <button
            type="button"
            className={styles.chip}
            aria-label="Quantize"
            aria-pressed={quantize}
            data-on={quantize ? "" : undefined}
            onClick={() => setQuantize((on) => !on)}
          >
            Q
          </button>
          <button
            type="button"
            className={styles.padMenu}
            aria-label="Player menu"
            aria-haspopup="menu"
            aria-expanded={deckMenuAt !== null}
            onClick={(event) => {
              // Opened from the button's corner, as rekordbox's is.
              const box = event.currentTarget.getBoundingClientRect();
              setDeckMenuAt({ x: box.left, y: box.bottom });
            }}
          >
            ≡
          </button>
        </div>
        )}
        {metronomeMenu ? <ContextMenu x={metronomeMenu.x} y={metronomeMenu.y} label="Metronome sound"
          context={{inPlaylist: false, hasFile: true, readOnly: false}}
          rows={([1,2,3] as const).map(sound => ({label: `Sound ${sound}`, action: String(sound), checked: preferences.audio.metronomeSound === sound}))}
          onClose={() => setMetronomeMenu(null)} onChoose={choice => {
            const sound = Number(choice) as 1 | 2 | 3;
            void getBackend().then(async backend => {
              await backend.setMetronome(sound, preferences.audio.metronomeVolume);
              updatePreferences("audio", {metronomeSound: sound});
            }).catch(() => onError?.("The metronome sound could not be changed."));
          }} /> : null}
        {deckMenuAt ? (
          <ContextMenu
            x={deckMenuAt.x}
            y={deckMenuAt.y}
            rows={deckMenu({
              waveformColor: viewPrefs.waveformColor,
              beatCount: viewPrefs.beatCount,
              waveformClick: viewPrefs.waveformClick,
              hasLoop: playback.loop !== null && track !== null,
              devices,
            })}
            label="Player menu"
            context={{ inPlaylist: false, hasFile: true, readOnly }}
            onChoose={chooseFromDeckMenu}
            onClose={() => setDeckMenuAt(null)}
          />
        ) : null}

        {viewPrefs.tempoSlider ? (
          <div className={styles.tempoOverlay}>
            <TempoSlider tempo={playback.tempo} onTempo={playback.setTempo}
              idle={playback.idle} synced={synced} masterTempo={playback.masterTempo}
              onMasterTempo={playback.setMasterTempo} onReset={resetTempo}
              resetLocked={tempoResetLocked} onUnlock={() => setTempoResetLocked(false)} />
          </div>
        ) : null}
      </div>
      <aside className={styles.side} aria-label="Cue list">
        {panel === "info" ? (
          <DeckInfo track={track} details={details} />
        ) : panel === "hotCue" ? (
          /* Eight slots, always: an empty one is a slot you can fill, and
             hiding it makes the list read as a shorter track. A row rather
             than a button, as the memory list's are, because the ✕ inside a
             set row is one: the row calls the cue, the ✕ clears it. */
          <div className={styles.cueList}>
            {PADS.map((letter) => {
              const cue = hot.at(letter);
              return (
                <div
                  key={letter}
                  role="button"
                  tabIndex={cue ? 0 : -1}
                  className={styles.cueRow}
                  aria-label={`Hot cue ${letter}`}
                  aria-disabled={cue ? undefined : true}
                  data-empty={cue ? undefined : ""}
                  onClick={() => hot.press(letter)}
                  onContextMenu={cue ? (event) => {
                    event.preventDefault(); event.stopPropagation();
                    setCueColorMenu({x: event.clientX, y: event.clientY, cue});
                  } : undefined}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      hot.press(letter);
                    }
                  }}
                >
                  <span
                    className={styles.cueChip}
                    data-set={cue ? "" : undefined}
                    style={cueStyle(undefined, cue?.colour, viewPrefs.hotCueColor)}
                  >
                    {letter}
                  </span>
                  {cue ? (
                    <>
                      <span className={styles.cueTime}>{splitTime(cue.positionMs / 1000).main}</span>
                      <span className={styles.cueName}>{cue.comment || "CUE(Auto)"}</span>
                      <button
                        type="button"
                        className={styles.cueDelete}
                        aria-label={`Clear hot cue ${letter}`}
                        title={hot.canEdit ? `Clear Hot Cue ${letter}` : READ_ONLY_REASON}
                        disabled={!hot.canEdit || cue.id === ""}
                        onClick={(event) => {
                          // The row underneath calls the cue; a clear is not
                          // also a jump.
                          event.stopPropagation();
                          hot.clear(letter);
                        }}
                      >
                        ✕
                      </button>
                    </>
                  ) : null}
                </div>
              );
            })}
          </div>
        ) : (
          <div className={styles.cueList}>
            {cuesFor(cues, panel).map((cue) => (
              /* A row rather than a button, because the ✕ inside it is one:
                 the row seeks, the ✕ deletes, and a button cannot hold a
                 button. Keyed by the cue's id so a deleted row leaves rather
                 than the one after it re-rendering as it. */
              <div
                key={cue.id || `m-${cue.positionMs}`}
                role="button"
                tabIndex={0}
                className={styles.cueRow}
                data-loop={cue.outMs > 0 ? "" : undefined}
                onClick={() => callCue(cue)}
                onContextMenu={(event) => {
                  event.preventDefault(); event.stopPropagation();
                  setCueColorMenu({x: event.clientX, y: event.clientY, cue});
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    callCue(cue);
                  }
                }}
              >
                {cue.colour ? <span className={styles.memoryCueDot} style={{background: cue.colour}} aria-hidden /> : null}
                <span className={styles.cueTime}>{memoryTime(cue.positionMs)}</span>
                <span className={styles.cueName}>{cue.comment || "CUE(Auto)"}</span>
                <button
                  type="button"
                  className={styles.cueDelete}
                  aria-label={`Delete memory cue ${memoryTime(cue.positionMs)}`}
                  title={tip(memoryCueEditReason ?? "Delete Memory Cue")}
                  disabled={!memory.canEdit || cue.id === ""}
                  onClick={(event) => {
                    // The row underneath seeks; a delete is not also a jump.
                    event.stopPropagation();
                    memory.remove(cue);
                  }}
                >
                  ✕
                </button>
              </div>
            ))}
            {/* The boxes below the cues, so the panel is the same grid whether
                the track has ten memory cues or none. Nothing to press: the
                ✕ is drawn dimmed, as the capture draws it, and is not a
                control. */}
            {Array.from({ length: Math.max(0, MEMORY_ROWS - cuesFor(cues, panel).length) }, (_, i) => (
              <div key={`empty-${i}`} className={styles.cueRow} data-blank="" aria-hidden>
                <span className={styles.cueDelete}>✕</span>
              </div>
            ))}
          </div>
        )}
        <div className={styles.sideTabs} role="tablist" aria-label="Cue list view">
          {PANELS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={panel === tab.id}
              className={styles.sideTab}
              data-on={panel === tab.id || undefined}
              onClick={() => setPanel(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>
      </aside>

      {cueColorMenu ? <CueColorMenu
        x={cueColorMenu.x} y={cueColorMenu.y} memory={cueColorMenu.cue.memory}
        onClose={() => setCueColorMenu(null)}
        onChoose={(colour) => {
          if (!readOnly && cueColorMenu.cue.id !== "") {
            writeCue((edits) => edits.setCueColour(cueColorMenu.cue.id, colour));
          }
        }}
      /> : null}

      {jumpMenu ? (
        <JumpMenu
          x={jumpMenu.x}
          y={jumpMenu.y}
          current={jumpSizeId}
          anchor={jumpButton.current}
          onPick={setJumpSizeId}
          onClose={() => setJumpMenu(null)}
        />
      ) : null}
    </section>
  );
});
