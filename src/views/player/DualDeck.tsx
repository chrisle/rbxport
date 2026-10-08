import { useHoldRepeat } from "./useHoldRepeat";
import type { GridEditorActions } from "./useGridEditor";
/**
 * What a deck draws in the two-deck layouts and nowhere else.
 *
 * rekordbox's 2 PLAYER deck is not the 1 PLAYER deck at half height. Its title
 * row carries the sleeve, the sync buttons and the readouts together; its
 * overview runs the deck's full width; and it has no pad row at all — a single
 * control row of grid, MEMORY, loop and tempo buttons sits between the phrase
 * bar and a detail waveform that takes everything left. Every size here is a
 * `playerDual*` token scanned off the 2 PLAYER capture with both decks loaded
 * (`docs/screenshots`, Screenshot 2026-09-08 at 3.00.12 PM).
 *
 * `Player` still owns the deck: the engine, the cues, the frame loop and the
 * keys. These are the rows it draws when it is told `dual`, given the values
 * it already holds.
 */
import { memo, type ReactNode } from "react";

import type { RowDto } from "@/ipc/types";
import { LoopInIcon, LoopOutIcon, MagnifierMinusIcon, MagnifierPlusIcon } from "@/components/icons";
import { READ_ONLY_REASON } from "./useMemoryCues";
import styles from "./DualDeck.module.css";
import { TempoToggle } from "./TempoToggle";
import { TimeReadouts, type PositionSource } from "./TimeReadouts";
import { useTooltip } from "@/store/usePreferences";

export interface DualHeadProps {
  track: RowDto | null;
  positionSource: PositionSource;
  total: number;
  /** The sleeve button, which `Player` builds because it owns load and eject. */
  sleeve: ReactNode;
  keyControl: ReactNode;
  bpmX100: number;
  baseBpmX100: number;
  onBpmChange: (bpm: number) => void;
  /** BEAT SYNC: pull this deck to the master. Disabled while this deck is it. */
  onBeatSync: () => void;
  /** BEAT SYNC is lit: the deck is following the master's tempo. */
  synced: boolean;
  isMaster: boolean;
  onMaster: (() => void) | undefined;
}

/**
 * The title row: sleeve, title over artist, and at the right two rows of
 * controls — KEY SYNC and BEAT SYNC over the readouts, the key shift and
 * MASTER.
 *
 * Deck B draws the same row at the bottom of its panel; the stylesheet moves
 * it there, and the row itself is the same either way up.
 */
export const DualHead = memo(function DualHead({
  track, positionSource, total, sleeve, keyControl, bpmX100, baseBpmX100, onBpmChange, onBeatSync, synced, isMaster, onMaster,
}: DualHeadProps) {
  const tip = useTooltip();
  return (
    <div className={styles.head} data-testid="player-head-row">
      {sleeve}
      <div className={styles.text}>
        <span className={styles.title} data-testid="player-title">
          {track ? track.title : ""}
        </span>
        <span className={styles.artist} data-testid="player-artist">
          {track ? track.artist : ""}
        </span>
      </div>
      <div className={styles.right}>
        {/* Rekordbox's readouts, hairline-separated: the remaining and
            elapsed times share a cell, then the key, then the BPM. Drawn
            only with a track, as the one-deck title row does. */}
        <div className={styles.readouts}>
          {track ? (
            <>
              <span className={styles.cell}>
                <TimeReadouts source={positionSource} total={total} classes={styles} />
              </span>
              <TempoToggle className={styles.cell} bpmX100={bpmX100} baseBpmX100={baseBpmX100} onBpmChange={onBpmChange} />
            </>
          ) : null}
        </div>
        {/* KEY SYNC shifts the key to the master's — "Enable/Disable Key
            Sync." in german.lang. There is no key shifting in the engine, so
            it is drawn and inert, with the reason. The capture lights deck A's
            BEAT SYNC blue: in rekordbox it is a toggle that keeps the deck
            following the master. Here it is a press that matches the deck
            once, so it is never lit — a lit toggle that is not one would lie. */}
        <button
          type="button"
          className={styles.syncButton}
          aria-label="Key sync"
          disabled
          title={tip("Key sync needs a key shifter, which the engine does not have yet.")}
        >
          KEY SYNC
        </button>
        <button
          type="button"
          className={styles.syncButton}
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
          onClick={onBeatSync}
        >
          BEAT SYNC
        </button>
        {keyControl}
        <button
          type="button"
          className={styles.masterButton}
          aria-label="Sync master"
          aria-pressed={isMaster}
          data-on={isMaster ? "" : undefined}
          onClick={onMaster}
        >
          MASTER
        </button>
      </div>
    </div>
  );
});

export interface DualControlsProps {
  gridEditor: GridEditorActions;
  readOnly: boolean;
  /** MEMORY, the one memory-cue control the row draws; the rest stay on keys. */
  memory: {
    canEdit: boolean;
    store: () => void;
  };
  quantize: boolean;
  onQuantize: () => void;
  /** The loop controls. `Player` owns the loop; the row only draws it. */
  loop: DualLoop;
}

export interface DualLoop {
  /** AU: a beat loop of `beats` from the head. MA: IN and OUT by hand. */
  mode: "auto" | "manual";
  onMode: (mode: "auto" | "manual") => void;
  /** The beat loop length, 1/4 to 32 beats. */
  beats: number;
  onShorter: () => void;
  onLonger: () => void;
  /** A loop is playing. */
  active: boolean;
  /** MA: an IN is set and waits for its OUT. */
  pendingIn: boolean;
  /** A track with a grid to count beats on. */
  canLoop: boolean;
  /** No track to loop. */
  idle: boolean;
  /** A loop range exists, playing or not. */
  hasLoop: boolean;
  /** The length field: start a beat loop, or exit the one that plays. */
  onToggle: () => void;
  onIn: () => void;
  onOut: () => void;
}

/**
 * The control row, which stands in for the pad row of the one-deck layout.
 *
 * Left to right, as the capture has it: the three grid-shift buttons, MEMORY,
 * AU | MA, the loop length with a step either side,
 * the two loop buttons; then at the right, Q. The grid buttons
 * use the shared grid editor, and the loop buttons drive `Player`'s loop.
 */
export const DualControls = memo(function DualControls({
  gridEditor, readOnly, memory, quantize, onQuantize, loop,
}: DualControlsProps) {
  const hold = useHoldRepeat();
  const gridReason = "Edit the beat grid";
  const tip = useTooltip();
  return (
    <div className={styles.controls} role="group" aria-label="Deck controls" data-testid="player-controls">
      <div className={styles.group}>
        <button type="button" className={styles.icon} aria-label="Shift the grid earlier" {...hold(repeat => gridEditor.shift(-1, repeat))} disabled={!gridEditor.canEdit || gridEditor.fromMs !== null} title={tip(gridReason)}>
          <span className={styles.gridGlyph} data-dir="back" aria-hidden />
        </button>
        <button type="button" className={styles.mark} aria-label="Mark the downbeat here" onClick={gridEditor.mark} disabled={!gridEditor.canEdit || gridEditor.fromMs !== null} title={tip(gridReason)}>
          <span className={styles.markGlyph} aria-hidden />
        </button>
        <button type="button" className={styles.icon} aria-label="Shift the grid later" {...hold(repeat => gridEditor.shift(1, repeat))} disabled={!gridEditor.canEdit || gridEditor.fromMs !== null} title={tip(gridReason)}>
          <span className={styles.gridGlyph} data-dir="forward" aria-hidden />
        </button>
      </div>

      {/* MEMORY stores the cue point: `Set Memory Cue` in german.lang, on M.
          The capture draws MEMORY alone — no ◀ ▶ ✕ beside it — so calling and
          deleting stay on their keys, B, N and X. */}
      <button
        type="button"
        className={styles.memory}
        aria-label="Set memory cue"
        title={tip(readOnly ? READ_ONLY_REASON : "Set Memory Cue (M)")}
        disabled={!memory.canEdit}
        onClick={memory.store}
      >
        MEMORY
      </button>

      {/* AU | MA: "Change Auto Beat Loop/Manual Loop display" in german.lang. */}
      <div className={styles.group} role="group" aria-label="Loop mode">
        <button
          type="button"
          className={styles.chip}
          data-on={loop.mode === "auto" || undefined}
          aria-pressed={loop.mode === "auto"}
          title={tip("Auto Beat Loop")}
          onClick={() => loop.onMode("auto")}
        >
          AU
        </button>
        <button
          type="button"
          className={styles.chip}
          data-on={loop.mode === "manual" || undefined}
          aria-pressed={loop.mode === "manual"}
          title={tip("Manual Loop")}
          onClick={() => loop.onMode("manual")}
        >
          MA
        </button>
      </div>

      {/* The beat loop length — "Switch the page of beat length" in
          german.lang. The field starts a loop of that length, or exits the
          loop that plays; a step changes the length of a playing loop. */}
      <div className={styles.loopLength} role="group" aria-label="Beat loop length">
        <button type="button" className={styles.step} aria-label="Shorter loop" disabled={loop.beats <= 0.25} onClick={loop.onShorter}>‹</button>
        <button
          type="button"
          className={styles.loopField}
          data-on={loop.active || undefined}
          aria-pressed={loop.active}
          aria-label={loop.active ? "Exit loop" : `${loop.beats} beat loop`}
          title={tip(loop.active ? "Exit the loop" : `Loop ${loop.beats} beat${loop.beats === 1 ? "" : "s"} from here`)}
          disabled={!loop.canLoop}
          onClick={loop.onToggle}
        >
          {loop.beats < 1 ? `1/${Math.round(1 / loop.beats)}` : loop.beats}
        </button>
        <button type="button" className={styles.step} aria-label="Longer loop" disabled={loop.beats >= 32} onClick={loop.onLonger}>›</button>
      </div>

      {/* Loop In and Loop Out — german.lang's names. In AU, IN starts a beat
          loop of the length above; in MA, IN and OUT set its ends. OUT with
          no IN waiting is RELOOP/EXIT. */}
      <div className={styles.loops} role="group" aria-label="Loop">
        <button
          type="button"
          className={styles.icon}
          aria-label="Loop in"
          data-on={loop.active || loop.pendingIn || undefined}
          disabled={loop.mode === "auto" ? !loop.canLoop : loop.idle}
          title={tip(loop.mode === "auto" ? `Loop ${loop.beats} beat${loop.beats === 1 ? "" : "s"} from here` : "Loop In")}
          onClick={loop.onIn}
        >
          <LoopInIcon className={styles.loopGlyph} />
        </button>
        <button
          type="button"
          className={styles.icon}
          aria-label="Loop out"
          data-on={loop.active || undefined}
          disabled={!loop.pendingIn && !loop.hasLoop}
          title={tip(loop.pendingIn ? "Loop Out" : "Reloop/Exit")}
          onClick={loop.onOut}
        >
          <LoopOutIcon className={styles.loopGlyph} />
        </button>
      </div>

      <span className={styles.spacer} />

      <button
        type="button"
        className={styles.chip}
        aria-label="Quantize"
        aria-pressed={quantize}
        data-on={quantize ? "" : undefined}
        onClick={onQuantize}
      >
        Q
      </button>
    </div>
  );
});

/**
 * The zoom cluster the pair shares: + over the centre line, − under it, RST
 * between. The capture draws one for both decks, floating over the two
 * detail waveforms where they meet, so it belongs to the shell rather than to
 * either deck; pressing it zooms both.
 */
export function DualZoom({ onZoom }: { onZoom: (by: number) => void }) {
  return (
    <div className={styles.zoom} role="group" aria-label="Waveform zoom">
      <button type="button" className={styles.zoomButton} aria-label="Zoom in" onClick={() => onZoom(-1)}>
        <MagnifierPlusIcon className={styles.zoomGlyph} />
      </button>
      <span className={styles.zoomReset} aria-hidden>RST</span>
      <button type="button" className={styles.zoomButton} aria-label="Zoom out" onClick={() => onZoom(1)}>
        <MagnifierMinusIcon className={styles.zoomGlyph} />
      </button>
    </div>
  );
}
