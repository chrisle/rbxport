/** Beat-grid controls recovered from rekordbox's BeatGridAdjustment/TapButton. */
import { useCallback, useEffect, useRef, useState } from "react";
import type { DeckId, GridEdit, GridEditOptions, GridState } from "@/ipc/types";
import { getBackend } from "@/ipc/client";
import { useTranslation } from "@/i18n";
import { SHIFT_MS, HELD_SHIFT_MS, tapTempo, tapTimeout, withTap } from "@/lib/gridEdit";

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
}
export interface GridEditorActions {
  state: GridState | null;
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
  if (typeof error === "object" && error !== null && "message" in error && typeof error.message === "string" && error.message.trim()) return error.message;
  return "The beat grid could not be saved.";
}
export function useGridEditor(deck: GridEditorDeck): GridEditorActions {
  const { trackId, deck: deckId, state, setState, positionMs, readOnly, onError, isDynamicFrom, confirmDynamic } = deck;
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
  const currentTrack = useRef(trackId);
  currentTrack.current = trackId;
  const cancelTaps = useCallback(() => {
    if (tapTimer.current) clearTimeout(tapTimer.current);
    tapTimer.current = null;
    tapsRef.current = [];
    setTaps([]);
  }, []);
  useEffect(() => { setFromMs(null); cancelTaps(); return () => { if (tapTimer.current) clearTimeout(tapTimer.current); }; }, [trackId, cancelTaps]);
  useEffect(() => { if (readOnly || state?.locked) cancelTaps(); }, [readOnly, state?.locked, cancelTaps]);

  const run = useCallback((action: (edits: Awaited<ReturnType<typeof getBackend>>["edits"]) => Promise<GridState>, after?: () => void) => {
    queue.current = queue.current.then(async () => {
      if (currentTrack.current !== trackId) return;
      try {
        const backend = await getBackend();
        const next = await action(backend.edits);
        if (currentTrack.current !== trackId) return;
        setState(next);
        after?.();
        onError?.(null);
      } catch (error) { if (currentTrack.current === trackId) onError?.(describe(error)); }
    });
  }, [trackId, setState, onError]);
  const edit = useCallback((change: GridEdit, boundary = fromMs, transaction?: string, after?: () => void) => {
    if (!canEdit || trackId === null) return;
    const options: GridEditOptions = { deck: deckId };
    if (deck.durationMs !== undefined) options.durationMs = deck.durationMs;
    if (boundary !== null) options.fromMs = boundary;
    if (transaction) options.transaction = transaction;
    run(async edits => {
      if ((change.kind === "stretch" || change.kind === "tempo") && isDynamicFrom?.(boundary)) {
        // The backend's dialog, not window.confirm: WKWebView answers
        // window.confirm with Cancel when the host draws no panel, so on
        // macOS the edit silently did nothing.
        const question = t("This section has tempo changes. Replace them with a constant tempo?");
        const allowed = await (confirmDynamic?.() ?? (await getBackend()).confirm(question));
        if (!allowed) return state;
        options.allowDynamic = true;
      }
      return edits.gridEdit(trackId, change, options);
    }, after);
  }, [canEdit, trackId, deckId, fromMs, run, isDynamicFrom, confirmDynamic, state, deck.durationMs, t]);
  const mark = useCallback(() => { if (fromMs === null) edit({ kind: "downbeat", timeMs: playheadMs() }); }, [edit, fromMs, playheadMs]);
  const shift = useCallback((direction: -1 | 1, held = false) => {
    if (fromMs === null) edit({ kind: "nudge", ms: direction * (held ? HELD_SHIFT_MS : SHIFT_MS) });
  }, [edit, fromMs]);
  const stretch = useCallback((direction: -1 | 1, held = false) => {
    edit({ kind: "stretch", byMs: -direction * (held ? HELD_SHIFT_MS : SHIFT_MS), timeMs: playheadMs() });
  }, [edit, playheadMs]);
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
  return { state, hasGrid, canEdit, fromMs, tapBpmX100: tapTempo(taps), tap, mark, shift, stretch, double, halve, adjustAll, adjustFrom, align, setBpm, undo, redo, toggleLock };
}
