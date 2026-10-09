/**
 * Library tree: Collection, Playlists (folders and lists), Histories, Devices,
 * and the Explorer's folders.
 *
 * Flattened to a single array of visible nodes so it virtualizes the same way
 * the track table does; thousands of playlists cost the same as ten.
 */
import { SearchField } from "@/components/SearchField";
import { TREE_SEARCH_OPTIONS } from "@/lib/search";

import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { TreeNode } from "@/ipc/types";
import styles from "./TreeView.module.css";
import { DeviceIcon, EjectIcon, FolderIcon, HistoryIcon, ListIcon, NoteIcon, SmartListIcon } from "@/components/icons";
import { ContextMenu } from "@/components/ContextMenu";
import { deviceTreeMenu, treeMenu, type MenuTarget } from "@/lib/contextMenus";
import {
  branchIds, childrenOf, containerOf, emptySources, newlyClosed, nodesForSource, sourceOf,
  subtreeIds, toggle, visibleNodes, searchTree, type TreeSearchScope, type Source,
} from "@/lib/tree";
import { SourceRail } from "./SourceRail";
import { browseListVars } from "@/lib/preferences";
import { usePreferences } from "@/store/usePreferences";
import type { TreeExpansion } from "@/lib/session";

/**
 * The name, while it is being typed over.
 *
 * Opens with the whole name selected, as renaming in a file manager does, so
 * typing replaces it and a click puts the caret somewhere instead. Enter and
 * a click elsewhere commit; Escape puts the old name back.
 */
function RenameField({ name, onCommit, onCancel }: {
  name: string;
  onCommit: (name: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState(name);
  // Committed once: Enter moves the focus away, and the blur that follows
  // would otherwise write the same name a second time.
  const done = useRef(false);
  const finish = (commit: boolean) => {
    if (done.current) return;
    done.current = true;
    if (commit) onCommit(text.trim());
    else onCancel();
  };
  return (
    <input
      className={styles.rename}
      aria-label={`Rename ${name}`}
      value={text}
      autoFocus
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setText(e.target.value)}
      // The row beneath would otherwise select, drag or open its menu.
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.stopPropagation()}
      onDragStart={(e) => e.preventDefault()}
      onBlur={() => finish(true)}
      onKeyDown={(e) => {
        if (e.key === "Enter") finish(true);
        else if (e.key === "Escape") finish(false);
        else return;
        e.preventDefault();
        e.stopPropagation();
      }}
    />
  );
}

const Row = memo(function Row({
  node, selected, branch, open, onSelect, onToggle, droppable, onDropTracks, onDropFiles, onMenu, count,
  renaming, onRename, onRenameEnd, onRenameStart, doubleClickToEdit,
  movable, moveEdge, onMoveStart, onMoveOver, onMoveDrop, onMoveEnd,
  onEjectDevice, ejecting, deviceBusy,
}: {
  node: TreeNode;
  selected: boolean;
  /** This row is being renamed, so its name is an input rather than a label. */
  renaming: boolean;
  onRename: ((node: TreeNode, name: string) => void) | undefined;
  onRenameEnd: (() => void) | undefined;
  onRenameStart: ((node: TreeNode) => void) | undefined;
  doubleClickToEdit: boolean;
  /** This node can be picked up and moved somewhere else in the tree. */
  movable: boolean;
  /**
   * How this row would take the drop: a line above or below it for a place
   * among its siblings, `into` for a folder that would swallow it.
   */
  moveEdge: "above" | "below" | "into" | null;
  onMoveStart: ((node: TreeNode) => void) | undefined;
  onMoveOver: ((node: TreeNode, edge: "above" | "below" | "into") => void) | undefined;
  onMoveDrop: (() => void) | undefined;
  onMoveEnd: (() => void) | undefined;
  /** The playlist's track count, when Preferences asks for it on the tree. */
  count: number | undefined;
  /** Whether a track drag could land here. */
  droppable: boolean;
  onDropTracks: ((playlistId: string) => void) | undefined;
  /** Files dragged in from outside the app, dropped on this row. */
  onDropFiles: ((playlistId: string, files: File[]) => void) | undefined;
  onMenu: ((node: TreeNode, at: { x: number; y: number }) => void) | undefined;
  /** Whether anything sits under this node, so it can be opened at all. */
  branch: boolean;
  open: boolean;
  onSelect: (node: TreeNode) => void;
  onToggle: (node: TreeNode) => void;
  onEjectDevice: ((node: TreeNode) => void) | undefined;
  ejecting: boolean;
  deviceBusy: boolean;
}) {
  // A history node is a session, or the year or month one is filed under —
  // one kind, told apart by whether anything sits beneath it. A month whose
  // every session was deleted has nothing beneath it but is still a folder,
  // which the backend says by sending it an open/closed state.
  const historyFolder = node.kind === "history" && (branch || node.expanded !== undefined);
  const Icon =
    node.kind === "folder" || node.kind === "directory" || historyFolder ||
    node.kind === "devicePlaylists" || node.kind === "deviceFolder"
      ? FolderIcon
      : node.kind === "history"
        ? HistoryIcon
        : node.kind === "allTracks" || node.kind === "deviceAllTracks"
          ? NoteIcon
          : node.kind === "device" || node.kind === "deviceLibrary"
            ? DeviceIcon
            : node.kind === "smartPlaylist"
              ? SmartListIcon
              : ListIcon;
  // Whether the drag is over this row right now. Only the row under the
  // pointer is marked — every playlist lighting up for the whole drag read
  // as a grid of errors — and a drag that ends elsewhere clears it.
  const [over, setOver] = useState(false);
  const pressedOnSelected = useRef(false);
  // A file dragged in from Finder/Explorer never sets `dragging`/`droppable`
  // (nothing inside the app started that drag), so it is its own path: any
  // playlist row that was given `onDropFiles` takes one, gated on the
  // browser's own file-drag signal instead.
  const fileDroppable = node.kind === "playlist" && Boolean(onDropFiles);
  useEffect(() => {
    if (!droppable && !fileDroppable) setOver(false);
  }, [droppable, fileDroppable]);
  return (
    <div
      className={styles.node}
      data-selected={selected || undefined}
      data-kind={node.kind}
      data-file-drop-playlist={fileDroppable ? node.id : undefined}
      data-ejecting={ejecting || undefined}
      style={{ paddingLeft: `${14 + node.depth * 20}px` }}
      // A note is information, not a place: nothing to select.
      onMouseDown={() => node.kind !== "note" && onSelect(node)}
      draggable={movable && !renaming}
      onKeyDown={(e) => {
        if (e.key !== "F2" || renaming || !onRenameStart) return;
        e.preventDefault();
        onRenameStart(node);
      }}
      onDragStart={(e) => {
        if (!movable) return;
        // Stops the drag being read as a track drag by the rows' own handlers.
        e.stopPropagation();
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", node.id);
        onMoveStart?.(node);
      }}
      onDragEnd={() => onMoveEnd?.()}
      onDragOver={(e) => {
        // A node being moved takes precedence: the same row is both a place to
        // put tracks and a place in the tree, and only one of those is in hand.
        if (onMoveOver) {
          e.preventDefault();
          e.dataTransfer.dropEffect = "move";
          const box = e.currentTarget.getBoundingClientRect();
          const third = box.height / 3;
          // A folder's middle swallows the node; its edges place it alongside.
          const edge =
            node.kind === "folder" && e.clientY > box.top + third && e.clientY < box.bottom - third
              ? "into"
              : e.clientY > box.top + box.height / 2
                ? "below"
                : "above";
          onMoveOver(node, edge);
          return;
        }
        // A file drag from outside the app: `dataTransfer.files` is empty
        // until the drop, but `types` carries "Files" throughout, which is
        // the browser's own signal that this is worth taking.
        if (!droppable && fileDroppable && e.dataTransfer.types.includes("Files")) {
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
          setOver(true);
          return;
        }
        // Only a playlist takes tracks: a folder holds playlists, and dropping
        // into one would have to invent which.
        if (!droppable) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setOver(true);
      }}
      onDragLeave={(e) => {
        // Leaving for one of the row's own children is not leaving the row.
        if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
        setOver(false);
      }}
      onDrop={(e) => {
        setOver(false);
        if (onMoveDrop) {
          e.preventDefault();
          onMoveDrop();
          return;
        }
        if (!droppable && fileDroppable && e.dataTransfer.files.length > 0) {
          e.preventDefault();
          onDropFiles?.(node.id, Array.from(e.dataTransfer.files));
          return;
        }
        if (!droppable) return;
        e.preventDefault();
        onDropTracks?.(node.id);
      }}
      data-move={moveEdge ?? undefined}
      onContextMenu={(e) => {
        // Only the kinds that have a menu: the fixed roots and a stick's
        // own row and headings, which have nothing to offer here.
        if (!MENU_KINDS.has(node.kind) || !onMenu) return;
        e.preventDefault();
        onMenu(node, { x: e.clientX, y: e.clientY });
      }}
      data-droppable={(droppable || fileDroppable) || undefined}
      data-over={((droppable || fileDroppable) && over) || undefined}
      role="treeitem"
      aria-selected={selected}
      aria-expanded={branch ? open : undefined}
      tabIndex={selected ? 0 : -1}
    >
      <span
        className={styles.twisty}
        data-open={(branch && open) || undefined}
        data-leaf={!branch || undefined}
        // Stops the row's own mousedown from also selecting: opening a folder
        // and moving to it are different intentions.
        onMouseDown={(e) => {
          if (!branch) return;
          e.stopPropagation();
          e.preventDefault();
          onToggle(node);
        }}
        role={branch ? "button" : undefined}
        aria-label={branch ? `${open ? "Collapse" : "Expand"} ${node.name}` : undefined}
      />
      {node.kind === "collection" || node.kind === "histories" || node.kind === "explorer" ||
      node.kind === "note" ? null : (
        <Icon className={styles.icon} />
      )}
      {renaming ? (
        <RenameField
          name={node.name}
          onCommit={(name) => {
            // An unchanged or emptied name is a cancel: a playlist with no
            // name at all cannot be picked out of the tree again.
            if (name !== "" && name !== node.name) onRename?.(node, name);
            onRenameEnd?.();
          }}
          onCancel={() => onRenameEnd?.()}
        />
      ) : (
        <span className={styles.label}
          onMouseDown={(e) => {
            // Capture selection before the row's mousedown selects it.
            pressedOnSelected.current = selected && e.button === 0 && !e.shiftKey && !e.metaKey && !e.ctrlKey;
          }}
          onClick={() => {
            if (!doubleClickToEdit && pressedOnSelected.current) onRenameStart?.(node);
          }}
          onDoubleClick={(e) => {
            if (doubleClickToEdit && !e.shiftKey && !e.metaKey && !e.ctrlKey) onRenameStart?.(node);
          }}
        >{node.name}</span>
      )}
      {count !== undefined && !renaming ? (
        <span className={styles.count} aria-label={`${count} tracks`}>({count})</span>
      ) : null}
      {node.kind === "device" && onEjectDevice ? (
        <button
          type="button"
          className={styles.ejectButton}
          aria-label={`Eject ${node.name}`}
          title={`Safely eject ${node.name}`}
          disabled={deviceBusy}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            onEjectDevice(node);
          }}
        >
          <EjectIcon />
        </button>
      ) : null}
    </div>
  );
});

/** The tree kinds a right-click opens a menu over. */
const MENU_KINDS: ReadonlySet<TreeNode["kind"]> = new Set([
  "collection", "playlist", "smartPlaylist", "folder", "devicePlaylists", "deviceFolder", "devicePlaylist",
]);

/** A stick's own playlists and folders, which its menus and renames act on. */
function isDeviceMenuKind(kind: TreeNode["kind"]): kind is "devicePlaylists" | "deviceFolder" | "devicePlaylist" {
  return kind === "devicePlaylists" || kind === "deviceFolder" || kind === "devicePlaylist";
}

/** The measured row pitch, `--s-row-height`. */
const TREE_ROW_H = 25;

export interface TreeViewProps {
  nodes: readonly TreeNode[];
  selectedId: string | null;
  onSelect: (node: TreeNode) => void;
  /** Write a playlist or folder to the stick mounted at `path`. */
  onExport?: (node: TreeNode, path: string) => void;
  /** The sticks Export Playlist and Export Folder list, by mount path. */
  exportDevices?: readonly MenuTarget[];
  /** Export a playlist to a file: an m3u8 or rekordbox's tab-separated txt. */
  onExportFile?: (node: TreeNode, format: "m3u8" | "txt") => void;
  /** Create, delete and rename, which the shell owns because they write. */
  onCreatePlaylist?: (parent: TreeNode) => void;
  onCreateFolder?: (parent: TreeNode) => void;
  onDeleteNode?: (node: TreeNode) => void;
  onRenameNode?: (node: TreeNode, name: string) => void;
  /**
   * Put `node` under `parent` at `index` among that parent's children.
   *
   * `parent` is `TREE_ROOT` for the top of the playlists. Absent where the
   * tree cannot be rearranged, which is what makes the rows undraggable.
   */
  onMoveNode?: ((node: TreeNode, parent: string, index: number) => void) | undefined;
  /** rekordbox is running, so every write is greyed rather than raced. */
  readOnly?: boolean;
  /** Preferences: the number of tracks after each playlist's name. */
  showCounts?: boolean;
  /** True while tracks are being dragged, so playlists can offer themselves. */
  dragging?: boolean;
  /** Drop the dragged tracks onto a playlist. */
  onDropTracks?: (playlistId: string) => void;
  /** Files dragged in from outside the app (Finder, Explorer), dropped onto a playlist. */
  onDropFiles?: ((playlistId: string, files: File[]) => void) | undefined;
  /**
   * A lazy node was opened: read what is under it. The Explorer's folders,
   * whose children are not known until somebody looks.
   */
  onExpand?: (node: TreeNode) => void;
  /** Main-tree folder choices restored from the previous app session. */
  initialExpansion?: TreeExpansion;
  /** Keeps main-tree folder choices current for the next app session. */
  onExpansionChange?: (expansion: TreeExpansion) => void;
  /** Opens the Sync Manager from the foot of the rail. */
  onOpenSync?: () => void;
  /** Create New Intelligent Playlist under the node, and Edit the Intelligent Playlist. */
  onCreateSmartPlaylist?: (parent: TreeNode) => void;
  onEditSmartPlaylist?: (node: TreeNode) => void;
  /** Add Artwork on a playlist or folder. */
  onAddArtwork?: (node: TreeNode) => void;
  /** Sort Items: a folder's children put in name order. */
  onSortItems?: (folder: TreeNode) => void;
  /** Add To Shortcut: the rail takes the node as a button of its own. */
  onAddToShortcut?: (node: TreeNode) => void;
  /** The rail's shortcut buttons, what opens one, and what deletes one. */
  railShortcuts?: readonly { id: string; name: string; selected: boolean }[];
  onOpenShortcut?: (id: string) => void;
  onDeleteShortcut?: (id: string) => void;
  /** Safely eject a connected volume from its row in the Devices tree. */
  onEjectDevice?: (node: TreeNode) => void;
  /**
   * A stick's own playlists: create under its Playlists heading or a folder,
   * rename and delete. Each acts on the library the node belongs to.
   */
  onDeviceCreate?: (parent: TreeNode, folder: boolean) => void;
  onDeviceRename?: (node: TreeNode, name: string) => void;
  onDeviceDelete?: (node: TreeNode) => void;
  ejectingDeviceId?: string | null;
  deviceBusy?: boolean;
}

export const TreeView = memo(function TreeView({
  nodes, selectedId, onSelect, dragging, onDropTracks, onDropFiles, onExport, exportDevices, onExportFile,
  onCreatePlaylist, onCreateFolder, onDeleteNode, onRenameNode, onMoveNode, readOnly = false,
  onExpand, showCounts = false, onOpenSync,
  initialExpansion, onExpansionChange,
  onCreateSmartPlaylist, onEditSmartPlaylist, onAddArtwork, onAddToShortcut, onSortItems,
  railShortcuts, onOpenShortcut, onDeleteShortcut,
  onEjectDevice, ejectingDeviceId, deviceBusy = false,
  onDeviceCreate, onDeviceRename, onDeviceDelete,
}: TreeViewProps) {
  const { advanced: { doubleClickToEdit }, view } = usePreferences();
  // Browse › FontSize and Line Space apply to the tree as they do the list.
  const listVars = browseListVars(view, TREE_ROW_H);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<TreeSearchScope>("all");
  /** The tree menu: where it is, and which node it was opened on. */
  const [menu, setMenu] = useState<{ x: number; y: number; node: TreeNode } | null>(null);
  /** The row whose name is being typed over, if any. */
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const endRename = useCallback(() => setRenamingId(null), []);
  const beginRename = useCallback((node: TreeNode) => {
    if (!readOnly && onRenameNode &&
        (node.kind === "playlist" || node.kind === "smartPlaylist" || node.kind === "folder")) {
      setRenamingId(node.id);
    }
    // A stick's playlist or folder renames in place, as rekordbox's does
    // [OBS 7.2.14: a click on the selected row]; not while it is busy.
    if (!readOnly && !deviceBusy && onDeviceRename && (node.kind === "devicePlaylist" || node.kind === "deviceFolder")) {
      setRenamingId(node.id);
    }
  }, [readOnly, onRenameNode, deviceBusy, onDeviceRename]);
  const renameNode = useCallback((node: TreeNode, name: string) => {
    if (node.kind === "devicePlaylist" || node.kind === "deviceFolder") onDeviceRename?.(node, name);
    else onRenameNode?.(node, name);
  }, [onRenameNode, onDeviceRename]);
  const canRename = Boolean(onRenameNode) || Boolean(onDeviceRename);
  useEffect(() => {
    if (readOnly) endRename();
  }, [readOnly, endRename]);

  /** The node in hand while it is being dragged somewhere else in the tree. */
  const [moving, setMoving] = useState<TreeNode | null>(null);
  const [moveTo, setMoveTo] = useState<
    { node: TreeNode; edge: "above" | "below" | "into" } | null
  >(null);
  const endMove = useCallback(() => {
    setMoving(null);
    setMoveTo(null);
  }, []);

  /** Everything the node in hand would take with it, which it cannot land in. */
  const carriedIds = useMemo(
    () => (moving ? subtreeIds(nodes, moving) : null),
    [moving, nodes],
  );

  const onMoveOver = useCallback(
    (node: TreeNode, edge: "above" | "below" | "into") => {
      // Its own subtree is not a destination, and neither is anything that is
      // not part of the playlists.
      if (carriedIds?.has(node.id) === true) return;
      if (node.kind !== "playlist" && node.kind !== "folder") return;
      setMoveTo((at) => (at?.node.id === node.id && at.edge === edge ? at : { node, edge }));
    },
    [carriedIds],
  );

  const onMoveDrop = useCallback(() => {
    const node = moving;
    const target = moveTo;
    endMove();
    if (!onMoveNode || !node || !target) return;
    if (target.edge === "into") {
      // A folder's middle says "in here", not where in here, so it appends.
      onMoveNode(node, target.node.id, childrenOf(nodes, target.node.id).length);
      return;
    }
    const parent = containerOf(nodes, target.node);
    // Counted with the node lifted out, which is how the backend reads it.
    const siblings = childrenOf(nodes, parent).filter((n) => n.id !== node.id);
    const at = siblings.findIndex((n) => n.id === target.node.id);
    if (at < 0) return;
    onMoveNode(node, parent, target.edge === "below" ? at + 1 : at);
  }, [moving, moveTo, endMove, onMoveNode, nodes]);
  // Which nodes are closed. Seeded from the tree the backend sent — it marks
  // what should open, and 187 history sessions filed by year and month would
  // otherwise arrive on top of the playlists — and the user's own toggles take
  // over from there.
  const [expansion, setExpansion] = useState(() => ({
    collapsed: new Set(initialExpansion?.collapsed ?? []),
    expanded: new Set(initialExpansion?.expanded ?? []),
  }));
  const collapsed = expansion.collapsed;
  // Every id seeded so far. Only a node the tree has never seen is seeded:
  // re-seeding on every later tree would shut whatever the user had opened
  // each time a playlist changed, and the Explorer's folders arrive a level
  // at a time, each level closed, long after the first tree.
  const seen = useRef<Set<string>>(new Set([
    ...(initialExpansion?.collapsed ?? []),
    ...(initialExpansion?.expanded ?? []),
  ]));
  useEffect(() => {
    const fresh = newlyClosed(nodes, seen.current);
    for (const node of nodes) seen.current.add(node.id);
    if (fresh.length === 0) return;
    setExpansion((current) => {
      const next = new Set(current.collapsed);
      for (const id of fresh) next.add(id);
      return { collapsed: next, expanded: current.expanded };
    });
  }, [nodes]);

  // Persist explicit choices after every toggle (and after newly discovered
  // default-closed nodes are seeded). The parent owns storage so this view is
  // still reusable by the independently stateful Sub-Browser.
  useEffect(() => {
    onExpansionChange?.({
      collapsed: [...expansion.collapsed],
      expanded: [...expansion.expanded],
    });
  }, [expansion, onExpansionChange]);

  // Restoring an open Explorer folder has to read it before its remembered
  // descendants can appear. As each level arrives this effect runs again;
  // useExplorer de-duplicates folders that are loaded or already pending.
  useEffect(() => {
    if (!onExpand) return;
    for (const node of nodes) {
      if (node.lazy === true && expansion.expanded.has(node.id)) onExpand(node);
    }
  }, [nodes, expansion.expanded, onExpand]);

  const onToggle = useCallback(
    (node: TreeNode) => {
      const wasCollapsed = collapsed.has(node.id);
      setExpansion((current) => {
        const nextCollapsed = toggle(current.collapsed, node.id);
        const nextExpanded = new Set(current.expanded);
        if (wasCollapsed) nextExpanded.add(node.id);
        else nextExpanded.delete(node.id);
        return { collapsed: nextCollapsed, expanded: nextExpanded };
      });
      // Opening a lazy node is the moment to read what is under it. Told on
      // every open, and the owner ignores what it already holds.
      if (node.lazy === true && wasCollapsed) onExpand?.(node);
    },
    [collapsed, onExpand],
  );

  const empty = useMemo(() => emptySources(nodes), [nodes]);
  // Both derived in one pass each; per-row lookups would scan the array.
  const branches = useMemo(() => branchIds(nodes), [nodes]);

  // The rail filters: the tree shows one section at a time, the one the
  // selection is in, which is what rekordbox does [OBS 7.2.11] — Histories
  // lit shows the sessions and nothing else. Where you are is read from the
  // selection rather than kept apart from it, so the two cannot disagree.
  const source = useMemo(() => sourceOf(nodes, selectedId), [nodes, selectedId]);
  const visible = useMemo(
    () => query.trim() ? searchTree(nodes, query, scope) : visibleNodes(nodesForSource(nodes, source), collapsed),
    [nodes, source, collapsed, query, scope],
  );
  // Set by a rail click, read once the selection has moved: the section's
  // heading is scrolled to the top, or as near it as the list's end allows;
  // the list ends at its last row, so the bottom of the scroll is the last
  // playlist at the bottom of the pane. A click on a node scrolls nothing;
  // it was in view to be clicked.
  const list = useRef<HTMLDivElement>(null);
  const jumped = useRef(false);
  const jumpTo = useCallback(
    (wanted: Source) => {
      const first = nodesForSource(nodes, wanted)[0];
      if (!first) return;
      jumped.current = true;
      onSelect(first);
    },
    [nodes, onSelect],
  );
  useEffect(() => {
    if (!jumped.current) return;
    jumped.current = false;
    const row = list.current?.querySelector<HTMLElement>("[data-selected]");
    row?.scrollIntoView({ block: "start" });
  }, [selectedId]);

  return (
    <nav className={styles.tree} aria-label="Library">
      <SourceRail
        selected={source}
        onSelect={jumpTo}
        empty={empty}
        onOpenSync={onOpenSync}
        shortcuts={railShortcuts}
        onOpenShortcut={onOpenShortcut}
        onDeleteShortcut={onDeleteShortcut}
      />
      <div className={styles.content}>
      <SearchField className={styles.search} value={query} onChange={setQuery} scope={scope} onScopeChange={setScope}
        options={TREE_SEARCH_OPTIONS} menuWidth={154} label="Search library tree" scopeLabel="Tree search scope" />
      <div className={styles.nodes} role="tree" ref={list} style={listVars}>
        {visible.map((node) => (
          <Row
            key={node.id}
            node={node}
            selected={node.id === selectedId}
            branch={branches.has(node.id)}
            open={!collapsed.has(node.id)}
            onSelect={onSelect}
            onToggle={onToggle}
            droppable={Boolean(dragging) && node.kind === "playlist"}
            onDropTracks={onDropTracks}
            onDropFiles={onDropFiles}
            onMenu={(node, at) => setMenu({ ...at, node })}
            count={showCounts && (node.kind === "playlist" || node.kind === "devicePlaylist") ? node.childCount : undefined}
            renaming={!readOnly && node.id === renamingId}
            onRename={readOnly ? undefined : renameNode}
            onRenameEnd={endRename}
            onRenameStart={readOnly || !canRename ? undefined : beginRename}
            doubleClickToEdit={doubleClickToEdit}
            movable={
              Boolean(onMoveNode) &&
              (node.kind === "playlist" || node.kind === "smartPlaylist" || node.kind === "folder")
            }
            moveEdge={moveTo?.node.id === node.id ? moveTo.edge : null}
            onMoveStart={onMoveNode ? setMoving : undefined}
            onMoveOver={moving ? onMoveOver : undefined}
            onMoveDrop={moving ? onMoveDrop : undefined}
            onMoveEnd={endMove}
            onEjectDevice={onEjectDevice}
            ejecting={node.id === ejectingDeviceId}
            deviceBusy={deviceBusy}
          />
        ))}
        {visible.length === 0 ? (
          <p className={styles.emptyNote}>Nothing here yet.</p>
        ) : null}
      </div>

      </div>

      {menu && isDeviceMenuKind(menu.node.kind) ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          rows={deviceTreeMenu(menu.node.kind)}
          label={menu.node.kind === "deviceFolder" ? "Folder" : menu.node.kind === "devicePlaylists" ? "Playlists" : "Playlist"}
          context={{ inPlaylist: false, hasFile: false, readOnly: readOnly || deviceBusy }}
          onChoose={(action) => {
            switch (action) {
              case "deviceCreatePlaylist":
                onDeviceCreate?.(menu.node, false);
                break;
              case "deviceCreateFolder":
                onDeviceCreate?.(menu.node, true);
                break;
              case "deviceDelete":
                onDeviceDelete?.(menu.node);
                break;
            }
          }}
          onClose={() => setMenu(null)}
        />
      ) : menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          rows={treeMenu(
            menu.node.kind === "collection" ? "collection" : menu.node.kind === "folder" ? "folder" : menu.node.kind === "smartPlaylist" ? "smartPlaylist" : "playlist",
            exportDevices,
          )}
          label={menu.node.kind === "folder" ? "Folder" : menu.node.kind === "collection" ? "Playlists" : "Playlist"}
          context={{ inPlaylist: true, hasFile: true, readOnly }}
          onChoose={(action) => {
            if (action.startsWith("exportTo:")) {
              onExport?.(menu.node, action.slice("exportTo:".length));
              return;
            }
            switch (action) {
              case "exportM3u8":
                onExportFile?.(menu.node, "m3u8");
                break;
              case "exportTxt":
                onExportFile?.(menu.node, "txt");
                break;
              case "createPlaylist":
                onCreatePlaylist?.(menu.node);
                break;
              case "createFolder":
                onCreateFolder?.(menu.node);
                break;
              case "createSmartPlaylist":
                onCreateSmartPlaylist?.(menu.node);
                break;
              case "editSmartPlaylist":
                onEditSmartPlaylist?.(menu.node);
                break;
              case "addArtwork":
                onAddArtwork?.(menu.node);
                break;
              case "addToShortcut":
                onAddToShortcut?.(menu.node);
                break;
              case "sortItems":
                onSortItems?.(menu.node);
                break;
              case "rename":
                beginRename(menu.node);
                break;
              case "delete":
                onDeleteNode?.(menu.node);
                break;
              default:
                break;
            }
          }}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </nav>
  );
});
