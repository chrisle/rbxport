/**
 * Which tree nodes are visible, given what the user has collapsed.
 *
 * The backend hands back one flat array with a `depth` per node, which is what
 * lets the tree virtualize the same way the track table does. Collapsing has
 * to work on that flat shape rather than on a nested one, so this is a single
 * pass: once a node is collapsed, everything deeper than it is skipped until
 * the depth comes back up.
 */
import { TREE_ROOT, type RelatedCriterion, type TreeNode } from "@/ipc/types";
import { isDeviceLibraryKind } from "./deviceLibrary";

/**
 * Where a new playlist made from `node`'s menu goes: the folder itself, or
 * the folder a playlist is in, or the root of the tree — the id the backend
 * takes as a parent.
 *
 * The flat array carries no parent ids: a playlist's folder is the nearest
 * node above it one level up, and a playlist at depth 1 is under the
 * `Playlists` heading, which is not a folder anything can be made in.
 */
export function parentFor(nodes: readonly TreeNode[], node: TreeNode): string {
  if (node.kind === "folder") return node.id;
  const at = nodes.findIndex((n) => n.id === node.id);
  for (let i = at - 1; i >= 0; i -= 1) {
    const above = nodes[i];
    if (above === undefined || above.depth >= node.depth) continue;
    return above.kind === "folder" ? above.id : TREE_ROOT;
  }
  return TREE_ROOT;
}

/**
 * The node a node actually sits under, `TREE_ROOT` for the top level.
 *
 * Not `parentFor`, which answers a different question: that one takes a
 * folder to mean "inside this folder", because it is asked where a new
 * playlist should go. This one is asked where a node already is.
 */
export function containerOf(nodes: readonly TreeNode[], node: TreeNode): string {
  const at = nodes.findIndex((n) => n.id === node.id);
  for (let i = at - 1; i >= 0; i -= 1) {
    const above = nodes[i];
    if (above === undefined || above.depth >= node.depth) continue;
    return above.kind === "folder" ? above.id : TREE_ROOT;
  }
  return TREE_ROOT;
}

/**
 * The nodes sitting directly under `parent`, in tree order.
 *
 * One pass over the flat array rather than a `containerOf` per node, which
 * would walk back up the tree for every playlist in the library.
 */
export function childrenOf(nodes: readonly TreeNode[], parent: string): TreeNode[] {
  const out: TreeNode[] = [];
  if (parent === TREE_ROOT) {
    // The playlists at the top level: the run under the Playlists heading,
    // which ends where the tree comes back up for the next source.
    const start = nodes.findIndex((n) => n.kind === "collection");
    if (start < 0) return out;
    for (let i = start + 1; i < nodes.length; i += 1) {
      const node = nodes[i];
      if (node === undefined || node.depth === 0) break;
      if (node.depth === 1) out.push(node);
    }
    return out;
  }
  const start = nodes.findIndex((n) => n.id === parent);
  if (start < 0) return out;
  const depth = nodes[start]?.depth ?? 0;
  for (let i = start + 1; i < nodes.length; i += 1) {
    const node = nodes[i];
    if (node === undefined || node.depth <= depth) break;
    if (node.depth === depth + 1) out.push(node);
  }
  return out;
}

/**
 * `node` and everything filed under it.
 *
 * What a move has to refuse to land inside: a folder put into its own subtree
 * is detached from the tree and never seen again.
 */
export function subtreeIds(nodes: readonly TreeNode[], node: TreeNode): Set<string> {
  const ids = new Set<string>([node.id]);
  const start = nodes.findIndex((n) => n.id === node.id);
  if (start < 0) return ids;
  for (let i = start + 1; i < nodes.length; i += 1) {
    const below = nodes[i];
    if (below === undefined || below.depth <= node.depth) break;
    ids.add(below.id);
  }
  return ids;
}

/** A node can be collapsed only if something sits under it. */
export function hasChildren(nodes: readonly TreeNode[], index: number): boolean {
  const node = nodes[index];
  const next = nodes[index + 1];
  return node !== undefined && next !== undefined && next.depth > node.depth;
}

/**
 * The ids of every node with something under it, in one pass.
 *
 * Asking `hasChildren` per rendered row means finding that row's index first,
 * which is a scan — O(n²) over the whole tree, and this library has 683
 * playlists.
 *
 * A lazy node is a branch whether or not its children have arrived: the
 * Explorer's folders open on demand, and a folder with no twisty could never
 * be asked.
 */
export function branchIds(nodes: readonly TreeNode[]): Set<string> {
  const out = new Set<string>();
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if (node && (node.lazy === true || hasChildren(nodes, i))) out.add(node.id);
  }
  return out;
}

/**
 * The closed nodes among those the tree has not seen before.
 *
 * The backend marks what should open — histories closed, playlists open — and
 * a node arriving later, the way the Explorer's folders do when their parent
 * is opened, has to be seeded the same way. Only new ids, so the user's own
 * toggles on everything already in the tree are left alone.
 */
export function newlyClosed(
  nodes: readonly TreeNode[],
  seen: ReadonlySet<string>,
): string[] {
  const out: string[] = [];
  for (const node of nodes) {
    if (node.expanded === false && !seen.has(node.id)) out.push(node.id);
  }
  return out;
}

/**
 * The nodes to render, in order, with everything under a collapsed node
 * removed.
 *
 * `collapsed` holds ids the user has closed. A node missing from it is open,
 * so a tree with nothing collapsed renders whole — which is what the backend
 * hands back.
 */
export function visibleNodes(
  nodes: readonly TreeNode[],
  collapsed: ReadonlySet<string>,
): TreeNode[] {
  const out: TreeNode[] = [];
  // The depth below which nodes are hidden, or null when nothing is hidden.
  let hiddenBelow: number | null = null;

  for (const node of nodes) {
    if (hiddenBelow !== null) {
      if (node.depth > hiddenBelow) continue;
      // Back up to the collapsed node's level or shallower: visible again.
      hiddenBelow = null;
    }
    out.push(node);
    if (collapsed.has(node.id)) hiddenBelow = node.depth;
  }
  return out;
}

/** Adds or removes an id, returning a new set so React sees the change. */
export function toggle(collapsed: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(collapsed);
  if (!next.delete(id)) next.add(id);
  return next;
}

/**
 * Which part of the library the tree is showing.
 *
 * No Collection: the tree opens with All Tracks at the top and always in
 * sight, so a button whose whole job is to scroll to the first row is a
 * shortcut to where you already are.
 */
export type Source = "playlists" | "related" | "histories" | "tagList" | "explorer" | "devices";

/**
 * The Related Tracks section: rekordbox's three criteria under a heading,
 * the same in every library, so they are the frontend's own rows rather
 * than the backend's. [DOC: the rekordbox manual; no capture of the panel
 * exists here, so the rows are drawn as any other section's are.]
 */
export const RELATED_NODES: readonly TreeNode[] = [
  { id: "related", name: "Related Tracks", kind: "related", depth: 0, expanded: true },
  { id: "related:bpmKey", name: "BPM + KEY", kind: "relatedCriterion", depth: 1 },
  { id: "related:genreRecent", name: "Same genre in 30 days", kind: "relatedCriterion", depth: 1 },
  { id: "related:artist", name: "Same artist", kind: "relatedCriterion", depth: 1 },
  // rekordbox's Track Suggestion is a window of its own from the icon
  // column ("Display Track Suggestion window", captured 2026-09-18: a panel
  // titled Era with LIST / MASTER / A source buttons and a Collection
  // scope), which this app leaves at two boxes by request; it sits here as
  // a fourth row about the loaded track.
  { id: "related:suggestion", name: "Track Suggestion", kind: "relatedCriterion", depth: 1 },
];

/** The criterion a Related Tracks node names, or `null` for the heading. */
export function relatedCriterionOf(id: string): RelatedCriterion | null {
  switch (id) {
    case "related:bpmKey": return "bpmKey";
    case "related:genreRecent": return "genreRecent";
    case "related:artist": return "artist";
    case "related:suggestion": return "suggestion";
    default: return null;
  }
}

/**
 * The Tag List: rekordbox's one temporary list. rekordbox 7.2.11 shows it
 * nowhere on its rail, its sub-browser rail or its icon column (all read
 * off their tooltips 2026-09-18); this app gives it a rail source of its
 * own. One row, since the list has no children: it opens as a view.
 */
export const TAG_LIST_NODE: TreeNode = { id: "tagList", name: "Tag List", kind: "tagList", depth: 0 };

/** Visible sources. Related Tracks stays hidden until its criteria are ready. */
export function withSources(nodes: readonly TreeNode[]): TreeNode[] {
  return [
    ...nodes.filter((n) => n.kind !== "related" && n.kind !== "relatedCriterion" && n.kind !== "tagList"),
    TAG_LIST_NODE,
  ];
}

/**
 * The nodes belonging to one source: what the tree shows while that rail
 * button is lit.
 *
 * rekordbox's rail is a filter — Histories shows the sessions and nothing
 * else, Playlists shows All Tracks and the playlists [OBS 7.2.11]. The backend
 * sends one flat tree covering every section, so the rail narrows it here
 * rather than asking for a different tree — switching sections is then
 * instant and costs no round trip.
 */
export function nodesForSource(nodes: readonly TreeNode[], source: Source): TreeNode[] {
  switch (source) {
    case "histories":
      // The section's own heading first, so jumping there lands on it.
      return nodes.filter((n) => n.kind === "histories" || n.kind === "history");
    case "devices":
      // Each stick, and what its own libraries hold under it.
      return nodes.filter((n) => n.kind === "device" || isDeviceLibraryKind(n.kind));
    case "related":
      return nodes.filter((n) => n.kind === "related" || n.kind === "relatedCriterion");
    case "tagList":
      return nodes.filter((n) => n.kind === "tagList");
    case "explorer":
      // The heading first, as with histories: jumping there lands on the
      // section, which rekordbox opens as an empty Explorer. A note is the
      // Explorer's own — a folder cut short says how many were left out.
      return nodes.filter((n) => n.kind === "explorer" || n.kind === "directory" || n.kind === "note");
    case "playlists":
      // All Tracks belongs here: rekordbox shows it above the playlists and
      // in no other section.
      return nodes.filter(
        (n) =>
          n.kind === "allTracks" ||
          n.kind === "collection" ||
          n.kind === "folder" ||
          n.kind === "playlist" ||
          n.kind === "smartPlaylist",
      );
  }
}

/** Sources with nothing under them, so the rail can dim rather than hide them. */
export function emptySources(nodes: readonly TreeNode[]): Set<Source> {
  const empty = new Set<Source>();
  for (const source of ["playlists", "histories", "explorer", "devices"] as const) {
    if (nodesForSource(nodes, source).length === 0) empty.add(source);
  }
  return empty;
}

/**
 * Which section a selected node belongs to, so the rail can show where you are.
 *
 * Defaults to playlists when nothing is selected, because that is what the
 * tree opens on — and All Tracks belongs to it too, since it sits at the top
 * of the same tree and the rail no longer has a button of its own for it.
 */
export function sourceOf(nodes: readonly TreeNode[], selectedId: string | null): Source {
  const node = nodes.find((n) => n.id === selectedId);
  switch (node?.kind) {
    case "history":
    case "histories":
      return "histories";
    case "device":
    case "deviceLibrary":
    case "deviceAllTracks":
    case "devicePlaylists":
    case "deviceFolder":
    case "devicePlaylist":
      return "devices";
    case "explorer":
    case "directory":
      return "explorer";
    case "related":
    case "relatedCriterion":
      return "related";
    case "tagList":
      return "tagList";
    default:
      return "playlists";
  }
}

export type TreeSearchScope = "all" | "playlist" | "smartPlaylist" | "folder";

/** Keep matches and their ancestors, so duplicate names retain their context. */
export function searchTree(nodes: readonly TreeNode[], query: string, scope: TreeSearchScope): TreeNode[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return [...nodes];
  const keep = new Set<string>();
  const ancestors: TreeNode[] = [];
  for (const node of nodes) {
    while (ancestors.length && ancestors[ancestors.length - 1]!.depth >= node.depth) ancestors.pop();
    const matchesKind = scope === "all" || node.kind === scope;
    if (matchesKind && node.name.toLocaleLowerCase().includes(needle)) {
      keep.add(node.id);
      for (const parent of ancestors) keep.add(parent.id);
    }
    ancestors.push(node);
  }
  return nodes.filter(node => keep.has(node.id));
}
