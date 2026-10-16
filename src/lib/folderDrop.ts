import { getBackend } from "@/ipc/client";
import { TREE_ROOT, type Backend, type TreeNode } from "@/ipc/types";
import type { FolderDropClash } from "@/lib/preferences";

type Translate = (text: string, values?: Readonly<Record<string, string | number>>) => string;

/** What a drop of folders onto the playlist tree came to. */
export interface FolderDropResult {
  /** The status line to show. */
  message: string;
  /** True when the drop held no folder at all, so it is a refusal. */
  refused: boolean;
  /** The tracks that landed, for Auto Analysis. */
  tracks: { id: string; title: string }[];
}

/** What to do with one dropped folder whose name a list beside it has. */
export type FolderClashChoice = Exclude<FolderDropClash, "ask">;

/** The answer to the Ask me question, and whether it holds for the rest of the drop. */
export interface FolderClashAnswer {
  choice: FolderClashChoice;
  /** Use this answer for every later clash of the same drop, without asking. */
  all: boolean;
}

/**
 * Preferences › Playlists' choice for a same-named folder, and how to ask
 * when that choice is Ask me: `ask` hears the folder's name.
 */
export interface FolderClashPolicy {
  mode: FolderDropClash;
  ask: (name: string) => Promise<FolderClashAnswer>;
}

/**
 * Folders dragged in from outside the app and dropped on the Playlists root
 * or a playlist folder: each becomes a playlist under `parent`, named after
 * it and holding every audio file under it, subfolders flattened in. This is
 * what rekordbox 7.2.19 does (`TreeViewer::treeMessageImportExternalFoldersToList`
 * 0x1015677ec) [OBS, static]: a loose file in the drop is ignored and a
 * folder with no audio makes nothing.
 *
 * A list that already has the folder's name is where rbxport parts from
 * rekordbox, which asks about each one (#355, Chris's 2026-10-12 decision):
 * `clash.mode` says what happens. `copy` (the default) keeps the old list and
 * imports the folder beside it under the next free name, which the summary
 * reports; `replace` replaces the old list, and the summary says how many
 * lists and track entries that removed (each replacement is one Edit > Undo
 * step, #426); `keep` leaves
 * it and skips the folder; only `ask` asks, and an answer given for the rest
 * of the drop is not asked again.
 *
 * rekordbox reads the drop's insert index once and puts every folder there,
 * so a later folder lands before an earlier one; each call gets the index the
 * previous one settled on.
 *
 * Each folder is one backend call and one transaction, so a Stop (`signal`)
 * lands between folders: the folders before it are whole playlists, the
 * ones after are not touched (#291). A Stop pressed while Ask me is waiting
 * for an answer leaves that folder untouched too. `progress` hears how many
 * folders are done after each.
 *
 * Kept out of `App` so the initial bundle does not carry it.
 */
export async function importFolderDrop(
  backend: Pick<Backend, "importFolderPlaylist">,
  parent: string,
  paths: readonly string[],
  t: Translate,
  clash: FolderClashPolicy,
  signal?: AbortSignal,
  progress: (done: number) => void = () => undefined,
): Promise<FolderDropResult> {
  const made: string[] = [];
  const renamed: { name: string; copy: string }[] = [];
  const replaced: { name: string; lists: number; tracks: number }[] = [];
  const tracks: { id: string; title: string }[] = [];
  let folders = 0;
  const skipped: string[] = [];
  let kept = 0;
  let at: number | null = null;
  let done = 0;
  // The choice every clash of this drop gets without asking, once known.
  let settled: FolderClashChoice | null = clash.mode === "ask" ? null : clash.mode;
  for (const path of paths) {
    if (signal?.aborted) break;
    try {
      let result = await backend.importFolderPlaylist(path, parent, undefined, at, settled === "copy");
      at = result.at ?? at;
      if (result.folder) folders += 1;
      if (result.conflict) {
        let choice = settled;
        if (choice === null) {
          const answer = await clash.ask(result.name);
          // Stopped while the question was up: this folder is not touched.
          if (signal?.aborted) break;
          choice = answer.choice;
          if (answer.all) settled = choice;
        }
        if (choice === "keep") {
          kept += 1;
          continue;
        }
        result = choice === "replace"
          ? await backend.importFolderPlaylist(path, parent, result.conflict, at)
          : await backend.importFolderPlaylist(path, parent, undefined, at, true);
        at = result.at ?? at;
      }
      if (result.playlist) {
        made.push(result.renamed ?? result.name);
        if (result.renamed) renamed.push({ name: result.name, copy: result.renamed });
        if (result.replaced) replaced.push({ name: result.name, ...result.replaced });
      }
      tracks.push(...result.tracks);
      skipped.push(...result.skipped);
    } finally {
      done += 1;
      progress(done);
    }
  }
  if (folders === 0 && !signal?.aborted) {
    return { message: t("Drop folders onto Playlists or a playlist folder to make playlists of them."), refused: true, tracks };
  }
  const copies = renamed
    .map(({ name, copy }) => ` ${t("A list named {name} was already there, so the folder was imported as {copy}.", { name, copy })}`)
    .join("");
  const replacements = replaced
    .map(({ name, lists, tracks }) => ` ${t(
      "Replaced the old {name}. Lists removed: {lists}. Tracks taken out of them: {tracks}; they stay in the collection.",
      { name, lists, tracks },
    )}`)
    .join("");
  // The first skipped file and why, so a lone skip explains itself.
  const tail = skipped.length === 0
    ? ""
    : ` ${skipped.length === 1 ? t("1 file skipped.") : t("{count} files skipped.", { count: skipped.length })} ${skipped[0]}.`;
  const message = made.length === 0
    ? `${kept > 0 || signal?.aborted ? t("No playlist was made.") : t("No playlist was made: the folders hold no audio files.")}${tail}`
    : `${t("Made playlists: {names}.", { names: made.join(", ") })}${replacements}${copies}${tail}`;
  return { message, refused: false, tracks };
}

/**
 * A folder drop, its paths already resolved: runs [`importFolderDrop`],
 * refreshes the tree and says how it went on the status line. `target` is a
 * playlist folder's id, or `"playlists"` for the Playlists root; `clash` is
 * what a same-named list does. It runs as a status-bar job, whose `signal`
 * stops it between folders and whose `progress` hears each folder done.
 */
export async function dropFolders(
  target: string,
  paths: string[],
  t: Translate,
  report: (message: string) => void,
  refuse: (message: string) => void,
  setTree: (tree: TreeNode[]) => void,
  /** Queues the new tracks for analysis; absent while Auto Analysis is off. */
  analyse: ((tracks: { id: string; title: string }[]) => void) | undefined,
  clash: FolderClashPolicy,
  signal?: AbortSignal,
  progress?: (done: number) => void,
): Promise<void> {
  const parent = target === "playlists" ? TREE_ROOT : target;
  const backend = await getBackend();
  try {
    const done = await importFolderDrop(backend, parent, paths, t, clash, signal, progress);
    (done.refused ? refuse : report)(done.message);
    if (done.tracks.length > 0) analyse?.(done.tracks);
  } catch (e) {
    refuse(e instanceof Error ? e.message : "Those folders could not be imported.");
  } finally {
    // After a folder that failed too: the folders before it are playlists.
    setTree(await backend.playlistTree());
  }
}
