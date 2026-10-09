/** Beat-grid controls recovered from rekordbox's BeatGridAdjustment/TapButton. */
import { useCallback, useEffect, useRef, useState } from "react";
import type { DeckId, GridEdit, GridEditOptions, GridState } from "@/ipc/types";
import { getBackend } from "@/ipc/client";
import { useTranslation } from "@/i18n";
import { SHIFT_MS, HELD_SHIFT_MS, MAX_STRETCH_MS, tapTempo, tapTimeout, withTap } from "@/lib/gridEdit";

export interface GridEditorDeck {
  trackId: string | null;
  deck: DeckId;
  state: GridState | null;
  setState: (state: GridState) => void;
  positionMs: () => number;
  readOnly: boolean;
  durationMs?: number;
  isDynamicFrom?: (fromMs: number | null) => boolean;
  confirmDynamic?: () => boolean | Promise<boolean>;
  onError?: ((message: string | null) => void) | undefined;
  /**
   * Called on each shift press with its milliseconds, before the save. The
   * deck plays the shifted grid from here, so the shift sounds at once.
   */
  onNudge?: ((ms: number) => void) | undefined;
}
export interface GridEditorActions {
  state: GridState | null;
  /** True while a shift press is not saved yet. */
  nudging: boolean;
  hasGrid: boolean;
  canEdit: boolean;
  fromMs: number | null;
  tapBpmX100: number | null;
  tap: () => void;
  mark: () => void;
  shift: (direction: -1 | 1, held?: boolean) => void;
  stretch: (direction: -1 | 1, held?: boolean) => void;
  double: () => void;
  halve: () => void;
  adjustAll: () => void;
  adjustFrom: () => void;
  align: () => void;
  setBpm: (value: string) => void;
  undo: () => void;
  redo: () => void;
  toggleLock: () => void;
}
function describe(error: unknown): string {
  // Tauri rejects with a bare string when a call never reaches a command
  // (unknown command, refused permission, arguments it cannot read); saying
  // it beats a generic failure nobody can trace (#107).
  if (typeof error === "string" && error.trim()) return error;
  if (typeof error === "object" && error !== null && "message" in error && typeof error.message === "string" && error.message.trim()) return error.message;
  return "The beat grid could not be saved.";
}
export function useGridEditor(deck: GridEditorDeck): GridEditorActions {
  const { trackId, deck: deckId, state, setState, positionMs, readOnly, onError, isDynamicFrom, confirmDynamic, onNudge } = deck;
  const t = useTranslation();
  // The playhead can sit up to five seconds before the track starts, but the
  // grid has no time before zero and the command takes unsigned
  // milliseconds: a negative time was refused before it reached the backend,
  // so the edit failed with no reason. The start of the track is the nearest
  // time the grid has.
  const playheadMs = useCallback(() => Math.max(0, Math.round(positionMs())), [positionMs]);
  const hasGrid = trackId !== null && state !== null && state.beats > 0;
  const canEdit = hasGrid && !readOnly && !state.locked;
  const [fromMs, setFromMs] = useState<number | null>(null);
  const [taps, setTaps] = useState<number[]>([]);
  const tapsRef = useRef<number[]>([]);
  const tapAnchor = useRef(0);
  const tapRun = useRef(0);
  const session = useRef(crypto.randomUUID());
  const tapTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queue = useRef(Promise.resolve());
  // The shift presses not saved yet. One save runs at a time and the presses
  // that come during it go out together in the next, so a held button never
  // queues more than one save behind the one in flight.
  const pendingNudge = useRef(0);
  const nudgeQueued = useRef(false);
  const [nudging, setNudging] = useState(false);
  // The stretch presses not saved yet, the same way: a stretch rewrites the
  // tempo, so each save is a database write and a library reload too, and a
  // held widen/narrow saving every repeat ran on for seconds after release
  // (#196). One batch waits behind the save in flight; its target is the
  // beat under the playhead at its first press.
  const pendingStretch = useRef<{ byMs: number; timeMs: number; fromMs: number | null } | null>(null);
  const currentTrack = useRef(trackId);
  currentTrack.current = trackId;
  const cancelTaps = useCallback(() => {
    if (tapTimer.current) clearTimeout(tapTimer.current);
    tapTimer.current = null;
    tapsRef.current = [];
    setTaps([]);
  }, []);
  useEffect(() => {
    pendingNudge.current = 0;
    nudgeQueued.current = false;
    pendingStretch.current = null;
    setNudging(false);
  }, [trackId]);
  useEffect(() => { setFromMs(null); cancelTaps(); return () => { if (tapTimer.current) clearTimeout(tapTimer.current); }; }, [trackId, cancelTaps]);
  useEffect(() => { if (readOnly || state?.locked) cancelTaps(); }, [readOnly, state?.locked, cancelTaps]);

  // A null result is an action that had nothing to send.
  const run = useCallback((action: (edits: Awaited<ReturnType<typeof getBackend>>["edits"]) => Promise<GridState | null>,
    after?: () => void, done?: () => void) => {
    queue.current = queue.current.then(async () => {
      try {
        if (currentTrack.current !== trackId) return;
        const backend = await getBackend();
        const next = await action(backend.edits);
        if (currentTrack.current !== trackId || next === null) return;
        setState(next);
        after?.();
        onError?.(null);
      } catch (error) { if (currentTrack.current === trackId) onError?.(describe(error)); }
      finally { done?.(); }
    });
  }, [trackId, setState, onError]);
  // `take` names the edit when its turn in the queue comes, so presses made
  // while it waited can still join it; null sends nothing.
  const send = useCallback((take: () => { change: GridEdit; transaction?: string | undefined } | null, boundary: number | null, after?: () => void) => {
    if (!canEdit || trackId === null) return;
    const options: GridEditOptions = { deck: deckId };
    if (deck.durationMs !== undefined) options.durationMs = deck.durationMs;
    if (boundary !== null) options.fromMs = boundary;
    run(async edits => {
      const taken = take();
      if (taken === null) return null;
      const { change, transaction } = taken;
      if (transaction) options.transaction = transaction;
      if ((change.kind === "stretch" || change.kind === "tempo") && isDynamicFrom?.(boundary)) {
        // The backend's dialog, not window.confirm. tauri-plugin-dialog's
        // init script replaces window.confirm with a call to a
        // `plugin:dialog|confirm` command that 2.7 no longer registers, so it
        // rejected with a bare string and no dialog was ever drawn: on Linux
        // every tempo edit over tempo changes failed with "could not be
        // saved" (#107; the macOS edit that did nothing in #86 fits too).
        const question = t("This section has tempo changes. Replace them with a constant tempo?");
        const allowed = await (confirmDynamic?.() ?? (await getBackend()).confirm(question));
        if (!allowed) return state;
        options.allowDynamic = true;
      }
      return edits.gridEdit(trackId, change, options);
    }, after);
  }, [canEdit, trackId, deckId, run, isDynamicFrom, confirmDynamic, state, deck.durationMs, t]);
  const edit = useCallback((change: GridEdit, boundary = fromMs, transaction?: string, after?: () => void) => {
    send(() => ({ change, transaction }), boundary, after);
  }, [send, fromMs]);
  const mark = useCallback(() => { if (fromMs === null) edit({ kind: "downbeat", timeMs: playheadMs() }); }, [edit, fromMs, playheadMs]);
  const shift = useCallback((direction: -1 | 1, held = false) => {
    if (fromMs !== null || !canEdit || trackId === null) return;
    const ms = direction * (held ? HELD_SHIFT_MS : SHIFT_MS);
    onNudge?.(ms);
    pendingNudge.current += ms;
    if (nudgeQueued.current) return;
    nudgeQueued.current = true;
    setNudging(true);
    // No deck: the deck already plays the shifted grid, and a save that ends
    // behind newer presses must not put an older grid on its metronome.
    const options: GridEditOptions = {};
    if (deck.durationMs !== undefined) options.durationMs = deck.durationMs;
    run(edits => {
      nudgeQueued.current = false;
      const sum = pendingNudge.current;
      pendingNudge.current = 0;
      return sum === 0 ? Promise.resolve(null) : edits.gridEdit(trackId, { kind: "nudge", ms: sum }, options);
    }, undefined, () => { if (!nudgeQueued.current) setNudging(false); });
  }, [fromMs, canEdit, trackId, onNudge, deck.durationMs, run]);
  const stretch = useCallback((direction: -1 | 1, held = false) => {
    if (!canEdit || trackId === null) return;
    const byMs = -direction * (held ? HELD_SHIFT_MS : SHIFT_MS);
    const open = pendingStretch.current;
    // The backend moves the target beat at most MAX_STRETCH_MS a save, so a
    // batch that would pass it starts the next one instead.
    if (open !== null && open.fromMs === fromMs && Math.abs(open.byMs + byMs) <= MAX_STRETCH_MS) {
      open.byMs += byMs;
      return;
    }
    const batch = { byMs, timeMs: playheadMs(), fromMs };
    pendingStretch.current = batch;
    send(() => {
      if (pendingStretch.current === batch) pendingStretch.current = null;
      return batch.byMs === 0 ? null : { change: { kind: "stretch", byMs: batch.byMs, timeMs: batch.timeMs } };
    }, fromMs);
  }, [canEdit, trackId, fromMs, playheadMs, send]);
  const double = useCallback(() => edit({ kind: "double" }), [edit]);
  const halve = useCallback(() => edit({ kind: "halve" }), [edit]);
  const adjustAll = useCallback(() => { if (canEdit) { cancelTaps(); setFromMs(null); } }, [canEdit, cancelTaps]);
  const adjustFrom = useCallback(() => {
    if (!canEdit) return;
    cancelTaps();
    const at = playheadMs();
    edit({ kind: "align", timeMs: at }, at, undefined, () => setFromMs(at));
  }, [canEdit, cancelTaps, playheadMs, edit]);
  const align = useCallback(() => { if (fromMs === null) edit({ kind: "align", timeMs: playheadMs() }); }, [edit, fromMs, playheadMs]);
  const setBpm = useCallback((value: string) => {
    const bpm = Number(value);
    if (!value.trim() || !Number.isFinite(bpm) || bpm < 40 || bpm > 499) { onError?.("Enter a BPM from 40 to 499."); return; }
    edit({ kind: "tempo", bpmX100: Math.round(bpm * 100), anchorMs: fromMs ?? 0 });
  }, [edit, fromMs, onError]);
  const tap = useCallback(() => {
    if (!canEdit || fromMs !== null) return;
    const next = withTap(tapsRef.current, performance.now());
    if (next.length === 1) { tapAnchor.current = playheadMs(); tapRun.current++; }
    tapsRef.current = next;
    setTaps(next);
    if (tapTimer.current) clearTimeout(tapTimer.current);
    if (next.length) tapTimer.current = setTimeout(cancelTaps, tapTimeout(next));
    const bpmX100 = tapTempo(next);
    if (bpmX100 !== null) edit({ kind: "tap", bpm: 60_000 * (next.length - 1) / (next[next.length - 1]! - next[0]!), anchorMs: tapAnchor.current }, null, `${session.current}:${deckId}:${trackId}:${tapRun.current}`);
  }, [canEdit, fromMs, playheadMs, cancelTaps, edit, deckId, trackId]);
  const undo = useCallback(() => { if (hasGrid && !readOnly && trackId !== null) { cancelTaps(); run(edits => edits.gridUndo(trackId, deckId)); } }, [hasGrid, readOnly, trackId, deckId, run, cancelTaps]);
  const redo = useCallback(() => { if (hasGrid && !readOnly && trackId !== null) { cancelTaps(); run(edits => edits.gridRedo(trackId, deckId)); } }, [hasGrid, readOnly, trackId, deckId, run, cancelTaps]);
  const toggleLock = useCallback(() => {
    if (hasGrid && !readOnly && trackId !== null && state !== null) { cancelTaps(); run(edits => edits.gridLock(trackId, !state.locked)); }
  }, [hasGrid, readOnly, trackId, state, cancelTaps, run]);
  return { state, nudging, hasGrid, canEdit, fromMs, tapBpmX100: tapTempo(taps), tap, mark, shift, stretch, double, halve, adjustAll, adjustFrom, align, setBpm, undo, redo, toggleLock };
}
