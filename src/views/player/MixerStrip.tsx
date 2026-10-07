/**
 * The mixer strip: a gain and three kill buttons per deck, and the crossfader
 * between them.
 *
 * Every size and colour here was measured off the two-player capture by
 * scanning its pixels — the strip is 48pt wide, the kill buttons 36×15 on a
 * 20pt pitch, the fader 53pt of travel with a 28×3 handle. See the `mixer*`
 * tokens.
 *
 * It exists only where there are two decks, which is what rekordbox does: the
 * one-player layout has no mixer and no crossfader, and a deck nobody has
 * touched a fader for plays at the level of its file.
 *
 * Nothing here holds audio state of its own. The engine owns the strip — see
 * `crates/rbl-deck/src/mixer.rs` — and these are the knobs that reach it.
 */
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { DeckId, EqBand } from "@/ipc/types";
import { getBackend } from "@/ipc/client";
import { detectPlatform, dispatchBinding, eqKillBand } from "@/lib/shortcuts";
import { usePreferencesContext } from "@/store/usePreferences";
import styles from "./MixerStrip.module.css";

/** High to low, as the strip is drawn and as the mixer names them. */
const BANDS: ReadonlyArray<{ id: EqBand; label: string }> = [
  { id: "high", label: "HIGH" },
  { id: "mid", label: "MID" },
  { id: "low", label: "LOW" },
];

/** The knob's travel in pixels: a full sweep is this far under the pointer. */
const KNOB_TRAVEL = 120;

/**
 * How close to the middle the crossfader snaps.
 *
 * The centre is a position people aim for and cannot hit by hand on a 53pt
 * fader, so it is a detent — three pixels of it, which is under a millimetre.
 */
const DETENT = 3;

/** Trim as a dB reading, which is what the strip prints above the knob. */
function trimLabel(trim: number): string {
  if (trim <= 0) return "-∞dB";
  const db = 20 * Math.log10(trim);
  return `${db > 0 ? "+" : ""}${db.toFixed(1)}dB`;
}

/** One deck's half of the strip: the gain block and the three kill buttons. */
const Channel = memo(function Channel({
  deck, flipped,
}: {
  deck: DeckId;
  /** Deck B's half is the mirror of deck A's, as the capture has it. */
  flipped?: boolean;
}) {
  const [trim, setTrim] = useState(1);
  const [killed, setKilled] = useState<ReadonlySet<EqBand>>(() => new Set());
  const drag = useRef<{ y: number; from: number } | null>(null);

  const send = useCallback(
    (next: number) => {
      setTrim(next);
      void (async () => {
        const backend = await getBackend();
        await backend.setChannelTrim(deck, next);
      })().catch(() => {
        // A mixer that cannot reach the engine is a mixer with nothing behind
        // it; the knob still moves, and the deck plays as it did.
      });
    },
    [deck],
  );

  const toggle = useCallback(
    (band: EqBand) => {
      setKilled((current) => {
        const next = new Set(current);
        if (!next.delete(band)) next.add(band);
        void (async () => {
          const backend = await getBackend();
          await backend.setChannelKill(deck, band, next.has(band));
        })().catch(() => {});
        return next;
      });
    },
    [deck],
  );

  // The Keyboard pane's kill keys for this deck (unbound until assigned).
  const platform = useMemo(detectPlatform, []);
  const overrides = usePreferencesContext().preferences.keyboard.overrides;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const hit = dispatchBinding(event, platform, event.target as HTMLElement | null, overrides);
      if (hit?.action === undefined || hit.deck !== deck) return;
      const band = eqKillBand(hit.action);
      if (band === null) return;
      event.preventDefault();
      if (!event.repeat) toggle(band);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [deck, overrides, platform, toggle]);

  const onKnobDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { y: event.clientY, from: trim };
    },
    [trim],
  );

  const onKnobMove = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const from = drag.current;
      if (!from) return;
      // Up is louder, which is the direction the knob turns.
      const moved = (from.y - event.clientY) / KNOB_TRAVEL;
      send(Math.min(Math.max(from.from + moved * 2, 0), 2));
    },
    [send],
  );

  const onKnobUp = useCallback(() => {
    drag.current = null;
  }, []);

  const gain = (
    <div className={styles.gain}>
      <span className={styles.readout}>{trimLabel(trim)}</span>
      <div
        className={styles.knob}
        role="slider"
        tabIndex={0}
        aria-label={`Deck ${deck.toUpperCase()} gain`}
        aria-valuemin={0}
        aria-valuemax={2}
        aria-valuenow={Number(trim.toFixed(2))}
        aria-valuetext={trimLabel(trim)}
        onPointerDown={onKnobDown}
        onPointerMove={onKnobMove}
        onPointerUp={onKnobUp}
        onPointerCancel={onKnobUp}
        onDoubleClick={() => send(1)}
        onKeyDown={(event) => {
          if (event.key === "ArrowUp") send(Math.min(trim + 0.05, 2));
          if (event.key === "ArrowDown") send(Math.max(trim - 0.05, 0));
        }}
      >
        {/* The tick, at the angle the gain is at: 0 points down-left, unity
            straight up, and the top of the travel down-right. */}
        <span
          className={styles.tick}
          style={{ rotate: `${(trim / 2) * 300 - 150}deg` }}
          aria-hidden
        />
      </div>
      {/* Auto Gain, which is what rekordbox's A-GAIN means. The knob is the
          trim until there is an analysed gain to switch it to. */}
      <span className={styles.caption}>A-GAIN</span>
    </div>
  );

  const bands = (
    <div className={styles.bands}>
      {BANDS.map((band) => (
        <button
          key={band.id}
          type="button"
          className={styles.band}
          aria-pressed={killed.has(band.id)}
          data-killed={killed.has(band.id) || undefined}
          onClick={() => toggle(band.id)}
        >
          {band.label}
        </button>
      ))}
    </div>
  );

  return (
    <div className={styles.channel} data-flipped={flipped || undefined}>
      {flipped ? bands : gain}
      {flipped ? gain : bands}
    </div>
  );
});

export interface MixerStripProps {
  /** Told, so the app can keep the fader across a layout change. */
  onCrossfade?: (position: number) => void;
  /** Where the fader starts: 0 is deck A alone, 1 is deck B alone. */
  crossfade?: number;
}

export function MixerStrip({ crossfade = 0.5, onCrossfade }: MixerStripProps) {
  const [at, setAt] = useState(crossfade);
  const track = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void (async () => {
      const backend = await getBackend();
      await backend.setCrossfade(at);
    })().catch(() => {});
    onCrossfade?.(at);
  }, [at, onCrossfade]);

  const move = useCallback((clientY: number) => {
    const box = track.current?.getBoundingClientRect();
    if (!box || box.height === 0) return;
    const raw = (clientY - box.top) / box.height;
    const clamped = Math.min(Math.max(raw, 0), 1);
    // The detent, in the fader's own units.
    const snap = DETENT / box.height;
    setAt(Math.abs(clamped - 0.5) < snap ? 0.5 : clamped);
  }, []);

  return (
    <div className={styles.strip} role="group" aria-label="Mixer">
      <Channel deck="a" />
      <div className={styles.fader}>
        <span className={styles.deckLabel}>A</span>
        <div
          ref={track}
          className={styles.faderTrack}
          role="slider"
          tabIndex={0}
          aria-label="Crossfader"
          aria-orientation="vertical"
          aria-valuemin={0}
          aria-valuemax={1}
          aria-valuenow={Number(at.toFixed(2))}
          aria-valuetext={at === 0.5 ? "Both decks" : at < 0.5 ? "Towards A" : "Towards B"}
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
            move(event.clientY);
          }}
          onPointerMove={(event) => {
            if (event.buttons === 0) return;
            move(event.clientY);
          }}
          onDoubleClick={() => setAt(0.5)}
          onKeyDown={(event) => {
            if (event.key === "ArrowUp") setAt((v) => Math.max(v - 0.05, 0));
            if (event.key === "ArrowDown") setAt((v) => Math.min(v + 0.05, 1));
          }}
        >
          <span className={styles.faderHandle} style={{ top: `${at * 100}%` }} aria-hidden />
        </div>
        <span className={styles.deckLabel}>B</span>
      </div>
      <Channel deck="b" flipped />
    </div>
  );
}
