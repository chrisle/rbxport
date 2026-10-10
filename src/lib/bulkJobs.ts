import type { ImportReport } from "@/ipc/types";
import { slices } from "./jobQueue";

/**
 * The slice loops behind the status bar's long library jobs (#291): import
 * files, remove tracks from the collection. Each slice is one backend call
 * and one database transaction, so a Stop lands between slices and the
 * library holds every slice before it, whole, and nothing of the ones after.
 * Kept apart from `App` so that is testable without a component.
 */

/** Files per import call, tracks per removal call. */
export const IMPORT_SLICE = 100;
export const REMOVE_SLICE = 100;

/** What an import came to, over every slice it reached. */
export interface SlicedImport {
  /** Files sent to the backend, which is all of them unless it was stopped. */
  reached: number;
  imported: number;
  skipped: number;
  existing: number;
  /** The tracks that were added, for Auto Analysis. */
  tracks: ImportReport["tracks"];
  /** Every track the files stand for, added or already held, in file order. */
  ids: string[];
}

/**
 * Imports `files` a slice at a time, stopping between slices once `signal`
 * is aborted. `progress` hears how many files have been sent after each
 * slice. A slice that fails throws; the slices before it stay imported.
 */
export async function importInSlices(
  files: readonly string[],
  importSlice: (paths: string[]) => Promise<ImportReport>,
  signal: AbortSignal,
  progress: (reached: number, sofar: SlicedImport) => void = () => undefined,
  size = IMPORT_SLICE,
): Promise<SlicedImport> {
  const done: SlicedImport = { reached: 0, imported: 0, skipped: 0, existing: 0, tracks: [], ids: [] };
  for (const slice of slices(files, size)) {
    if (signal.aborted) break;
    const result = await importSlice(slice);
    done.reached += slice.length;
    done.imported += result.imported;
    done.skipped += result.skipped.length;
    done.existing += result.existing.length;
    done.tracks.push(...result.tracks);
    // Files the library already held still stand for their tracks.
    for (const track of [...result.tracks, ...result.existing]) done.ids.push(track.id);
    progress(done.reached, done);
  }
  return done;
}

/**
 * Removes `ids` from the collection a slice at a time, stopping between
 * slices once `signal` is aborted, and answers how many were removed.
 * `progress` hears the running count after each slice. A slice that fails
 * throws; the slices before it stay removed.
 */
export async function removeInSlices(
  ids: readonly string[],
  removeSlice: (ids: string[]) => Promise<unknown>,
  signal: AbortSignal,
  progress: (removed: number) => void = () => undefined,
  size = REMOVE_SLICE,
): Promise<number> {
  let removed = 0;
  for (const slice of slices(ids, size)) {
    if (signal.aborted) break;
    await removeSlice(slice);
    removed += slice.length;
    progress(removed);
  }
  return removed;
}
