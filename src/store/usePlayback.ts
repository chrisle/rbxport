/**
 * Playback for the preview player.
 *
 * The audio itself is in Rust — `crates/rbl-deck` — rather than on an
 * `<audio>` element. A media element has no primitive for phase-locked beat
 * sync, key sync or audible drag-scrub, which is what the 2-player view needs;
 * see `docs/pre-release/player-engine.md`.
 *
 * Position does not come back from a command. The engine emits one tick ten
 * times a second carrying both decks' frame counters, and every frame in
 * between is that anchor plus the time since it arrived. Sixty ticks a second
 * would be IPC churn and the interface would still have to interpolate.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getBackend } from "@/ipc/client";
import type { AppErrorDto, Backend, DeckId, Tick } from "@/ipc/types";
import { canPlay } from "@/ipc/audio";
import { extrapolate, follow, NO_ANCHOR, pinned, SNAP_SECONDS, type Anchor } from "@/lib/clock";

export interface Playback {
  /** True while audio is actually running. */
  playing: boolean;
  /**
   * Seconds elapsed, as React state — updated on each tick, ten times a
   * second, which is as fine as the readouts get and as often as the waveform
   * is worth redrawing.
   *
   * The playhead does not use this: see `subscribe`.
   */
  position: number;
  /** Seconds total, or 0 before the deck has said. */
  duration: number;
  /** Nothing to play: no track, or a build with no engine behind it. */
  idle: boolean;
  error: string | null;
  toggle: () => void;
  /**
   * Starts a stopped deck after `delayMs` of silence: quantized play on a
   * synced deck, held for the master's next beat. The engine counts the wait
   * in its own output frames; the playhead here waits the same time.
   */
  playAfter: (delayMs: number) => void;
  seek: (seconds: number) => void;
  /** Seek by fraction, for clicking the waveform. */
  seekFraction: (fraction: number) => void;
  /** The deck's loop, as the engine reports it; null for none. */
  loop: DeckLoop | null;
  /** Sets a loop between two points, in seconds, and turns it on. */
  setLoop: (inSeconds: number, outSeconds: number) => void;
  /** RELOOP (true) or EXIT (false). */
  setLoopActive: (on: boolean) => void;
  clearLoop: () => void;
  /**
   * Dragging a waveform, with the audio following the pointer.
   *
   * `scrubBegin` starts the deck if it was stopped and remembers that it was,
   * `scrubTo` moves it, and `scrubEnd` puts the transport back the way it was
   * found. Measured on the reference library: seeking sixty times a second
   * while playing leaves 2 % of buffers empty and thirty times a second none
   * at all, so the audio follows a drag without a cache behind it.
   */
  scrubBegin: () => void;
  scrubTo: (seconds: number) => void;
  /**
   * Ends a drag. `snap` can move the landing place: a synced deck lands in
   * phase with the master. It gets where the drag let go, in seconds.
   */
  scrubEnd: (snap?: (seconds: number) => number) => void;
  /**
   * How fast the deck is playing, as a multiple of the file's own speed.
   *
   * 1 is the track as recorded. What BPM that comes to is the caller's to work
   * out from the track, because the deck does not know the track's.
   */
  tempo: number;
  /** Whether the pitch is held while the speed changes: rekordbox's MT. */
  masterTempo: boolean;
  /** Semitones from the track's own key: the KEY SHIFT buttons. */
  keyShift: number;
  /** Whether this build can shift a key at all (the Rubber Band backend). */
  shiftsKey: boolean;
  setKeyShift: (semitones: number) => void;
  setTempo: (tempo: number) => void;
  /** One step of the tempo control, up or down. */
  nudgeTempo: (direction: number) => void;
  setMasterTempo: (on: boolean) => void;
  /** Where playback is right now, without waiting for a render. */
  positionRef: React.RefObject<number>;
  /**
   * Where playback is at this instant, extrapolated from the last tick
   * rather than read from the last frame: `positionRef` moves with the
   * frame loop, which is up to a frame behind. Sync reads this, because a
   * beat is only a few frames.
   */
  positionNow: () => number;
  /**
   * Every frame while playing, and once on each seek.
   *
   * The playhead is driven from here rather than from `position`: state feeds
   * the whole player subtree, so a faster tick only re-renders it faster. A
   * listener writes a transform straight to its own element instead.
   *
   * Returns its own unsubscribe.
   */
  subscribe: (listener: (seconds: number) => void) => () => void;
}

/** A deck's loop, in seconds. */
export interface DeckLoop {
  inSeconds: number;
  outSeconds: number;
  /** Inside it (RELOOP) or out of it with the range kept (EXIT). */
  active: boolean;
}

/** The preview player is deck A; the 2-player layout adds B. */
const DEFAULT_DECK: DeckId = "a";

/**
 * How long letting go waits for the seek it asked for before trusting ticks
 * again.
 *
 * Ticks arrive every 100 ms and the seek is one command behind them, so the
 * first tick or two after a drag still carries where the read head was
 * mid-drag. The wait normally ends on the tick that agrees with where the drag
 * ended; this is the bound, so a deck that never seeks — unloaded while being
 * dragged — cannot leave the playhead frozen.
 */
const LANDING_MS = 500;

/** What went wrong, in the words of whoever knows. */
const FALLBACK = "This track could not be played.";

/**
 * Which track each deck was last told to hold.
 *
 * Kept outside the hook because the hook does not outlive the deck's view:
 * FULL BROWSER unmounts the player, and 1 PLAYER puts deck B away. The engine
 * keeps playing through both, and a remount used to load the same track
 * again, which stopped it and put the head back at the start. A null is a
 * deck that was told to hold nothing, or whose load failed. The engine does
 * not report which track it holds, which is why this is remembered here
 * rather than asked.
 */
interface HeldLoad {
  trackId: string;
  loadId: number;
}

const held = new Map<DeckId, HeldLoad | null>();
let nextLoadId = 0;

function allocateLoadId(): number {
  nextLoadId = nextLoadId >= Number.MAX_SAFE_INTEGER ? 1 : nextLoadId + 1;
  return nextLoadId;
}

/**
 * The reason, not a shrug.
 *
 * Every one of these failures arrives carrying why: a command rejects with an
 * `AppError` whose message says whether the file is missing or the audio
 * device would not open, and the deck's own error event carries what the
 * decoder said. Roughly one track in thirty of the reference library sits on a
 * volume that is not mounted, and "This track could not be played" leaves the
 * only useful fact — plug the drive in — on the floor.
 */
export function reasonFrom(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as Partial<AppErrorDto>).message;
    if (typeof message === "string" && message.trim() !== "") return message;
  }
  return FALLBACK;
}

export function usePlayback(trackId: string | null, DECK: DeckId = DEFAULT_DECK, renderPosition = true): Playback {
  const [playing, setPlaying] = useState(false);
  const [tempo, setTempoState] = useState(1);
  const [masterTempo, setMasterTempoState] = useState(false);
  const [keyShift, setKeyShiftState] = useState(0);
  const [shiftsKey, setShiftsKey] = useState(true);
  const [loop, setLoopState] = useState<DeckLoop | null>(null);
  const [position, setPositionState] = useState(0);
  // The deck opts out: its small time readout subscribes to the frame clock.
  // Other callers retain the tick-driven position API.
  const setPosition = useCallback((seconds: number) => {
    if (renderPosition) setPositionState(seconds);
  }, [renderPosition]);
  const [duration, setDuration] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // The live position, and who wants it every frame.
  const positionRef = useRef(0);
  const listeners = useRef(new Set<(seconds: number) => void>());
  // The last tick, which every frame in between is measured from.
  const anchor = useRef<Anchor>(NO_ANCHOR);
  /** The generation the playhead last snapped to. */
  const shownGeneration = useRef(0);
  /** The selected load, and the one whose audio is actually ready. */
  const selectedTrack = useRef(trackId);
  selectedTrack.current = trackId;
  const targetLoad = useRef<HeldLoad | null>(held.get(DECK) ?? null);
  const readyLoad = useRef(0);
  const loadPending = useRef(false);
  const applyingLoad = useRef(0);
  /** Transport/settings intent survives a load and an audio-engine rebuild. */
  const desiredPlaying = useRef(false);
  const desiredDelay = useRef<number | null>(null);
  const transportVersion = useRef(0);
  const deferredSeek = useRef<number | null>(null);
  const desiredTempo = useRef(1);
  const desiredMasterTempo = useRef(false);
  const desiredKeyShift = useRef(0);
  const desiredLoop = useRef<DeckLoop | null>(null);
  /** Whether a drag is running, so a move is aimed rather than seeked. */
  const scrubbing = useRef(false);
  /** When a drag let go, until the seek that ends it comes back. */
  const landing = useRef<number | null>(null);
  /** The seek a drag is waiting to send, coalesced to one a frame. */
  const pending = useRef<number | null>(null);
  const flushing = useRef(0);

  const emit = useCallback((seconds: number) => {
    positionRef.current = seconds;
    for (const listener of listeners.current) listener(seconds);
  }, []);

  const subscribe = useCallback((listener: (seconds: number) => void) => {
    listeners.current.add(listener);
    return () => {
      listeners.current.delete(listener);
    };
  }, []);

  const currentLoad = useCallback((request: HeldLoad) => {
    const current = targetLoad.current;
    return current?.loadId === request.loadId && current.trackId === request.trackId;
  }, []);

  /** Applies everything pressed while this load was still opening. */
  const completeLoad = useCallback((request: HeldLoad) => {
    if (!currentLoad(request) || !loadPending.current || readyLoad.current === request.loadId) return;
    readyLoad.current = request.loadId;
    loadPending.current = false;
    applyingLoad.current = request.loadId;
    setError(null);
    void (async () => {
      try {
        const backend = await getBackend();
        if (!currentLoad(request)) return;
        await Promise.all([
          backend.deckTempo(DECK, desiredTempo.current),
          backend.deckMasterTempo(DECK, desiredMasterTempo.current),
          backend.deckKeyShift(DECK, desiredKeyShift.current),
        ]);
        if (!currentLoad(request)) return;
        const seekTo = deferredSeek.current;
        deferredSeek.current = null;
        if (seekTo !== null) await backend.deckSeek(DECK, seekTo * 1000);
        const wantedLoop = desiredLoop.current;
        if (wantedLoop !== null) {
          await backend.deckSetLoop(DECK, wantedLoop.inSeconds * 1000, wantedLoop.outSeconds * 1000);
          if (!wantedLoop.active) await backend.deckLoopActive(DECK, false);
        }
        if (!currentLoad(request)) return;
        if (desiredPlaying.current) {
          const wait = desiredDelay.current;
          desiredDelay.current = null;
          setPlaying(true);
          anchor.current = {
            ...anchor.current,
            playing: true,
            at: performance.now() + (wait ?? 0),
          };
          if (wait !== null && wait > 0) await backend.deckPlayAfter(DECK, wait);
          else await backend.deckPlay(DECK);
        } else {
          setPlaying(false);
        }
      } catch (failure) {
        if (currentLoad(request)) {
          desiredPlaying.current = false;
          setPlaying(false);
          setError(reasonFrom(failure));
        }
      } finally {
        if (applyingLoad.current === request.loadId) applyingLoad.current = 0;
      }
    })();
  }, [currentLoad, DECK]);

  /** Starts a uniquely identified load; stale completions cannot satisfy it. */
  const beginLoad = useCallback((nextTrack: string, restore: boolean) => {
    const request = { trackId: nextTrack, loadId: allocateLoadId() };
    targetLoad.current = request;
    held.set(DECK, request);
    readyLoad.current = 0;
    loadPending.current = true;
    applyingLoad.current = 0;
    scrubbing.current = false;
    landing.current = null;
    pending.current = null;
    if (restore) deferredSeek.current = positionRef.current;
    else deferredSeek.current = null;
    const at = restore ? positionRef.current : 0;
    anchor.current = pinned(anchor.current, at, performance.now());
    setPosition(at);
    setPlaying(false);
    setError(null);
    if (!restore) {
      setDuration(0);
      setLoopState(null);
      desiredLoop.current = null;
    }
    emit(at);
    void (async () => {
      try {
        const backend = await getBackend();
        if (currentLoad(request)) await backend.deckLoad(DECK, nextTrack, request.loadId);
      } catch (failure) {
        if (currentLoad(request)) {
          held.set(DECK, null);
          targetLoad.current = null;
          loadPending.current = false;
          desiredPlaying.current = false;
          setPlaying(false);
          setError(reasonFrom(failure));
        }
      }
    })();
  }, [currentLoad, DECK, emit, setPosition]);

  /** Takes a tick as the truth about where the deck is. */
  const anchorOn = useCallback(
    (tick: Tick) => {
      const deck = DECK === "b" ? tick.b : tick.a;
      const target = targetLoad.current;
      if (target !== null && deck.loadId !== undefined && deck.loadId !== target.loadId) return;
      if (target !== null && loadPending.current) completeLoad(target);
      if (target !== null && applyingLoad.current === target.loadId) return;
      if (target !== null) readyLoad.current = target.loadId;
      const rate = tick.sampleRate;
      const now = performance.now();
      setPlaying(deck.playing);
      desiredPlaying.current = deck.playing;
      setDuration(rate > 0 ? deck.totalFrames / rate : 0);
      // The engine's word for both, so a deck loaded by something else still
      // shows what it is doing.
      setTempoState(deck.tempo > 0 ? deck.tempo : 1);
      desiredTempo.current = deck.tempo > 0 ? deck.tempo : 1;
      setMasterTempoState(deck.masterTempo);
      desiredMasterTempo.current = deck.masterTempo;
      setKeyShiftState(deck.keyShift);
      desiredKeyShift.current = deck.keyShift;
      setShiftsKey(tick.shiftsKey);
      setLoopState((current) => {
        const next =
          rate > 0 && deck.loopOutFrames > deck.loopInFrames
            ? { inSeconds: deck.loopInFrames / rate, outSeconds: deck.loopOutFrames / rate, active: deck.looping }
            : null;
        // Same loop, same object: a tick must not re-render every reader.
        if (
          current === next ||
          (current !== null && next !== null && current.inSeconds === next.inSeconds &&
            current.outSeconds === next.outSeconds && current.active === next.active)
        ) {
          return current;
        }
        desiredLoop.current = next;
        return next;
      });
      // A drag owns the playhead, and the deck's head is not under the
      // pointer: it is rate-limited so the drag stays audible, so it trails a
      // fast hand and rests a block past a still one. Taking it as the anchor
      // jerked the waveform back ten times a second while the hand held
      // steady, and the frame loop then ran it forward again at playback
      // speed. Pinned instead, at whatever the pointer last said.
      const hold = () => {
        anchor.current = pinned({ ...anchor.current, sampleRate: rate }, positionRef.current, now);
        // The drag bumps a generation at each end. Swallowed here, or the
        // playhead would later snap to a head it was deliberately pinned off.
        shownGeneration.current = deck.generation;
      };
      if (scrubbing.current) {
        hold();
        return;
      }
      if (landing.current !== null) {
        // Let go, but the seek that ends the drag is a command behind the
        // ticks, so the next one or two still carry the mid-drag head. What
        // says the seek has landed is the tick agreeing with where the drag
        // ended — not a generation, because the drag bumps one of those at
        // each end and a drag shorter than a tick has both still in flight.
        // A tick that already agrees is taken, because taking it moves
        // nothing.
        const reported = rate > 0 ? deck.frames / rate : 0;
        const waiting =
          Math.abs(reported - positionRef.current) > SNAP_SECONDS &&
          now - landing.current < LANDING_MS;
        if (waiting) {
          hold();
          return;
        }
        landing.current = null;
      }
      anchor.current = {
        frames: deck.frames,
        // A start held for the beat: the engine's frames say how much of the
        // wait is left, and the head stands still for that long from now.
        at: rate > 0 && deck.startInFrames > 0 ? now + (deck.startInFrames / rate) * 1000 : now,
        startsAt: rate > 0 && deck.startInFrames > 0 ? now + (deck.startInFrames / rate) * 1000 : undefined,
        sampleRate: rate,
        playing: deck.playing,
        generation: deck.generation,
        rate: deck.tempo > 0 ? deck.tempo : 1,
      };
      const at = extrapolate(anchor.current, performance.now());
      setPosition(at);
      // A load or a seek moves the playhead deliberately; anything else is
      // drift, and is eased in rather than jumped.
      if (!deck.playing || deck.generation !== shownGeneration.current) {
        shownGeneration.current = deck.generation;
        emit(at);
      }
    },
    [completeLoad, emit, DECK, setPosition],
  );

  // The deck reports itself loaded, or says why it could not be.
  useEffect(() => {
    if (!canPlay) return;
    let live = true;
    let stop: (() => void) | undefined;
    void (async () => {
      const backend = await getBackend();
      const unlistenTick = backend.onDeckTick((tick) => {
        if (live) anchorOn(tick);
      });
      const unlistenEvent = backend.onDeckEvent((event) => {
        if (!live || event.deck !== DECK) return;
        const target = targetLoad.current;
        if (target === null || event.loadId !== target.loadId) return;
        if (event.message !== null) {
          // A deck that could not open the file holds nothing, so the next
          // mount asks again rather than trusting a load that never landed.
          held.set(DECK, null);
          targetLoad.current = null;
          loadPending.current = false;
          applyingLoad.current = 0;
          desiredPlaying.current = false;
          setPlaying(false);
          setError(event.message.trim() === "" ? FALLBACK : event.message);
          return;
        }
        setError(null);
        if (event.sampleRate > 0) setDuration(event.totalFrames / event.sampleRate);
        completeLoad(target);
      });
      const unlistenReset = backend.onDeckReset?.(() => {
        if (!live) return;
        const current = selectedTrack.current;
        if (current !== null) beginLoad(current, true);
      }) ?? (() => undefined);
      if (!live) {
        unlistenTick();
        unlistenEvent();
        unlistenReset();
        return;
      }
      stop = () => {
        unlistenTick();
        unlistenEvent();
        unlistenReset();
      };
      // What the deck holds right now, so a reload does not start at zero.
      const state = await backend.deckState();
      const deck = DECK === "b" ? state.b : state.a;
      const target = targetLoad.current;
      if (target !== null && deck.loadId !== undefined && deck.loadId !== target.loadId && !loadPending.current) {
        beginLoad(target.trackId, true);
      } else {
        anchorOn(state);
      }
    })();
    return () => {
      live = false;
      stop?.();
    };
  }, [anchorOn, beginLoad, completeLoad, DECK]);

  // Point the deck at the selected track. Loading does not start playback:
  // choosing a track in the browser should not make noise.
  useEffect(() => {
    if (!canPlay) return;
    // The deck already holds this track: the view was put away and brought
    // back around it, and the engine never stopped. Asking again would. Where
    // it has got to arrives with `deckState` above, and with the next tick.
    const current = held.get(DECK);
    if ((current?.trackId ?? null) === trackId) return;
    if (trackId !== null) {
      // PLAY/PAUSE belongs to the deck, not to the file. Loading another
      // track while PLAY is engaged starts that track as soon as it is ready;
      // loading on a stopped deck leaves it cued.
      desiredDelay.current = null;
      beginLoad(trackId, false);
      return;
    }
    held.set(DECK, null);
    targetLoad.current = null;
    readyLoad.current = 0;
    loadPending.current = false;
    applyingLoad.current = 0;
    desiredPlaying.current = false;
    desiredDelay.current = null;
    deferredSeek.current = null;
    desiredLoop.current = null;
    anchor.current = NO_ANCHOR;
    setPosition(0);
    setDuration(0);
    setPlaying(false);
    setLoopState(null);
    setError(null);
    emit(0);
    void getBackend().then((backend) => backend.deckUnload(DECK)).catch((failure) => setError(reasonFrom(failure)));
  }, [trackId, beginLoad, emit, DECK, setPosition]);

  // One frame loop for the whole player, running only while audio is, so an
  // idle window schedules nothing.
  useEffect(() => {
    if (!playing) return;
    let frame = 0;
    let last = performance.now();
    const tick = () => {
      frame = requestAnimationFrame(tick);
      const now = performance.now();
      const target = extrapolate(anchor.current, now);
      // Ordinary tick arrivals must not reset this frame's elapsed motion.
      // Only a scheduled start or a pinned scrub head pauses that motion.
      const advance = anchor.current.playing
        ? Math.max(0, now - Math.max(last, anchor.current.startsAt ?? last)) * anchor.current.rate / 1000
        : 0;
      const next = follow(positionRef.current, target, now - last, advance);
      last = now;
      emit(next);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing, emit]);

  const idle = !canPlay || trackId === null;
  const isLoaded = useCallback(() => {
    const target = targetLoad.current;
    return target !== null && readyLoad.current === target.loadId;
  }, []);
  const isReady = useCallback(() => {
    return isLoaded() && applyingLoad.current === 0;
  }, [isLoaded]);

  const toggle = useCallback(() => {
    if (idle) return;
    // While a load is pending the visible transport is stopped, but a play
    // may already be queued. A second press cancels that intent.
    const wanted = isReady() ? !playing : !desiredPlaying.current;
    desiredPlaying.current = wanted;
    desiredDelay.current = null;
    const version = ++transportVersion.current;
    // The button follows at once rather than on the next tick, which is up to
    // a tenth of a second away.
    if (!isReady()) {
      // Keep the transport intent visible (and cancellable by CUE release),
      // while the pinned anchor prevents the playhead from running early.
      setPlaying(wanted);
      anchor.current = pinned(anchor.current, positionRef.current, performance.now());
      return;
    }
    setPlaying(wanted);
    anchor.current = { ...anchor.current, playing: wanted, at: performance.now() };
    void (async () => {
      try {
        const backend = await getBackend();
        if (version !== transportVersion.current || !isReady()) return;
        if (wanted) await backend.deckPlay(DECK);
        else await backend.deckPause(DECK);
      } catch (failure) {
        desiredPlaying.current = false;
        setPlaying(false);
        setError(reasonFrom(failure));
      }
    })();
  }, [idle, isReady, playing, DECK]);

  const playAfter = useCallback(
    (delayMs: number) => {
      if (idle || playing) return;
      const wait = Number.isFinite(delayMs) ? Math.max(0, delayMs) : 0;
      desiredPlaying.current = true;
      desiredDelay.current = wait;
      const version = ++transportVersion.current;
      if (!isReady()) {
        setPlaying(true);
        anchor.current = pinned(anchor.current, positionRef.current, performance.now());
        return;
      }
      setPlaying(true);
      // Anchored in the future: `extrapolate` holds the head still until then.
      anchor.current = { ...anchor.current, playing: true, at: performance.now() + wait };
      void (async () => {
        try {
          const backend = await getBackend();
          if (version !== transportVersion.current || !isReady()) return;
          await backend.deckPlayAfter(DECK, wait);
        } catch (failure) {
          desiredPlaying.current = false;
          setPlaying(false);
          setError(reasonFrom(failure));
        }
      })();
    },
    [idle, isReady, playing, DECK],
  );

  const seek = useCallback(
    (seconds: number) => {
      if (idle || !Number.isFinite(seconds)) return;
      const at = Math.max(-5, seconds);
      // Locally first: the head must move under the pointer, not a tick later.
      anchor.current = {
        ...anchor.current,
        frames: anchor.current.sampleRate > 0 ? at * anchor.current.sampleRate : 0,
        at: performance.now(),
      };
      setPosition(at);
      emit(at);
      if (!isLoaded()) {
        deferredSeek.current = at;
        return;
      }
      void (async () => {
        try {
          const backend = await getBackend();
          // Not rounded: a whole millisecond is 44 frames at 44.1 kHz and
          // 96 at 96, and a cue point is a place in the music rather
          // than a rounded one.
          await backend.deckSeek(DECK, at * 1000);
        } catch (failure) {
          setError(reasonFrom(failure));
        }
      })();
    },
    [idle, emit, isLoaded, DECK, setPosition],
  );

  /**
   * Audio follows the pointer while a waveform is dragged.
   *
   * The engine does the work — see `crates/rbl-deck/src/scrub.rs`. It reads a
   * decoded window at the drag's own rate, so the pitch follows the hand and
   * pulling backwards plays backwards, which is what a record does. The
   * transport is not touched: a drag sounds whether or not the deck was
   * playing, and letting go leaves it as it was found.
   */
  const scrubBegin = useCallback(() => {
    if (idle || scrubbing.current || !isReady()) return;
    scrubbing.current = true;
    landing.current = null;
    // The head stops running the moment it is grabbed. A playing deck keeps
    // its transport — the drag is not a pause — but what is drawn is the hand,
    // and the hand has not moved yet.
    anchor.current = pinned(anchor.current, positionRef.current, performance.now());
    void (async () => {
      try {
        const backend = await getBackend();
        await backend.deckScrubBegin(DECK);
      } catch (failure) {
        setError(reasonFrom(failure));
      }
    })();
  }, [idle, isReady, DECK]);

  /**
   * Where the drag is now.
   *
   * The playhead and the waveform move on the spot; the message behind them is
   * coalesced to one a frame, because a trackpad emits pointer moves faster
   * than the screen refreshes and the engine only needs the latest.
   */
  const scrubTo = useCallback(
    (seconds: number) => {
      if (idle || !isReady() || !Number.isFinite(seconds)) return;
      const at = Math.max(seconds, -5);
      // Pinned, not merely moved: a drag on a playing deck must not carry on
      // running forward between pointer moves, which is what made a steady
      // hand look like a shaking one.
      anchor.current = pinned(anchor.current, at, performance.now());
      setPosition(at);
      emit(at);
      pending.current = at;
      if (flushing.current) return;
      flushing.current = requestAnimationFrame(() => {
        flushing.current = 0;
        const target = pending.current;
        pending.current = null;
        if (target === null) return;
        void (async () => {
          try {
            const backend = await getBackend();
            // Not rounded: the engine works the head's speed out from how far
            // this moved since the last one, and a whole millisecond is 44
            // frames — enough to quantise a slow drag's speed into a stall and
            // a lurch.
            if (scrubbing.current) await backend.deckScrubTo(DECK, target * 1000);
            else await backend.deckSeek(DECK, target * 1000);
          } catch (failure) {
            setError(reasonFrom(failure));
          }
        })();
      });
    },
    [idle, emit, isReady, DECK, setPosition],
  );

  /** Lets go. The playhead stays where the drag left it. */
  /**
   * One press of the tempo control, as a fraction of the file's speed.
   *
   * A tenth of a percent, which is what a CDJ's ± buttons move by on the
   * finest setting: it is the step somebody uses to hold a beat, and a coarser
   * one would be a control for changing key rather than for mixing.
   */
  const TEMPO_STEP = 0.001;

  const setTempo = useCallback(
    (next: number) => {
      if (idle || !Number.isFinite(next)) return;
      const safe = Math.min(Math.max(next, 0.5), 2);
      desiredTempo.current = safe;
      // Shown at once rather than on the next tick: a fader that answers a
      // tenth of a second later is a fader people press twice.
      setTempoState(safe);
      if (!isLoaded()) return;
      void (async () => {
        try {
          const backend = await getBackend();
          await backend.deckTempo(DECK, safe);
        } catch (failure) {
          setError(reasonFrom(failure));
        }
      })();
    },
    [idle, isLoaded, DECK],
  );

  const nudgeTempo = useCallback(
    (direction: number) => setTempo(tempo + TEMPO_STEP * Math.sign(direction)),
    [setTempo, tempo],
  );

  const setKeyShift = useCallback(
    (semitones: number) => {
      if (idle || !Number.isFinite(semitones)) return;
      const safe = Math.max(-12, Math.min(12, Math.round(semitones)));
      desiredKeyShift.current = safe;
      setKeyShiftState(safe);
      if (!isLoaded()) return;
      void (async () => {
        try {
          const backend = await getBackend();
          await backend.deckKeyShift(DECK, safe);
        } catch (failure) {
          setError(reasonFrom(failure));
        }
      })();
    },
    [idle, isLoaded, DECK],
  );

  const setMasterTempo = useCallback(
    (on: boolean) => {
      if (idle) return;
      desiredMasterTempo.current = on;
      setMasterTempoState(on);
      if (!isLoaded()) return;
      void (async () => {
        try {
          const backend = await getBackend();
          await backend.deckMasterTempo(DECK, on);
        } catch (failure) {
          setError(reasonFrom(failure));
        }
      })();
    },
    [idle, isLoaded, DECK],
  );

  const scrubEnd = useCallback((snap?: (seconds: number) => number) => {
    if (!scrubbing.current) return;
    scrubbing.current = false;
    // Still pinned: the seek is a command behind the ticks, so the next one or
    // two still carry where the head was mid-drag. See `LANDING_MS`.
    landing.current = performance.now();
    // Where the drag last aimed has to reach the deck before the drag ends,
    // because that is what the deck lands on. A click is over well inside one
    // frame, so the rAF that coalesces moves would still be holding the only
    // position anybody asked for when the end arrived, and the press would
    // land back where it started.
    if (flushing.current) {
      cancelAnimationFrame(flushing.current);
      flushing.current = 0;
    }
    let target = pending.current;
    pending.current = null;
    if (snap) {
      const at = target ?? positionRef.current;
      const snapped = snap(at);
      if (Number.isFinite(snapped) && Math.abs(snapped - at) > 0.001) {
        target = Math.max(snapped, -5);
        anchor.current = pinned(anchor.current, target, performance.now());
        setPosition(target);
        emit(target);
      }
    }
    void (async () => {
      try {
        const backend = await getBackend();
        if (target !== null) await backend.deckScrubTo(DECK, Math.round(target * 1000));
        await backend.deckScrubEnd(DECK);
      } catch (failure) {
        setError(reasonFrom(failure));
      }
    })();
  }, [DECK, emit, setPosition]);

  // A drag that is still pending when the player goes away must not fire.
  useEffect(
    () => () => {
      if (flushing.current) cancelAnimationFrame(flushing.current);
    },
    [],
  );

  const seekFraction = useCallback(
    (fraction: number) => {
      if (duration <= 0) return;
      seek(Math.min(Math.max(fraction, 0), 1) * duration);
    },
    [duration, seek],
  );

  const positionNow = useCallback(
    () => (anchor.current.playing ? extrapolate(anchor.current, performance.now()) : positionRef.current),
    [],
  );

  // The loop is the engine's: shown from the tick, so what is drawn is what
  // sounds. A failure is reported as a deck error, like a seek's.
  const loopCall = useCallback(
    (call: (backend: Backend) => Promise<void>) => {
      if (idle || !isLoaded()) return;
      void (async () => {
        try {
          await call(await getBackend());
        } catch (failure) {
          setError(failure instanceof Error ? failure.message : FALLBACK);
        }
      })();
    },
    [idle, isLoaded],
  );
  const setLoop = useCallback(
    (inSeconds: number, outSeconds: number) => {
      if (!Number.isFinite(inSeconds) || !Number.isFinite(outSeconds) || outSeconds <= inSeconds) return;
      const next = { inSeconds: Math.max(0, inSeconds), outSeconds, active: true };
      desiredLoop.current = next;
      setLoopState(next);
      loopCall((b) => b.deckSetLoop(DECK, next.inSeconds * 1000, next.outSeconds * 1000));
    },
    [loopCall, DECK],
  );
  const setLoopActive = useCallback((on: boolean) => {
    if (desiredLoop.current !== null) {
      desiredLoop.current = { ...desiredLoop.current, active: on };
      setLoopState(desiredLoop.current);
    }
    loopCall((b) => b.deckLoopActive(DECK, on));
  }, [loopCall, DECK]);
  const clearLoop = useCallback(() => {
    desiredLoop.current = null;
    setLoopState(null);
    loopCall((b) => b.deckClearLoop(DECK));
  }, [loopCall, DECK]);

  return useMemo(() => ({
    playing, position, duration, idle, error, toggle, playAfter, seek, seekFraction,
    scrubBegin, scrubTo, scrubEnd, positionRef, positionNow, subscribe,
    tempo, masterTempo, keyShift, shiftsKey, setKeyShift, setTempo, nudgeTempo, setMasterTempo,
    loop, setLoop, setLoopActive, clearLoop,
  }), [playing, position, duration, idle, error, toggle, playAfter, seek, seekFraction,
    scrubBegin, scrubTo, scrubEnd, positionNow, subscribe, tempo, masterTempo, keyShift, shiftsKey,
    setKeyShift, setTempo, nudgeTempo, setMasterTempo, loop, setLoop, setLoopActive, clearLoop]);
}
