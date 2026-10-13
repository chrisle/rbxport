/**
 * Each deck's Q (quantize) button, remembered between sessions.
 *
 * rekordbox 7.2.19 keeps one flag per deck in its settings file,
 * `Player%d_Quantize`: every press writes it (`PlayerComponent::quantizePressed`,
 * `SettingFile::setBoolValue` @0x102290db0), and the deck reads it back when
 * its player view is set up (`PlayerWaveView::updateQuantizeSettings`
 * @0x100c8f1e8 `getBoolValue(.., true)` -> `DjPlayerUnit::setQuantizeUnit`
 * -> `enableQuantize`) [OBS static, parity/issue-277]. So a deck is on the
 * first time, and after that stays as it was left: a DJ who turns Q off to call
 * hot cues at once does not find it back on at the next start.
 */
import type { DeckId } from "@/ipc/types";

const STORAGE_KEY = "rbl.deckQuantize.v1";

/** rekordbox's default when the deck has no saved flag. */
export const DEFAULT_DECK_QUANTIZE = true;

function readAll(): Partial<Record<DeckId, unknown>> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed: unknown = raw === null ? null : JSON.parse(raw);
    return parsed !== null && typeof parsed === "object" ? parsed : {};
  } catch {
    // Private browsing, a disabled store, or a half-written value.
    return {};
  }
}

/** The deck's saved Q, or on when nothing (or nothing usable) is saved. */
export function loadDeckQuantize(deck: DeckId): boolean {
  const saved = readAll()[deck];
  return typeof saved === "boolean" ? saved : DEFAULT_DECK_QUANTIZE;
}

/** Saves the deck's Q, leaving the other deck's alone. */
export function saveDeckQuantize(deck: DeckId, on: boolean): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ ...readAll(), [deck]: on }));
  } catch {
    // Nowhere to keep it: the button still works for this session.
  }
}
