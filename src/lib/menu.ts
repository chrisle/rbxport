/**
 * What a native menu item does.
 *
 * The shell sends one event carrying the item's id and nothing else, so the
 * decision — which action, and whether it is allowed right now — is made here
 * rather than in a listener. Import writes to the shared library, so it is
 * refused while rekordbox holds it, exactly as the other write paths are.
 */


export type MenuAction =
  | "settings"
  | "import"
  | "import-folder"
  | "import-xml"
  | "import-itunes"
  | "export-xml"
  | "missing"
  | "info"
  | "sub"
  | "layout-one"
  | "layout-two"
  | "layout-simple"
  | "layout-browser"
  | "report-bug"
  | "tempo-slider"
  | "updates";

export interface MenuCommand {
  action: MenuAction;
  /** Writes to the library, so rekordbox running is a refusal, not a race. */
  writes: boolean;
}

const COMMANDS: Record<string, MenuCommand> = {
  settings: { action: "settings", writes: false },
  import: { action: "import", writes: true },
  "import-folder": { action: "import-folder", writes: true },
  "import-xml": { action: "import-xml", writes: true },
  "import-itunes": { action: "import-itunes", writes: true },
  "export-xml": { action: "export-xml", writes: false },
  missing: { action: "missing", writes: true },
  info: { action: "info", writes: false },
  sub: { action: "sub", writes: false },
  // ⌘7/8/9/0, which is where rekordbox's Export key map puts them.
  "layout-one": { action: "layout-one", writes: false },
  "layout-two": { action: "layout-two", writes: false },
  "layout-simple": { action: "layout-simple", writes: false },
  "layout-browser": { action: "layout-browser", writes: false },
  "report-bug": { action: "report-bug", writes: false },
  "tempo-slider": { action: "tempo-slider", writes: false },
  updates: { action: "updates", writes: false },
};

/** The command an item id names, or null when it is not one of ours. */
export function menuCommand(id: string): MenuCommand | null {
  return COMMANDS[id] ?? null;
}

/**
 * What to do about a menu click: run the action, or say why not.
 *
 * Returning the refusal rather than silently ignoring the click matters —
 * a menu item that does nothing reads as a broken app.
 */
export function resolveMenu(
  id: string,
  readOnly: boolean,
  /** Library Protection is on in Preferences: the refusal says so instead. */
  protectedLibrary = false,
): { action: MenuAction } | { refused: string } | null {
  const command = menuCommand(id);
  if (!command) return null;
  if (command.writes && readOnly) {
    return { refused: refusal(protectedLibrary) };
  }
  return { action: command.action };
}

/** Why a write was refused, in the words the status bar shows. */
export function refusal(protectedLibrary: boolean): string {
  return protectedLibrary
    ? "Editing is locked by Library Protection. Turn it off in Preferences to edit."
    : "Editing is locked while rekordbox is running. Quit rekordbox to enable editing.";
}
