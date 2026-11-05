/**
 * AppleScript's side of the window.
 *
 * A script's reads never come here: the backend answers them from what it
 * holds. What comes here is what the window holds — which track a deck
 * shows, PLAY, LINK, the preferences, an export and its progress — so a
 * script does those things the way a click does. `src-tauri/src/scripting/`
 * sends each one as `script:request` and waits for the reply.
 */
import type { DeckId, ScriptReply, ScriptRequest } from "@/ipc/types";
import { sanitisePreferences, type Preferences } from "@/lib/preferences";

export type ScriptHandler = (args: Record<string, unknown>) => unknown;

/** Runs a request with the handler for its action and says how it went. */
export async function answer(
  handlers: Readonly<Record<string, ScriptHandler>>,
  request: ScriptRequest,
): Promise<ScriptReply> {
  const handler = Object.hasOwn(handlers, request.action) ? handlers[request.action] : undefined;
  if (!handler) return { error: `The window cannot ${request.action}.` };
  try {
    return { value: (await handler(request.args ?? {})) ?? null };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

/**
 * What a deck's player lets a script do, read at the moment it asks: the
 * player registers these while it is on screen, and a layout without it
 * has none.
 */
export interface DeckControls {
  /** The id of the track it holds, or null. */
  track(): string | null;
  /** Nothing to play yet: no track, or one still opening. */
  idle(): boolean;
  /** Play intent, including a start queued while the next track opens. */
  playing(): boolean;
  /** Why the track could not be opened, when it could not. */
  error(): string | null;
  /** The PLAY button, with the sync and quantize it applies. */
  togglePlay(): void;
}

const decks = new Map<DeckId, DeckControls>();

export function registerDeck(deck: DeckId, controls: DeckControls): () => void {
  decks.set(deck, controls);
  return () => {
    if (decks.get(deck) === controls) decks.delete(deck);
  };
}

export function deckControls(deck: DeckId): DeckControls | undefined {
  return decks.get(deck);
}

/** Deck 1 or 2, as the dictionary numbers the window's decks. */
export function deckNumber(deck: DeckId): number {
  return deck === "b" ? 2 : 1;
}

/**
 * Waits for a deck to take a track the window was just given: until it
 * holds `trackId` and can play it, or says why it cannot.
 */
export async function whenLoaded(deck: DeckId, trackId: string, timeoutMs = 15_000): Promise<void> {
  const started = Date.now();
  for (;;) {
    const controls = deckControls(deck);
    if (controls && controls.track() === trackId) {
      const problem = controls.error();
      if (problem) throw new Error(problem);
      if (!controls.idle()) return;
    }
    if (Date.now() - started > timeoutMs) {
      throw new Error(`Deck ${deckNumber(deck)} did not finish loading the track.`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

/**
 * PLAY or pause, when the deck is not already doing it. Refused on a deck
 * with nothing loaded, which has no PLAY to press.
 */
export function setPlaying(deck: DeckId, wanted: boolean): void {
  const controls = deckControls(deck);
  if (!controls || controls.idle()) throw new Error(`Deck ${deckNumber(deck)} has no track loaded.`);
  if (controls.playing() !== wanted) controls.togglePlay();
}

/**
 * The preferences with one choice changed, by `pane.field` as the dictionary
 * names settings. Throws, and changes nothing, for a name that is not a
 * setting or a value the choice would not keep: the stored set is checked
 * the same way at start, and a value it would replace was not accepted.
 */
export function withSetting(current: Preferences, path: string, value: unknown): Preferences {
  const [pane, field, ...rest] = path.split(".");
  const panes = current as unknown as Record<string, Record<string, unknown> | undefined>;
  const section = pane && Object.hasOwn(panes, pane) ? panes[pane] : undefined;
  if (!pane || !field || rest.length > 0 || pane === "keyboard" || !section || !Object.hasOwn(section, field)) {
    throw new Error(`There is no setting called ${path}.`);
  }
  const next = sanitisePreferences({ ...panes, [pane]: { ...section, [field]: value } });
  const kept = (next as unknown as Record<string, Record<string, unknown> | undefined>)[pane]?.[field];
  if (JSON.stringify(kept) !== JSON.stringify(value)) {
    throw new Error(`${path} cannot be set to ${JSON.stringify(value)}.`);
  }
  return next;
}
