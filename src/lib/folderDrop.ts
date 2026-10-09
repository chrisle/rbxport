import { getBackend } from "@/ipc/client";
import { TREE_ROOT, type Backend, type TreeNode } from "@/ipc/types";

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

/**
 * Folders dragged in from outside the app and dropped on the Playlists root
 * or a playlist folder: each becomes a playlist under `parent`, named after
 * it and holding every audio file under it, subfolders flattened in. This is
 * what rekordbox 7.2.19 does (`TreeViewer::treeMessageImportExternalFoldersToList`
 * 0x1015677ec) [OBS, static]: a loose file in the drop is ignored, a folder
 * with no audio makes nothing, and a list that already has the name is
 * replaced only if the user says so, with rekordbox's own question.
 *
 * rekordbox reads the drop's insert index once and puts every folder there,
 * so a later folder lands before an earlier one; each call gets the index the
 * previous one settled on.
 *
 * Kept out of `App` so the initial bundle does not carry it.
 */
export async function importFolderDrop(
  backend: Pick<Backend, "importFolderPlaylist" | "confirm">,
  parent: string,
  paths: readonly string[],
  t: Translate,
): Promise<FolderDropResult> {
  const made: string[] = [];
  const tracks: { id: string; title: string }[] = [];
  let folders = 0;
  let skipped = 0;
  let kept = 0;
  let at: number | null = null;
  for (const path of paths) {
    let result = await backend.importFolderPlaylist(path, parent, undefined, at);
    at = result.at ?? at;
    if (result.folder) folders += 1;
    if (result.conflict) {
      const replace = await backend.confirm(
        `${t("One or several lists with the same name already exist.")}\n${t("Do you want to replace them with the one you're importing?")}`,
      );
      if (!replace) {
        kept += 1;
        continue;
      }
      result = await backend.importFolderPlaylist(path, parent, result.conflict, at);
      at = result.at ?? at;
    }
    if (result.playlist) made.push(result.name);
    tracks.push(...result.tracks);
    skipped += result.skipped.length;
  }
  if (folders === 0) {
    return { message: t("Drop folders onto Playlists or a playlist folder to make playlists of them."), refused: true, tracks };
  }
  const tail = skipped === 0
    ? ""
    : ` ${skipped === 1 ? t("1 file skipped.") : t("{count} files skipped.", { count: skipped })}`;
  const message = made.length === 0
    ? `${kept > 0 ? t("No playlist was made.") : t("No playlist was made: the folders hold no audio files.")}${tail}`
    : `${t("Made playlists: {names}.", { names: made.join(", ") })}${tail}`;
  return { message, refused: false, tracks };
}

/**
 * A folder drop, its paths already resolved: runs [`importFolderDrop`],
 * refreshes the tree and says how it went on the status line. `target` is a
 * playlist folder's id, or `"playlists"` for the Playlists root.
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
): Promise<void> {
  const parent = target === "playlists" ? TREE_ROOT : target;
  try {
    report(paths.length === 1
      ? t("Importing 1 folder…")
      : t("Importing {count} folders…", { count: paths.length }));
    const backend = await getBackend();
    const done = await importFolderDrop(backend, parent, paths, t);
    setTree(await backend.playlistTree());
    (done.refused ? refuse : report)(done.message);
    if (done.tracks.length > 0) analyse?.(done.tracks);
  } catch (e) {
    refuse(e instanceof Error ? e.message : "Those folders could not be imported.");
  }
}
