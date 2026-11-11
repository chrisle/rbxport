/**
 * The simple player: one strip across the top of the window.
 *
 * Measured off the SIMPLE PLAYER capture in `docs/screenshots` (2026-09-09,
 * 9.03.41 PM, 2x): the PLAY ring in its own column on the left, the sleeve,
 * then the readout row — title, artist, remaining and elapsed time, key, BPM,
 * each behind a hairline — over the overview waveform with its hot cue badges
 * and the position bar, and the rating box on the far right. No detail
 * waveform, no transport rail, no cue list; the browser starts 3pt under it.
 *
 * Presentational. `Player` owns the deck — the engine, the cues, the frame
 * loop that moves the head — and hands this the pieces it draws, so switching
 * layouts changes what is on screen and nothing about what is playing. The
 * refs are the frame loop's: it writes transforms straight into them, which
 * is why the head and the position bar are not rendered from state.
 */
import { memo } from "react";

import type { Cue, DeckId, RowDto } from "@/ipc/types";
import { Artwork } from "@/components/Artwork";
import { EjectIcon, RecordIcon } from "@/components/icons";
import type { BeatGrid } from "@/lib/player";
import { TempoToggle } from "./TempoToggle";
import { TimeReadouts, type PositionSource } from "./TimeReadouts";
import type { PressProps } from "./usePress";
import { CueMarkers, OverviewTempoMarkers } from "./Player";
import { WaveformDetail } from "./WaveformDetail";
import { RatingStar } from "@/components/RatingStar";
import styles from "./SimplePlayer.module.css";
import { usePreferences, useTooltip } from "@/store/usePreferences";
import { formatKey } from "@/lib/preferences";

export interface SimplePlayerProps {
  track: RowDto | null;
  deck: DeckId;
  /** The deck's own element, for the shell's "who has the keyboard" check. */
  shell: React.RefObject<HTMLElement | null>;
  armed: boolean;
  /** Whether a track is being dragged over the app and this deck takes it. */
  droppable: boolean;
  onDragOver: (event: React.DragEvent<HTMLElement>) => void;
  onDrop: (event: React.DragEvent<HTMLElement>) => void;
  playing: boolean;
  idle: boolean;
  onToggle: () => void;
  /** Shared frame clock; only the readouts subscribe to displayed tenths. */
  positionSource: PositionSource;
  /** Seconds in the track, or 0 while nothing is loaded. */
  total: number;
  bpmX100: number;
  baseBpmX100: number;
  onBpmChange: (bpm: number) => void;
  cues: readonly Cue[];
  grid: BeatGrid;
  /** Where CUE returns to, in seconds: the amber triangle under the overview. */
  cuePoint: number;
  /** The overview's element and its measured size, for the canvas. */
  overviewRef: React.Ref<HTMLDivElement>;
  overview: { width: number; height: number };
  /** Written by the frame loop, not rendered. */
  overviewHead: React.RefObject<HTMLSpanElement | null>;
  scrubFill: React.RefObject<HTMLDivElement | null>;
  scrubPress: PressProps;
  onEject?: (() => void) | undefined;
  onLoadSelected?: (() => void) | undefined;
  /** Whether the track has a waveform to draw, finished or being drawn. */
  drawn: boolean;
}

export const SimplePlayer = memo(function SimplePlayer({
  track, deck, shell, armed, droppable, onDragOver, onDrop,
  playing, idle, onToggle, positionSource, total, bpmX100, baseBpmX100, onBpmChange, cues, grid, cuePoint,
  overviewRef, overview, overviewHead, scrubFill, scrubPress,
  onEject, onLoadSelected, drawn,
}: SimplePlayerProps) {

  const tip = useTooltip();
  const { keyDisplay } = usePreferences().view;
  return (
    <section
      ref={shell}
      className={styles.strip}
      aria-label={deck === "b" ? "Preview player B" : "Preview player"}
      data-testid="simple-player"
      data-armed={armed ? "" : undefined}
      data-empty={track ? undefined : ""}
      data-droppable={droppable || undefined}
      onDragOver={onDragOver}
      onDrop={onDrop}
    >
      {/* The PLAY column: one ring, centred in a panel-coloured column. The
          capture has no CUE, no skips and no beat jump here — those belong to
          the full deck. */}
      <div className={styles.transport} role="group" aria-label={deck === "b" ? "Deck B transport" : "Deck A transport"}>
        <button
          type="button"
          className={styles.play}
          data-on={playing ? "" : undefined}
          aria-label={playing ? "Pause" : "Play"}
          onClick={onToggle}
          disabled={idle}
        >
          <span className={styles.playGlyph} aria-hidden />
          <span className={styles.pauseGlyph} aria-hidden />
        </button>
      </div>

      {/* The sleeve, which is the eject button loaded and the load button
          empty, as the full deck's is. */}
      <button
        type="button"
        className={styles.artwork}
        aria-label={track ? "Eject" : "Load the selected track"}
        title={tip(track ? "Eject" : "Load the selected track")}
        onClick={track ? onEject : onLoadSelected}
        disabled={track ? !onEject : !onLoadSelected}
      >
        {/* The record on the dark well: the capture's empty sleeve, and the
            same asset the track list and the full deck draw. */}
        <RecordIcon className={styles.disc} aria-hidden />
        {track?.hasArtwork ? (
          <Artwork trackId={track.id} className={styles.sleeve} />
        ) : null}
        {track ? <EjectIcon className={styles.eject} /> : null}
      </button>

      <div className={styles.main}>
        <div className={styles.readouts}>
          <span className={styles.title} data-testid="simple-player-title">
            {track ? track.title : ""}
          </span>
          {track ? (
            <>
              <span className={styles.artist} data-testid="simple-player-artist">{track.artist}</span>
              <span className={styles.times}>
                <TimeReadouts source={positionSource} total={total} classes={styles} testId="simple-player-time" />
              </span>
              <span className={styles.key} data-testid="simple-player-key">{formatKey(track.key, keyDisplay)}</span>
              <span className={styles.bpm} data-testid="simple-player-bpm">
                <TempoToggle bpmX100={bpmX100} baseBpmX100={baseBpmX100} onBpmChange={onBpmChange} />
              </span>
            </>
          ) : null}
        </div>

        {/* The overview and the position bar, one scrubber: the pointer goes
            where you put it and holding it down drags the head. The head runs
            from just under the badges to the bottom of the panel, through
            the position bar, which is what the capture draws. */}
        <div
          className={styles.band}
          {...scrubPress}
        >
          <div
            ref={overviewRef}
            className={styles.overview}
            data-testid="simple-player-overview"
            role="progressbar"
            aria-label="Position"
            aria-valuemin={0}
            aria-valuemax={Math.round(total)}
            aria-valuenow={Math.round(positionSource.positionRef.current)}
          >
            {track && drawn ? (
              <WaveformDetail
                trackId={track.id}
                progress={0.5}
                span={1}
                width={overview.width}
                height={overview.height}
                half
              />
            ) : null}
            <CueMarkers cues={cues} totalMs={total * 1000} />
            <OverviewTempoMarkers grid={grid} totalMs={total * 1000} />
          </div>
          <div className={styles.scrub} aria-hidden>
            <div ref={scrubFill} className={styles.scrubFill} />
          </div>
          {/* The cue point, an amber triangle with its apex on the overview's
              baseline. The full deck keeps the point and does not draw it;
              this capture does. */}
          {track && total > 0 ? (
            <i
              className={styles.cuePoint}
              data-testid="simple-player-cue-point"
              style={{ left: `${Math.min(cuePoint / total, 1) * 100}%` }}
              aria-hidden
            />
          ) : null}
          <span
            ref={overviewHead}
            className={styles.playhead}
            data-testid="simple-player-head"
            aria-hidden
          />
        </div>
      </div>

      {/* The rating box: a dark field with the stars in its first row and an
          empty row under a hairline. Read-only — the star's writer is the
          browser's, and the row the deck holds is a snapshot that would not
          follow the edit. */}
      <div className={styles.rating} aria-label="Rating">
        <div className={styles.field}>
          <span className={styles.stars} data-testid="simple-player-stars" aria-label={`${track?.rating ?? 0} of 5`}>
            {[1, 2, 3, 4, 5].map((star) => (
              <span key={star}><RatingStar lit={(track?.rating ?? 0) >= star} className={styles.starIcon} /></span>
            ))}
          </span>
          <span className={styles.fieldRow} aria-hidden />
        </div>
      </div>
    </section>
  );
});
