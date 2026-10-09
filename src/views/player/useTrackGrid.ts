/**
 * A track's beat grid, kept current.
 *
 * The whole grid, once per track, as raw bytes — see `Backend.trackBeats`
 * for why not a window at a time — and fetched again whenever the backend
 * says the track's grid changed: an edit from the GRID panel on this deck
 * or the other, an undo, re-analysis, or a full reload of the library. The panel's own
 * state (tempo, undo, redo, lock) rides along, from the same events, so the
 * beats the waveform draws and the BPM the field prints never disagree.
 *
 * A track without a grid — not analysed, or its file gone — has an empty
 * grid and no state, and the panel greys itself.
 */
import { useEffect, useState } from "react";

import type { GridState, RowDto } from "@/ipc/types";
import { getBackend } from "@/ipc/client";
import { NO_BEATS, parseBeatGrid, type BeatGrid } from "@/lib/player";

export interface TrackGrid {
  grid: BeatGrid;
  /** The panel's view of the grid, or `null` while there is none. */
  state: GridState | null;
  /** Replaces the state from a command's own answer, ahead of the refetch. */
  setState: (state: GridState) => void;
  /**
   * The track `grid` was read for. After a switch the previous track's grid
   * stays until the new one arrives, so a reader that must not act on the
   * wrong track's beats checks this first.
   */
  gridTrackId: string | null;
}

export function useTrackGrid(track: RowDto | null): TrackGrid {
  const [grid, setGrid] = useState<BeatGrid>(NO_BEATS);
  const [state, setState] = useState<GridState | null>(null);
  const [gridTrackId, setGridTrackId] = useState<string | null>(null);

  useEffect(() => {
    if (!track || !track.analysed) {
      setGrid(NO_BEATS);
      setState(null);
      setGridTrackId(track?.id ?? null);
      return;
    }
    const id = track.id;
    let live = true;
    let stopGrid: (() => void) | undefined;
    let stopAnalysis: (() => void) | undefined;
    let stopLibrary: (() => void) | undefined;
    let request = 0;

    const fetch = async () => {
      const current = ++request;
      const backend = await getBackend();
      const [bytes, found] = await Promise.all([
        backend.trackBeats(id),
        // A track with no grid answers `notFound`; that is a state, not
        // an error the deck has to show.
        backend.gridState(id).catch(() => null),
      ]);
      // The track may have changed while this was in flight.
      if (!live || current !== request) return;
      setGrid(parseBeatGrid(bytes));
      setState(found);
      setGridTrackId(id);
    };

    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      // Subscribed before the first fetch lands, so an edit made in the gap
      // is not missed.
      stopGrid = backend.onGridChanged((changed) => {
        if (changed === id) void fetch();
      });
      stopAnalysis = backend.onAnalysisChanged((changed) => {
        if (changed === id) void fetch();
      });
      stopLibrary = backend.onLibraryChanged(() => {
        void fetch();
      });
      await fetch();
    })();

    return () => {
      live = false;
      stopGrid?.();
      stopAnalysis?.();
      stopLibrary?.();
    };
  }, [track]);

  return { grid, state, setState, gridTrackId };
}
