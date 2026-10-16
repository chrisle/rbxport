/**
 * The commands the keyboard, the menus and the on-screen controls share.
 *
 * A command is what a key in the Keyboard pane is bound to (an `Action` in
 * `shortcuts.ts`): Play/Pause, Analyze Track, Locate track loaded on deck.
 * What it does belongs to the part of the window that holds the state it acts
 * on — a deck, a track list, the shell — so that part registers it here
 * while it is on screen, and every way of asking for it calls `runCommand`.
 * A keyboard handler then only decides whether a key press is a command at
 * all (the key map, where the focus is) and leaves what happens to the
 * command, so a menu item, a button or a script asking for the same command
 * gets exactly what the key gets (#431).
 *
 * rekordbox is built the same way [OBS static, rekordbox 7.2.19 macOS]: its
 * keys are `juce::ApplicationCommandManager` command ids (the Keyboard
 * pane's table, `getAllCommands`/`getCommandInfo`), and its menu bar's items
 * are `PopupMenu::addCommandItem` calls on those same ids
 * (`MainComponent::getMenuForIndex` @0x100964638), so a key and a menu item
 * for one command reach the same `perform`.
 */
import type { Action } from "./shortcuts";

/**
 * Which one of several of the same thing a command is for: a deck's
 * commands are Player A's or Player B's, a track list's the main browser's
 * or the sub-browser's. A command the whole window answers has none.
 */
export type CommandScope = "a" | "b" | "main" | "sub";

/** How a command was asked for. */
export interface CommandInput {
  /**
   * The press is a held key's auto-repeat. A command that is one per press
   * (Set Hot Cue, Memory Cue, SYNC) ignores it; a menu item or a button
   * never repeats.
   */
  repeat: boolean;
}

/**
 * Runs a command. Returns false when it does not apply right now — the deck
 * is empty, nothing is selected — so a key that asked for it is left to
 * the browser and the OS, exactly as an unbound key is.
 */
export type CommandHandler = (input: CommandInput) => boolean;

export type CommandHandlers = Partial<Record<Action, CommandHandler>>;

/**
 * The registered handlers, newest last. A second registration for the same
 * command and scope (a remount before the old one has gone) takes over, and
 * releasing it hands the command back to the one before.
 */
const registry = new Map<string, CommandHandler[]>();

function slot(action: Action, scope: CommandScope | undefined): string {
  return scope === undefined ? action : `${action}@${scope}`;
}

/** Registers what a command does. Returns the function that releases it. */
export function registerCommand(action: Action, handler: CommandHandler, scope?: CommandScope): () => void {
  const key = slot(action, scope);
  const list = registry.get(key) ?? [];
  list.push(handler);
  registry.set(key, list);
  let live = true;
  return () => {
    if (!live) return;
    live = false;
    const current = registry.get(key);
    if (!current) return;
    const index = current.lastIndexOf(handler);
    if (index >= 0) current.splice(index, 1);
    if (current.length === 0) registry.delete(key);
  };
}

/** Registers several commands for one scope; the result releases them all. */
export function registerCommands(handlers: CommandHandlers, scope?: CommandScope): () => void {
  const releases: (() => void)[] = [];
  for (const [action, handler] of Object.entries(handlers) as [Action, CommandHandler | undefined][]) {
    if (handler) releases.push(registerCommand(action, handler, scope));
  }
  return () => {
    for (const release of releases) release();
  };
}

/** Whether anything on screen answers a command. */
export function hasCommand(action: Action, scope?: CommandScope): boolean {
  return (registry.get(slot(action, scope))?.length ?? 0) > 0;
}

/**
 * Runs a command: the one entry point a key, a menu item and a button all
 * call. Returns false when nothing on screen answers it or it does not
 * apply right now.
 */
export function runCommand(action: Action, options: { scope?: CommandScope; repeat?: boolean } = {}): boolean {
  const list = registry.get(slot(action, options.scope));
  const handler = list?.[list.length - 1];
  if (!handler) return false;
  return handler({ repeat: options.repeat === true });
}
