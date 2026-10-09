/**
 * Relocate over one track or several, as rekordbox 7.2.19 does it.
 *
 * [OBS rekordbox 7.2.19 macOS arm64, static] The track menu's Relocate
 * (`ListViewer::popupEventRelocateTrack` @0x1003f43e0) and the Missing File
 * Manager's (`MissingFileComponent::buttonClicked` @0x1012a8d50) both run
 * `MissingFileTable::relocateSelectedFiles(…, SearchMode 0, …)`
 * @0x1012a6818 over the selected tracks, in list order:
 *
 * - A track whose file is there is passed over.
 * - The first missing one gets a file chooser (`showFileChooser`
 *   @0x1012a82c0), titled "Choose a new fullpath for : <file name>", showing
 *   files of its extension only, opened where the last chosen file was.
 *   Cancelling ends the whole run.
 * - A chosen file the collection already holds is refused with "This file is
 *   already in the collection." (title "Missing File Manager", OK), and
 *   nothing is written; otherwise the track is pointed at it.
 * - Every later missing track first asks, Yes or No, under the title
 *   "Missing File Manager": "Would you like rekordbox to find other missing
 *   file using the location of this track ?" and, on the next line, the
 *   title of the track just chosen for. Yes looks for that track and every
 *   one after it where the chosen file's move says it would be
 *   (`relocateSelectedFiles` `$_2`, see the backend's `relocate_by_location`),
 *   says "rekordbox found N files." and ends. No opens the chooser for it.
 *
 * The old and new folders the "location" is taken from lose the trailing
 * folder names they share first (`$_1` @0x1012a77b8): a track moved from
 * `/Users/a/Music/X/1.mp3` to `/Volumes/B/Music/X/1.mp3` makes the rest look
 * under `/Volumes/B` for what was under `/Users/a`.
 *
 * rbxport says "RBXport" where rekordbox names itself, as it does elsewhere.
 */
import type { Backend, MissingTrack } from "@/ipc/types";

/** What the run needs from the user and the library. */
export interface RelocateSteps {
  /** The file chooser for a track, opened in `folder`; null is a cancel. */
  choose(track: MissingTrack, folder: string | null): Promise<string | null>;
  /** Points a track at a file; false when the collection already has it. */
  relocate(track: MissingTrack, path: string): Promise<boolean>;
  /** "This file is already in the collection." */
  alreadyInCollection(): Promise<void>;
  /** Asks to look for the rest using the location of `title`'s file. */
  askFindOthers(title: string): Promise<boolean>;
  /** Looks for these tracks moved from `from` to `to`; how many were found. */
  findOthers(ids: string[], from: string, to: string): Promise<number>;
  /** "… found N files." */
  found(count: number): Promise<void>;
}

/**
 * Where the last chosen file was: rekordbox keeps it for the session
 * (`browse::ListViewer::lastRelocateLocation`), for the menu and the manager
 * alike, and opens the next chooser there.
 */
let lastFolder: string | null = null;

/** Forgets the last chooser folder, for tests. */
export function forgetRelocateFolder(): void {
  lastFolder = null;
}

/** The folder a path is in, as `juce::File::getParentDirectory` gives it. */
export function folderOf(path: string): string {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (at > 0) return path.slice(0, at);
  return at === 0 ? path.slice(0, 1) : "";
}

/** The last part of a path: the file's name. */
export function fileNameOf(path: string): string {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return path.slice(at + 1);
}

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/**
 * The old and new folders without the trailing folder names they share,
 * `\` read as `/` [OBS `$_1` @0x1012a789c..0x1012a798c].
 */
export function movedRoots(oldFolder: string, newFolder: string): [from: string, to: string] {
  let from = oldFolder.replaceAll("\\", "/");
  let to = newFolder.replaceAll("\\", "/");
  for (;;) {
    const a = from.lastIndexOf("/");
    const b = to.lastIndexOf("/");
    if (a < 1 || b < 1) break;
    if (!same(from.slice(a + 1), to.slice(b + 1))) break;
    from = from.slice(0, a);
    to = to.slice(0, b);
  }
  return [from, to];
}

/**
 * Runs Relocate over `tracks`, the selected missing tracks in list order,
 * fetched as the run reaches them. Resolves to how many were pointed at a
 * file.
 */
export async function relocateTracks(tracks: AsyncIterator<MissingTrack>, steps: RelocateSteps): Promise<number> {
  let first = true;
  let from = "";
  let to = "";
  let chosenTitle = "";
  let relocated = 0;
  for (;;) {
    const next = await tracks.next();
    if (next.done === true) break;
    const track = next.value;
    if (!first && lastFolder !== null && (await steps.askFindOthers(chosenTitle))) {
      const ids = [track.id];
      for (let more = await tracks.next(); more.done !== true; more = await tracks.next()) ids.push(more.value.id);
      const found = await steps.findOthers(ids, from, to);
      await steps.found(found);
      return relocated + found;
    }
    const picked = await steps.choose(track, lastFolder);
    if (picked === null) break;
    chosenTitle = track.title;
    if (await steps.relocate(track, picked)) relocated += 1;
    else await steps.alreadyInCollection();
    lastFolder = folderOf(picked);
    [from, to] = movedRoots(folderOf(track.path), lastFolder);
    first = false;
  }
  return relocated;
}

/** Fetches the named tracks that are missing, a page at a time, in order. */
export async function* missingAmong(
  ids: readonly string[],
  fetch: (page: string[]) => Promise<MissingTrack[]>,
  page = 100,
): AsyncGenerator<MissingTrack> {
  for (let at = 0; at < ids.length; at += page) {
    for (const track of await fetch(ids.slice(at, at + page))) yield track;
  }
}

/** `useTranslation()`'s function. */
type Translate = (text: string, values?: Readonly<Record<string, string | number>>) => string;

/** The steps over the app's own dialogs and backend. */
export function relocateSteps(backend: Backend, t: Translate): RelocateSteps {
  const manager = () => t("Missing File Manager");
  return {
    choose: (track, folder) => {
      const name = fileNameOf(track.path);
      return backend.chooseRelocateFile(`${t("Choose a new fullpath for")} : ${name}`, name, folder);
    },
    relocate: (track, path) => backend.relocateTrack(track.id, path),
    alreadyInCollection: () => backend.tell(t("This file is already in the collection."), manager()),
    askFindOthers: (title) => backend.confirm(
      `${t("Would you like RBXport to find other missing file using the location of this track ?")}\n${title}`,
      { yes: t("Yes"), no: t("No"), title: manager() },
    ),
    findOthers: (ids, from, to) => backend.relocateByLocation(ids, from, to),
    found: (count) => backend.tell(t("RBXport found {count} files.", { count }), manager()),
  };
}
