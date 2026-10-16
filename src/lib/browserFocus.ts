/**
 * Which part of the browser has the keyboard focus: the main or the
 * sub-browser's track list or tree, or none of them.
 *
 * rekordbox's Track and Playlist menu items are live only while a track list
 * or the tree has the keyboard focus [OBS static: `BrowseBasicView::
 * isActiveCommand` @0x1003153cc hands Track commands to the focused
 * `ListViewer`, and `ListViewer::isActiveCommand` @0x1003dc540 answers false
 * unless its list box is the focused component; `TreeViewer::isActiveCommand`
 * @0x10155c434 wants the tree or its list focused. OBS: on a fresh
 * rekordbox every item is greyed until a list or the tree is clicked, issue
 * #513]. The webview has no such focus for the list and the tree — their
 * keys are window-wide (see `commands.ts`) — so the last press in one of
 * them stands in for it, the way the list's Delete key already decides
 * whether it is spoken to.
 *
 * A press outside the browser (a deck, a button, a menu or a dialog) leaves
 * it where it was [ASSUME: JUCE buttons and decks do not take the keyboard
 * focus]; a press in a text field takes it away, as rekordbox's search box
 * does.
 */

export type BrowserSide = "main" | "sub";

export type BrowserFocus = { pane: "list" | "tree"; side: BrowserSide } | null;

let current: BrowserFocus = null;
const listeners = new Set<() => void>();

export function browserFocus(): BrowserFocus {
  return current;
}

export function setBrowserFocus(next: BrowserFocus): void {
  if (current?.pane === next?.pane && current?.side === next?.side) return;
  current = next;
  for (const listener of [...listeners]) listener();
}

/** Calls `listener` whenever the focus moves; returns the unsubscribe. */
export function onBrowserFocus(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * What a press on `target` does to the focus: the pane it landed in, null
 * for a text field, or undefined when it landed outside the browser.
 */
export function paneAt(target: Element | null): BrowserFocus | undefined {
  if (target === null) return undefined;
  if (target.closest('input, textarea, select, [contenteditable="true"]')) return null;
  const pane = target.closest("[data-browser-pane]");
  if (pane === null) return undefined;
  const kind = pane.getAttribute("data-browser-pane");
  const side = pane.getAttribute("data-browser-side") === "sub" ? "sub" : "main";
  if (kind === "list" || kind === "tree") return { pane: kind, side };
  return undefined;
}

/** Follows presses in `win`; returns the function that stops. */
export function followBrowserFocus(win: Window): () => void {
  const onDown = (event: MouseEvent) => {
    const target = event.target instanceof Element ? event.target : null;
    const next = paneAt(target);
    if (next !== undefined) setBrowserFocus(next);
  };
  win.addEventListener("mousedown", onDown, true);
  return () => {
    win.removeEventListener("mousedown", onDown, true);
  };
}
