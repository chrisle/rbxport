/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, GridState, GridEdit } from "@/ipc/types";
import { SHIFT_MS, HELD_SHIFT_MS } from "@/lib/gridEdit";
import { useGridEditor, type GridEditorActions } from "./useGridEditor";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

/** A grid the panel is happy with: 128.00 BPM over four hundred beats. */
const gridState = (patch: Partial<GridState> = {}): GridState => ({
  bpmX100: 12_800,
  beats: 400,
  canUndo: false,
  canRedo: false,
  locked: false,
  ...patch,
});

let host: HTMLDivElement;
let root: Root;
/** What the hook returned on the last render. */
let grid: GridEditorActions;
/** The playhead the panel reads when a button goes down, in milliseconds. */
let playhead: number;
/** `performance.now()`, moved by hand so a tap run is exact. */
let clock: number;
let setState: ReturnType<typeof vi.fn>;
let onError: ReturnType<typeof vi.fn>;
let confirm: ReturnType<typeof vi.fn>;
let onNudge: ReturnType<typeof vi.fn>;
let edits: {
  gridEdit: ReturnType<typeof vi.fn>;
  gridUndo: ReturnType<typeof vi.fn>;
  gridRedo: ReturnType<typeof vi.fn>;
  gridLock: ReturnType<typeof vi.fn>;
};

interface ProbeProps {
  trackId: string | null;
  state: GridState | null;
  readOnly: boolean;
  dynamic?: boolean;
}

/** Mounts the hook and hands the test what it returned. */
function Probe({ trackId, state, readOnly, dynamic = false }: ProbeProps) {
  grid = useGridEditor({
    trackId,
    deck: "a",
    state,
    setState,
    positionMs: () => playhead,
    readOnly,
    onError,
    isDynamicFrom: () => dynamic,
    onNudge,
  });
  return null;
}

function mount(props: Partial<ProbeProps> = {}) {
  const full: ProbeProps = { trackId: "t1", state: gridState(), readOnly: false, ...props };
  act(() => {
    root.render(<Probe {...full} />);
  });
}

/**
 * Lets the writes a button fires off reach the backend and come back. Every
 * step of `run` is a microtask — the backend is already resolved here — so a
 * few turns of the queue is the whole round trip.
 */
const settle = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });

/** Moves the clock and the timers together, the way time actually passes. */
function tick(ms: number) {
  clock += ms;
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  clock = 1_000;
  vi.spyOn(performance, "now").mockImplementation(() => clock);
  playhead = 0;
  setState = vi.fn();
  onError = vi.fn();
  onNudge = vi.fn();
  edits = {
    gridEdit: vi.fn(() => Promise.resolve(gridState())),
    gridUndo: vi.fn(() => Promise.resolve(gridState())),
    gridRedo: vi.fn(() => Promise.resolve(gridState())),
    gridLock: vi.fn(() => Promise.resolve(gridState())),
  };
  confirm = vi.fn(() => Promise.resolve(true));
  __setBackend({ edits, confirm } as unknown as Backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("recovered grid control behavior", () => {
  it("sends click and held shifts and stretches with the playhead", async () => {
    mount(); playhead = 2500;
    act(() => grid.shift(-1)); await settle();
    act(() => grid.shift(1, true)); await settle();
    act(() => grid.stretch(1)); await settle();
    expect(edits.gridEdit.mock.calls.map(call => call[1] as GridEdit)).toEqual([
      { kind: "nudge", ms: -SHIFT_MS }, { kind: "nudge", ms: HELD_SHIFT_MS },
      { kind: "stretch", byMs: -1, timeMs: 2500 },
    ]);
  });
  it("plays each shift at once and saves the presses made during a save as one", async () => {
    let finish: (state: GridState) => void = () => {};
    edits.gridEdit.mockImplementationOnce(() => new Promise<GridState>(resolve => { finish = resolve; }));
    mount();
    act(() => grid.shift(1)); await settle();
    expect(grid.nudging).toBe(true);
    act(() => { grid.shift(1, true); grid.shift(1, true); grid.shift(-1); });
    expect(onNudge.mock.calls.map(call => call[0] as number)).toEqual([SHIFT_MS, HELD_SHIFT_MS, HELD_SHIFT_MS, -SHIFT_MS]);
    await settle();
    expect(edits.gridEdit).toHaveBeenCalledTimes(1);
    act(() => finish(gridState())); await settle();
    expect(edits.gridEdit.mock.calls.map(call => call[1] as GridEdit)).toEqual([
      { kind: "nudge", ms: SHIFT_MS }, { kind: "nudge", ms: 2 * HELD_SHIFT_MS - SHIFT_MS },
    ]);
    expect(edits.gridEdit.mock.calls.every(call => (call[2] as { deck?: string }).deck === undefined)).toBe(true);
    expect(grid.nudging).toBe(false);
  });
  it("saves nothing when the presses cancel out", async () => {
    mount();
    act(() => { grid.shift(1); grid.shift(-1); }); await settle();
    expect(edits.gridEdit).not.toHaveBeenCalled();
    expect(grid.nudging).toBe(false);
  });
  it("aligns from here, suppresses whole-grid controls, and clears scope without a write", async () => {
    mount(); playhead = 2400;
    act(() => grid.adjustFrom()); await settle();
    expect(grid.fromMs).toBe(2400);
    expect(edits.gridEdit).toHaveBeenLastCalledWith("t1", { kind: "align", timeMs: 2400 }, { deck: "a", fromMs: 2400 });
    act(() => { grid.mark(); grid.shift(1); grid.tap(); }); await settle();
    expect(edits.gridEdit).toHaveBeenCalledTimes(1);
    act(() => grid.adjustAll()); await settle();
    expect(grid.fromMs).toBeNull();
    expect(edits.gridEdit).toHaveBeenCalledTimes(1);
  });
  it("applies the second tap immediately and groups subsequent taps", async () => {
    mount(); playhead = 1300;
    act(() => grid.tap()); await settle();
    expect(edits.gridEdit).not.toHaveBeenCalled();
    tick(500); act(() => grid.tap()); await settle();
    expect(edits.gridEdit.mock.calls[0]?.[1]).toEqual({ kind: "tap", bpm: 120, anchorMs: 1300 });
    tick(500); act(() => grid.tap()); await settle();
    expect(edits.gridEdit.mock.calls[1]?.[2].transaction).toBe(edits.gridEdit.mock.calls[0]?.[2].transaction);
    tick(751); await settle();
    expect(grid.tapBpmX100).toBeNull();
    expect(edits.gridEdit).toHaveBeenCalledTimes(2);
  });
  it("validates typed BPM and sends a tempo edit", async () => {
    mount(); act(() => { grid.setBpm("20"); grid.setBpm("500"); }); await settle();
    expect(edits.gridEdit).not.toHaveBeenCalled();
    act(() => grid.setBpm("128.5")); await settle();
    expect(edits.gridEdit.mock.calls[0]?.[1]).toEqual({ kind: "tempo", bpmX100: 12850, anchorMs: 0 });
  });
  it("refuses edits and lock changes in a read-only library", async () => {
    mount({readOnly: true});
    act(() => { grid.double(); grid.mark(); grid.tap(); grid.toggleLock(); }); await settle();
    expect(edits.gridEdit).not.toHaveBeenCalled(); expect(edits.gridLock).not.toHaveBeenCalled();
  });
  it("allows unlocking a locked grid but refuses edits", async () => {
    mount({state: gridState({locked: true})});
    act(() => { grid.double(); grid.toggleLock(); }); await settle();
    expect(edits.gridEdit).not.toHaveBeenCalled(); expect(edits.gridLock).toHaveBeenCalledWith("t1", false);
  });
  it("clears scope when loading another track", async () => {
    mount(); act(() => grid.adjustFrom()); await settle();
    mount({trackId: "t2"}); expect(grid.fromMs).toBeNull();
  });
  it("asks through the backend's dialog before flattening tempo changes", async () => {
    // window.confirm is answered Cancel by WKWebView on macOS (issue #86).
    const native = vi.spyOn(window, "confirm").mockReturnValue(false);
    mount({ dynamic: true });
    act(() => grid.setBpm("128")); await settle(); await settle();
    expect(native).not.toHaveBeenCalled();
    expect(confirm).toHaveBeenCalledWith("This section has tempo changes. Replace them with a constant tempo?");
    expect(edits.gridEdit).toHaveBeenLastCalledWith("t1", { kind: "tempo", bpmX100: 12800, anchorMs: 0 }, { deck: "a", allowDynamic: true });
    confirm.mockResolvedValue(false);
    act(() => grid.stretch(1)); await settle(); await settle();
    expect(edits.gridEdit).toHaveBeenCalledTimes(1);
  });
  it("sends a playhead before the track's start as its start", async () => {
    // The deck can sit up to five seconds before zero; the command takes
    // unsigned milliseconds (#107: "set 1st beat" there failed to save).
    mount(); playhead = -1234.4;
    act(() => { grid.mark(); grid.align(); grid.stretch(1); }); await settle(); await settle(); await settle();
    expect(edits.gridEdit.mock.calls.map(call => call[1] as GridEdit)).toEqual([
      { kind: "downbeat", timeMs: 0 }, { kind: "align", timeMs: 0 }, { kind: "stretch", byMs: -1, timeMs: 0 },
    ]);
    act(() => grid.adjustFrom()); await settle();
    expect(edits.gridEdit).toHaveBeenLastCalledWith("t1", { kind: "align", timeMs: 0 }, { deck: "a", fromMs: 0 });
    act(() => grid.adjustAll());
    act(() => grid.tap()); tick(500); act(() => grid.tap()); await settle();
    expect(edits.gridEdit.mock.calls.at(-1)?.[1]).toEqual({ kind: "tap", bpm: 120, anchorMs: 0 });
  });
  it("reports failed writes", async () => {
    edits.gridEdit.mockRejectedValue({message: "write failed"}); mount();
    act(() => grid.double()); await settle(); expect(onError).toHaveBeenCalledWith("write failed");
  });
});
