/**
 * The Pro DJ LINK strip along the bottom, drawn as rekordbox draws it in
 * EXPORT (docs/screenshots 2026-09-13 11.18.36 off, 11.16.28 on).
 *
 * It appears as soon as a player or mixer is heard on the network and keeps
 * its full height from then on: off, it is the LINK button alone at the left
 * of an otherwise empty strip; on, the decks and the mixer sit beside it,
 * centred, as rekordbox arranges them — players either side of the mixer.
 *
 * Each deck names its player, says whether it is the tempo master, and shows
 * what it has loaded from us. While a track is dragged from the library a deck
 * is a drop target: dropping tells that CDJ to load it.
 *
 * Only what the link reports is drawn. The status packets give the loaded
 * track, the play state and the master; they do not give the playhead, so the
 * jog does not turn and there is no waveform here — rekordbox draws none in
 * this strip either.
 */
import { type DragEvent, useEffect, useRef, useState } from "react";

import { Artwork } from "@/components/Artwork";
import { LinkIcon, LinkOnIcon } from "@/components/icons";
import { useTooltip } from "@/store/usePreferences";
import type { LinkPeerSeen, LinkPlayer, LinkStatus } from "@/ipc/types";
import styles from "./LinkDeckStrip.module.css";

export interface LinkDeckStripProps {
  /** Devices heard on the network, whether or not LINK is on. */
  peers: LinkPeerSeen[];
  /** The LINK session, or null before the first status has arrived. */
  link: LinkStatus | null;
  /** Turn LINK on or off. */
  onToggle: () => void;
  /** True while a start/stop is in flight. */
  busy?: boolean;
  /** True while a track is being dragged from the library. */
  dragging?: boolean;
  /** Called when a track is dropped onto a player deck. */
  onDropToPlayer?: ((playerNumber: number) => void) | undefined;
  /** Become the network's tempo master, or resign. */
  onSetMaster?: ((on: boolean) => void) | undefined;
  /** Nudge the master tempo by `deltaBpm` (rekordbox's −/+ is ±1). */
  onNudgeMaster?: ((deltaBpm: number) => void) | undefined;
  /** Take the current master player's tempo as the master tempo (⟳). */
  onTakeMasterTempo?: (() => void) | undefined;
}

export function LinkDeckStrip({
  peers,
  link,
  onToggle,
  busy = false,
  dragging = false,
  onDropToPlayer,
  onSetMaster,
  onNudgeMaster,
  onTakeMasterTempo,
}: LinkDeckStripProps) {
  const tip = useTooltip();
  const [explaining, setExplaining] = useState(false);
  const box = useRef<HTMLDivElement>(null);

  const on = link?.on ?? false;
  // A source of our own is not something to link to; only real players and
  // mixers count towards showing the strip.
  const others = peers.filter((p) => p.kind === "player" || p.kind === "mixer");
  // Off with a reason it cannot come on at all: rekordbox holds the ports.
  const blocked = !on && !!link?.problem;

  // The popover only lives while blocked; anything outside it, or Escape,
  // closes it, the way the context menu does.
  useEffect(() => {
    if (!explaining) return undefined;
    const outside = (event: MouseEvent) => {
      if (!box.current?.contains(event.target as Node)) setExplaining(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") setExplaining(false);
    };
    window.addEventListener("mousedown", outside, true);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", outside, true);
      window.removeEventListener("keydown", key);
    };
  }, [explaining]);

  // Nothing to link to, or LINK cannot be turned on (rekordbox holds the
  // ports): no strip at all. A LINK button that cannot do anything is
  // noise, and Preferences › PRO DJ LINK spells the reason out.
  if (!on && (others.length === 0 || blocked)) {
    return null;
  }

  const linked = link?.players ?? [];
  const players = linked.filter((p) => p.kind === "player");
  const mixers = linked.filter((p) => p.kind !== "player");
  // rekordbox seats the mixer between the decks rather than after them.
  const half = Math.ceil(players.length / 2);
  const droppable = dragging && !!onDropToPlayer;

  return (
    <div
      className={styles.strip}
      data-on={on || undefined}
      data-blocked={blocked || undefined}
      data-testid="link-deck-strip"
      ref={box}
    >
      <button
        type="button"
        className={styles.linkButton}
        aria-pressed={blocked ? undefined : on}
        aria-expanded={blocked ? explaining : undefined}
        disabled={busy}
        onClick={blocked ? () => setExplaining((v) => !v) : onToggle}
        title={tip(
          blocked
            ? "PRO DJ LINK is unavailable. Click to see why."
            : on
              ? "PRO DJ LINK is on. Players on the network can browse and play this library. Click to turn it off."
              : others.length === 1
                ? "1 device on the network. Turn PRO DJ LINK on to serve this library to it."
                : `${others.length} devices on the network. Turn PRO DJ LINK on to serve this library to them.`,
        )}
        data-testid="link-button"
      >
        {on ? <LinkOnIcon className={styles.glyph} /> : <LinkIcon className={styles.glyph} />}
        <span className={styles.linkLabel}>LINK</span>
      </button>

      {blocked ? (
        <button
          type="button"
          className={styles.warning}
          aria-expanded={explaining}
          onClick={() => setExplaining((v) => !v)}
        >
          unavailable
        </button>
      ) : null}

      {on && link ? (
        <MasterClock
          master={link.master}
          bpm={link.masterBpm}
          canTakeTempo={linked.some((p) => p.master)}
          onSetMaster={onSetMaster}
          onNudge={onNudgeMaster}
          onTakeTempo={onTakeMasterTempo}
        />
      ) : null}

      {on ? (
        <div className={styles.decks} aria-label="Players on the link">
          {players.slice(0, half).map((p) => (
            <PlayerDeck key={p.number} player={p} droppable={droppable} onDrop={onDropToPlayer} />
          ))}
          {mixers.map((m) => (
            <MixerCell key={m.number} device={m} />
          ))}
          {players.slice(half).map((p) => (
            <PlayerDeck key={p.number} player={p} droppable={droppable} onDrop={onDropToPlayer} />
          ))}
        </div>
      ) : null}

      {blocked && explaining ? (
        <div className={styles.explain} role="dialog" aria-label="Why PRO DJ LINK is unavailable">
          {link?.problem}
        </div>
      ) : null}
    </div>
  );
}

// WebKit can refuse the drop when only dragover was accepted. Take the
// entering drag too, as playlist targets do, without loading until drop.
function takePlayerDrag(event: DragEvent<HTMLDivElement>) {
  event.preventDefault();
  event.dataTransfer.dropEffect = "copy";
}

function PlayerDeck({
  player,
  droppable,
  onDrop,
}: {
  player: LinkPlayer;
  droppable: boolean;
  onDrop?: ((playerNumber: number) => void) | undefined;
}) {
  const loaded = player.loaded !== null;
  return (
    <div
      className={styles.deck}
      data-master={player.master || undefined}
      data-loaded={loaded || undefined}
      data-playing={player.playing || undefined}
      data-drop-target={droppable ? true : undefined}
      aria-label={`Player ${player.number}`}
      onDragEnter={droppable ? takePlayerDrag : undefined}
      onDragOver={droppable ? takePlayerDrag : undefined}
      onDrop={
        droppable && onDrop
          ? (e) => {
              e.preventDefault();
              onDrop(player.number);
            }
          : undefined
      }
    >
      <div className={styles.deckHead}>
        <span className={styles.deckNo}>{player.number}</span>
        {/* PLAY while playing; CUE while sitting at the cue point; blank
            otherwise. The slot stays so MASTER and SYNC do not move. */}
        <span className={styles.lamp}>
          {player.playing ? "PLAY" : player.cued ? "CUE" : ""}
        </span>
        <span className={styles.master} data-on={player.master || undefined}>
          MASTER
        </span>
        <span className={styles.sync} data-on={player.sync || undefined}>SYNC</span>
      </div>
      <div className={styles.deckBody}>
        {player.loaded ? (
          <>
            <span className={styles.sleeveBox}>
              <Artwork trackId={player.loaded.id} className={styles.sleeve} />
            </span>
            <span className={styles.trackTitle} title={player.loaded.title}>{player.loaded.title}</span>
          </>
        ) : (
          <>
            <span className={styles.jog} aria-hidden />
            <span className={styles.empty}>{droppable ? "Drop to load" : ""}</span>
          </>
        )}
      </div>
    </div>
  );
}

/**
 * The tempo-master clock, as rekordbox draws it at the strip's left: the
 * number over LINK, the master BPM in a box with −/+, the MASTER toggle and
 * the ⟳ button that takes the current master player's tempo. Orange when we
 * are master, grey when we are not — the box still shows the tempo we would
 * drive, as rekordbox holds the last value.
 */
function MasterClock({
  master,
  bpm,
  canTakeTempo,
  onSetMaster,
  onNudge,
  onTakeTempo,
}: {
  master: boolean;
  bpm: number;
  canTakeTempo: boolean;
  onSetMaster?: ((on: boolean) => void) | undefined;
  onNudge?: ((deltaBpm: number) => void) | undefined;
  onTakeTempo?: (() => void) | undefined;
}) {
  const tip = useTooltip();
  return (
    <div className={styles.masterClock} data-master={master || undefined}>
      <div className={styles.mcTop}>
        <div className={styles.mcBpm}>
          <span className={styles.mcBpmValue}>{bpm.toFixed(2)}</span>
          <button
            type="button"
            className={styles.mcNudge}
            onClick={() => onNudge?.(-1)}
            title={tip("Nudge the master tempo down 1 BPM.")}
            aria-label="Master tempo down"
          >
            −
          </button>
          <button
            type="button"
            className={styles.mcNudge}
            onClick={() => onNudge?.(1)}
            title={tip("Nudge the master tempo up 1 BPM.")}
            aria-label="Master tempo up"
          >
            +
          </button>
        </div>
      </div>
      <div className={styles.mcBottom}>
        <button
          type="button"
          className={styles.mcMaster}
          aria-pressed={master}
          onClick={() => onSetMaster?.(!master)}
          title={tip(
            master
              ? "This computer is the tempo master. Click to resign."
              : "Make this computer the tempo master; players set to SYNC follow this tempo.",
          )}
        >
          MASTER
        </button>
        <button
          type="button"
          className={styles.mcRecycle}
          disabled={!canTakeTempo}
          onClick={() => onTakeTempo?.()}
          title={tip(
            canTakeTempo
              ? "Take the current master player's tempo as the master tempo."
              : "No player is master, so there is no tempo to take.",
          )}
          aria-label="Take the master player's tempo"
        >
          <RecycleIcon className={styles.mcIcon} />
        </button>
      </div>
    </div>
  );
}

/** The two-curved-arrows glyph rekordbox uses for "take the master's tempo". */
function RecycleIcon({ className }: { className?: string | undefined }) {
  return (
    <svg className={className} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" aria-hidden>
      <path d="M3 6.5a5 5 0 0 1 8.5-2.3M13 9.5a5 5 0 0 1-8.5 2.3" strokeLinecap="round" />
      <path d="M11.5 1.5v2.8h-2.8M4.5 14.5v-2.8h2.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function MixerCell({ device }: { device: LinkPlayer }) {
  return (
    <div className={styles.mixer} aria-label={`Mixer ${device.number}`}>
      <span className={styles.mixerLabel}>{device.kind === "mixer" ? "MIXER" : device.name}</span>
      <div className={styles.mixerLamps}>
        <span className={styles.mixerMaster} data-on={device.master || undefined}>
          MASTER
        </span>
        <span className={styles.mixerLinkCue} data-on={device.linkCue || undefined}>
          LINK CUE
        </span>
      </div>
    </div>
  );
}
