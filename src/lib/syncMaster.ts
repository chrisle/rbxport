/**
 * Which deck is the sync master in the two-deck layouts, and which decks
 * follow it, as rekordbox's EXPORT mode decides them.
 *
 * - No deck is the master until one plays: "The Player which starts the
 *   playback first becomes the Sync Master" [rekordbox 7.2.18 manual p.121,
 *   EXPORT mode]. Both decks loaded and stopped show MASTER unlit on both
 *   [OBS rekordbox 7 Export 2 PLAYER, chris-win11,
 *   parity/issue-128/rekordbox-loaded-both.png], and A's MASTER lights once A
 *   plays [rekordbox-b1.png]. rekordbox ranks a deck as a master candidate
 *   only while it is playing with a beat grid
 *   [static, rekordbox 7.2.19 arm64:
 *   rbxfrm::DjSyncMasterBehavior::getSubPriorityAsSyncMaster @0x102b6c0a4].
 * - BEAT SYNC is the deck's own on/off state, lit or not whichever deck is
 *   the master: the button's lit flag comes from the player's sync state and
 *   the master flag never disables it [static:
 *   djplay::SyncButtonSpecification::getButtonState @0x100f2fb70, called from
 *   djplay::TrackInfoView::updateSyncButtonDrawing @0x101a5324c, which never
 *   calls setEnabled]. The master's BEAT SYNC is drawn at full strength like
 *   the other deck's [OBS rekordbox-b1.png].
 * - A deck only follows while some other deck is the master.
 *
 * [UNKNOWN] whether rekordbox moves MASTER when the master pauses, and what a
 * BEAT SYNC pressed before either deck plays does beyond lighting; here the
 * master stays put and the lit deck follows once the other deck takes MASTER.
 */

import type { DeckId } from "@/ipc/types";

export interface SyncState {
  /** The sync master, or null before either deck has played. */
  master: DeckId | null;
  /** BEAT SYNC lit, per deck. */
  on: Record<DeckId, boolean>;
}

export const INITIAL_SYNC: SyncState = { master: null, on: { a: false, b: false } };

/** A deck started playing: the first to do so takes MASTER. */
export function deckStartedPlaying(state: SyncState, deck: DeckId): SyncState {
  return state.master === null ? { ...state, master: deck } : state;
}

/** MASTER pressed on a deck. Neither deck's BEAT SYNC changes. */
export function pressMaster(state: SyncState, deck: DeckId): SyncState {
  return state.master === deck ? state : { ...state, master: deck };
}

/** BEAT SYNC pressed on a deck, the master included. */
export function toggleBeatSync(state: SyncState, deck: DeckId): SyncState {
  return { ...state, on: { ...state.on, [deck]: !state.on[deck] } };
}

/** Whether a deck follows the master's tempo and beat now. */
export function isFollowing(state: SyncState, deck: DeckId): boolean {
  return state.on[deck] && state.master !== null && state.master !== deck;
}
