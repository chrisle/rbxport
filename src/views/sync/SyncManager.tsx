/**
 * The Sync Manager: which playlists go to which sticks.
 *
 * Three columns, as rekordbox's has — iTunes, rekordbox, Device — with a SYNC
 * button between each pair. The left SYNC imports the ticked iTunes playlists
 * into the library; the right SYNC writes the ticked library playlists to every
 * ticked device, so two sticks synced together are the same stick twice.
 *
 * SYNC to a device only writes to it. What a stick brings back — cues and
 * grids, play history, CDJ/mixer settings, ratings — comes back only when
 * Import is clicked, and which of those it brings is set in Preferences, as
 * rekordbox brings device edits back only through Update Collection (#199).
 *
 * The iTunes column reads Music.app's shared `Library.xml` (auto-detected, or
 * chosen from a file dialog) and imports only the playlists that are ticked.
 *
 * Nothing here holds the library: the tree is the same flat list the shell
 * fetches, and every count and name on a device comes from the backend
 * reading the stick.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, LoaderCircle, Search, Settings2, X } from "lucide-react";

import { EjectIcon, FolderIcon, ListIcon, SmartListIcon } from "@/components/icons";
import { getBackend } from "@/ipc/client";
import type { Device, DeviceSyncState, ExportReport, FormatLayout, ItunesLibrary, MissingExportFile, SyncDeviceReport, SyncEstimate, TreeNode } from "@/ipc/types";
import { formatSpace } from "@/lib/devices";
import { errorMessage } from "@/lib/errorMessage";
import { detectPlatform } from "@/lib/shortcuts";
import { askToReplaceLists } from "@/lib/xmlImport";
import { nodesForSource, subtreeIds, toggle, visibleNodes } from "@/lib/tree";
import { startWindowDrag, toggleWindowMaximise } from "@/lib/windowDrag";
import { usePreferences } from "@/store/usePreferences";
import { useExportProgress } from "@/store/useExportProgress";
import { canStopExport, isExportActive } from "@/lib/exportProgress";
import { ExportProgressDetails } from "@/components/ExportProgressDetails";
import { FormatDeviceDialog } from "./FormatDeviceDialog";
import { MissingFilesDialog } from "./MissingFilesDialog";
import { useTranslation } from "@/i18n";
import styles from "./SyncManager.module.css";

export interface SyncManagerProps {
  /** Drawn as a window of its own, so the shell's chrome is around it. */
  windowed?: boolean;
  onClose: () => void;
  /** A sync finished, so the shell can re-read what its devices hold. */
  onSynced?: () => void;
  /** The playlists, the devices and rekordbox's state have all been read once. */
  onReady?: () => void;
  /**
   * Opens Preferences on a pane where the shell has no Preferences window to
   * open (the browser build), so it is drawn over this one.
   */
  onOpenPreferences?: ((pane: "usbExport") => void) | undefined;
}

/** Ticked, not ticked, or a folder with some of its playlists ticked. */
type Tick = "on" | "off" | "some";

/** What Import can bring back from a stick, in the order it does them. */
type ImportKind = "cues" | "history" | "settings" | "ratings";
/** How many left-out tracks the sync-finished dialog lists before pointing at the log. */
const MAX_FAILURES_SHOWN = 10;
/** What the backend says while the library is still being read, and how soon to ask again. */
const LIBRARY_LOADING = "has not finished loading";
const LIBRARY_RETRY_MS = 3000;

const IMPORT_KINDS: readonly { kind: ImportKind; noun: string }[] = [
  { kind: "cues", noun: "cues and beat grids" },
  { kind: "history", noun: "play history" },
  { kind: "settings", noun: "CDJ/mixer settings" },
  { kind: "ratings", noun: "track ratings" },
];

/**
 * The playlists and folders alone: no All Tracks, no Playlists heading,
 * because neither is a thing a stick can be given.
 */
function playlistNodes(tree: readonly TreeNode[]): TreeNode[] {
  return nodesForSource(tree, "playlists").filter(
    (n) => n.kind === "folder" || n.kind === "playlist" || n.kind === "smartPlaylist",
  );
}

/** A playlist that can be materialized onto a device, whether stored or rule-based. */
function isExportablePlaylist(node: TreeNode): boolean {
  return node.kind === "playlist" || node.kind === "smartPlaylist";
}

/** Every playlist under `folder`, itself excluded. */
function playlistsUnder(nodes: readonly TreeNode[], folder: TreeNode, byId: ReadonlyMap<string, TreeNode>): string[] {
  const out: string[] = [];
  for (const id of subtreeIds(nodes, folder)) {
    const node = byId.get(id);
    if (id !== folder.id && node && isExportablePlaylist(node)) out.push(id);
  }
  return out;
}

/**
 * A checkbox that can be part-way: a folder with some of its playlists
 * ticked. `indeterminate` is a property, not an attribute, so it is set by
 * hand after every render.
 */
function TickBox({
  state, label, disabled, onChange,
}: {
  state: Tick;
  label: string;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (box.current) box.current.indeterminate = state === "some";
  }, [state]);
  return (
    <input
      ref={box}
      type="checkbox"
      className={styles.tick}
      checked={state === "on"}
      aria-checked={state === "some" ? "mixed" : state === "on"}
      aria-label={label}
      disabled={disabled}
      onChange={(e) => onChange(e.currentTarget.checked)}
    />
  );
}

export function SyncManager({ windowed = false, onClose, onSynced, onReady, onOpenPreferences }: SyncManagerProps) {
  const t = useTranslation();
  const exportJobs = useExportProgress();
  const window_ = useRef<HTMLDivElement>(null);
  const [tree, setTree] = useState<readonly TreeNode[]>([]);
  const [devices, setDevices] = useState<readonly Device[]>([]);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  // rekordbox's column is one tree under a [Playlists] row that can be
  // closed like any folder; it opens open.
  const [allOpen, setAllOpen] = useState(true);
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  const [tickedDevices, setTickedDevices] = useState<ReadonlySet<string>>(new Set());
  // Every stick opens open, showing the playlists it holds, as rekordbox's
  // Device column shows the selected device's playlists (#178). These are
  // the ones closed by hand.
  const [collapsedDevices, setCollapsedDevices] = useState<ReadonlySet<string>>(new Set());
  const [deviceErrors, setDeviceErrors] = useState<ReadonlyMap<string, string>>(new Map());
  const [states, setStates] = useState<ReadonlyMap<string, DeviceSyncState>>(new Map());
  const [estimates, setEstimates] = useState<ReadonlyMap<string, SyncEstimate>>(new Map());
  const [estimatesLoading, setEstimatesLoading] = useState(false);
  const [query, setQuery] = useState("");
  const [loadingTree, setLoadingTree] = useState(true);
  // Sync Manager can open before the library has finished loading; the tree is
  // asked for again every few seconds until it can answer.
  const [libraryLoading, setLibraryLoading] = useState(false);
  const [treeError, setTreeError] = useState("");
  // True from the start: the first scan begins on mount, and nothing may
  // offer SYNC or say "no devices" before it has answered.
  const [loadingDevices, setLoadingDevices] = useState(true);
  const [devicesError, setDevicesError] = useState("");
  // This window may live in its own webview, so it keeps its own live view
  // of rekordbox's process lock instead of relying on the main window.
  // Unknown is locked: do not briefly enable SYNC before the first check.
  const [rekordboxOpen, setRekordboxOpen] = useState<boolean | null>(null);
  const [operation, setOperation] = useState<"sync" | "import" | "eject" | "format" | "itunes" | null>(null);
  // The iTunes / Music library shown in the left column, its ticks, and which
  // of its folders are closed. Null until the auto-detect answers, or when no
  // shared Library.xml is found and the DJ has not chosen one.
  const [itunes, setItunes] = useState<ItunesLibrary | null>(null);
  const [itunesTicked, setItunesTicked] = useState<ReadonlySet<string>>(new Set());
  const [itunesCollapsed, setItunesCollapsed] = useState<ReadonlySet<string>>(new Set());
  // iTunes and Music.app do not exist on Linux, so there the manager is two
  // columns (rbxport, Device) and never looks for an iTunes library.
  const showItunes = useMemo(() => !detectPlatform().linux, []);
  const [itunesLoading, setItunesLoading] = useState(showItunes);
  const [itunesError, setItunesError] = useState("");
  const [ejectingPath, setEjectingPath] = useState<string | null>(null);
  const [missingPrompt, setMissingPrompt] = useState<{ files: MissingExportFile[]; resolve: (proceed: boolean) => void } | null>(null);
  const [formatTarget, setFormatTarget] = useState<Device | null>(null);
  const busy = operation !== null || [...exportJobs.values()].some(isExportActive);
  const [ejectAfterSync, setEjectAfterSync] = useState(false);
  /** What is happening now, or what happened: one line, or one per stick. */
  const [status, setStatus] = useState<string[]>([]);
  const [showStatusDetails, setShowStatusDetails] = useState(false);
  // What the status line summarises: the tracks left out, one to a line.
  const [statusDetail, setStatusDetail] = useState<string[]>([]);
  const [lastImport, setLastImport] = useState<readonly ImportKind[] | null>(null);
  const [importFailed, setImportFailed] = useState(false);
  const [completedReports, setCompletedReports] = useState<ReadonlyMap<string, ExportReport>>(new Map());
  // DJ System in Preferences is what a stick with no settings of its own
  // gets, as it is on every export from the shell.
  const preferences = usePreferences();
  const stickDefaults = preferences.djSystem;
  // What Import brings back is set in Preferences, not here: no ticks to
  // set and no question to answer each time (#199).
  const importChosen: Record<ImportKind, boolean> = {
    cues: preferences.usbExport.importButtonCues,
    history: preferences.usbExport.importButtonHistory,
    settings: preferences.usbExport.importButtonSettings,
    ratings: preferences.usbExport.importButtonRatings,
  };
  /** Why a kind cannot be imported right now, or null when it can. */
  const importBlocked = (kind: ImportKind): string | null =>
    kind === "cues" && preferences.advanced.protectLibrary ? t("Turn off Library Protection to import cues and grids.")
    : kind === "history" && preferences.advanced.protectLibrary ? t("Turn off Library Protection to import play history.")
    : kind === "history" && rekordboxOpen !== false ? t("Quit rekordbox to import play history.")
    : kind === "ratings" && preferences.advanced.protectLibrary ? t("Turn off Library Protection to import track ratings.")
    : kind === "ratings" && rekordboxOpen !== false ? t("Quit rekordbox to import track ratings.")
    : null;
  const chosenKinds = IMPORT_KINDS.filter(({ kind }) => importChosen[kind]);
  const importKinds = chosenKinds.map(({ kind }) => kind).filter(kind => !importBlocked(kind));
  // Why each chosen kind is left out, said on the button and after an Import.
  const importBlockedNotes = [...new Set(chosenKinds.flatMap(({ kind }) => importBlocked(kind) ?? []))];
  const importTitle = chosenKinds.length === 0 ? t("Nothing is set to import. Choose what Import brings back in Preferences.")
    : importKinds.length === 0 ? importBlockedNotes.join(" ")
    : t("Import {kinds} from the selected USB devices.", { kinds: IMPORT_KINDS.filter(({ kind }) => importKinds.includes(kind)).map(({ noun }) => t(noun)).join(", ") });

  const deleteUnlistedMusic = preferences.usbExport.deleteUnlistedMusic;
  const compatibilityFormat = preferences.usbExport.maximumCompatibility ? preferences.usbExport.conversionFormat : undefined;
  const verifyAll = preferences.usbExport.verifyAllFiles;

  const nodes = useMemo(() => playlistNodes(tree), [tree]);
  const playlists = useMemo(() => nodes.filter(isExportablePlaylist), [nodes]);
  const byId = useMemo(() => new Map(nodes.map((n) => [n.id, n] as const)), [nodes]);
  const search = query.trim().toLocaleLowerCase();
  const visible = useMemo(() => search
    ? playlists.filter(node => node.name.toLocaleLowerCase().includes(search))
    : allOpen ? visibleNodes(nodes, collapsed) : [], [nodes, playlists, collapsed, search, allOpen]);
  const selectedCount = playlists.filter(node => ticked.has(node.id)).length;
  // The [Playlists] row's box, as rekordbox's: every playlist, some, or none.
  const allState: Tick = selectedCount === 0 ? "off" : selectedCount === playlists.length ? "on" : "some";
  // What the selection would use on each ticked stick, asked for once the
  // ticking has settled. A key rather than the device list: that is re-read
  // on a timer and would restart the estimate every time.
  const estimateDevices = [...tickedDevices].sort().join("\n");
  const capacityRevision = devices.map(d => `${d.path}:${d.totalBytes}:${d.freeBytes}:${d.fileSystem}`).join("\n");
  const reported = completedReports.size;
  useEffect(() => {
    const ids = nodes.filter((n) => isExportablePlaylist(n) && ticked.has(n.id)).map((n) => n.id);
    const destinations = estimateDevices === "" ? [] : estimateDevices.split("\n");
    if (ids.length === 0 || destinations.length === 0) {
      setEstimates(new Map());
      setEstimatesLoading(false);
      return;
    }
    setEstimates(new Map());
    setEstimatesLoading(true);
    let live = true;
    const timer = window.setTimeout(() => {
      void getBackend()
        .then((backend) => backend.estimateSync(ids, destinations, deleteUnlistedMusic, compatibilityFormat))
        .then((found) => { if (live) { setEstimates(new Map(found.map((e) => [e.path, e] as const))); setEstimatesLoading(false); } })
        .catch(() => { if (live) { setEstimates(new Map(destinations.map(path => [path, { path, copyBytes: 0, freeBytes: 0, reuseBytes: 0, tracksNew: 0, tracksChanged: 0, tracksKept: 0, tracksRemoved: 0, tracksMissing: 0, approximate: true, unknownSize: true, error: t("Could not calculate space. Refresh the device and try again.") }]))); setEstimatesLoading(false); } });
    }, 200);
    return () => { live = false; window.clearTimeout(timer); };
  }, [nodes, ticked, estimateDevices, capacityRevision, deleteUnlistedMusic, compatibilityFormat, reported, t]);
  const playlistCount = selectedCount === 1 ? t("{count} playlist", { count: selectedCount }) : t("{count} playlists", { count: selectedCount });
  const deviceCount = tickedDevices.size === 1 ? t("{count} USB device", { count: tickedDevices.size }) : t("{count} USB devices", { count: tickedDevices.size });
  const selectionSummary = t("{playlists} → {devices}", { playlists: playlistCount, devices: deviceCount });
  const selectionHint = selectedCount === 0 && tickedDevices.size === 0 ? t("Select playlists and a USB device.")
    : selectedCount === 0 ? t("Select playlists to sync.")
    : tickedDevices.size === 0 ? t("Select a USB device to sync to.")
    : t("Selected playlists will sync to each selected device.");
  const syncHint = rekordboxOpen
    ? t("Quit rekordbox to enable synchronization.")
    : rekordboxOpen === null
      ? t("Checking whether rekordbox is running…")
      : selectionHint;

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        const backend = await getBackend();
        const summary = await backend.librarySummary();
        if (live) setRekordboxOpen(summary.readOnly);
      } catch {
        // Preserve the last known state during a temporary backend failure.
      } finally {
        if (live) timer = setTimeout(() => void refresh(), 2000);
      }
    };
    void refresh();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, []);

  // The same tree the shell fetches, on open and again whenever the library
  // changes, so a playlist made while this is open can be ticked. Its
  // folders open one level deep, as rekordbox's manager opens them: the top
  // folders show, and what is inside them waits to be asked for. A re-read
  // keeps the folders as they were opened or closed; only folders it has
  // not seen before start closed.
  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    const seen = new Set<string>();
    // Each read is numbered so a slow one cannot overwrite a newer one.
    let latest = 0;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const read = async () => {
      const mine = ++latest;
      clearTimeout(retry);
      let waiting = false;
      try {
        const backend = await getBackend();
        const tree = await backend.playlistTree();
        if (!live || mine !== latest) return;
        const nodes = playlistNodes(tree);
        const fresh = nodes.filter((n) => n.kind === "folder" && n.depth > 1 && !seen.has(n.id)).map((n) => n.id);
        for (const node of nodes) seen.add(node.id);
        const present = new Set(nodes.map((n) => n.id));
        setTree(tree);
        setTreeError("");
        setLibraryLoading(false);
        setCollapsed((current) => new Set([...[...current].filter((id) => present.has(id)), ...fresh]));
        // A deleted playlist is not something SYNC can be asked for.
        setTicked((current) => {
          const kept = [...current].filter((id) => present.has(id));
          return kept.length === current.size ? current : new Set(kept);
        });
      } catch (e) {
        if (live && mine === latest) {
          waiting = errorMessage(e).includes(LIBRARY_LOADING);
          setLibraryLoading(waiting);
          if (waiting) retry = setTimeout(() => void read(), LIBRARY_RETRY_MS);
          else setTreeError("Couldn’t load playlists. Reopen Sync Manager to try again.");
        }
      } finally {
        if (live && mine === latest && !waiting) setLoadingTree(false);
      }
    };
    void read();
    void getBackend().then((backend) => {
      if (!live) return;
      stop = backend.onLibraryChanged(() => { void read(); });
    });
    return () => {
      live = false;
      clearTimeout(retry);
      stop?.();
    };
  }, []);

  const refreshDevices = useCallback(async () => {
    setLoadingDevices(true);
    setDevicesError("");
    try {
      const backend = await getBackend();
      const found = await backend.listDevices();
      setDevices(found);
      // A stick that was pulled is not a destination any more.
      const present = new Set(found.map((d) => d.path));
      setTickedDevices((current) => new Set([...current].filter((path) => present.has(path))));
      return found;
    } catch (e) {
      setDevicesError("Couldn’t read USB devices. Click Refresh to try again.");
      throw e;
    } finally { setLoadingDevices(false); }
  }, []);
  useEffect(() => {
    void refreshDevices().catch(() => {
      // No devices to list: the column says so.
    });
  }, [refreshDevices]);

  const firstReadDone = !loadingTree && !loadingDevices && rekordboxOpen !== null;
  useEffect(() => {
    if (firstReadDone) onReady?.();
  }, [firstReadDone, onReady]);

  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    void getBackend().then(backend => {
      if (!live) return;
      stop = backend.onDevicesChanged(() => {
        if (live) void refreshDevices().catch(() => {});
      });
    });
    return () => { live = false; stop?.(); };
  }, [refreshDevices]);

  useEffect(() => {
    window_.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  /** Ticks a stick's recorded selection back on, adding to what is ticked. */
  const restoreSelection = useCallback((read: DeviceSyncState) => {
    if (read.selected.length === 0) return;
    setTicked((current) => {
      const next = new Set(current);
      for (const playlist of read.selected) if (byId.has(playlist.libraryId)) next.add(playlist.libraryId);
      return next;
    });
  }, [byId]);

  /** Reads what a stick holds and was last given, and ticks that back on. */
  const readDevice = useCallback(async (path: string, restore: boolean) => {
    const backend = await getBackend();
    const read = await backend.deviceSyncState(path);
    setDeviceErrors(current => { const next = new Map(current); next.delete(path); return next; });
    setStates((current) => new Map(current).set(path, read));
    if (restore) restoreSelection(read);
  }, [restoreSelection]);

  const tickDevice = useCallback((path: string, on: boolean) => {
    setTickedDevices((current) => {
      const next = new Set(current);
      if (on) next.add(path);
      else next.delete(path);
      return next;
    });
    // Ticking a stick brings its last selection back: the union across
    // ticked sticks, so a second stick adds to the first's rather than
    // replacing it. Unticking takes nothing away.
    if (on) {
      void readDevice(path, true).catch(e => {
        setDeviceErrors(current => new Map(current).set(path, e instanceof Error ? e.message : "Could not read this USB device."));
      });
    }
  }, [readDevice]);

  // Each stick is read as soon as it is listed, so what it holds shows
  // without ticking or opening it (#178). A stick is read once per
  // appearance, not on every re-scan; one pulled and plugged back is new.
  const listed = useRef(new Set<string>());
  // A stick has appeared since one was last chosen for the window.
  const [choosePending, setChoosePending] = useState(false);
  useEffect(() => {
    const present = new Set(devices.map((d) => d.path));
    for (const path of listed.current) if (!present.has(path)) listed.current.delete(path);
    const fresh = devices.filter((d) => !listed.current.has(d.path)).map((d) => d.path);
    if (fresh.length === 0) return;
    for (const path of fresh) listed.current.add(path);
    // What was read from a path before it was pulled is not this stick's.
    setStates((current) => {
      if (!fresh.some((path) => current.has(path))) return current;
      const next = new Map(current);
      for (const path of fresh) next.delete(path);
      return next;
    });
    setChoosePending(true);
    for (const path of fresh) {
      void readDevice(path, false).catch(e => setDeviceErrors(current => new Map(current).set(path, e instanceof Error ? e.message : t("Could not read this USB device."))));
    }
  }, [devices, readDevice, t]);

  // rekordbox's Sync Manager opens on a device, choosing the first one whose
  // sync record it finds, and shows that record's selection ticked
  // (PlaylistSyncManager's constructor and handleCommonMessage, rekordbox
  // 7.2.19). Here the first stick with a record from this library is ticked
  // and its selection comes back, as ticking it by hand does. A stick with no
  // record is left unticked: nothing is guessed from names or paths, and
  // ticking it would only make it a destination. Nothing is chosen once a
  // stick is already ticked.
  useEffect(() => {
    if (!choosePending || loadingTree || loadingDevices || busy) return;
    if (devices.some((d) => !states.has(d.path) && !deviceErrors.has(d.path))) return;
    setChoosePending(false);
    if (tickedDevices.size > 0) return;
    const recorded = devices.find((d) => (states.get(d.path)?.selected ?? []).some((p) => byId.has(p.libraryId)));
    const read = recorded ? states.get(recorded.path) : undefined;
    if (!recorded || !read) return;
    setTickedDevices(new Set([recorded.path]));
    restoreSelection(read);
  }, [choosePending, loadingTree, loadingDevices, busy, devices, states, deviceErrors, tickedDevices, byId, restoreSelection]);

  const tickState = useCallback((node: TreeNode): Tick => {
    if (node.kind !== "folder") return ticked.has(node.id) ? "on" : "off";
    const under = playlistsUnder(nodes, node, byId);
    if (under.length === 0) return "off";
    const on = under.filter((id) => ticked.has(id)).length;
    return on === 0 ? "off" : on === under.length ? "on" : "some";
  }, [ticked, nodes, byId]);

  const tickNode = useCallback((node: TreeNode, on: boolean) => {
    const ids = node.kind === "folder" ? playlistsUnder(nodes, node, byId) : [node.id];
    setTicked((current) => {
      const next = new Set(current);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  }, [nodes, byId]);

  // The iTunes column: the same tree machinery as the rekordbox column, over
  // its own flat list of folders and playlists.
  const itunesNodes = useMemo<readonly TreeNode[]>(() => itunes?.tree ?? [], [itunes]);
  const itunesById = useMemo(() => new Map(itunesNodes.map((n) => [n.id, n] as const)), [itunesNodes]);
  const itunesPlaylists = useMemo(() => itunesNodes.filter((n) => n.kind === "playlist"), [itunesNodes]);
  const itunesVisible = useMemo(() => visibleNodes(itunesNodes, itunesCollapsed), [itunesNodes, itunesCollapsed]);
  const itunesSelectedCount = itunesPlaylists.filter((n) => itunesTicked.has(n.id)).length;

  const itunesTickState = useCallback((node: TreeNode): Tick => {
    if (node.kind !== "folder") return itunesTicked.has(node.id) ? "on" : "off";
    const under = playlistsUnder(itunesNodes, node, itunesById);
    if (under.length === 0) return "off";
    const on = under.filter((id) => itunesTicked.has(id)).length;
    return on === 0 ? "off" : on === under.length ? "on" : "some";
  }, [itunesTicked, itunesNodes, itunesById]);

  const itunesTickNode = useCallback((node: TreeNode, on: boolean) => {
    const ids = node.kind === "folder" ? playlistsUnder(itunesNodes, node, itunesById) : [node.id];
    setItunesTicked((current) => {
      const next = new Set(current);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      return next;
    });
  }, [itunesNodes, itunesById]);

  // A library's folders open one level deep, as the rekordbox column's do.
  const seedItunesCollapsed = (tree: readonly TreeNode[]) =>
    new Set(tree.filter((n) => n.kind === "folder" && n.depth > 1).map((n) => n.id));

  // The library at its usual place, read once on open. A miss is normal —
  // Music.app writes the XML only when sharing is on — and leaves the column
  // offering a file picker rather than showing an error.
  useEffect(() => {
    if (!showItunes) return;
    let live = true;
    setItunesLoading(true);
    void getBackend()
      .then((backend) => backend.itunesDefaultLibrary())
      .then((library) => {
        if (!live || !library) return;
        setItunes(library);
        setItunesCollapsed(seedItunesCollapsed(library.tree));
      })
      .catch(() => { if (live) setItunesError("Couldn’t read the iTunes library."); })
      .finally(() => { if (live) setItunesLoading(false); });
    return () => { live = false; };
  }, [showItunes]);

  const chooseItunes = useCallback(() => {
    if (busy) return;
    setItunesError("");
    setItunesLoading(true);
    void (async () => {
      try {
        const backend = await getBackend();
        const library = await backend.chooseItunesLibrary();
        if (!library) return;
        setItunes(library);
        setItunesTicked(new Set());
        setItunesCollapsed(seedItunesCollapsed(library.tree));
      } catch (e) {
        setItunesError(errorMessage(e));
      } finally {
        setItunesLoading(false);
      }
    })();
  }, [busy]);

  const canImportItunes = rekordboxOpen === false && itunes !== null && itunesSelectedCount > 0 && !busy;

  const importItunes = useCallback(() => {
    if (!itunes || !canImportItunes) return;
    // In tree order, so the playlists land filed as they are in iTunes.
    const ids = itunesNodes.filter((n) => isExportablePlaylist(n) && itunesTicked.has(n.id)).map((n) => n.id);
    setOperation("itunes");
    setImportFailed(false);
    setShowStatusDetails(false);
    setStatus([t("Importing from iTunes…")]);
    void (async () => {
      try {
        const backend = await getBackend();
        // Close the gap between the last poll and the click: rekordbox holds
        // the database, and importing writes to it.
        const summary = await backend.librarySummary();
        setRekordboxOpen(summary.readOnly);
        if (summary.readOnly) {
          setStatus([t("Quit rekordbox to import from iTunes.")]);
          return;
        }
        // rekordbox asks before replacing same-named lists (#152); Cancel
        // imports nothing.
        const report = await backend.importItunesSelected(itunes.path, ids, () => askToReplaceLists(backend, t));
        if (report === null) {
          setStatus([]);
          return;
        }
        // Show the imported playlists in the rekordbox column at once.
        setTree(await backend.playlistTree());
        setItunesTicked(new Set());
        const added = report.imported + report.existing;
        const lines = [`Imported ${report.playlists} playlists from iTunes (${added} tracks, ${report.imported} new).`];
        if (report.skipped.length > 0) lines.push(`Skipped ${report.skipped.length} tracks.`, ...report.skipped);
        setStatus(lines);
      } catch (e) {
        setImportFailed(true);
        setStatus([errorMessage(e)]);
      } finally {
        setOperation(null);
      }
    })();
  }, [itunes, canImportItunes, itunesNodes, itunesTicked, t]);

  const canSync = rekordboxOpen === false && selectedCount > 0 && tickedDevices.size > 0 && !busy && !loadingDevices;

  /** Formatting needs diskutil, so only macOS offers it. */
  const canFormat = useMemo(() => detectPlatform().mac, []);

  /** Rejects with the failure, so the dialog that asked can show it. */
  const formatDevice = async (device: Device, layout: FormatLayout) => {
    setOperation("format");
    setStatus([t("Formatting {device}…", { device: device.name })]);
    try {
      const backend = await getBackend();
      await backend.formatDevice(device.path, layout);
      // A FAT volume comes back upper-cased, so it may mount somewhere new:
      // what was read from the old path is gone either way.
      const forget = <T,>(current: ReadonlyMap<string, T>) => { const next = new Map(current); next.delete(device.path); return next; };
      setTickedDevices(current => new Set([...current].filter(path => path !== device.path)));
      setCollapsedDevices(current => new Set([...current].filter(path => path !== device.path)));
      setStates(forget);
      setDeviceErrors(forget);
      setCompletedReports(forget);
      // Read again when it is listed, wherever it mounts.
      listed.current.delete(device.path);
      await refreshDevices().catch(() => {});
      setStatus([t("{device}: Formatted.", { device: device.name })]);
      onSynced?.();
    } catch (e) {
      setStatus([t("{device}: Could not format. {reason}", { device: device.name, reason: errorMessage(e) })]);
      throw e;
    } finally {
      setOperation(null);
    }
  };

  const ejectDevice = async (device: Device) => {
    if (busy || loadingDevices) return;
    setOperation("eject");
    setEjectingPath(device.path);
    setStatus([`Ejecting ${device.name}…`]);
    try {
      const backend = await getBackend();
      await backend.ejectDevice(device.path);
      setDevices(current => current.filter(d => d.path !== device.path));
      setTickedDevices(current => new Set([...current].filter(path => path !== device.path)));
      setCollapsedDevices(current => new Set([...current].filter(path => path !== device.path)));
      setStates(current => { const next = new Map(current); next.delete(device.path); return next; });
      setDeviceErrors(current => { const next = new Map(current); next.delete(device.path); return next; });
      setStatus([`${device.name}: Safely ejected.`]);
      onSynced?.();
    } catch (e) {
      setStatus([`${device.name}: Could not eject. ${errorMessage(e)}`]);
    } finally {
      setEjectingPath(null);
      setOperation(null);
    }
  };

  const sync = useCallback(() => {
    if (!canSync) return;
    // In tree order, so the playlists land on the stick as they are filed.
    const playlists = nodes.filter((n) => isExportablePlaylist(n) && ticked.has(n.id)).map((n) => n.id);
    setOperation("sync");
    setCompletedReports(new Map());
    setStatus([t("Preparing for export…")]);
    setStatusDetail([]);
    void (async () => {
      let stop = () => {};
      try {
        const backend = await getBackend();
        // The device list can go stale between the last mount notification and
        // this click. Read it immediately before any import or export so a
        // stale /Volumes/name path cannot be handed to the sync workers.
        const currentDevices = await refreshDevices();
        const destinations = currentDevices.filter((d) => tickedDevices.has(d.path)).map((d) => d.path);
        const nameOf = (path: string) => currentDevices.find((d) => d.path === path)?.name ?? path;
        if (destinations.length === 0) {
          setStatus(["The selected USB device is no longer connected. Refresh and select it again."]);
          return;
        }
        // Close the interval between the last poll and the click: rekordbox
        // may have launched while the button was still visibly enabled.
        const summary = await backend.librarySummary();
        setRekordboxOpen(summary.readOnly);
        if (summary.readOnly) {
          setStatus([t("Quit rekordbox to enable synchronization.")]);
          return;
        }
        const missing = await backend.validateExportFiles(playlists);
        if (missing.length > 0) {
          const proceed = await new Promise<boolean>(resolve => setMissingPrompt({ files: missing, resolve }));
          setMissingPrompt(null);
          if (!proceed) {
            setStatus([t("Export cancelled because files are missing.")]);
            return;
          }
        }
        // SYNC only writes to the sticks; nothing on them comes back into the
        // library unless Import is clicked (#199).
        const ineligible = new Map<string, string>();
        for (const path of destinations) {
          if (estimates.get(path)?.enoughSpace === false) ineligible.set(path, t("Not enough temporary space to sync safely"));
        }
        const eligible = destinations.filter(path => !ineligible.has(path));
        if (eligible.length === 0) {
          setStatus([...ineligible].map(([path, message]) => `${nameOf(path)}: ${message}`));
          return;
        }
        stop = backend.onSyncProgress((progress) => {
          if (progress.state === "writing") setStatus([t("Writing to {device}…", { device: nameOf(progress.path) })]);
          if (progress.state === "ejecting") setStatus([t("Ejecting {device}…", { device: nameOf(progress.path) })]);
        });
        const reports: SyncDeviceReport[] = [
          ...await backend.syncDevices(playlists, eligible, stickDefaults, false, ejectAfterSync, deleteUnlistedMusic, compatibilityFormat, verifyAll),
          ...[...ineligible].map(([path, error]) => ({ path, error })),
        ];
        setCompletedReports(new Map(reports.flatMap(r => r.report ? [[r.path, r.report] as const] : [])));
        const outcomes = reports.flatMap(r => r.error
          ? [`${nameOf(r.path)}: ${r.error}`]
          : r.ejected ? [`${nameOf(r.path)}: Safely ejected.`]
          : r.ejectError ? [`${nameOf(r.path)}: Not ejected: ${r.ejectError}`] : []);
        const hasWarnings = reports.some(r => r.report && ((r.report.warnings?.length ?? 0) + r.report.skipped.length + r.report.failed.length > 0));
        setStatus(outcomes.length > 0 ? outcomes : [hasWarnings ? t("Sync complete with warnings.") : t("Sync complete.")]);
        setStatusDetail(reports.flatMap(r => [
          ...(r.report?.skipped ?? []).map(track => `${nameOf(r.path)}: ${t("Missing or unreadable")}: ${track}`),
          ...(r.report?.failed ?? []).map(track => `${nameOf(r.path)}: ${t("Could not be written")}: ${track}`),
          ...(r.report?.warnings ?? []).map(warning => `${nameOf(r.path)}: ${warning.reason === "source-unavailable"
            ? t("{track}: source unavailable; existing USB version kept", { track: warning.title })
            : warning.reason === "analysis-kept" ? t("{track}: source analysis unavailable; existing USB analysis kept", { track: warning.title })
            : warning.reason === "analysis-unavailable" ? t("{track}: exported without analysis", { track: warning.title })
            : warning.reason === "analysis-stale" ? t("{track}: audio changed; reanalyze it before exporting waveforms and cues", { track: warning.title })
            : t("{track}: could not update; existing USB version kept", { track: warning.title })}${warning.detail ? ` · ${warning.detail}` : ""}`),
          ...(r.report && !r.report.verified ? [`${nameOf(r.path)}: ${t("The written library did not read back correctly.")}`] : []),
        ]));
        // A track that could not be written does not stop the sync; tell the
        // user which ones were left out and why.
        const failures = reports.flatMap(r => (r.report?.failed ?? []).map(f => `${nameOf(r.path)}: ${f}`));
        if (failures.length > 0) {
          const shown = failures.slice(0, MAX_FAILURES_SHOWN);
          const more = failures.length - shown.length;
          void backend.tell(
            [t("The sync finished, but these tracks could not be written and were left out:"), ...shown,
              ...(more > 0 ? [t("…and {count} more. See the log for the full list.", { count: more })] : [])].join("\n\n"),
            t("Some tracks were left out"),
          );
        }
        // What the sticks hold now, without touching the ticks.
        await refreshDevices().catch(() => {});
        await Promise.all(reports.filter(r => !r.ejected).map(({ path }) => readDevice(path, false).catch(() => {})));
        onSynced?.();
      } catch (e) {
        setStatus([e instanceof Error || (e && typeof e === "object" && "message" in e) || typeof e === "string" ? errorMessage(e) : t("The sync could not be written.")]);
      } finally {
        stop();
        setOperation(null);
      }
    })();
  }, [canSync, nodes, ticked, tickedDevices, estimates, stickDefaults, ejectAfterSync, deleteUnlistedMusic, compatibilityFormat, verifyAll, refreshDevices, readDevice, onSynced, t]);

  /** One line for a cue import: what changed, and what already matched. */
  const cuesResult = (device: string, result: { tracks: number; skipped: number; unchanged?: number }) => {
    const unchanged = result.unchanged ?? 0;
    if (result.tracks === 0 && unchanged > 0 && result.skipped === 0) {
      return t("{device}: cues and beat grids already match your library; nothing was changed.", { device });
    }
    const parts = [result.tracks === 1
      ? t("{device}: updated {count} track", { device, count: result.tracks })
      : t("{device}: updated {count} tracks", { device, count: result.tracks })];
    if (unchanged > 0) parts.push(t("{count} already up to date", { count: unchanged }));
    if (result.skipped > 0) parts.push(t("skipped {count}", { count: result.skipped }));
    return `${parts.join("; ")}.`;
  };

  const runUsbImport = (kinds: readonly ImportKind[], notes: readonly string[] = []) => {
    if (busy || tickedDevices.size === 0 || kinds.length === 0) return;
    setOperation("import");
    setLastImport(kinds);
    setImportFailed(false);
    setShowStatusDetails(false);
    setStatus([]);
    void (async () => {
      try {
        const backend = await getBackend();
        // No question first: clicking Import is the request, and what it
        // brings back was chosen in Preferences. rekordbox may have opened
        // since the last poll, though, and it holds the database.
        if (kinds.includes("ratings") && (await backend.librarySummary()).readOnly) throw new Error(t("Quit rekordbox to import track ratings."));
        // What was chosen but cannot be imported now, and why.
        const results: string[] = [...notes];
        for (const device of devices.filter(d => tickedDevices.has(d.path))) {
          // One kind at a time, so each is reported on its own and one that
          // fails does not keep the others from being brought in.
          for (const { kind, noun } of IMPORT_KINDS.filter(({ kind }) => kinds.includes(kind))) {
            setStatus([t("Waiting for USB activity to finish, then importing {kind} from {device}…", { kind: t(noun), device: device.name })]);
            try {
              const result = await backend.importUsb(device.path, kind === "cues", kind === "history", kind === "settings", kind === "ratings");
              if (kind === "cues") results.push(cuesResult(device.name, result));
              else if (kind === "history") results.push(result.histories === 1
                ? t("{device}: imported {count} play-history entry.", { device: device.name, count: result.histories })
                : result.histories
                  ? t("{device}: imported {count} play-history entries.", { device: device.name, count: result.histories })
                  : t("{device}: no new play-history entries.", { device: device.name }));
              else if (kind === "ratings") results.push(result.ratings === 1
                ? t("{device}: imported {count} track rating.", { device: device.name, count: result.ratings })
                : result.ratings
                  ? t("{device}: imported {count} track ratings.", { device: device.name, count: result.ratings })
                  : t("{device}: no new track ratings.", { device: device.name }));
              // Kept as RBXport's My Settings: a stick synced later that has
              // none of its own is given them, as rekordbox's imported My
              // Settings go to the sticks it writes. Nothing in the library
              // changes, so say where they went rather than imply an update.
              else results.push(result.settings === 1
                ? t("{device}: imported {count} CDJ/mixer settings file. Sync gives it to USB devices that have no settings of their own.", { device: device.name, count: result.settings })
                : result.settings
                  ? t("{device}: imported {count} CDJ/mixer settings files. Sync gives them to USB devices that have no settings of their own.", { device: device.name, count: result.settings })
                  : t("{device}: no CDJ/mixer settings files found.", { device: device.name }));
              if (result.warnings?.length) results.push(...result.warnings.map(warning => `${device.name}: ${warning}`));
            } catch (e) {
              setImportFailed(true);
              results.push(`${device.name}: Couldn’t import ${noun}. ${errorMessage(e)}`);
            }
          }
        }
        setStatus(results);
        onSynced?.();
      } catch (e) {
        setImportFailed(true);
        setStatus([errorMessage(e)]);
      }
      finally { setOperation(null); }
    })();
  };

  /** The USB Export pane, where what Import brings back is chosen. */
  const openUsbExportPreferences = () => {
    void getBackend()
      .then(backend => backend.openPreferences("usbExport"))
      .catch(() => false)
      .then(opened => { if (!opened) onOpenPreferences?.("usbExport"); });
  };

  const body = (
    <div
      ref={window_}
      className={styles.window}
      data-windowed={windowed || undefined}
      // The backdrop closes on click; the window must not pass its own through.
      onMouseDown={(e) => e.stopPropagation()}
      role="dialog"
      aria-modal={windowed ? undefined : "true"}
      aria-label="Sync Manager"
      tabIndex={-1}
    >
      <header
        className={styles.titlebar}
        onMouseDown={windowed ? startWindowDrag : undefined}
        onDoubleClick={windowed ? toggleWindowMaximise : undefined}
      >
        {windowed ? null : (
          <button type="button" className={styles.close} onClick={onClose} aria-label="Close">
            ✕
          </button>
        )}
        Sync Manager
      </header>
      <div className={styles.body}>
        {showItunes ? <>
        <section className={styles.column} aria-label="iTunes">
          <div className={styles.headingRow}>
            <div>
              <h2 className={styles.heading}>iTunes</h2>
              <p className={styles.columnNote}>
                {itunes ? `${itunesSelectedCount} of ${itunesPlaylists.length} playlists selected` : "No iTunes library"}
              </p>
            </div>
            <button type="button" className={styles.refresh} disabled={busy || itunesLoading} onClick={chooseItunes}>
              {itunes ? "Change…" : "Choose…"}
            </button>
          </div>
          <div className={styles.list} role="tree" aria-label="iTunes playlists">
            {itunesVisible.map((node) => {
              const folder = node.kind === "folder";
              const open = !itunesCollapsed.has(node.id);
              const state = itunesTickState(node);
              return (
                <div
                  key={node.id}
                  className={styles.row}
                  role="treeitem"
                  aria-expanded={folder ? open : undefined}
                  aria-selected={state === "on"}
                  style={{ paddingLeft: `${8 + (node.depth - 1) * 18}px` }}
                >
                  <button
                    type="button"
                    className={styles.twisty}
                    data-open={folder && open ? "" : undefined}
                    data-leaf={folder ? undefined : ""}
                    aria-label={folder ? (open ? `Collapse ${node.name}` : `Expand ${node.name}`) : undefined}
                    tabIndex={folder ? 0 : -1}
                    onClick={() => folder && setItunesCollapsed((current) => toggle(current, node.id))}
                  />
                  <label className={styles.rowSelection}>
                    {folder ? <FolderIcon className={styles.icon} /> : <ListIcon className={styles.icon} />}
                    <span className={styles.name}>{node.name}</span>
                    <TickBox state={state} label={node.name} disabled={busy} onChange={(on) => itunesTickNode(node, on)} />
                  </label>
                </div>
              );
            })}
            {itunesLoading ? <p className={styles.empty}>Reading iTunes library…</p>
              : itunesError ? <p className={styles.empty} role="alert">{itunesError}</p>
              : !itunes ? <p className={styles.empty}>No iTunes or Music library was found. Turn on “Share Library XML with other applications” in Music, then choose the file.</p>
              : itunesNodes.length === 0 ? <p className={styles.empty}>This iTunes library has no playlists.</p> : null}
          </div>
        </section>

        <div className={styles.middleSlim}>
          <button
            type="button"
            className={styles.sync}
            onClick={importItunes}
            disabled={!canImportItunes}
            title={rekordboxOpen ? "Quit rekordbox to import from iTunes." : undefined}
            aria-label="Import selected iTunes playlists"
            aria-busy={operation === "itunes" || undefined}
          >
            {operation === "itunes" ? <><LoaderCircle size={16} className={styles.spinner} aria-hidden="true" /> Importing…</> : <>SYNC <ArrowRight size={16} aria-hidden="true" /></>}
          </button>
        </div>
        </> : null}

        <section className={styles.column} aria-label="rbxport">
          <div className={styles.headingRow}>
            <div><h2 className={styles.heading}>rbxport</h2><p className={styles.columnNote}>{selectedCount} of {playlists.length} playlists selected</p></div>
            <button type="button" className={styles.textButton} disabled={busy || selectedCount === 0}
              onClick={() => setTicked(new Set())}>Clear selection</button>
          </div>
          <div className={styles.search}>
            <Search size={14} aria-hidden="true" />
            <input type="search" aria-label="Search playlists" placeholder="Search playlists" value={query} onChange={e => setQuery(e.currentTarget.value)} />
            {query ? <button type="button" aria-label="Clear playlist search" onClick={() => setQuery("")}><X size={14} aria-hidden="true" /></button> : null}
          </div>
          <div className={styles.list} role="tree" aria-label="Playlists">
            {/* rekordbox roots this column at [Playlists], whose box ticks or
                clears every playlist at once. A search lists matches flat,
                so the root is not shown then. */}
            {!search && nodes.length > 0 ? (
              <div
                className={styles.row}
                role="treeitem"
                aria-expanded={allOpen}
                aria-selected={allState === "on"}
                style={{ paddingLeft: "8px" }}
              >
                <button
                  type="button"
                  className={styles.twisty}
                  data-open={allOpen ? "" : undefined}
                  aria-label={allOpen ? `Collapse ${t("Playlists")}` : `Expand ${t("Playlists")}`}
                  onClick={() => setAllOpen((open) => !open)}
                />
                <label className={styles.rowSelection}>
                  <FolderIcon className={styles.icon} />
                  <span className={styles.name}>{t("Playlists")}</span>
                  <TickBox state={allState} label={t("Playlists")} disabled={busy || playlists.length === 0}
                    onChange={(on) => setTicked(on ? new Set(playlists.map((n) => n.id)) : new Set())} />
                </label>
              </div>
            ) : null}
            {visible.map((node) => {
              const folder = node.kind === "folder";
              const open = !collapsed.has(node.id);
              const state = tickState(node);
              return (
                <div
                  key={node.id}
                  className={styles.row}
                  role="treeitem"
                  aria-expanded={folder ? open : undefined}
                  aria-selected={state === "on"}
                  style={{ paddingLeft: `${8 + (search ? 0 : node.depth) * 18}px` }}
                >
                  <button
                    type="button"
                    className={styles.twisty}
                    data-open={folder && open ? "" : undefined}
                    data-leaf={folder ? undefined : ""}
                    aria-label={folder ? (open ? `Collapse ${node.name}` : `Expand ${node.name}`) : undefined}
                    tabIndex={folder ? 0 : -1}
                    onClick={() => folder && setCollapsed((current) => toggle(current, node.id))}
                  />
                  <label className={styles.rowSelection}>
                  {folder ? <FolderIcon className={styles.icon} />
                    : node.kind === "smartPlaylist" ? <SmartListIcon className={styles.icon} />
                    : <ListIcon className={styles.icon} />}
                  <span className={styles.name}>{node.name}</span>
                  <TickBox state={state} label={node.name} disabled={busy} onChange={(on) => tickNode(node, on)} />
                  </label>
                </div>
              );
            })}
            {libraryLoading ? <p className={styles.empty} role="status">{t("Please wait (loading database)…")}</p>
              : loadingTree ? <p className={styles.empty}>Loading playlists…</p>
              : treeError ? <p className={styles.empty} role="alert">{treeError}</p>
              : nodes.length === 0 ? <p className={styles.empty}>No playlists yet. Create a playlist in your library to get started.</p>
              : visible.length === 0 ? <p className={styles.empty}>No playlists match “{query.trim()}”.</p> : null}
          </div>
        </section>

        <div className={styles.middle}>
          <div className={styles.syncActions}>
          <div className={styles.actionRow}>
          <button
            type="button"
            className={`${styles.sync} ${styles.actionButton}`}
            onClick={sync}
            disabled={!canSync}
            title={rekordboxOpen ? "Quit rekordbox to enable synchronization." : undefined}
            aria-label="SYNC"
            aria-busy={busy || undefined}
            aria-describedby="sync-selection-hint"
          >
            {operation === "sync" ? <><LoaderCircle size={16} className={styles.spinner} aria-hidden="true" /> Syncing…</> : <>SYNC <ArrowRight size={16} aria-hidden="true" /></>}
          </button>
          <button type="button" className={`${styles.button} ${styles.actionPreferences}`} onClick={openUsbExportPreferences}
            aria-label={t("Change sync preferences")} title={t("Change sync preferences")}>
            <Settings2 size={14} aria-hidden="true" />
          </button>
          </div>
          <label className={styles.option}>
            <input type="checkbox" className={styles.tick} checked={ejectAfterSync} disabled={busy}
              aria-label="Eject after syncing" onChange={e => setEjectAfterSync(e.currentTarget.checked)} />
            Eject after syncing
          </label>
          </div>
          <div className={styles.importActions}>
          <div className={styles.actionRow}>
          <button type="button" className={`${styles.sync} ${styles.actionButton}`} onClick={() => runUsbImport(importKinds, importBlockedNotes)}
            disabled={busy || tickedDevices.size === 0 || importKinds.length === 0}
            title={importTitle}>
            {operation === "import" ? <><LoaderCircle size={16} className={styles.spinner} aria-hidden="true" /> {t("Importing…")}</> : <><ArrowLeft size={16} aria-hidden="true" /> {t("Import")}</>}
          </button>
          <button type="button" className={`${styles.button} ${styles.actionPreferences}`} onClick={openUsbExportPreferences}
            aria-label={t("Change import preferences")} title={t("Change import preferences")}>
            <Settings2 size={14} aria-hidden="true" />
          </button>
          </div>
          {importFailed && lastImport ? <button type="button" className={styles.button} onClick={() => runUsbImport(lastImport)} disabled={busy || tickedDevices.size === 0}>
            Retry import
          </button> : null}
          </div>
        </div>

        <section className={styles.column} aria-label="Device">
          <div className={styles.headingRow}>
            <div><h2 className={styles.heading}>Device</h2><p className={styles.columnNote}>{tickedDevices.size} of {devices.length} selected</p></div>
            <button
              type="button"
              className={styles.refresh}
              onClick={() => void refreshDevices().catch(() => {})}
              disabled={busy || loadingDevices}
            >
              {loadingDevices ? "Refreshing…" : "Refresh"}
            </button>
          </div>
          {devicesError ? <p className={styles.empty} role="alert">{devicesError}</p> : null}
          <div className={styles.list} role="tree" aria-label="Devices">
            {devices.map((device) => {
              const on = tickedDevices.has(device.path);
              const expanded = !collapsedDevices.has(device.path);
              const read = states.get(device.path);
              const job = exportJobs.get(device.path);
              const fileSystem = device.fileSystem?.toUpperCase().replace(/^VFAT$|^MSDOS$/, "FAT") || "Unknown filesystem";
              const free = device.totalBytes > 0 ? `${device.freeBytes === 0 ? "0.0 GB" : formatSpace(device.freeBytes)} free (${Math.round(device.freeBytes / device.totalBytes * 100)}%)` : "Space unknown";
              const freePercent = device.totalBytes > 0 ? Math.max(0, Math.min(100, device.freeBytes / device.totalBytes * 100)) : null;
              const usedPercent = freePercent === null ? null : 100 - freePercent;
              const used = device.totalBytes > 0 ? formatSpace(Math.max(0, device.totalBytes - device.freeBytes)) || "0.0 GB" : "";
              // What the ticked playlists would add, net of what the sync
              // takes off: the stick's own copies are already in "used".
              const estimate = on ? estimates.get(device.path) : undefined;
              const net = estimate?.netAdditionalBytes ?? (estimate ? estimate.copyBytes - estimate.freeBytes : 0);
              const adding = Math.max(0, net);
              const freeing = Math.max(0, -net);
              const projectedFree = estimate?.projectedFreeBytes ?? Math.max(0, device.freeBytes - net);
              const retainedPercent = usedPercent === null ? null : Math.max(0, usedPercent - freeing / device.totalBytes * 100);
              const addPercent = usedPercent !== null && device.totalBytes > 0 ? Math.min(100 - usedPercent, adding / device.totalBytes * 100) : 0;
              const overBy = adding - device.freeBytes;
              const approx = estimate?.approximate ? "~" : "";
              const estimateTitle = estimate ? [
                t("{count} tracks to copy ({size})", { count: estimate.tracksNew + estimate.tracksChanged, size: formatSpace(estimate.copyBytes) || "0 MB" }),
                t("{count} already on the stick ({size})", { count: estimate.tracksKept, size: formatSpace(estimate.reuseBytes) || "0 MB" }),
                t("{count} to remove ({size})", { count: estimate.tracksRemoved, size: formatSpace(estimate.freeBytes) || "0 MB" }),
              ].join("\n") : undefined;
              return <div key={device.path} className={styles.device} role="treeitem" aria-expanded={expanded} data-ticked={on || undefined}>
                <div className={styles.row}>
                  <button type="button" className={styles.twisty} data-open={expanded ? "" : undefined}
                    aria-label={`${expanded ? "Collapse" : "Expand"} ${device.name}`} disabled={busy} onClick={() => {
                      setCollapsedDevices(current => toggle(current, device.path));
                      if (!expanded) void readDevice(device.path, false).catch(e => setDeviceErrors(current => new Map(current).set(device.path, e instanceof Error ? e.message : "Could not read this USB device.")));
                    }} />
                  <label className={styles.rowSelection}>
                  <span className={styles.name} title={device.path}>{device.name}</span>
                  <TickBox state={on ? "on" : "off"} label={device.name} disabled={busy} onChange={(next) => tickDevice(device.path, next)} />
                  </label>
                  <button type="button" className={styles.ejectButton}
                    aria-label={`Eject ${device.name}`} title={`Safely eject ${device.name}`}
                    disabled={busy || loadingDevices} aria-busy={ejectingPath === device.path || undefined}
                    onClick={() => void ejectDevice(device)}>
                    {ejectingPath === device.path
                      ? <LoaderCircle size={14} className={styles.spinner} aria-hidden="true" />
                      : <EjectIcon />}
                  </button>
                </div>
                <div className={styles.storage}>
                  <p className={styles.capacity}>{fileSystem}{device.totalBytes > 0 ? ` · ${formatSpace(device.totalBytes)} total` : ""}</p>
                  <div className={styles.spaceBar} role={freePercent === null ? "img" : "meter"}
                    aria-label={`${device.name} storage used`} aria-valuemin={usedPercent === null ? undefined : 0}
                    aria-valuemax={usedPercent === null ? undefined : 100} aria-valuenow={retainedPercent === null ? undefined : Math.min(100, retainedPercent + addPercent)}
                    aria-valuetext={usedPercent === null ? t("Space unknown") : estimate ? t("{used} used now; {free} left after sync", { used, free: `${approx}${formatSpace(projectedFree) || "0 MB"}` }) : t("{used} used; {free}", { used, free })} title={estimateTitle ? `${free}\n${estimateTitle}` : free}>
                    {retainedPercent !== null ? <span style={{ width: `${retainedPercent}%` }} /> : null}
                    {addPercent > 0 ? <span className={styles.pending} data-over={overBy > 0 || undefined} style={{ width: `${addPercent}%` }} /> : null}
                  </div>
                  <div className={styles.storageLabels}>
                    {usedPercent !== null ? <span><i aria-hidden="true" />{used} used</span> : null}
                    {adding > 0 ? <span className={styles.pendingLabel} data-over={overBy > 0 || undefined}><i aria-hidden="true" />{overBy > 0
                      ? t("{size} to add, {over} more than fits", { size: `${approx}${formatSpace(adding)}`, over: formatSpace(overBy) })
                      : t("{size} to add", { size: `${approx}${formatSpace(adding)}` })}</span> : null}
                    {adding === 0 && freeing > 0 ? <span>{t("Frees {size}", { size: formatSpace(freeing) })}</span> : null}
                    <span>{free}</span>
                  </div>
                  {on && estimatesLoading ? <p aria-live="polite">{t("Calculating space…")}</p> : estimate?.error ? <p aria-live="polite">{estimate.error}</p> : estimate ? <p aria-live="polite">
                    {estimate.unknownSize ? t("Space estimate is incomplete") : t("{space} left after sync", { space: `${approx}${formatSpace(projectedFree) || "0 MB"}` })}
                    {estimate.peakBytes !== undefined ? ` · ${t("Needs {space} free while syncing", { space: `${approx}${formatSpace(estimate.peakBytes) || "0 MB"}` })}` : ""}
                    {estimate.enoughSpace === false ? <span role="alert"> · {t("Not enough temporary space to sync safely")}</span> : null}
                    {(estimate.keptExisting ?? 0) > 0 ? ` · ${t("{count} existing tracks kept because their sources are unavailable", { count: estimate.keptExisting ?? 0 })}` : ""}
                  </p> : null}
                  {job ? <div className={styles.exportProgress} data-state={job.state}>
                    <ExportProgressDetails job={job} name={device.name} buttonClass={styles.button} />
                    {job.state === "failed" ? <span className={styles.exportError} role="alert">
                      {job.title || "The export failed before the device could be verified. Check that it is connected, writable, and has enough free space."}
                    </span> : null}
                  </div> : null}
                  {completedReports.has(device.path) ? (() => {
                    const report = completedReports.get(device.path)!;
                    return <div className={styles.exportReport} aria-label={`${device.name} export report`}>
                      <span><strong>{report.tracksAdded}</strong> tracks added</span>
                      <span><strong>{report.removed}</strong> tracks removed</span>
                      <span><strong>{report.tracksUpdated}</strong> tracks updated</span>
                      <span><strong>{Math.max(0, report.tracks - report.tracksAdded - report.tracksUpdated)}</strong> unchanged</span>
                      {report.skipped.length > 0 ? <span className={styles.exportWarning}><strong>{report.skipped.length}</strong> missing</span> : null}
                      {(report.keptExisting ?? 0) > 0 ? <span className={styles.exportWarning}>{t("{count} existing versions kept", { count: report.keptExisting ?? 0 })}</span> : null}
                      {(report.converted ?? 0) > 0 ? <span>{t("{count} tracks converted", { count: report.converted ?? 0 })}</span> : null}
                      {report.upToDate ? <span>{t("Already up to date")}</span> : null}
                    </div>;
                  })() : null}
                </div>
                {!expanded && deviceErrors.has(device.path) ? <p className={styles.capacity} role="alert">{deviceErrors.get(device.path)}</p> : null}
                {expanded ? <div className={styles.library} role="group" aria-label={`${device.name} library`}>
                  {deviceErrors.has(device.path) ? <p role="alert">{deviceErrors.get(device.path)}</p> : read === undefined ? <div className={styles.libraryNote}>Reading…</div>
                    : <DeviceLibraries libraries={read.libraries ?? [{ name: "Device Library", nodes: read.onDevice.map((name, i) => ({ id: String(i+1), parentId: "0", name, folder: false })) }]} />}
                  {canFormat ? <button type="button" className={`${styles.button} ${styles.formatButton}`} disabled={busy}
                    onClick={() => setFormatTarget(device)}>{t("Format USB…")}</button> : null}
                </div> : null}
              </div>;
            })}
            {devices.length === 0 ? <p className={styles.empty}>{loadingDevices ? "Looking for USB devices…" : "Connect a USB device, then click Refresh."}</p> : null}
          </div>
        </section>
      </div>
      <footer className={styles.footer}>
        <div className={styles.status} data-error={importFailed || undefined} role={importFailed ? "alert" : "status"} aria-live="polite">
          {busy ? <LoaderCircle size={16} className={styles.spinner} aria-hidden="true" /> : null}
          <div className={showStatusDetails ? styles.statusDetails : undefined} title={status.join("\n")}>
          {status.length > 0 ? status.join(" · ") : null}
          {showStatusDetails && statusDetail.length > 0
            ? <ul className={styles.statusList}>{statusDetail.map((line, i) => <li key={i}>{line}</li>)}</ul> : null}
          {status.length === 0 ? <span className={styles.selectionSummary}>{selectionSummary}</span> : null}
          {status.length === 0 ? <span id="sync-selection-hint" className={styles.idleStatus} data-warn={rekordboxOpen || undefined}>{syncHint}</span> : null}
          </div>
          {status.length > 1 || importFailed || statusDetail.length > 0 ? <button type="button" className={styles.detailsButton} onClick={() => setShowStatusDetails(open => !open)}>
            {showStatusDetails ? "Hide details" : "Show details"}
          </button> : null}
        </div>
        {status.length > 0 ? <span id="sync-selection-hint" hidden>{syncHint}</span> : null}
        {[...exportJobs.values()].filter(job => isExportActive(job) && canStopExport(job)).length > 1 ? <button type="button" className={styles.button}
          onClick={() => void getBackend().then(backend => Promise.all([...exportJobs.values()].filter(job => isExportActive(job) && canStopExport(job)).map(job => backend.cancelExport(job.path))))
            .catch(() => setStatus([t("Could not stop the export. Try again.")]))}>
          {t("Stop all exports")}
        </button> : null}
        <button type="button" className={styles.button} onClick={onClose}>
          {busy ? "Run in background" : "Close"}
        </button>
      </footer>
    </div>
  );

  const dialogs = <>
    {missingPrompt ? <MissingFilesDialog files={missingPrompt.files} onChoose={missingPrompt.resolve} /> : null}
    {formatTarget ? <FormatDeviceDialog
      device={formatTarget}
      onFormat={layout => formatDevice(formatTarget, layout)}
      onClose={() => setFormatTarget(null)}
    /> : null}
  </>;

  if (windowed) return <>{body}{dialogs}</>;
  return (
    <>
      <div className={styles.backdrop} onMouseDown={onClose} role="presentation">
        {body}
      </div>
      {dialogs}
    </>
  );
}

function DeviceLibraries({ libraries }: { libraries: NonNullable<DeviceSyncState["libraries"]> }) {
  if (libraries.length === 0) return <p className={styles.libraryNote}>No libraries on this device yet.</p>;
  return <>{libraries.map(library => {
    const children = (parent: string, ancestors: Set<string>): React.ReactNode => library.nodes
      .filter(node => node.parentId === parent && !ancestors.has(node.id))
      .map(node => node.folder ? <details key={node.id} open role="treeitem">
        <summary><FolderIcon className={styles.icon} /> {node.name}</summary>
        <div role="group">{children(node.id, new Set([...ancestors, node.id]))}</div>
      </details> : <div key={node.id} className={styles.playlistLeaf} role="treeitem"><ListIcon className={styles.icon} /> {node.name}</div>);
    return <details key={library.name} open role="treeitem">
      <summary>{library.name}</summary>
      <div role="group"><details open role="treeitem">
        <summary><FolderIcon className={styles.icon} /> Playlists</summary>
        <div role="group">{library.nodes.length ? children("0", new Set()) : <p className={styles.libraryNote}>No playlists on this device yet.</p>}</div>
      </details></div>
    </details>;
  })}</>;
}
