/**
 * The Explorer section of the tree: the disk's folders as tree nodes.
 *
 * The backend answers two questions — where to start, and what is under one
 * folder — and nothing is read ahead. This module turns those answers into
 * the flat, depth-tagged node list the tree draws, so the Explorer virtualizes
 * and collapses the same way the playlists do. Pure functions over plain data,
 * so the shape is testable without a backend or a DOM.
 */
import type { ExplorerChildren, ExplorerRoot, TreeNode } from "@/ipc/types";

/** The heading's id. Selecting it opens an empty Explorer, as rekordbox does. */
export const EXPLORER_ROOT_ID = "explorer";

/**
 * Whether a row id names a file the library does not hold.
 *
 * The backend gives such a row `file:` and its path for an id, where a track
 * id is decimal. A deck can play one; nothing that writes to the library can
 * take one, and the shell says so rather than sending a write the database
 * would apply to no row and bump its counter for.
 */
export function isLooseId(id: string): boolean {
  return id.startsWith("file:");
}

/**
 * Whether a menu over these rows should treat them as files the library
 * does not hold. It follows the rows, not the view: a file the Explorer
 * lists stops being loose once imported, and its menu is a track's again
 * without leaving the Explorer. Any loose row in a mixed selection makes it
 * loose, so no write is offered that the loose file would turn away.
 */
export function hasLooseId(ids: Iterable<string>): boolean {
  for (const id of ids) if (isLooseId(id)) return true;
  return false;
}

const PREFIX = "dir:";
const ID = /^dir:(\d+):([^]*)$/;

/**
 * A folder's id in the tree: which root it was reached through, then its
 * path.
 *
 * The root is part of it because one folder can be in the tree twice. The
 * capture shows the home folder as a root and again under `Macintosh HD ›
 * Users`, and two rows with one id would be one selection and one React key.
 * The path alone still says what to read, so children fetched by way of one
 * root serve the other.
 */
export function explorerId(root: number, path: string): string {
  return `${PREFIX}${root}:${path}`;
}

/** The path behind a tree id, or null when the id is not a folder. */
export function explorerPath(id: string): string | null {
  const match = ID.exec(id);
  return match?.[2] ?? null;
}

/**
 * A child's path under its parent, with the separator the parent uses.
 *
 * The backend sends children as names, not paths — a thousand paths would
 * be past the response cap — so the path is put back together here. A
 * Windows path is told by its backslash; a root of `/` or `C:\` already ends
 * in its separator and must not get another.
 */
export function joinPath(parent: string, name: string): string {
  const separator = parent.includes("\\") ? "\\" : "/";
  return parent.endsWith(separator) ? `${parent}${name}` : `${parent}${separator}${name}`;
}

/**
 * The Explorer as tree nodes: the heading, the roots under it, and under
 * each folder that has been opened, what was found there.
 *
 * `children` holds one entry per folder the backend has answered for, keyed
 * by path; a folder absent from it has not been opened yet and is drawn as a
 * closed branch. Depth-first, so the array is in tree order and the tree's
 * own collapse logic applies to it unchanged.
 */
export function explorerNodes(
  roots: readonly ExplorerRoot[],
  children: ReadonlyMap<string, ExplorerChildren>,
): TreeNode[] {
  // No roots, no section: before the backend has answered, and on a machine
  // where it answers with nothing, a heading would lead nowhere — and the
  // rail dims what has nothing in it.
  if (roots.length === 0) return [];
  const out: TreeNode[] = [
    { id: EXPLORER_ROOT_ID, name: "Explorer", kind: "explorer", depth: 0, expanded: true },
  ];
  // Iterative, with a visited set: a loop on disk cannot happen through the
  // backend — it skips symlinks — but a tree that could hang on one is not
  // worth the saving of a few lines.
  const stack: { root: number; name: string; path: string; depth: number }[] = [];
  for (let i = roots.length - 1; i >= 0; i--) {
    const root = roots[i];
    if (root) stack.push({ root: i, name: root.name, path: root.path, depth: 1 });
  }
  const seen = new Set<string>();
  while (stack.length > 0) {
    const next = stack.pop();
    if (!next) continue;
    const id = explorerId(next.root, next.path);
    if (seen.has(id)) continue;
    seen.add(id);
    if (next.path.endsWith("\u0000more")) {
      out.push({ id, name: next.name, kind: "note", depth: next.depth });
      continue;
    }
    out.push({
      id,
      name: next.name,
      kind: "directory",
      depth: next.depth,
      // Closed until opened: what is under a folder is read when its twisty
      // is clicked, and a folder that has been opened stays whatever the
      // tree's own toggle says it is.
      expanded: false,
      lazy: true,
    });
    const under = children.get(next.path);
    if (!under) continue;
    // A folder the backend cut at its cap says how many more it holds, as a
    // line under the last one shown, so a card with fourteen thousand
    // folders does not look as if it had two.
    const left = under.total - under.names.length;
    if (left > 0) {
      stack.push({ root: next.root, name: moreNote(left), path: `${next.path}\u0000more`, depth: next.depth + 1 });
    }
    for (let i = under.names.length - 1; i >= 0; i--) {
      const name = under.names[i];
      if (name !== undefined) {
        stack.push({
          root: next.root, name, path: joinPath(next.path, name), depth: next.depth + 1,
        });
      }
    }
  }
  return out;
}

/** What the tree says under a folder the backend cut at its cap. */
export function moreNote(left: number): string {
  return `${left.toLocaleString("en")} more folder${left === 1 ? "" : "s"} not shown`;
}
