/**
 * The Sync Manager: which playlists go to which sticks.
 *
 * Three columns, as rekordbox's has — iTunes, rekordbox, Device — with a SYNC
 * button between each pair. The left SYNC imports the ticked iTunes playlists
 * into the library; the right SYNC writes the ticked library playlists to every
 * ticked device, so two sticks synced together are the same stick twice.
 *
 * The iTunes column reads Music.app's shared `Library.xml` (auto-detected, or
 * chosen from a file dialog) and imports only the playlists that are ticked.
 *
 * Nothing here holds the library: the tree is the same flat list the shell
 * fetches, and every count and name on a device comes from the backend
 * reading the stick.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, ArrowLeft, ArrowRight, LoaderCircle, Search, X } from "lucide-react";

import { EjectIcon, FolderIcon, ListIcon, SmartListIcon } from "@/components/icons";
import { getBackend } from "@/ipc/client";
import type { Device, DeviceSyncState, ExportReport, ItunesLibrary, TreeNode } from "@/ipc/types";
import { formatSpace } from "@/lib/devices";
import { errorMessage } from "@/lib/errorMessage";
import { nodesForSource, subtreeIds, toggle, visibleNodes } from "@/lib/tree";
import { startWindowDrag, toggleWindowMaximise } from "@/lib/windowDrag";
import { usePreferences } from "@/store/usePreferences";
import { useExportProgress, exportPercent } from "@/store/useExportProgress";
import { StopExport } from "@/components/StopExport";
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
}

/** Ticked, not ticked, or a folder with some of its playlists ticked. */
type Tick = "on" | "off" | "some";

/** What Import can bring back from a stick, in the order it does them. */
type ImportKind = "cues" | "history" | "settings";
const IMPORT_KINDS: readonly { kind: ImportKind; label: string; noun: string }[] = [
  { kind: "cues", label: "Cues and beat grids", noun: "cues and beat grids" },
  { kind: "history", label: "Play history", noun: "play history" },
  { kind: "settings", label: "CDJ/mixer settings", noun: "CDJ/mixer settings" },
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

export function SyncManager({ windowed = false, onClose, onSynced, onReady }: SyncManagerProps) {
  const t = useTranslation();
  const exportJobs = useExportProgress();
  const window_ = useRef<HTMLDivElement>(null);
  const [tree, setTree] = useState<readonly TreeNode[]>([]);
  const [devices, setDevices] = useState<readonly Device[]>([]);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  const [tickedDevices, setTickedDevices] = useState<ReadonlySet<string>>(new Set());
  const [expandedDevices, setExpandedDevices] = useState<ReadonlySet<string>>(new Set());
  const [deviceErrors, setDeviceErrors] = useState<ReadonlyMap<string, string>>(new Map());
  const [states, setStates] = useState<ReadonlyMap<string, DeviceSyncState>>(new Map());
  const [query, setQuery] = useState("");
  const [loadingTree, setLoadingTree] = useState(true);
  const [treeError, setTreeError] = useState("");
  // True from the start: the first scan begins on mount, and nothing may
  // offer SYNC or say "no devices" before it has answered.
  const [loadingDevices, setLoadingDevices] = useState(true);
  const [devicesError, setDevicesError] = useState("");
  // This window may live in its own webview, so it keeps its own live view
  // of rekordbox's process lock instead of relying on the main window.
  // Unknown is locked: do not briefly enable SYNC before the first check.
  const [rekordboxOpen, setRekordboxOpen] = useState<boolean | null>(null);
  const [operation, setOperation] = useState<"sync" | "import" | "eject" | "itunes" | null>(null);
  // The iTunes / Music library shown in the left column, its ticks, and which
  // of its folders are closed. Null until the auto-detect answers, or when no
  // shared Library.xml is found and the DJ has not chosen one.
  const [itunes, setItunes] = useState<ItunesLibrary | null>(null);
  const [itunesTicked, setItunesTicked] = useState<ReadonlySet<string>>(new Set());
  const [itunesCollapsed, setItunesCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [itunesLoading, setItunesLoading] = useState(true);
  const [itunesError, setItunesError] = useState("");
  const [ejectingPath, setEjectingPath] = useState<string | null>(null);
  const busy = operation !== null || [...exportJobs.values()].some(job => ["preparing", "checking", "copying", "database", "verifying", "publishing", "ejecting"].includes(job.state));
  const [ejectAfterSync, setEjectAfterSync] = useState(false);
  /** What is happening now, or what happened: one line, or one per stick. */
  const [status, setStatus] = useState<string[]>([]);
  const [showStatusDetails, setShowStatusDetails] = useState(false);
  const [lastImport, setLastImport] = useState<readonly ImportKind[] | null>(null);
  const [importFailed, setImportFailed] = useState(false);
  const [completedReports, setCompletedReports] = useState<ReadonlyMap<string, ExportReport>>(new Map());
  // DJ System in Preferences is what a stick with no settings of its own
  // gets, as it is on every export from the shell.
  const preferences = usePreferences();
  const stickDefaults = preferences.djSystem;
  const { importHistory, importSettings } = preferences.usbExport;
  // Import's ticks start at the defaults in Preferences, and follow them
  // until they are changed here.
  const [importTicks, setImportTicks] = useState<Partial<Record<ImportKind, boolean>>>({});
  const importDefaults: Record<ImportKind, boolean> = {
    cues: preferences.usbExport.importButtonCues,
    history: preferences.usbExport.importButtonHistory,
    settings: preferences.usbExport.importButtonSettings,
  };
  const importTicked = (kind: ImportKind) => importTicks[kind] ?? importDefaults[kind];
  /** Why a kind cannot be imported right now, or null when it can. */
  const importBlocked = (kind: ImportKind): string | null =>
    kind === "cues" && preferences.advanced.protectLibrary ? t("Turn off Library Protection to import cues and grids.")
    : kind === "history" && preferences.advanced.protectLibrary ? t("Turn off Library Protection to import play history.")
    : kind === "history" && rekordboxOpen !== false ? t("Quit rekordbox to import play history.")
    : null;
  const importKinds = IMPORT_KINDS.map(({ kind }) => kind).filter(kind => importTicked(kind) && !importBlocked(kind));
  const deleteUnlistedMusic = preferences.usbExport.deleteUnlistedMusic;
  const compatibilityFormat = preferences.usbExport.maximumCompatibility ? preferences.usbExport.conversionFormat : undefined;

  const nodes = useMemo(() => playlistNodes(tree), [tree]);
  const playlists = useMemo(() => nodes.filter(isExportablePlaylist), [nodes]);
  const byId = useMemo(() => new Map(nodes.map((n) => [n.id, n] as const)), [nodes]);
  const search = query.trim().toLocaleLowerCase();
  const visible = useMemo(() => search
    ? playlists.filter(node => node.name.toLocaleLowerCase().includes(search))
    : visibleNodes(nodes, collapsed), [nodes, playlists, collapsed, search]);
  const selectedCount = playlists.filter(node => ticked.has(node.id)).length;
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
    const read = async () => {
      const mine = ++latest;
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
        setCollapsed((current) => new Set([...[...current].filter((id) => present.has(id)), ...fresh]));
        // A deleted playlist is not something SYNC can be asked for.
        setTicked((current) => {
          const kept = [...current].filter((id) => present.has(id));
          return kept.length === current.size ? current : new Set(kept);
        });
      } catch {
        if (live && mine === latest) setTreeError("Couldn’t load playlists. Reopen Sync Manager to try again.");
      } finally {
        if (live && mine === latest) setLoadingTree(false);
      }
    };
    void read();
    void getBackend().then((backend) => {
      if (!live) return;
      stop = backend.onLibraryChanged(() => { void read(); });
    });
    return () => {
      live = false;
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

  /** Reads what a stick holds and was last given, and ticks that back on. */
  const readDevice = useCallback(async (path: string, restore: boolean) => {
    const backend = await getBackend();
    const read = await backend.deviceSyncState(path);
    setDeviceErrors(current => { const next = new Map(current); next.delete(path); return next; });
    setStates((current) => new Map(current).set(path, read));
    if (restore && read.selected.length > 0) {
      setTicked((current) => {
        const next = new Set(current);
        for (const playlist of read.selected) if (byId.has(playlist.libraryId)) next.add(playlist.libraryId);
        return next;
      });
    }
  }, [byId]);

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
  }, []);

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
        const report = await backend.importItunesSelected(itunes.path, ids);
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
      setExpandedDevices(current => new Set([...current].filter(path => path !== device.path)));
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
          const shown = missing.slice(0, 10).map((file) => `• ${file.title}\n  ${file.path}`).join("\n");
          const remaining = missing.length > 10 ? `\n${t("…and {count} more missing files.", { count: missing.length - 10 })}` : "";
          const question = `${t("{count} selected tracks have missing audio files and will be skipped.", { count: missing.length })}\n\n${shown}${remaining}\n\n${t("Continue anyway?")}`;
          const proceed = await backend.confirm(question, { yes: t("Yes"), no: t("No") });
          if (!proceed) {
            setStatus([t("Export cancelled because files are missing.")]);
            return;
          }
        }
        // Import only after SYNC is clicked and preflight is accepted, before
        // exporting can replace the selected devices' history/settings.
        const importNotes: string[] = [];
        if (importHistory || importSettings) {
          for (const path of destinations) {
            setStatus([`${nameOf(path)}: Importing USB history/settings…`]);
            try {
              const imported = await backend.importUsb(path, false, importHistory, importSettings);
              if (imported.histories) importNotes.push(`${nameOf(path)}: imported ${imported.histories} play-history entries.`);
              if (imported.settings) importNotes.push(`${nameOf(path)}: imported ${imported.settings} CDJ/mixer settings files.`);
              importNotes.push(...(imported.warnings ?? []).map(warning => `${nameOf(path)}: ${warning}`));
            } catch (e) {
              throw new Error(`${nameOf(path)}: Couldn’t import before syncing. ${errorMessage(e)}`, { cause: e });
            }
          }
        }
        stop = backend.onSyncProgress((progress) => {
          if (progress.state === "writing") setStatus([t("Writing to {device}…", { device: nameOf(progress.path) })]);
          if (progress.state === "ejecting") setStatus([t("Ejecting {device}…", { device: nameOf(progress.path) })]);
        });
        const reports = await backend.syncDevices(playlists, destinations, stickDefaults, false, ejectAfterSync, deleteUnlistedMusic, compatibilityFormat);
        setCompletedReports(new Map(reports.flatMap(r => r.report ? [[r.path, r.report] as const] : [])));
        const outcomes = reports.flatMap(r => r.error
          ? [`${nameOf(r.path)}: ${r.error}`]
          : r.ejected ? [`${nameOf(r.path)}: Safely ejected.`]
          : r.ejectError ? [`${nameOf(r.path)}: Not ejected: ${r.ejectError}`] : []);
        setStatus([...importNotes, ...(outcomes.length > 0 ? outcomes : [t("Sync complete.")])]);
        // What the sticks hold now, without touching the ticks.
        await refreshDevices().catch(() => {});
        await Promise.all(reports.filter(r => !r.ejected).map(({ path }) => readDevice(path, false).catch(() => {})));
        onSynced?.();
      } catch (e) {
        setStatus([e instanceof Error ? e.message : t("The sync could not be written.")]);
      } finally {
        stop();
        setOperation(null);
      }
    })();
  }, [canSync, nodes, ticked, tickedDevices, importHistory, importSettings, stickDefaults, ejectAfterSync, deleteUnlistedMusic, compatibilityFormat, refreshDevices, readDevice, onSynced, t]);

  const runUsbImport = (kinds: readonly ImportKind[]) => {
    if (busy || tickedDevices.size === 0 || kinds.length === 0) return;
    setOperation("import");
    setLastImport(kinds);
    setImportFailed(false);
    setShowStatusDetails(false);
    setStatus([]);
    void (async () => {
      try {
        const backend = await getBackend();
        if (kinds.includes("cues") && !await backend.confirm("Import cue and beat-grid changes from the selected USB devices? This replaces cues and grids for matching tracks in your library.")) return;
        const results: string[] = [];
        for (const device of devices.filter(d => tickedDevices.has(d.path))) {
          // One kind at a time, so each is reported on its own and one that
          // fails does not keep the others from being brought in.
          for (const { kind, noun } of IMPORT_KINDS.filter(({ kind }) => kinds.includes(kind))) {
            setStatus([t("Waiting for USB activity to finish, then importing {kind} from {device}…", { kind: noun, device: device.name })]);
            try {
              const result = await backend.importUsb(device.path, kind === "cues", kind === "history", kind === "settings");
              if (kind === "cues") results.push(`${device.name}: updated ${result.tracks} tracks${result.skipped ? `; skipped ${result.skipped}` : ""}.`);
              else if (kind === "history") results.push(result.histories ? `${device.name}: imported ${result.histories} play-history entries.` : `${device.name}: no new play-history entries.`);
              else results.push(result.settings ? `${device.name}: imported ${result.settings} CDJ/mixer settings files.` : `${device.name}: no CDJ/mixer settings files found.`);
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
                  style={{ paddingLeft: `${8 + (search ? 0 : node.depth - 1) * 18}px` }}
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
            {loadingTree ? <p className={styles.empty}>Loading playlists…</p>
              : treeError ? <p className={styles.empty} role="alert">{treeError}</p>
              : nodes.length === 0 ? <p className={styles.empty}>No playlists yet. Create a playlist in your library to get started.</p>
              : visible.length === 0 ? <p className={styles.empty}>No playlists match “{query.trim()}”.</p> : null}
          </div>
        </section>

        <div className={styles.middle}>
          <div className={styles.syncActions}>
          <button
            type="button"
            className={styles.sync}
            onClick={sync}
            disabled={!canSync}
            title={rekordboxOpen ? "Quit rekordbox to enable synchronization." : undefined}
            aria-label="SYNC"
            aria-busy={busy || undefined}
            aria-describedby="sync-selection-hint"
          >
            {operation === "sync" ? <><LoaderCircle size={16} className={styles.spinner} aria-hidden="true" /> Syncing…</> : <>SYNC <ArrowRight size={16} aria-hidden="true" /></>}
          </button>
          <label className={styles.option}>
            <input type="checkbox" className={styles.tick} checked={ejectAfterSync} disabled={busy}
              aria-label="Eject after syncing" onChange={e => setEjectAfterSync(e.currentTarget.checked)} />
            Eject after syncing
          </label>
          </div>
          <div className={styles.importActions}>
          <fieldset className={styles.importKinds} disabled={busy}>
            <legend className={styles.importLegend}>Import from USB</legend>
            {IMPORT_KINDS.map(({ kind, label }) => {
              const blocked = importBlocked(kind);
              return (
                <label key={kind} className={styles.option} title={blocked ?? undefined}>
                  <input type="checkbox" className={styles.tick} checked={importTicked(kind) && !blocked} disabled={blocked !== null}
                    onChange={e => { const on = e.currentTarget.checked; setImportTicks(ticks => ({ ...ticks, [kind]: on })); }} />
                  {label}
                </label>
              );
            })}
          </fieldset>
          <button type="button" className={styles.button} onClick={() => runUsbImport(importKinds)}
            disabled={busy || tickedDevices.size === 0 || importKinds.length === 0}
            title="Import the ticked items from the selected USB devices to rbxport">
            {operation === "import" ? <><LoaderCircle size={14} className={styles.spinner} aria-hidden="true" /> Importing…</> : <><ArrowLeft size={14} aria-hidden="true" /> Import</>}
          </button>
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
              const expanded = expandedDevices.has(device.path);
              const read = states.get(device.path);
              const job = exportJobs.get(device.path);
              const fileSystem = device.fileSystem?.toUpperCase().replace(/^VFAT$|^MSDOS$/, "FAT") || "Unknown filesystem";
              // Linux exposes FAT volumes through the `vfat` driver, even when the
              // volume itself is FAT32. Treat that kernel driver name as compatible
              // so the same stick does not get a warning only on Linux.
              const fat32Recommended = !["FAT32", "VFAT"].includes(device.fileSystem?.trim().toUpperCase() ?? "");
              const free = device.totalBytes > 0 ? `${device.freeBytes === 0 ? "0.0 GB" : formatSpace(device.freeBytes)} free (${Math.round(device.freeBytes / device.totalBytes * 100)}%)` : "Space unknown";
              const freePercent = device.totalBytes > 0 ? Math.max(0, Math.min(100, device.freeBytes / device.totalBytes * 100)) : null;
              const usedPercent = freePercent === null ? null : 100 - freePercent;
              const used = device.totalBytes > 0 ? formatSpace(Math.max(0, device.totalBytes - device.freeBytes)) || "0.0 GB" : "";
              return <div key={device.path} className={styles.device} role="treeitem" aria-expanded={expanded} data-ticked={on || undefined}>
                <div className={styles.row}>
                  <button type="button" className={styles.twisty} data-open={expanded ? "" : undefined}
                    aria-label={`${expanded ? "Collapse" : "Expand"} ${device.name}`} disabled={busy} onClick={() => {
                      setExpandedDevices(current => toggle(current, device.path));
                      if (!expanded) void readDevice(device.path, false).catch(e => setDeviceErrors(current => new Map(current).set(device.path, e instanceof Error ? e.message : "Could not read this USB device.")));
                    }} />
                  <label className={styles.rowSelection}>
                  <span className={styles.name} title={device.path}>{device.name}</span>
                  {fat32Recommended ? <span className={styles.filesystemWarning} title={t("Pioneer DJ recommends FAT32")}>
                    <AlertTriangle aria-label={t("Pioneer DJ recommends FAT32")} />
                  </span> : null}
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
                    aria-valuemax={usedPercent === null ? undefined : 100} aria-valuenow={usedPercent ?? undefined}
                    aria-valuetext={usedPercent === null ? "Space unknown" : `${used} used; ${free}`} title={free}>
                    {usedPercent !== null ? <span style={{ width: `${usedPercent}%` }} /> : null}
                  </div>
                  <div className={styles.storageLabels}>{usedPercent !== null ? <span><i aria-hidden="true" />{used} used</span> : null}<span>{free}</span></div>
                  {job ? <div className={styles.exportProgress} data-state={job.state}>
                    <progress data-state={job.state} aria-label={`Exporting ${device.name}`} max={100} value={exportPercent(job)} />
                    <span>{job.state === "cancelled" ? "Export stopped" : job.state === "failed" ? "Export failed" : job.state === "done" ? "Export complete" : job.state === "preparing" ? "Preparing for export" : job.state === "checking" ? `Checking — ${job.title || device.name}` : job.state === "database" ? "Building databases" : job.state === "verifying" ? "Verifying databases" : job.state === "publishing" ? "Publishing safely" : job.state === "ejecting" ? `Ejecting ${device.name}` : `Exporting — ${job.title || device.name}`} ({exportPercent(job)}%)</span>
                    {job.state === "preparing" || job.state === "checking" || job.state === "copying" || job.state === "database" ? <StopExport path={job.path} className={styles.button} /> : null}
                    {job.state === "failed" ? <span className={styles.exportError} role="alert">
                      {job.title || "The export failed before the device could be verified. Check that it is connected, writable, and has enough free space."}
                    </span> : null}
                  </div> : null}
                  {completedReports.has(device.path) ? (() => {
                    const report = completedReports.get(device.path)!;
                    return <div className={styles.exportReport} aria-label={`${device.name} export report`}>
                      <span><strong>{Math.max(0, report.tracks - report.reused)}</strong> updated</span>
                      <span><strong>{report.reused}</strong> unchanged</span>
                      <span><strong>+{report.playlistsAdded}</strong> playlists</span>
                      <span><strong>−{report.playlistsRemoved}</strong> playlists</span>
                      {report.skipped.length > 0 ? <span className={styles.exportWarning}><strong>{report.skipped.length}</strong> missing</span> : null}
                    </div>;
                  })() : null}
                </div>
                {!expanded && deviceErrors.has(device.path) ? <p className={styles.capacity} role="alert">{deviceErrors.get(device.path)}</p> : null}
                {expanded ? <div className={styles.library} role="group" aria-label={`${device.name} library`}>
                  {deviceErrors.has(device.path) ? <p role="alert">{deviceErrors.get(device.path)}</p> : read === undefined ? <div className={styles.libraryNote}>Reading…</div>
                    : <DeviceLibraries libraries={read.libraries ?? [{ name: "Device Library", nodes: read.onDevice.map((name, i) => ({ id: String(i+1), parentId: "0", name, folder: false })) }]} />}
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
          {status.length === 0 ? <span className={styles.selectionSummary}>{selectionSummary}</span> : null}
          {status.length === 0 ? <span id="sync-selection-hint" className={styles.idleStatus} data-warn={rekordboxOpen || undefined}>{syncHint}</span> : null}
          </div>
          {status.length > 1 || importFailed ? <button type="button" className={styles.detailsButton} onClick={() => setShowStatusDetails(open => !open)}>
            {showStatusDetails ? "Hide details" : "Show details"}
          </button> : null}
        </div>
        {status.length > 0 ? <span id="sync-selection-hint" hidden>{syncHint}</span> : null}
        <button type="button" className={styles.button} onClick={onClose}>
          {busy ? "Run in background" : "Close"}
        </button>
      </footer>
    </div>
  );

  if (windowed) return body;
  return (
    <div className={styles.backdrop} onMouseDown={onClose} role="presentation">
      {body}
    </div>
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
