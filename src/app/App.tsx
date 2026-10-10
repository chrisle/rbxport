import { ConfirmHost } from "@/components/ConfirmDialog";
import type { DuplicateAnswer } from "@/components/DuplicateTracksDialog";
import { JobQueue, slices, type JobView } from "@/lib/jobQueue";
import { importInSlices, removeInSlices } from "@/lib/bulkJobs";
import { useBackupProgress } from "@/store/useBackupProgress";
import { useExportProgress } from "@/store/useExportProgress";
import { reportStartupPaint } from "@/lib/startup";
import { useShowWindowWhenReady } from "@/lib/windowReady";
import { waveformKindOf } from "@/canvas";
import { useAutoJoinLink } from "@/store/useAutoJoinLink";
import { useEventCallback } from "@/store/useEventCallback";
/**
 * Export-mode shell.
 *
 * Composes the measured regions: top bar, library tree, track browser, status
 * bar. The preview player region is reserved but not yet implemented (it lands
 * with `rbl-audio` in Milestone 1).
 */
import type { TrackSearchField } from "@/lib/search";

import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { droppedFilePaths, getBackend, subscribeNativeFileDrops } from "@/ipc/client";
import type {
  Backend, DeckId, Device, ImportReport, LibraryProblem, LibrarySummary, RowDto, SortColumn, TrackField, TreeNode, ViewSpec,
} from "@/ipc/types";
import { TrackTable, type TrackDrag } from "@/views/browser/TrackTable";
import { clearWaveformPreviewCache } from "@/views/browser/WaveformPreview";
import { TreeView } from "@/views/tree/TreeView";
import { ConnectedTopBar } from "@/views/topbar/TopBar";
import { StatusBar } from "@/views/statusbar/StatusBar";
import { LinkDeckStrip } from "@/views/statusbar/LinkDeckStrip";
import styles from "./App.module.css";
import { detectPlatform, dispatch, isTyping, menuAccelerator } from "@/lib/shortcuts";
import { hasEditHistory, runEditHistory, setLibraryEditHistory } from "@/lib/editHistory";
import { transposeKey } from "@/lib/camelot";
import { gainToKnob, KNOB_FULL, knobToGain } from "@/lib/volume";
import { clampWidth, TREE_BOUNDS } from "@/lib/splitter";
import { exportSummary } from "@/lib/exportSummary";
import { deviceId, devicePath, renamedDevice } from "@/lib/devices";
import { DEVICE_ASKS, deviceNodeId, deviceParentFor, devicePlaylistsOf, isDeviceLibraryKind, parseDeviceNodeId } from "@/lib/deviceLibrary";
import { useDeviceLibraries } from "@/store/useDeviceLibraries";
import { refusal, resolveMenu } from "@/lib/menu";
import { nextSort, specForNode, type SortState } from "@/lib/viewSpec";
import {
  DEFAULT_SUB_TREE_WIDTH, DEFAULT_SUB_WIDTH, loadSession, saveSession, SEEDED_NODES, SEEDED_ROWS,
  type TrafficLightSource,
} from "@/lib/session";
import { startWindowDrag, toggleWindowMaximise } from "@/lib/windowDrag";
import { AppCost } from "@/views/topbar/AppCost";
import { useLimiter } from "@/store/useLimiter";
import { onPreviewError, stopPreview } from "@/store/usePreview";
import { useUpdater } from "@/store/useUpdater";
import { UpdateReadyNotice } from "@/views/update/UpdateReadyNotice";
import { MasterOutputProvider, MasterOutputConnection, useMasterControls, useMasterDisplay } from "@/store/MasterOutput";
import { asLayout, deckCount, isFullDeck, type PlayerLayout } from "@/lib/layout";
import { FIELD_LABEL, InfoPanel } from "@/views/info/InfoPanel";
import { SubBrowser } from "@/views/subbrowser/SubBrowser";
import { RightRail } from "@/views/browser/RightRail";
import { useColumns, type ColumnContext } from "@/store/useColumns";
import { useExplorer } from "@/store/useExplorer";
import { importLoose, isLooseId } from "@/lib/explorer";
import { childrenOf, containerOf, parentFor, withSources } from "@/lib/tree";
import { JUMP_SIZE_ID } from "@/lib/player";
import type { Deck as SyncDeck } from "@/lib/sync";
import { LayoutDualIcon } from "@/components/icons";
import { Player } from "@/views/player/Player";
import { MixerStrip } from "@/views/player/MixerStrip";
import { DualZoom } from "@/views/player/DualDeck";
import type { PreferencesTarget } from "@/views/settings/Preferences";
import { PreferencesProvider, usePreferencesStore } from "@/store/usePreferences";
import type { PreferencePane } from "@/lib/preferences";
import { answer, deckNumber, setPlaying, whenLoaded, withSetting, type ScriptHandler } from "@/lib/scripting";
import { useAnalysis } from "@/store/useAnalysis";
import type { AnalysisChoice } from "@/views/analysis/AnalysisDialog";
import { autoAnalysisOffer, takeRemainingPages } from "@/lib/autoAnalysis";
import type { LibraryQuestion } from "@/views/library/NewLibraryDialog";
import type { QueueItem } from "@/lib/queue";
import { TrackFilter } from "@/views/browser/TrackFilter";
import { EMPTY_FILTER, toSpecFilter, type FilterState } from "@/lib/trackFilter";
import type { AnalysisResult, DevicePlaylistEdit, FilterValues, LinkPeerSeen, LinkStatus, RelocateSearch, SmartRule, TrackLookups, UnanalysedTracks } from "@/ipc/types";
import { missingAmong, relocateSteps, relocateTracks } from "@/lib/relocate";
import { useTooltip } from "@/store/usePreferences";
import { useTranslation } from "@/i18n";
import { nativeMenuLabels } from "@/lib/nativeMenu";

/**
 * The metadata fields a row already carries, so an edit to one can be shown
 * before the backend has answered. The others live only in the information
 * panel's record, which is re-read anyway.
 */
const ROW_FIELDS: ReadonlySet<TrackField> = new Set<TrackField>([
  "title", "artist", "album", "genre", "label",
]);

const DuplicateTracksDialog = lazy(() => import("@/components/DuplicateTracksDialog").then(m => ({ default: m.DuplicateTracksDialog })));
const AnalysisDialog = lazy(() => import("@/views/analysis/AnalysisDialog").then(m => ({ default: m.AnalysisDialog })));
const NewLibraryDialog = lazy(() => import("@/views/library/NewLibraryDialog").then(m => ({ default: m.NewLibraryDialog })));
const DevicePanel = lazy(() => import("@/views/devices/DevicePanel").then(m => ({ default: m.DevicePanel })));
const ReportBug = lazy(() => import("@/views/report/ReportBug").then(m => ({ default: m.ReportBug })));
const UpdateManager = lazy(() => import("@/views/update/UpdateManager").then(m => ({ default: m.UpdateManager })));
const Preferences = lazy(() => import("@/views/settings/Preferences").then(m => ({ default: m.Preferences })));
const SyncManager = lazy(() => import("@/views/sync/SyncManager").then(m => ({ default: m.SyncManager })));
const SmartPlaylistEditor = lazy(() => import("@/views/tree/SmartPlaylistEditor").then(m => ({ default: m.SmartPlaylistEditor })));
const MissingFileManager = lazy(() => import("@/views/library/MissingFileManager").then(m => ({ default: m.MissingFileManager })));

const SHOW_MAIN_SUPPORT = true;

/**
 * How many tracks one backend call handles in a long operation. Each call is
 * one database write, so a slice is the unit the status bar's progress moves
 * in and the longest a Stop waits.
 */
const ADD_SLICE = 100;

function ConnectedPreferences(props: Omit<React.ComponentProps<typeof Preferences>, "reduction" | "vu" | "peakLeft" | "peakRight">) {
  const master = useMasterDisplay();
  return <Preferences {...props} reduction={master.reduction} vu={master.vu}
    peakLeft={master.peakLeft} peakRight={master.peakRight} />;
}

function useClock(): string {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    // Tick on the minute boundary rather than polling: idle CPU budget is 0.5%.
    let timer: ReturnType<typeof setTimeout>;
    const schedule = () => {
      timer = setTimeout(() => {
        setNow(new Date());
        schedule();
      }, 60_000 - (Date.now() % 60_000));
    };
    schedule();
    return () => clearTimeout(timer);
  }, []);
  return `${now.getHours() % 12 || 12}:${String(now.getMinutes()).padStart(2, "0")}`;
}

export function App() {
  return <MasterOutputProvider><AppBody /></MasterOutputProvider>;
}

function AppBody() {
  const t = useTranslation();
  useShowWindowWhenReady();
  useEffect(() => { reportStartupPaint("shell-painted"); }, []);
  useEffect(() => {
    void getBackend().then((backend) => backend.setMenuLabels(nativeMenuLabels(t))).catch(() => undefined);
  }, [t]);
  useEffect(() => {
    // Native file drags can land anywhere, including outside a drop target.
    // Cancel WebKit's file navigation without stopping playlist/deck handlers.
    const preventFileNavigation = (event: DragEvent) => {
      if (event.dataTransfer?.types.includes("Files") || event.dataTransfer?.files.length) {
        event.preventDefault();
      }
    };
    // Accept file drags first so WebKit dispatches a DOM drop instead of
    // falling back to native navigation before a drop handler can run.
    document.addEventListener("dragover", preventFileNavigation, true);
    document.addEventListener("drop", preventFileNavigation, true);
    return () => {
      document.removeEventListener("dragover", preventFileNavigation, true);
      document.removeEventListener("drop", preventFileNavigation, true);
    };
  }, []);
  // Read once, synchronously, so the first render is already the layout the
  // window closed with rather than the default that then jumps.
  const [restored] = useState(loadSession);

  const [tree, setTree] = useState<readonly TreeNode[]>(restored.tree);
  const [treeExpansion, setTreeExpansion] = useState(restored.treeExpansion);
  const [editHistory, setEditHistory] = useState({
    canUndo: false, canRedo: false, undoLabel: null as string | null, redoLabel: null as string | null,
  });
  // Connected volumes. Asked for, never polled: a 1 Hz scan of every mount
  // point is exactly the kind of idle work the budgets forbid.
  const [devices, setDevices] = useState<readonly Device[]>([]);
  // What each stick's own libraries hold, read when a stick is opened.
  const deviceLibraries = useDeviceLibraries(devices);
  const [syncing, setSyncing] = useState(false);
  const [ejectingDeviceId, setEjectingDeviceId] = useState<string | null>(null);
  const ejectingDeviceRef = useRef(false);
  // Closed by default, which is what browseSetting.xml records for the user's
  // own rekordbox (`ListInfo open="0"`).
  const [infoOpen, setInfoOpen] = useState(restored.infoOpen);
  // Closed unless it was open at exit; browseSetting.xml records
  // `SubBrowse open="0"` for a first run.
  const [subOpen, setSubOpen] = useState(restored.subOpen);
  // The sub-browser's width and its tree's, as last dragged. Clamped to the
  // window by the panel itself, so these are what was asked for.
  const [subWidth, setSubWidth] = useState(restored.subWidth);
  const [subTreeWidth, setSubTreeWidth] = useState(restored.subTreeWidth);
  // The Track Filter bar. Its open state is kept across runs, its picks are
  // not: a library that comes back already narrowed reads as a broken one.
  const [filterOpen, setFilterOpen] = useState(restored.filterOpen);
  const [filterState, setFilterState] = useState<FilterState>(EMPTY_FILTER);
  const [filterValues, setFilterValues] = useState<FilterValues | null>(null);
  // Why the library is not there, when it is not. Shown instead of "Loading…",
  // which is a lie once the load has failed.
  const [loadError, setLoadError] = useState<string | null>(null);
  // What to ask when there is no library to load: none anywhere, or one
  // configured on a drive that is not connected.
  const [missingLibrary, setMissingLibrary] = useState<LibraryQuestion | null>(null);
  /** File › Display All Missing Files: the Missing File Manager is open. */
  const [missingFilesOpen, setMissingFilesOpen] = useState(false);
  const [summary, setSummary] = useState<LibrarySummary | null>(null);
  // Do not write the empty bootstrap selection over the session while the
  // backend is still restoring the node that was open at exit. WebKit gets
  // to the effect before the mock backend answers, which exposed the same
  // race a slower real library can hit.
  const [sessionReady, setSessionReady] = useState(false);
  const [version, setVersion] = useState<string | null>(null);
  // Seed the selected node along with the cached tree. Besides avoiding an
  // unnecessary blank title, this keeps the restored view coherent until the
  // backend replaces both with current data.
  const [selectedNode, setSelectedNode] = useState<TreeNode | null>(() =>
    restored.tree.find((node) => node.id === restored.selectedNodeId) ?? null,
  );
  // One piece of state, not two: updating `descending` from inside a `setSort`
  // updater made the toggle a side effect, and StrictMode's double invocation
  // cancelled it out.
  // Opens in the view's own order: a playlist's is the order somebody put it
  // in, and the collection has no more meaningful default.
  const [sortState, setSortState] = useState<SortState>(restored.sort);
  const [selectedCount, setSelectedCount] = useState(0);
  // The rows behind the selection, so they can be queued for analysis.
  const [selectedTracks, setSelectedTracks] = useState<{ id: string; title: string }[]>([]);
  // The row the player is showing. Set by the browser as the selection moves,
  // so the player reflects what is highlighted rather than nothing. It starts
  // empty on every run: a track restored from the last session would be loaded
  // before the library is read, which fails, and the deck would open on an
  // error over something nobody asked to hear.
  const [playerTrack, setPlayerTrack] = useState<RowDto | null>(null);
  // Deck B, which only exists in the two-player layouts. Loaded by dropping a
  // track on it: the browser's selection drives deck A alone, or picking
  // through a playlist would keep replacing whatever B was cued to.
  const [playerTrackB, setPlayerTrackB] = useState<RowDto | null>(null);
  // The one row the browser has selected, so an empty deck can be clicked to
  // take it. Selecting still loads nothing by itself.
  const [selectedRow, setSelectedRow] = useState<RowDto | null>(null);
  // The track ids the information panel follows: the selection of whichever
  // browser list changed last, in the order that list reports it. More than
  // one is a multiple selection, which the panel edits as a whole.
  const [infoSelection, setInfoSelection] = useState<readonly string[]>([]);
  // Where each deck's transport is drawn in the two-deck layouts. State rather
  // than a ref, because the players have to re-render once the slots exist.
  const [transportA, setTransportA] = useState<HTMLDivElement | null>(null);
  const [transportB, setTransportB] = useState<HTMLDivElement | null>(null);
  /**
   * DUAL CONTROL: one zoom and one beat-jump size for both decks.
   *
   * Off, each deck keeps its own; on, the shell holds them and hands the same
   * value to both, so a wheel over one waveform moves the other with it.
   */
  /**
   * Which deck the other syncs to.
   *
   * Deck A to begin with, because that is the one a single-player layout has
   * and the one a first track lands on. Only ever one, which is what MASTER
   * means.
   */
  const [syncMaster, setSyncMasterState] = useState<DeckId>("a");
  /**
   * BEAT SYNC held on, per deck. A deck follows the master's tempo for as
   * long as its button is lit; the master itself never follows, so making a
   * deck the master puts its own light out.
   */
  const [synced, setSynced] = useState<Record<DeckId, boolean>>({ a: false, b: false });
  const setSyncMaster = useCallback((deck: DeckId) => {
    setSyncMasterState(deck);
    setSynced((s) => (s[deck] ? { ...s, [deck]: false } : s));
  }, []);
  const toggleSync = useMemo(
    () => ({
      a: () => setSynced((s) => ({ ...s, a: !s.a })),
      b: () => setSynced((s) => ({ ...s, b: !s.b })),
    }),
    [],
  );
  /**
   * What each deck is playing at — its file's BPM times its tempo — so the
   * synced deck can be handed the master's and re-match when it moves.
   */
  const [playingBpm, setPlayingBpm] = useState<Record<DeckId, number | null>>({ a: null, b: null });
  const reportPlayingBpm = useMemo(
    () => ({
      a: (bpm: number | null) => setPlayingBpm((p) => (p.a === bpm ? p : { ...p, a: bpm })),
      b: (bpm: number | null) => setPlayingBpm((p) => (p.b === bpm ? p : { ...p, b: bpm })),
    }),
    [],
  );
  const leaderBpmX100 = playingBpm[syncMaster];
  /** Each deck's key shift in semitones, so the Traffic Light reads the key the deck sounds in. */
  const [keyShift, setKeyShift] = useState<Record<DeckId, number>>({ a: 0, b: 0 });
  const reportKeyShift = useMemo(
    () => ({
      a: (semitones: number) => setKeyShift((k) => (k.a === semitones ? k : { ...k, a: semitones })),
      b: (semitones: number) => setKeyShift((k) => (k.b === semitones ? k : { ...k, b: semitones })),
    }),
    [],
  );
  /**
   * How each deck reads the other for sync.
   *
   * Getters rather than state: a deck's position moves every frame and sync
   * reads it once, when the button goes down. Holding it as state would
   * re-render the shell sixty times a second for a number nobody is looking
   * at.
   */
  const syncA = useRef<() => SyncDeck | null>(() => null);
  const syncB = useRef<() => SyncDeck | null>(() => null);
  const publishSync = useMemo(
    () => ({
      a: (get: () => SyncDeck | null) => {
        syncA.current = get;
      },
      b: (get: () => SyncDeck | null) => {
        syncB.current = get;
      },
    }),
    [],
  );
  const peerSync = useMemo(
    () => ({ a: () => syncB.current(), b: () => syncA.current() }),
    [],
  );
  /**
   * A grid shift on one deck, handed to the other: a deck synced to the
   * shifted one moves with it. Each deck decides by its own BEAT SYNC
   * whether it moves.
   */
  const followA = useRef<(ms: number) => void>(() => {});
  const followB = useRef<(ms: number) => void>(() => {});
  const publishGridFollow = useMemo(
    () => ({
      a: (follow: (ms: number) => void) => {
        followA.current = follow;
      },
      b: (follow: (ms: number) => void) => {
        followB.current = follow;
      },
    }),
    [],
  );
  const gridNudged = useMemo(
    () => ({ a: (ms: number) => followB.current(ms), b: (ms: number) => followA.current(ms) }),
    [],
  );
  /**
   * The zoom cluster the two-deck layout shares, registered the same way:
   * one + RST − over the line where the two details meet, and a press
   * zooms both decks. DUAL CONTROL off, each deck still keeps its own zoom
   * for the wheel; the cluster is simply pressed on both.
   */
  const zoomA = useRef<(by: number) => void>(() => {});
  const zoomB = useRef<(by: number) => void>(() => {});
  const publishZoom = useMemo(
    () => ({
      a: (zoom: (by: number) => void) => {
        zoomA.current = zoom;
      },
      b: (zoom: (by: number) => void) => {
        zoomB.current = zoom;
      },
    }),
    [],
  );
  const zoomBoth = useCallback((by: number) => {
    zoomA.current(by);
    zoomB.current(by);
  }, []);
  // Remembered across runs: a DUAL CONTROL left on comes back on.
  const [dual, setDual] = useState(restored.dualControl);
  const [waveformZoom, setWaveformZoom] = useState(restored.waveformZoom);
  const setZoomA = useCallback((bars: number) => {
    setWaveformZoom((zoom) => zoom.a === bars ? zoom : { ...zoom, a: bars });
  }, []);
  const setZoomB = useCallback((bars: number) => {
    setWaveformZoom((zoom) => zoom.b === bars ? zoom : { ...zoom, b: bars });
  }, []);
  const [dualBars, setDualBars] = useState(restored.waveformZoom.a);
  const setLinkedZoom = useCallback((bars: number) => {
    setDualBars(bars);
    setWaveformZoom({ a: bars, b: bars });
  }, []);
  const [dualJump, setDualJump] = useState(JUMP_SIZE_ID);
  const linked = dual
    ? {
        jumpSize: dualJump,
        onJumpSize: setDualJump,
      }
    : {};
  const [query, setQuery] = useState("");
  const [searchField, setSearchField] = useState<TrackSearchField>("all");
  // The tree's width, dragged by the splitter. Held here because the grid that
  // sizes both panes lives here.
  const [treeWidth, setTreeWidth] = useState(restored.treeWidth);
  const bodyRef = useRef<HTMLDivElement>(null);
  const dragFrom = useRef<{ x: number; width: number } | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const clock = useClock();
  // The table's layout follows the kind of thing being browsed, as
  // browseSetting.xml does, rather than each individual playlist.
  const columnContext: ColumnContext =
    selectedNode?.kind === "playlist" || selectedNode?.kind === "smartPlaylist" || selectedNode?.kind === "devicePlaylist"
      ? "playlist"
      : selectedNode?.kind === "history"
        ? "history"
        : selectedNode?.kind === "directory" || selectedNode?.kind === "explorer"
          ? "folder"
          : "collection";
  const cols = useColumns(columnContext);
  // The Preferences window, and the pane it opens on: the missing-file
  // manager lives under Advanced, so the File menu opens it there. In the
  // shell it is a window of its own; in a browser, which has no windows to
  // open, it is drawn over this one.
  const [settingsOpen, setSettingsOpen] = useState<PreferencesTarget | null>(null);
  const openPreferences = useCallback((pane: PreferencesTarget) => {
    void getBackend().then(async (backend) => {
      const opened = await backend.openPreferences(pane).catch(() => false);
      if (!opened) setSettingsOpen(pane);
    });
  }, []);
  // The Sync Manager, the same way: a window of its own in the shell, and
  // drawn over this one in a browser.
  const [syncOpen, setSyncOpen] = useState(false);
  const openSyncManager = useCallback(() => {
    void getBackend().then(async (backend) => {
      const opened = await backend.openSyncWindow().catch(() => false);
      if (!opened) setSyncOpen(true);
    });
  }, []);
  const [reportOpen, setReportOpen] = useState(false);
  const openReport = useCallback(() => {
    void getBackend().then(backend => backend.openReportWindow()).then(opened => { if (!opened) setReportOpen(true); });
  }, []);
  const openSupport = useCallback(() => {
    void getBackend().then(backend => backend.openUrl("https://www.paypal.com/donate/?hosted_button_id=H6GGU8PHP8CJE")).catch(() => {});
  }, []);
  const prefs = usePreferencesStore();
  const { view: viewPrefs, advanced: advancedPrefs, analysis: analysisPrefs } = prefs.preferences;
  // Every write path reads this one flag: rekordbox holding the database,
  // or Library Protection in Preferences, refuse the same way.
  const backupJob = useBackupProgress();
  const exportJobs = useExportProgress();
  const exportBatch = [...exportJobs.values()];
  const exportRunning = exportBatch.some(job => ["preparing", "checking", "copying", "database", "verifying", "publishing", "ejecting"].includes(job.state));
  const readOnly = (summary?.readOnly ?? false) || advancedPrefs.protectLibrary;
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const refresh = async () => {
      try {
        const backend = await getBackend();
        if (!live) return;
        const info = await backend.librarySummary();
        if (live) {
          setSummary((current) => current && current.readOnly !== info.readOnly
            ? { ...current, readOnly: info.readOnly }
            : current);
        }
      } catch {
        // Keep the last known lock state if the library is temporarily unavailable.
      } finally {
        if (live) timer = setTimeout(() => void refresh(), 2000);
      }
    };
    timer = setTimeout(() => void refresh(), 2000);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      stop = backend.onEditHistory((history) => {
        setEditHistory(history);
        setLibraryEditHistory(history.undoLabel, history.redoLabel);
      });
    })();
    return () => {
      live = false;
      stop?.();
      setLibraryEditHistory(null, null);
    };
  }, []);
  // Checks on its own a while after launch when Preferences says so; the
  // menu and Preferences ask by hand.
  const updater = useUpdater(advancedPrefs.checkUpdates, advancedPrefs.updateFrequency);
  const checkForUpdates = updater.check;
  const openWhatsNew = useCallback(() => {
    const release = updater.state.phase === "ready" ? `#v${updater.state.ready.version}` : "";
    void getBackend()
      .then((backend) => backend.openUrl(`https://rbxport.com/whats-new/${release}`))
      .catch(() => {});
  }, [updater.state]);
  const [updateNoticeVisible, setUpdateNoticeVisible] = useState(false);
  useEffect(() => {
    setUpdateNoticeVisible(updater.state.phase === "ready");
  }, [updater.state.phase]);
  const updateNotice = updater.state.phase === "ready" && updateNoticeVisible ? updater.state : null;
  // DJ System in Preferences is what a stick with no settings of its own
  // gets on export; the same shape goes with every export call.
  const stickDefaults = prefs.preferences.djSystem;
  const deleteUnlistedMusic = prefs.preferences.usbExport.deleteUnlistedMusic;
  const compatibilityFormat = prefs.preferences.usbExport.maximumCompatibility ? prefs.preferences.usbExport.conversionFormat : undefined;
  /** How much of the window the deck takes, kept across restarts. */
  const [layout, setLayout] = useState<PlayerLayout>(restored.layout);
  /**
   * The Traffic Light: which deck's key the browser lights rows against —
   * the MASTER menu above the track list. The key itself is that deck's
   * loaded track's, so it follows the master when the master moves.
   */
  const [trafficLight, setTrafficLight] = useState<TrafficLightSource>(restored.trafficLight);
  const activeTrafficLight = trafficLight === "b" && deckCount(layout) < 2 ? "a" : trafficLight;
  const trafficDeck: DeckId = deckCount(layout) < 2 ? "a" : activeTrafficLight === "master" ? syncMaster : activeTrafficLight;
  /*
   * A new track on the master deck, or none, hands MASTER to the other deck
   * when that one holds a track, as rekordbox does: the deck that was
   * following keeps the tempo it was playing at, rather than jumping to the
   * new track's BPM [OBS rekordbox 7 Export, chris-win11, parity/issue-128;
   * manual p.168 "When changing or unloading a track on the deck of the sync
   * master the sync master is switched to the other deck"]. Its BEAT SYNC is
   * left as it was, so it follows again if MASTER comes back.
   */
  const masterTrackId = (syncMaster === "b" ? playerTrackB : playerTrack)?.id ?? null;
  const otherTrackId = (syncMaster === "b" ? playerTrack : playerTrackB)?.id ?? null;
  const twoDecks = deckCount(layout) >= 2;
  const masterHeld = useRef({ deck: syncMaster, track: masterTrackId });
  useEffect(() => {
    const held = masterHeld.current;
    masterHeld.current = { deck: syncMaster, track: masterTrackId };
    if (held.deck !== syncMaster || held.track === null || held.track === masterTrackId) return;
    if (!twoDecks || otherTrackId === null) return;
    setSyncMasterState(syncMaster === "a" ? "b" : "a");
  }, [syncMaster, masterTrackId, otherTrackId, twoDecks]);
  const trafficTrack = trafficDeck === "b" ? playerTrackB : playerTrack;
  const trafficKey = trafficTrack ? transposeKey(trafficTrack.key, keyShift[trafficDeck]) : null;
  const master = useMasterControls();
  // Read at start so the remembered setting reaches the engine before the
  // first thing plays, not when Settings is next opened.
  const limiter = useLimiter();

  // Preferences › Audio, handed to the engine: the rate and buffer the
  // device is opened with, and the metronome's click. On start and on every
  // change, from this window or the Preferences window's storage event.
  const audioPrefs = prefs.preferences.audio;
  useEffect(() => {
    void (async () => {
      const backend = await getBackend();
      try {
        await backend.setAudioConfig(audioPrefs.sampleRate, audioPrefs.bufferSize);
        await backend.setMetronome(audioPrefs.metronomeSound, audioPrefs.metronomeVolume);
      } catch (e) {
        console.warn("the audio settings did not reach the engine", e);
      }
    })();
  }, [audioPrefs]);
  useEffect(() => {
    let live = true;
    void getBackend()
      .then((backend) => backend.appVersion())
      .then((found) => {
        if (live) setVersion(found);
      })
      .catch(() => {
        // A build with no shell behind it has no version; the strip just
        // shows the name.
      });
    return () => {
      live = false;
    };
  }, []);
  // Long library operations wait their turn here, shown in the status bar.
  const jobError = useRef<(job: JobView, error: unknown) => void>(() => undefined);
  const [jobs] = useState(() => new JobQueue((job, error) => jobError.current(job, error)));
  const jobList = useSyncExternalStore(jobs.subscribe, jobs.getSnapshot);
  // An analysed track's BPM, key and waveform change: its row is drawn from
  // the answer at once, and the library is re-read once the run is over so
  // every view holds what was written. A running job that moves files (Auto
  // Relocate) holds the run: the status bar says it is queued, and no track
  // is read from where its file used to be (#286).
  const analysisHeld = jobList.some((job) => job.holdsAnalysis && job.state !== "queued");
  const analysis = useAnalysis(
    useCallback((id: string, result: AnalysisResult) => {
      setPendingEdits((edits) =>
        new Map(edits).set(id, {
          analysed: result.analysed ?? 1,
          bpmX100: result.bpmX100,
          key: result.key,
          durationSec: result.durationSec,
        }),
      );
    }, []),
    useCallback(() => {
      void getBackend().then((backend) => backend.reloadLibrary());
    }, []),
    analysisPrefs,
    analysisHeld,
  );
  // What is in flight out of the browser: a playlist takes the ids, a deck
  // takes the one row under the hand.
  const [draggedTracks, setDraggedTracks] = useState<TrackDrag | null>(null);
  /**
   * The last thing the app has to say, and whether it went wrong.
   *
   * One channel, two colours: "Added 3 tracks" and "rekordbox is running, so
   * the library is open read-only" arrive the same way and are not the same
   * kind of news.
   */
  const [note, setNote] = useState<{ text: string; failed: boolean; busy?: boolean } | null>(null);
  const [playerError, setPlayerError] = useState<string | null>(null);
  const report = useCallback((text: string) => setNote({ text, failed: false }), []);
  const refuse = useCallback((text: string) => setNote({ text, failed: true }), []);
  const stopJob = useCallback((id: number) => jobs.stop(id), [jobs]);
  jobError.current = (job, error) => refuse(error instanceof Error ? error.message : `${job.label} failed.`);
  // A waveform click whose track could not be previewed says why.
  useEffect(() => onPreviewError(refuse), [refuse]);
  // Choosing another playlist in the tree stops the preview, as rekordbox's
  // `BrowseListViewer::currentBrowseChanged` does when the tree's selected
  // item changed. The row with its stop button is gone with the old list,
  // and a preview left playing had nothing left to stop it (#242).
  const previewedNode = useRef(selectedNode?.id ?? null);
  useEffect(() => {
    const id = selectedNode?.id ?? null;
    if (id === previewedNode.current) return;
    previewedNode.current = id;
    void stopPreview();
  }, [selectedNode?.id]);
  const openLog = useCallback(() => {
    void getBackend()
      .then((backend) => backend.openLog())
      .catch((error) => refuse(error instanceof Error ? error.message : String(error)));
  }, [refuse]);
  useEffect(() => {
    setNote((current) => {
      if (!current?.failed) return current;
      if (summary?.readOnly === false && current.text === refusal(false)) return null;
      if (!advancedPrefs.protectLibrary && current.text === refusal(true)) return null;
      return current;
    });
  }, [summary?.readOnly, advancedPrefs.protectLibrary]);
  const explainEditLock = useCallback(() => {
    refuse(refusal(advancedPrefs.protectLibrary));
  }, [refuse, advancedPrefs.protectLibrary]);
  // Bumped whenever the library changes underneath us, which drops cached
  // pages. Without it an edit's effect never reached the table.
  const [libraryGeneration, setLibraryGeneration] = useState(0);
  // Tag List edits keep the generation, so the filter bar over the Tag List
  // follows this instead.
  const [tagListRevision, setTagListRevision] = useState(0);
  // Edits shown at once, dropped when the backend's reload lands. A write
  // makes the backend re-read the library — 243 ms on the real collection —
  // and waiting for that before a star fills in feels broken.
  const [pendingEdits, setPendingEdits] = useState<Map<string, Partial<RowDto>>>(
    () => new Map(),
  );
  // Read once: the platform cannot change while the window is open, and
  // deciding it per key press would run a regex on every stroke.
  const platform = useMemo(detectPlatform, []);

  useEffect(() => {
    let cancelled = false;
    let stopReady: (() => void) | undefined;
    let stopProblem: (() => void) | undefined;

    /** One attempt at the first load. False means the library is not up yet. */
    const attempt = async (backend: Backend) => {
      try {
        // Devices are deliberately not in here. A stick that cannot be read
        // must not stop the collection from appearing, and it used to: this
        // was one `Promise.all`, so any of the three failing left the window
        // on "Loading…" with nothing said.
        const [nodes, info] = await Promise.all([
          backend.playlistTree(),
          backend.librarySummary(),
        ]);
        if (cancelled) return true;
        setTree(withSources(nodes));
        setSummary(info);
        setLoadError(null);
        setMissingLibrary(null);
        // The playlist that was open at exit, when it is still there — it can
        // have been deleted between runs, so this is a lookup, not a promise.
        const remembered = nodes.find((n) => n.id === restored.selectedNodeId);
        setSelectedNode(remembered ?? nodes.find((n) => n.kind === "playlist") ?? nodes[0] ?? null);
        setSessionReady(true);
        void backend
          .listDevices()
          .then((volumes) => {
            if (!cancelled) setDevices(volumes);
          })
          .catch(() => {
            // Nothing to say: no devices is the normal case.
          });
        return true;
      } catch {
        // The usual reason is that the backend is still reading the library,
        // which the ready event below will tell us about.
        return false;
      }
    };

    void (async () => {
      const backend = await getBackend();
      if (cancelled) return;
      // Subscribed before the first attempt, not after. The library can become
      // ready in the gap between a failed attempt and a later subscription,
      // and that gap is exactly where the window used to get stuck.
      stopReady = backend.onLibraryReady(() => {
        void attempt(backend);
      });
      const applyProblem = (problem: LibraryProblem | null) => {
        if (cancelled || problem === null) return;
        if (problem.kind === "failed") {
          // A library that is there and would not open is reported, not asked about.
          setMissingLibrary(null);
          setLoadError(problem.message);
        } else {
          setMissingLibrary(problem);
        }
      };
      stopProblem = backend.onLibraryProblem(applyProblem);
      // Asked as well: with no library at all the backend gives up before
      // this window has subscribed, and the event is gone.
      if (!(await attempt(backend))) applyProblem(await backend.libraryProblem().catch(() => null));
    })();

    return () => {
      cancelled = true;
      stopReady?.();
      stopProblem?.();
    };
    // `restored` is read once and never changes, but the rule cannot know that
    // and the id is genuinely read here.
  }, [restored.selectedNodeId]);

  // LINK, for the status bar: on or off and how many players are on it.
  // Read once and then by event, so the strip follows Preferences' switch
  // without owning it.
  const [link, setLink] = useState<LinkStatus | null>(null);
  const [linkPeers, setLinkPeers] = useState<LinkPeerSeen[]>([]);
  const [linkBusy, setLinkBusy] = useState(false);
  useEffect(() => {
    let live = true;
    const stops: Array<() => void> = [];
    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      stops.push(backend.onLinkStatus((status) => live && setLink(status)));
      stops.push(backend.onLinkPeers((peers) => live && setLinkPeers(peers)));
      const [status, peers] = await Promise.all([backend.linkStatus(), backend.linkPeers()]);
      if (live) {
        setLink(status);
        setLinkPeers(peers);
      }
      // Re-check why LINK is off when the window regains focus: quitting
      // rekordbox frees the ports, and the "unavailable" warning should clear
      // without a restart rather than lingering after the reason is gone.
      const refresh = () => {
        void backend.linkStatus().then((s) => live && setLink(s));
      };
      window.addEventListener("focus", refresh);
      stops.push(() => window.removeEventListener("focus", refresh));
    })();
    return () => {
      live = false;
      stops.forEach((stop) => stop());
    };
  }, []);

  // On the interface chosen under DJ System, or the one the players are
  // reached through when none is.
  const linkInterface = stickDefaults.linkInterface;
  /** Turns LINK on or off and shows what came of it; the status says why not. */
  const setLinkOn = useCallback(async (on: boolean): Promise<LinkStatus> => {
    setLinkBusy(true);
    const backend = await getBackend();
    try {
      const status = on ? await backend.startLinkExport(
        linkInterface ?? undefined,
        {
          waveformColor: stickDefaults.waveformColor,
          waveformPosition: stickDefaults.waveformPosition,
          overviewWaveform: stickDefaults.overviewWaveform,
          keyDisplay: stickDefaults.keyDisplay,
        },
        stickDefaults.linkKeySort,
      ) : await backend.stopLinkExport();
      setLink(status);
      return status;
    } finally {
      setLinkBusy(false);
    }
  }, [linkInterface, stickDefaults.keyDisplay, stickDefaults.linkKeySort,
    stickDefaults.overviewWaveform, stickDefaults.waveformColor, stickDefaults.waveformPosition]);
  const toggleLink = useCallback(() => {
    void setLinkOn(!link?.on);
  }, [link?.on, setLinkOn]);
  const autoStartLink = useCallback(() => setLinkOn(true), [setLinkOn]);

  useAutoJoinLink(
    stickDefaults.autoJoinLink,
    linkPeers,
    link,
    linkBusy,
    autoStartLink,
  );

  // The tempo-master controls: each returns LINK's fresh status.
  const setLinkMaster = useCallback((on: boolean) => {
    void (async () => setLink(await (await getBackend()).setLinkMaster(on)))();
  }, []);
  const nudgeLinkMaster = useCallback((deltaBpm: number) => {
    void (async () => setLink(await (await getBackend()).nudgeLinkMaster(deltaBpm)))();
  }, []);
  const takeLinkMasterTempo = useCallback(() => {
    void (async () => setLink(await (await getBackend()).takeLinkMasterTempo()))();
  }, []);

  // The master player's BPM for the filter's `MASTER PLAYER ±` list: the
  // track on whichever deck is MASTER. `[ASSUME]` the track's own BPM, not
  // the deck's tempo-adjusted one — the tempo lives in the player and the
  // capture cannot say which rekordbox uses.
  const masterBpmX100 = (syncMaster === "b" ? playerTrackB : playerTrack)?.bpmX100 ?? null;
  // Related Tracks relate to the track on Player 1, as rekordbox's do.
  const relatedTo = playerTrack?.id ?? null;
  const spec: ViewSpec = useMemo(() => {
    const base = { ...specForNode(selectedNode, query, sortState, viewPrefs.keyDisplay, viewPrefs.keySort, relatedTo, deviceLibraries.revision), searchField };
    // Only while the bar is showing: hiding it puts the whole list back,
    // so a closed bar can never be silently narrowing the library.
    const filter = filterOpen ? toSpecFilter(filterState, masterBpmX100) : undefined;
    return filter ? { ...base, filter } : base;
  }, [selectedNode, sortState, query, searchField, filterOpen, filterState, masterBpmX100, viewPrefs.keyDisplay, viewPrefs.keySort, relatedTo, deviceLibraries.revision]);

  // What the bar's lists offer, from Rust, for the source and query alone.
  // Re-asked when either changes or the library does, and only while the bar
  // is open — a closed bar costs nothing.
  const tagListKey = selectedNode?.kind === "tagList" ? tagListRevision : 0;
  useEffect(() => {
    if (!filterOpen) return;
    let live = true;
    void (async () => {
      const backend = await getBackend();
      try {
        const values = await backend.filterValues({ ...specForNode(selectedNode, query, null, "classic", "alphabetical", relatedTo), searchField });
        if (live) setFilterValues(values);
      } catch {
        // The library is not up yet; the ready event re-runs this through
        // `libraryGeneration`.
      }
    })();
    return () => {
      live = false;
    };
  }, [filterOpen, selectedNode, query, searchField, libraryGeneration, relatedTo, tagListKey]);

  const handleSort = useCallback((column: SortColumn) => {
    setSortState((s) => nextSort(s, column));
  }, []);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let stopTagList: (() => void) | undefined;
    let live = true;
    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      stopTagList = backend.onTagListChanged(() => setTagListRevision((n) => n + 1));
      stop = backend.onLibraryChanged((generation) => {
        setLibraryGeneration(generation);
        // The reload carries the edits, so the overlay has done its job.
        setPendingEdits(new Map());
        // The tree can change too — a playlist gained tracks, or one was
        // deleted — so it is re-read rather than assumed still right.
        void backend.playlistTree().then((nodes) => setTree(withSources(nodes)));
      });
    })();
    return () => {
      live = false;
      stop?.();
      stopTagList?.();
    };
  }, []);

  const bounds = useCallback(
    () => ({ ...TREE_BOUNDS, available: bodyRef.current?.clientWidth ?? 0 }),
    [],
  );

  // Re-clamp when the window changes: a width that fitted a wide window can
  // leave the track list with nothing in a narrow one.
  useEffect(() => {
    const body = bodyRef.current;
    if (!body || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      setTreeWidth((w) => clampWidth(w, bounds()));
    });
    observer.observe(body);
    return () => {
      observer.disconnect();
    };
  }, [bounds]);

  const onSplitterDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      dragFrom.current = { x: event.clientX, width: treeWidth };
      // Capture, so the drag survives the pointer leaving the 4px handle.
      event.currentTarget.setPointerCapture(event.pointerId);
      event.preventDefault();
    },
    [treeWidth],
  );

  const onSplitterMove = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      const from = dragFrom.current;
      if (!from) return;
      setTreeWidth(clampWidth(from.width + (event.clientX - from.x), bounds()));
    },
    [bounds],
  );

  const onSplitterUp = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    dragFrom.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }, []);

  /** Runs one edit and reports what happened, refusals included. */
  const runEdit = useCallback(
    async (what: string, edit: (b: Awaited<ReturnType<typeof getBackend>>) => Promise<unknown>) => {
      // Library Protection is a choice the backend cannot see, so it is
      // refused here, with the same words the menu uses. rekordbox holding
      // the database is the backend's to refuse, checked as the write starts.
      if (advancedPrefs.protectLibrary) {
        refuse(refusal(true));
        return;
      }
      const backend = await getBackend();
      try {
        await edit(backend);
        report(what);
      } catch (e) {
        refuse(e instanceof Error ? e.message : "That could not be saved.");
      }
    },
    [report, refuse, advancedPrefs.protectLibrary],
  );

  /** Shows an edit at once, so the interface does not wait on the reload. */
  const showPending = useCallback((id: string, patch: Partial<RowDto>) => {
    setPendingEdits((edits) => new Map(edits).set(id, { ...edits.get(id), ...patch }));
  }, []);

  /**
   * A file the Explorer lists that the library does not hold cannot be
   * edited: the write would match no row. Said once, here, for every edit.
   */
  const refuseLoose = useCallback(
    (id: string) => {
      if (!isLooseId(id)) return false;
      refuse("That file is not in the collection. Import it first.");
      return true;
    },
    [refuse],
  );

  /** Rates one track from the list, or the information panel's whole selection. */
  const rateTracks = useCallback(
    (ids: readonly string[], stars: number) => {
      if (ids.some(refuseLoose)) return;
      for (const id of ids) showPending(id, { rating: stars });
      void runEdit(stars === 0 ? "Rating cleared." : `Rated ${stars} of 5.`, (b) =>
        b.edits.setTrackRating(ids, stars),
      );
    },
    [runEdit, showPending, refuseLoose],
  );
  const rateTrack = useCallback((id: string, stars: number) => rateTracks([id], stars), [rateTracks]);

  const commentTracks = useCallback(
    (ids: readonly string[], comment: string) => {
      if (ids.some(refuseLoose)) return;
      for (const id of ids) showPending(id, { comment });
      void runEdit("Comment saved.", (b) => b.edits.setTrackComment(ids, comment));
    },
    [runEdit, showPending, refuseLoose],
  );
  const commentTrack = useCallback((id: string, comment: string) => commentTracks([id], comment), [commentTracks]);

  const editTrackField = useCallback(
    (id: string, field: TrackField, value: string) => {
      if (refuseLoose(id)) return;
      // Shown before the round trip for the fields a row carries; the rest
      // belong to the information panel and arrive with the re-read.
      if (ROW_FIELDS.has(field)) showPending(id, { [field]: value });
      void runEdit(`${FIELD_LABEL[field]} saved.`, (b) =>
        b.edits.setTrackField([id], field, value),
      );
    },
    [runEdit, showPending, refuseLoose],
  );

  // Tracks a playlist already holds: left out, added again, or asked about
  // per Preferences. Null when the question was cancelled, which stops the add.
  const [duplicatePrompt, setDuplicatePrompt] = useState<{
    playlist: string; duplicates: number; total: number; settle: (answer: DuplicateAnswer | null) => void;
  } | null>(null);
  const resolveDuplicates = useCallback(
    async (playlistId: string, trackIds: string[], name: string): Promise<boolean | null> => {
      const backend = await getBackend();
      const held = await backend.edits.playlistDuplicates(playlistId, trackIds);
      if (held.length === 0) return false;
      const policy = advancedPrefs.duplicateTracks;
      if (policy !== "ask") return policy === "add";
      const answer = await new Promise<DuplicateAnswer | null>((settle) =>
        setDuplicatePrompt({ playlist: name, duplicates: held.length, total: trackIds.length, settle }));
      setDuplicatePrompt(null);
      if (!answer) return null;
      if (answer.remember) prefs.update("advanced", { duplicateTracks: answer.choice });
      return answer.choice === "add";
    },
    [advancedPrefs.duplicateTracks, prefs],
  );

  /**
   * Tracks (or Explorer files, imported on the way in) added to a playlist
   * as a status-bar job, a slice at a time, so a few hundred show how far
   * along they are and can be stopped between slices. Both a drag onto a
   * playlist and the Add To Playlist menu come here (#291).
   */
  const addTracksTo = useCallback(
    (playlistId: string, ids: readonly string[]) => {
      const name = tree.find((n) => n.id === playlistId)?.name ?? t("the playlist");
      jobs.add({
        label: t("Adding {count} to {name}", { count: ids.length, name }),
        total: ids.length,
        target: playlistId,
        pendingRows: ids.length,
        run: async ({ signal, update }) => {
          const backend = await getBackend();
          // Rows out of the Explorer that the library does not hold are
          // imported on the way in, as rekordbox's Add To Playlist does with
          // them [OBS 7, Winrig 2026-10-08].
          const { ids: trackIds, report: imported } = await importLoose(ids, (paths) => backend.importPaths(paths));
          if (imported && analysisPrefs.auto && imported.tracks.length > 0) analysis.add(imported.tracks);
          const skipped = imported?.skipped.length ?? 0;
          const tail = skipped > 0 ? `; ${skipped} skipped` : "";
          if (trackIds.length === 0) {
            refuse(`Nothing added to ${name}${tail}.`);
            return;
          }
          const allowDuplicates = await resolveDuplicates(playlistId, trackIds, name);
          if (allowDuplicates === null) {
            report(`Nothing added to ${name}${tail}.`);
            return;
          }
          // Tracks the playlist already holds and keeps once, so the note
          // counts only the ones that were added.
          const held = allowDuplicates ? 0 : (await backend.edits.playlistDuplicates(playlistId, trackIds)).length;
          update({ total: trackIds.length, pendingRows: trackIds.length });
          let added = 0;
          try {
            for (const slice of slices(trackIds, ADD_SLICE)) {
              if (signal.aborted) break;
              await backend.edits.addTracksToPlaylist(playlistId, slice, allowDuplicates);
              added += slice.length;
              update({ done: added, pendingRows: trackIds.length - added });
            }
          } finally {
            if (added > 0) setTree(withSources(await backend.playlistTree()));
          }
          const noun = (n: number) => `${n} track${n === 1 ? "" : "s"}`;
          const count = added - held;
          report(added < trackIds.length
            ? `Stopped: added ${added} of ${noun(trackIds.length)} to ${name}${tail}.`
            : count <= 0
              ? `Already in ${name}${tail}.`
              : `Added ${noun(count)} to ${name}${tail}.`);
        },
      });
    },
    [tree, report, refuse, analysisPrefs.auto, analysis, jobs, t, resolveDuplicates],
  );

  const addDraggedTo = useCallback(
    (playlistId: string) => {
      const ids = draggedTracks?.ids;
      setDraggedTracks(null);
      if (!ids || ids.length === 0) return;
      if (advancedPrefs.protectLibrary) {
        refuse(refusal(true));
        return;
      }
      addTracksTo(playlistId, ids);
    },
    [draggedTracks, refuse, advancedPrefs.protectLibrary, addTracksTo],
  );

  /**
   * Files dragged in from outside the app (Finder, Explorer) and dropped on
   * a playlist: imported, then added to that playlist, the way a dragged
   * track already is. Dropped on the Playlists root or a playlist folder,
   * each folder becomes a playlist there instead (see `dropFolders`).
   */
  const importDroppedPathsTo = useCallback(
    (playlistId: string, paths: string[]) => {
      if (advancedPrefs.protectLibrary) {
        refuse(refusal(true));
        return;
      }
      const target = tree.find((n) => n.id === playlistId);
      if (playlistId === "playlists" || target?.kind === "folder") {
        // Queued and shown in the status bar like any long import; each
        // folder is one transaction, so Stop lands between folders (#291).
        jobs.add({
          label: paths.length === 1 ? t("Importing 1 folder…") : t("Importing {count} folders…", { count: paths.length }),
          total: paths.length,
          stoppable: paths.length > 1,
          run: async ({ signal, update }) => {
            const { dropFolders } = await import("@/lib/folderDrop");
            await dropFolders(playlistId, paths, t, report, refuse, setTree, analysisPrefs.auto ? analysis.add : undefined,
              signal, (done) => update({ done }));
          },
        });
        return;
      }
      const name = target?.name ?? t("the playlist");
      jobs.add({
        label: t("Finding tracks for {name}", { name }),
        target: playlistId,
        run: async ({ signal, update }) => {
          const backend = await getBackend();
          // The folder is walked first, so every file is known before any is
          // imported and the bar can say how far along it is.
          const files = await backend.listImportFiles(paths);
          if (signal.aborted) return;
          if (files.length === 0) {
            refuse(`No tracks found to add to ${name}.`);
            return;
          }
          const total = files.length;
          const noun = (n: number) => `${n} track${n === 1 ? "" : "s"}`;
          // Importing is the first half of the bar and adding the second.
          update({ total: total * 2, done: 0, pendingRows: total, label: t("Importing {count} into {name}", { count: total, name }) });
          // One re-read of the library for the whole import, not one a
          // slice, and one even when a slice fails, so the slices that
          // landed are shown.
          let landed = 0;
          let sliced;
          try {
            sliced = await importInSlices(files, (slice) => backend.importPathsSlice(slice), signal, (reached, sofar) => {
              landed = sofar.imported;
              update({ done: reached });
            });
          } finally {
            if (landed > 0) await backend.reloadLibrary();
          }
          // Files the library already held still belong in the playlist.
          const { ids: toAdd, tracks: fresh, skipped, existing } = sliced;
          update({ label: t("Adding {count} to {name}", { count: total, name }) });
          const allowDuplicates = signal.aborted ? false : await resolveDuplicates(playlistId, toAdd, name);
          let added = 0;
          for (const slice of slices(toAdd, ADD_SLICE)) {
            if (signal.aborted || allowDuplicates === null) break;
            await backend.edits.addTracksToPlaylist(playlistId, slice, allowDuplicates);
            added += slice.length;
            update({ done: total + added, pendingRows: toAdd.length - added });
          }
          setTree(await backend.playlistTree());
          if (analysisPrefs.auto && fresh.length > 0) analysis.add(fresh);
          const already = existing > 0 ? `; ${existing} already in the library` : "";
          const left = skipped > 0 ? `; ${skipped} skipped` : "";
          report(signal.aborted
            ? `Stopped: ${noun(added)} added to ${name}${left}${already}.`
            : `Added ${noun(added)} to ${name}${left}${already}.`);
        },
      });
    },
    [tree, report, refuse, advancedPrefs.protectLibrary, analysisPrefs.auto, analysis, jobs, t, resolveDuplicates],
  );

  const importDroppedFilesTo = useCallback(
    (playlistId: string, files: File[]) => {
      // Resolve immediately: macOS's drag pasteboard belongs to the current
      // OS drag, not to the File object retained by this callback.
      void droppedFilePaths(files)
        .then((paths) => importDroppedPathsTo(playlistId, paths))
        .catch((e: unknown) => refuse(e instanceof Error ? e.message : "Those files could not be imported."));
    },
    [importDroppedPathsTo, refuse],
  );

  useEffect(() => subscribeNativeFileDrops((drop) => {
    const target = document.elementFromPoint(drop.x, drop.y);
    const row = target?.closest<HTMLElement>("[data-file-drop-playlist]");
    const playlistId = row?.dataset.fileDropPlaylist
      ?? (target?.closest('[data-testid="track-scroll"]') && selectedNode?.kind === "playlist"
        ? selectedNode.id
        : undefined);
    if (playlistId) importDroppedPathsTo(playlistId, drop.paths);
    else refuse("Drop files or folders onto a playlist to import them.");
  }), [selectedNode, importDroppedPathsTo, refuse]);

  /**
   * The same drop, for files dropped straight into the open playlist's own
   * list rather than onto its row in the tree — the tree row is easy to miss
   * with a full list open, and an empty playlist has no rows to aim at.
   * `undefined` outside a real playlist so the list gives no false promise
   * of a drop it would not act on.
   */
  const importDroppedFilesIntoOpen = useMemo(
    () =>
      readOnly || selectedNode?.kind !== "playlist"
        ? undefined
        : (files: File[]) => importDroppedFilesTo(selectedNode.id, files),
    [readOnly, selectedNode, importDroppedFilesTo],
  );

  /**
   * What loading a track onto a deck goes through: a track whose file is
   * missing is not loaded, the deck keeps what it had, and the status bar
   * says so in rekordbox's words [OBS rekordbox 7.2.19 static:
   * `UiPlayer::handleMessageDragAndDrop` @0x101abadc4 opens only a file that
   * is there, else shows `kPlayerOperateErrorLoadMissingFile`, "Load error.
   * The file could not be found.", @0x101abb0f4 and returns].
   */
  const deckLoaders = useMemo(() => {
    const guard = (set: (row: RowDto | null) => void) => (row: RowDto | null) => {
      if (row?.missing === true) {
        refuse(t("Load error. The file could not be found."));
        return;
      }
      set(row);
    };
    return { a: guard(setPlayerTrack), b: guard(setPlayerTrackB) };
  }, [refuse, t]);

  /**
   * Loading a dragged track into a deck.
   *
   * A pair of stable callbacks rather than one taking a deck id: `Player` is
   * memoized, and a closure built during render would give it a new prop every
   * frame the app draws.
   */
  const loadDroppedInto = useMemo(() => {
    const into = (put: (row: RowDto) => void) => () => {
      const row = draggedTracks?.row;
      setDraggedTracks(null);
      // Nothing is written: which track a deck is holding is not part of the
      // library, so this is the one drop that cannot be refused.
      if (row) put(row);
    };
    return { a: into(deckLoaders.a), b: into(deckLoaders.b) };
  }, [draggedTracks, deckLoaders]);

  // Dropping a track onto a CDJ row in the LINK strip tells that player to
  // load it from us over Pro DJ Link.
  const loadDroppedOnLink = useCallback(
    (playerNumber: number) => {
      const id = draggedTracks?.row.id;
      setDraggedTracks(null);
      if (!id) return;
      void (async () => {
        const backend = await getBackend();
        try {
          await backend.loadTrackOnLink(playerNumber, id);
        } catch (e) {
          refuse(e instanceof Error ? e.message : "That track could not be sent to the player.");
        }
      })();
    },
    [draggedTracks, refuse],
  );

  /** The same three decks, loaded from the track menu or from a click. */
  const loadInto = deckLoaders;
  const loadTrack = useCallback(
    (deck: DeckId, row: RowDto) => loadInto[deck === "b" ? "b" : "a"](row),
    [loadInto],
  );
  const loadSelectedInto = useMemo(() => {
    if (!selectedRow) return { a: undefined, b: undefined };
    return {
      a: () => deckLoaders.a(selectedRow),
      b: () => deckLoaders.b(selectedRow),
    };
  }, [selectedRow, deckLoaders]);

  /**
   * The tree's context menu, and the track's.
   *
   * Every one of these writes, so each says why it could not rather than
   * looking as if it worked — the same rule the drop handler above follows.
   */
  const afterWrite = useCallback(
    async (said: string) => {
      const backend = await getBackend();
      // The tree now, so the node that was made is there when the note says
      // so. The generation is the backend's alone, announced as
      // `library:changed` after every write: a count bumped here as well
      // could land on the number the backend announces for the next edit,
      // and a generation that does not change is a page that is not
      // refetched — a rating lit for a moment and went out.
      setTree(withSources(await backend.playlistTree()));
      report(said);
    },
    [report],
  );

  // A write, awaited: true once it is saved and reported, false if refused.
  const writeNow = useCallback(
    async (run: (backend: Backend) => Promise<string>): Promise<boolean> => {
      const backend = await getBackend();
      try {
        const said = await run(backend);
        await afterWrite(said);
        return true;
      } catch (e) {
        refuse(e instanceof Error ? e.message : "That could not be saved.");
        return false;
      }
    },
    [afterWrite, refuse],
  );
  const write = useCallback(
    (run: (backend: Backend) => Promise<string>) => {
      void writeNow(run);
    },
    [writeNow],
  );

  // rekordbox asks OK/Cancel ("Remove") before a track leaves a playlist, a
  // history or the Tag List, from the track menu and from the Delete key
  // [OBS static, rekordbox 7.2.19 arm64: `ListViewer::showPopupMenu`
  // @0x100407278/0x1004073a8/0x10040748c and `ListViewer::deleteKeyPressed`
  // @0x100405eb8 call `BrowseAlertWindow::showOkCancelBox` before
  // `deleteFromTagList` / `removeTrackOrderFromList`]. The message is passed
  // already translated.
  const confirmRemoval = useCallback(async (message: string): Promise<boolean> => {
    const backend = await getBackend();
    return backend.confirm(message, { yes: t("OK"), no: t("Cancel") });
  }, [t]);

  const createPlaylistIn = useCallback(
    (node: TreeNode) => {
      // rekordbox's own default name, from german.lang. It goes where the menu
      // was opened: first inside a folder, directly below a playlist.
      const parent = parentFor(tree, node);
      const index =
        node.kind === "folder" ? 0 : childrenOf(tree, parent).findIndex((n) => n.id === node.id) + 1;
      write(async (backend) => {
        await backend.edits.createPlaylist("New playlist", parent, index);
        return "Created New playlist.";
      });
    },
    [write, tree],
  );

  const createFolderIn = useCallback(
    (node: TreeNode) => {
      write(async (backend) => {
        await backend.edits.createFolder("New folder", parentFor(tree, node));
        return "Created New folder.";
      });
    },
    [write, tree],
  );

  // The intelligent playlist editor: over a new rule under a node, or over
  // an existing playlist's, read from the backend first.
  const [smartEditor, setSmartEditor] = useState<
    { mode: "create"; parent: string; name: string; rule: SmartRule } | { mode: "edit"; id: string; name: string; rule: SmartRule } | null
  >(null);
  // The My Tags a rule can name, read each time the editor opens so a tag
  // made since is there to pick.
  const [smartTags, setSmartTags] = useState<TrackLookups["myTagCategories"]>([]);
  const smartEditorOpen = smartEditor !== null;
  useEffect(() => {
    if (!smartEditorOpen) return;
    let live = true;
    void getBackend()
      .then((b) => b.trackLookups())
      .then((l) => {
        if (live) setSmartTags(l.myTagCategories);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [smartEditorOpen]);
  const createSmartPlaylistIn = useCallback(
    (node: TreeNode) => {
      setSmartEditor({
        mode: "create",
        parent: parentFor(tree, node),
        name: "Untitled Intelligent List",
        rule: { logic: "all", conditions: [] },
      });
    },
    [tree],
  );
  const editSmartPlaylist = useCallback(
    (node: TreeNode) => {
      void (async () => {
        const backend = await getBackend();
        try {
          const rule = await backend.smartRule(node.id);
          setSmartEditor({ mode: "edit", id: node.id, name: node.name, rule });
        } catch (e) {
          refuse(e instanceof Error ? e.message : "That rule could not be read.");
        }
      })();
    },
    [refuse],
  );
  const saveSmartPlaylist = useCallback(
    (name: string, rule: SmartRule) => {
      const editing = smartEditor;
      setSmartEditor(null);
      if (editing === null) return;
      write(async (backend) => {
        if (editing.mode === "create") {
          await backend.edits.createSmartPlaylist(name, editing.parent, rule);
          return `Created ${name}.`;
        }
        await backend.edits.setSmartRule(editing.id, rule);
        if (name !== editing.name) await backend.edits.renamePlaylist(editing.id, name);
        return `Saved ${name}.`;
      });
    },
    [smartEditor, write],
  );

  // Add Artwork on a playlist: pick a picture, file it with the playlist.
  const addPlaylistArtwork = useCallback(
    (node: TreeNode) => {
      void (async () => {
        const backend = await getBackend();
        const image = await backend.pickImage("Choose the artwork");
        if (image === null) return;
        write(async (b) => {
          await b.edits.addPlaylistArtwork(node.id, image);
          return `Artwork added to ${node.name}.`;
        });
      })();
    },
    [write],
  );

  // Sort Items: the folder's children in name order, case and accents
  // aside [ASSUME: whether rekordbox puts folders first is not captured],
  // one move each, from the top.
  const sortItems = useCallback(
    (folder: TreeNode) => {
      const children = childrenOf(tree, folder.id);
      if (children.length < 2) return;
      const sorted = [...children].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
      write(async (backend) => {
        for (const [index, node] of sorted.entries()) {
          await backend.edits.movePlaylist(node.id, folder.id, index);
        }
        return `Sorted ${folder.name}.`;
      });
    },
    [tree, write],
  );

  // Add To Shortcut: the rail takes the playlist as a button of its own.
  // rekordbox 7.2.11 takes the same playlist twice; a second button that
  // opens the same thing is nothing to a user, so this keeps one.
  const addToShortcut = useCallback(
    (node: TreeNode) => {
      if (viewPrefs.shortcuts.includes(node.id)) return;
      prefs.update("view", { shortcuts: [...viewPrefs.shortcuts, node.id] });
    },
    [prefs, viewPrefs.shortcuts],
  );
  const deleteShortcut = useCallback(
    (id: string) => {
      prefs.update("view", { shortcuts: viewPrefs.shortcuts.filter((other) => other !== id) });
    },
    [prefs, viewPrefs.shortcuts],
  );
  const railShortcuts = useMemo(
    () =>
      viewPrefs.shortcuts.flatMap((id) => {
        // A playlist deleted since it was made a shortcut is not drawn; it
        // is dropped from the list the next time one is added or removed.
        const node = tree.find((n) => n.id === id);
        return node ? [{ id, name: node.name, selected: selectedNode?.id === id }] : [];
      }),
    [viewPrefs.shortcuts, tree, selectedNode],
  );
  const openShortcut = useCallback(
    (id: string) => {
      const node = tree.find((n) => n.id === id);
      if (node) setSelectedNode(node);
    },
    [tree],
  );

  const deleteNode = useCallback(
    (node: TreeNode) => {
      write(async (backend) => {
        const history = await backend.edits.deletePlaylist(node.id);
        setEditHistory(history);
        setLibraryEditHistory(history.undoLabel, history.redoLabel);
        if (selectedNode?.id === node.id) setSelectedNode(null);
        return `Deleted ${node.name}.`;
      });
    },
    [write, selectedNode],
  );

  const runLibraryHistory = useCallback(
    (action: "undo" | "redo") => {
      write(async (backend) => {
        const history = action === "undo"
          ? await backend.edits.undoEdit()
          : await backend.edits.redoEdit();
        setEditHistory(history);
        setLibraryEditHistory(history.undoLabel, history.redoLabel);
        return action === "undo" ? "Edit undone." : "Edit redone.";
      });
    },
    [write],
  );

  const renameNode = useCallback(
    (node: TreeNode, name: string) => {
      write(async (backend) => {
        await backend.edits.renamePlaylist(node.id, name);
        return `Renamed to ${name}.`;
      });
    },
    [write],
  );

  const moveNode = useCallback(
    (node: TreeNode, parent: string, index: number) => {
      write(async (backend) => {
        await backend.edits.movePlaylist(node.id, parent, index);
        return `Moved ${node.name}.`;
      });
    },
    [write],
  );

  const reorderPlaylist = useCallback(
    (playlist: string, order: readonly string[]) => {
      if (order.length === 0) return;
      write(async (backend) => {
        await backend.edits.reorderPlaylist(playlist, [...order]);
        return "Playlist reordered.";
      });
    },
    [write],
  );
  const reorderPlaylistTracks = useCallback(
    (order: readonly string[]) => {
      if (spec.source.kind === "playlist") reorderPlaylist(spec.source.id, order);
    },
    [reorderPlaylist, spec.source],
  );

  /**
   * Whether the rows can be dragged into a new order.
   *
   * Only a playlist has an order of its own to change. It also has to be the
   * order on screen: sorted by a column, or narrowed by the search or the
   * filter, the rows are a rearrangement or a subset of the playlist, and
   * writing what is visible as the whole order would scramble the rest.
   */
  const canReorder =
    spec.source.kind === "playlist" &&
    !readOnly &&
    sortState.column === "trackNo" &&
    query === "" &&
    spec.filter === undefined;

  // Asked first, as rekordbox asks (its history wording); there is no undo.
  const removeTracksFromHistory = useCallback(
    async (history: string, ids: readonly string[]): Promise<boolean> => {
      if (ids.length === 0) return false;
      if (!(await confirmRemoval(t("Are you sure you want to remove the selected tracks?")))) return false;
      return writeNow(async (backend) => {
        await backend.edits.removeFromHistory(history, [...ids]);
        return `Removed ${ids.length} play${ids.length === 1 ? "" : "s"} from the history.`;
      });
    },
    [confirmRemoval, t, writeNow],
  );
  const removeFromHistory = useCallback(
    (ids: readonly string[]): Promise<boolean> => {
      if (spec.source.kind !== "history") return Promise.resolve(false);
      return removeTracksFromHistory(spec.source.id, ids);
    },
    [removeTracksFromHistory, spec.source],
  );

  const resetPlayCount = useCallback(
    (ids: readonly string[]) => {
      if (ids.length === 0) return;
      write(async (backend) => {
        await backend.edits.resetPlayCount([...ids]);
        return `DJ Play Count reset on ${ids.length} track${ids.length === 1 ? "" : "s"}.`;
      });
    },
    [write],
  );

  const convertMemoryCues = useCallback(
    (row: RowDto) => {
      void (async () => {
        const backend = await getBackend();
        try {
          const made = await backend.edits.convertMemoryCuesToHot(row.id);
          report(
            made === 0
              ? "No memory cue to convert, or every hot cue slot is taken."
              : `${made} memory cue${made === 1 ? "" : "s"} converted to hot cues.`,
          );
        } catch (e) {
          refuse(e instanceof Error ? e.message : "The cues could not be converted.");
        }
      })();
    },
    [report, refuse],
  );

  // Asked first, as rekordbox asks: the tracks leave every playlist as well
  // as the collection, and there is no undo in the window.
  const removeFromCollection = useCallback(
    async (ids: readonly string[]): Promise<boolean> => {
      if (ids.length === 0) return false;
      const backend = await getBackend();
      const count = `${ids.length} track${ids.length === 1 ? "" : "s"}`;
      const sure = await backend.confirm(
        `Remove ${count} from the collection? This can’t be undone. The files stay where they are.`,
      );
      if (!sure) return false;
      jobs.add({
        label: t("Removing {count} from the collection", { count: ids.length }),
        total: ids.length,
        run: async ({ signal, update }) => {
          // Each slice is one transaction: a Stop, or a slice that fails,
          // leaves the slices before it removed whole and the rest untouched.
          let removed = 0;
          try {
            await removeInSlices(ids, (slice) => backend.edits.removeFromCollection(slice), signal, (count) => {
              removed = count;
              update({ done: count });
            });
          } finally {
            if (removed > 0) setTree(withSources(await backend.playlistTree()));
          }
          const noun = `${removed} track${removed === 1 ? "" : "s"}`;
          report(removed < ids.length ? `Stopped: removed ${noun} from the collection.` : `Removed ${noun} from the collection.`);
        },
      });
      return true;
    },
    [jobs, report, t],
  );

  // A missing track's menu [OBS rekordbox 7.2.14, issue #201]. Auto
  // Relocate searches Preferences' Auto Relocate Search Folders for each
  // selected track's file name; Relocate asks for each selected track's file
  // in turn, as rekordbox's `relocateSelectedFiles` does (src/lib/relocate.ts).
  const relocateSearch = useMemo((): RelocateSearch => ({
    folders: advancedPrefs.relocateUserFolders ? [...advancedPrefs.relocateFolders] : [],
    music: advancedPrefs.relocateMusic,
    video: advancedPrefs.relocateVideo,
    desktop: advancedPrefs.relocateDesktop,
  }), [
    advancedPrefs.relocateUserFolders, advancedPrefs.relocateFolders, advancedPrefs.relocateMusic,
    advancedPrefs.relocateVideo, advancedPrefs.relocateDesktop,
  ]);
  // [OBS rekordbox 7.2.19 static] rekordbox runs the search under a
  // "Searching..." progress window (`UpdateMissingFiles`, a
  // `ThreadWithProgressWindow` built @0x1012a760c, run from
  // `relocateSelectedFiles` @0x1012a6bd0). Here it is a status-bar job of
  // that name, so a long search is seen to be running (#286). It is one
  // backend call, so it has no Stop.
  const searching = useCallback(
    <T,>(work: () => Promise<T>): Promise<T> =>
      jobs.perform({ label: t("Searching..."), holdsAnalysis: true }, () => work()),
    [jobs, t],
  );
  const autoRelocate = useCallback(
    (ids: readonly string[]) => {
      if (ids.length === 0) return;
      write(async (b) => {
        const done = await searching(() => b.autoRelocate(relocateSearch, [...ids]));
        return done.unresolved > 0
          ? t("{relocated} relocated, {unresolved} not found in the search folders.", { ...done })
          : t("{relocated} relocated.", { ...done });
      });
    },
    [write, searching, relocateSearch, t],
  );
  const relocate = useCallback(
    (ids: readonly string[]) => {
      if (ids.length === 0) return;
      write(async (b) => {
        await relocateTracks(missingAmong(ids, (page) => b.relocationTargets(page)), relocateSteps(b, t));
        return "";
      });
    },
    [write, t],
  );

  // Import To Collection, over the Explorer's files: their ids are their
  // paths behind `file:`.
  const importToCollection = useCallback(
    (ids: readonly string[]) => {
      const paths = ids.filter(isLooseId).map((id) => id.slice("file:".length));
      if (paths.length === 0) return;
      // [OBS rekordbox 7.2.19 static] rekordbox adds files to the
      // collection under a progress window titled "Adding track data to the
      // Collection" (english.lang). Here it is a status-bar job of that name,
      // in slices of one transaction each, so a few hundred files show how
      // far along they are and Stop keeps the slices already imported (#291).
      jobs.add({
        label: t("Adding track data to the Collection"),
        total: paths.length,
        run: async ({ signal, update }) => {
          const backend = await getBackend();
          let landed = 0;
          let imported;
          try {
            imported = await importInSlices(paths, (slice) => backend.importPathsSlice(slice), signal, (reached, sofar) => {
              landed = sofar.imported;
              update({ done: reached });
            });
          } finally {
            // One re-read for the whole import, and one after a failed slice
            // too, so the slices that landed are shown.
            if (landed > 0) await backend.reloadLibrary();
          }
          if (analysisPrefs.auto && imported.tracks.length > 0) analysis.add(imported.tracks);
          const values = {
            imported: imported.imported, total: imported.imported + imported.skipped,
            skipped: imported.skipped, existing: imported.existing, count: paths.length,
          };
          const said = signal.aborted
            ? t("Stopped: imported {imported} of {count} files.", values)
            : imported.skipped === 0
              ? `Imported ${values.imported} of ${values.total} files`
              : `Imported ${values.imported} of ${values.total} files; ${values.skipped} skipped`;
          const already = imported.existing > 0 ? `; ${imported.existing} already in the library` : "";
          await afterWrite(signal.aborted ? said : `${said}${already}.`);
        },
      });
    },
    [afterWrite, analysisPrefs.auto, analysis, jobs, t],
  );

  // Analysis Lock: the GRID panel's lock, set from the list on every
  // selected track.
  const analysisLock = useCallback(
    (ids: readonly string[], on: boolean) => {
      if (ids.length === 0) return;
      void (async () => {
        const backend = await getBackend();
        try {
          for (const id of ids) await backend.edits.gridLock(id, on);
          report(`${on ? "Locked" : "Unlocked"} the analysis of ${ids.length} track${ids.length === 1 ? "" : "s"}.`);
        } catch (e) {
          refuse(e instanceof Error ? e.message : "The lock could not be set.");
        }
      })();
    },
    [report, refuse],
  );

  // Add To Playlist. Files the Explorer lists that the library does not hold
  // are imported first, as rekordbox's menu offers it over them [OBS 7,
  // Winrig 2026-10-08] and as a file dropped on a playlist already is.
  const addToPlaylist = useCallback(
    (playlist: string, ids: readonly string[]) => {
      if (ids.length === 0) return;
      addTracksTo(playlist, ids);
    },
    [addTracksTo],
  );

  const addToTagList = useCallback(
    (ids: readonly string[]) => {
      if (ids.length === 0) return;
      write(async (backend) => {
        await backend.edits.addToTagList([...ids]);
        return `Added ${ids.length} track${ids.length === 1 ? "" : "s"} to the Tag List.`;
      });
    },
    [write],
  );

  const reloadTag = useCallback(
    (ids: readonly string[]) => {
      if (ids.length === 0) return;
      write(async (backend) => {
        await backend.edits.reloadTags([...ids]);
        return `Tags reloaded on ${ids.length} track${ids.length === 1 ? "" : "s"}.`;
      });
    },
    [write],
  );

  // Asked first, as rekordbox asks; there is no undo.
  const removeFromTagList = useCallback(
    async (ids: readonly string[]): Promise<boolean> => {
      if (ids.length === 0) return false;
      const sure = await confirmRemoval(t(
        "Are you sure you want to remove the selected track(s) from the Tag List?\nTrack(s) will be removed from the Tag Lists of all synced devices.",
      ));
      if (!sure) return false;
      return writeNow(async (backend) => {
        await backend.edits.removeFromTagList([...ids]);
        return `Removed ${ids.length} track${ids.length === 1 ? "" : "s"} from the Tag List.`;
      });
    },
    [confirmRemoval, t, writeNow],
  );

  // Export Track: onto a connected stick, in no playlist.
  const exportTrackTo = useCallback(
    (path: string, ids: readonly string[]) => {
      if (ids.length === 0) return;
      const device = devices.find((d) => d.path === path);
      const name = device?.name ?? "the device";
      setSyncing(true);
      report(`Writing ${ids.length} track${ids.length === 1 ? "" : "s"} to ${name}…`);
      void (async () => {
        try {
          const backend = await getBackend();
          const written = await backend.exportTracksToDevice([...ids], path, stickDefaults, compatibilityFormat);
          report(exportSummary(name, written));
          setDevices(await backend.listDevices());
        } catch (e) {
          if (e && typeof e === "object" && "kind" in e && e.kind === "cancelled" || e instanceof Error && e.message === "Export stopped.") report("Export stopped.");
          else refuse(e instanceof Error ? e.message : "That export could not be written.");
        } finally {
          setSyncing(false);
        }
      })();
    },
    [devices, report, refuse, stickDefaults, compatibilityFormat],
  );

  // Export Loop As WAV: where to, then the loop's stretch of the track.
  const exportLoop = useCallback(
    (trackId: string, title: string, inMs: number, outMs: number) => {
      void (async () => {
        const backend = await getBackend();
        try {
          const frames = await backend.exportLoopWav(trackId, title, inMs, outMs);
          if (frames === null) return;
          report(`Loop written: ${((outMs - inMs) / 1000).toFixed(2)} s of ${title}.`);
        } catch (e) {
          refuse(e instanceof Error ? e.message : "The loop could not be written.");
        }
      })();
    },
    [report, refuse],
  );

  // What the track menu's Add To Playlist and Export Track offer: the
  // playlists under their folders, and the connected sticks.
  const menuPlaylists = useMemo(() => {
    // Folders' names accumulate down the tree, which is in order, so a
    // playlist's container has been named by the time it is reached.
    const names = new Map<string, string>();
    const out: { id: string; name: string }[] = [];
    for (const node of tree) {
      if (node.kind !== "playlist" && node.kind !== "folder") continue;
      const above = names.get(containerOf(tree, node)) ?? "";
      const name = above === "" ? node.name : `${above} › ${node.name}`;
      names.set(node.id, name);
      if (node.kind === "playlist") out.push({ id: node.id, name });
    }
    return out;
  }, [tree]);
  const menuDevices = useMemo(() => devices.map((d) => ({ id: d.path, name: d.name })), [devices]);

  // Asked first, as rekordbox asks, though Edit › Undo can bring them back.
  const removeTracksFromPlaylist = useCallback(
    async (playlist: string, ids: readonly string[]): Promise<boolean> => {
      if (ids.length === 0) return false;
      const sure = await confirmRemoval(t(
        "Are you sure you want to remove the selected track(s) from the playlist?\nTrack(s) will be removed from the playlists of all synced devices.",
      ));
      if (!sure) return false;
      return writeNow(async (backend) => {
        await backend.edits.removeTracksFromPlaylist(playlist, [...ids]);
        return `Removed ${ids.length} track${ids.length === 1 ? "" : "s"}.`;
      });
    },
    [confirmRemoval, t, writeNow],
  );
  const removeFromPlaylist = useCallback(
    (ids: readonly string[]): Promise<boolean> => {
      if (spec.source.kind !== "playlist") return Promise.resolve(false);
      return removeTracksFromPlaylist(spec.source.id, ids);
    },
    [removeTracksFromPlaylist, spec.source],
  );

  const revealTrack = useCallback((row: RowDto) => {
    void (async () => {
      const backend = await getBackend();
      try {
        await backend.revealTrack(row.id);
      } catch (e) {
        refuse(e instanceof Error ? e.message : "That file could not be shown.");
      }
    })();
  }, [refuse]);

  // Clear the note after a moment: it reports an action, not a state.
  useEffect(() => {
    if (note === null || note.busy) return;
    const timer = setTimeout(() => setNote(null), note.failed ? 10000 : 4000);
    return () => {
      clearTimeout(timer);
    };
  }, [note]);

  // Analysis writes the result to the library, so it is refused the way any
  // other write is while rekordbox holds the file or the library is protected.
  const ANALYSIS_REFUSED = "The library is read-only, so nothing can be analysed.";
  const [analysisSelection, setAnalysisSelection] = useState<readonly QueueItem[] | null>(null);
  /** Capture either browser's selection before opening the settings dialog. */
  const analyseTracks = useEventCallback((tracks: readonly { id: string; title: string }[]) => {
    if (tracks.length === 0) return;
    if (readOnly) {
      refuse(ANALYSIS_REFUSED);
      return;
    }
    setAnalysisSelection(tracks.map(({ id, title }) => ({ id, title })));
  });
  const analyseSelection = useEventCallback(() => analyseTracks(selectedTracks));
  const reportMainSelection = useCallback((tracks: { id: string; title: string }[]) => {
    setSelectedTracks(tracks);
    setInfoSelection(tracks.map((t) => t.id));
  }, []);
  const reportSubSelection = useCallback((tracks: { id: string; title: string }[]) => {
    setInfoSelection(tracks.map((t) => t.id));
  }, []);
  /** Configure one track: the deck's own, from its menu. */
  const analyseOne = useCallback(
    (id: string, title: string) => {
      if (readOnly) {
        refuse(ANALYSIS_REFUSED);
        return;
      }
      setAnalysisSelection([{ id, title }]);
    },
    [readOnly, refuse],
  );
  // Once, at launch, with Auto Analysis on: rekordbox asks "Auto Analysis is
  // starting." before it analyses the Collection tracks it never analysed.
  const [autoAnalysis, setAutoAnalysis] = useState<UnanalysedTracks | null>(null);
  const autoAnalysisAsked = useRef(false);
  useEffect(() => {
    if (autoAnalysisAsked.current || !sessionReady || summary === null) return;
    autoAnalysisAsked.current = true;
    void getBackend()
      .then(backend => autoAnalysisOffer(backend, { auto: analysisPrefs.auto, readOnly }))
      .then(setAutoAnalysis)
      .catch(() => {
        // No prompt is the quiet outcome; analysis is still there on demand.
      });
  }, [sessionReady, summary, analysisPrefs.auto, readOnly]);
  const startAutoAnalysis = useEventCallback(async (offer: UnanalysedTracks, settings: AnalysisChoice) => {
    setAutoAnalysis(null);
    if (readOnly) {
      refuse(ANALYSIS_REFUSED);
      return;
    }
    // Gathered before queueing, so stopping the run cannot be undone by a
    // page arriving after it.
    const tracks = [...offer.tracks];
    try {
      await takeRemainingPages(await getBackend(), offer.next, page => tracks.push(...page));
    } catch {
      // Analyse what was found; the rest is offered again at the next launch.
    }
    analysis.add(tracks, settings);
  });

  // Import from a picker: files (Import) or whole folders (Import Folder). Both
  // land the same way — pick, import, refresh the tree, queue Auto Analysis —
  // so the only difference is which native picker opens and the status wording.
  const runImport = useCallback(
    async (choosing: string, pick: (backend: Backend) => Promise<ImportReport | null>) => {
      report(choosing);
      try {
        const backend = await getBackend();
        const imported = await pick(backend);
        if (imported === null) {
          setNote(null);
          return;
        }
        const total = imported.imported + imported.skipped.length;
        const already = imported.existing.length > 0 ? `; ${imported.existing.length} already in the library` : "";
        report(
          imported.skipped.length === 0
            ? `Imported ${imported.imported} of ${total} files${already}.`
            : `Imported ${imported.imported} of ${total} files; ${imported.skipped.length} skipped${already}.`,
        );
        setTree(await backend.playlistTree());
        // Auto Analysis in Preferences: what just landed goes straight into
        // the queue, as rekordbox does unless told not to.
        if (analysisPrefs.auto && imported.tracks.length > 0) analysis.add(imported.tracks);
      } catch (e) {
        refuse(e instanceof Error ? e.message : "Those files could not be imported.");
      }
    },
    [report, refuse, analysisPrefs.auto, analysis],
  );

  const importFromMenu = useCallback(
    () => runImport("Choosing files to import…", (backend) => backend.importFiles()),
    [runImport],
  );

  const importFolderFromMenu = useCallback(
    () => runImport("Choosing a folder to import…", (backend) => backend.importFolder()),
    [runImport],
  );

  // File > Import rekordbox xml / iTunes Library, loaded on demand to keep
  // the first paint small.
  const importXmlFromMenu = useCallback(async (source: "rekordbox" | "itunes" = "rekordbox") => {
    const { importCollection } = await import("@/lib/xmlImport");
    await importCollection(source, t, setNote, async (backend, imported) => {
      setTree(await backend.playlistTree());
      if (analysisPrefs.auto && imported.tracks.length > 0) analysis.add(imported.tracks);
    });
  }, [t, analysisPrefs.auto, analysis]);

  const exportXmlFromMenu = useCallback(async () => {
    report("Choosing where to write the XML…");
    try {
      const backend = await getBackend();
      const written = await backend.exportXml();
      if (written === null) {
        setNote(null);
        return;
      }
      report(`Wrote ${written.toLocaleString()} tracks and the playlists as XML.`);
    } catch (e) {
      refuse(e instanceof Error ? e.message : "The XML could not be written.");
    }
  }, [report, refuse]);

  // A menu item, by id. The shell sends the id and nothing else; what it
  // means, and whether it is allowed right now, is decided in one place —
  // and the keyboard reaches it the same way on the platforms where the
  // webview keeps the accelerators from the native menu.
  const runMenu = useCallback((id: string) => {
    if (id === "undo" || id === "redo") {
      const action = id;
      const canRunLibrary = action === "undo" ? editHistory.canUndo : editHistory.canRedo;
      // Text fields keep WebKit's own history, and an armed grid editor owns
      // its labelled history. Otherwise the latest reversible library edit is
      // the app-level action.
      if (!isTyping(document.activeElement) && !hasEditHistory(action) && canRunLibrary) {
        runLibraryHistory(action);
        return;
      }
      runEditHistory(id);
      return;
    }
    const outcome = resolveMenu(id, readOnly, advancedPrefs.protectLibrary);
    if (!outcome) return;
    if ("refused" in outcome) {
      refuse(outcome.refused);
      return;
    }
    if (outcome.action === "import") {
      void importFromMenu();
      return;
    }
    if (outcome.action === "import-folder") {
      void importFolderFromMenu();
      return;
    }
    if (outcome.action === "import-xml") {
      void importXmlFromMenu();
      return;
    }
    if (outcome.action === "import-itunes") {
      void importXmlFromMenu("itunes");
      return;
    }
    if (outcome.action === "export-xml") {
      void exportXmlFromMenu();
      return;
    }
    if (outcome.action === "info") {
      setInfoOpen((open) => !open);
      return;
    }
    if (outcome.action === "sub") {
      setSubOpen((open) => !open);
      return;
    }
    if (outcome.action === "report-bug") { openReport(); return; }
    if (outcome.action === "tempo-slider") {
      prefs.update("view", { tempoSlider: !viewPrefs.tempoSlider });
      return;
    }
    if (outcome.action === "updates") {
      checkForUpdates(true);
      return;
    }
    if (outcome.action.startsWith("layout-")) {
      setLayout(asLayout(outcome.action.slice("layout-".length)));
      return;
    }
    if (outcome.action === "missing") {
      setMissingFilesOpen(true);
      return;
    }
    openPreferences("view");
  }, [
    readOnly, advancedPrefs.protectLibrary, importFromMenu, importFolderFromMenu, importXmlFromMenu, exportXmlFromMenu, refuse,
    openPreferences, checkForUpdates, prefs, viewPrefs.tempoSlider, openReport,
    editHistory, runLibraryHistory,
  ]);

  // Native menu clicks.
  useEffect(() => {
    let stop: (() => void) | undefined;
    void (async () => {
      const backend = await getBackend();
      stop = backend.onMenu(runMenu);
    })();
    return () => stop?.();
  }, [runMenu]);

  // The keyboard: the shell's own shortcuts, and the menu accelerators the
  // webview keeps from the native menu on Windows.
  const keyOverrides = prefs.preferences.keyboard.overrides;
  /** The level Mute took the master down from, while it is muted. */
  const mutedFrom = useRef<number | null>(null);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // The native menu's accelerators, on the platforms where a keystroke
      // in the webview never reaches them (see `menuAccelerator`).
      const item = menuAccelerator(event, platform);
      if (item !== null) {
        // Let the webview keep its own text history for keyboard commands.
        if ((item === "undo" || item === "redo") && isTyping(event.target as HTMLElement | null)) return;
        event.preventDefault();
        runMenu(item);
        return;
      }
      const action = dispatch(event, platform, event.target as HTMLElement | null, keyOverrides);
      if (action === null) return;
      // Only the actions handled here are swallowed; everything else falls
      // through to the browser and the OS.
      switch (action) {
        case "volumeUp":
        case "volumeDown": {
          // A knob reading a step: rekordbox's command + F12 and F11.
          event.preventDefault();
          const reading = Math.round(gainToKnob(master.level));
          const next = action === "volumeUp" ? Math.min(reading + 1, KNOB_FULL) : Math.max(reading - 1, 0);
          if (next > 0) mutedFrom.current = null;
          master.setLevel(knobToGain(next));
          break;
        }
        case "mute":
          // Mute remembers where the knob was, and a second press puts it back.
          event.preventDefault();
          if (event.repeat) break;
          if (mutedFrom.current !== null) {
            master.setLevel(mutedFrom.current);
            mutedFrom.current = null;
          } else if (master.level > 0) {
            mutedFrom.current = master.level;
            master.setLevel(0);
          }
          break;
        case "focusSearch":
          event.preventDefault();
          searchRef.current?.focus();
          searchRef.current?.select();
          break;
        case "clearSearch":
          // Escape in an empty box should blur rather than do nothing, so the
          // next arrow key reaches the track list.
          if (query === "") {
            searchRef.current?.blur();
          } else {
            setQuery("");
          }
          break;
        case "analyseSelection":
          event.preventDefault();
          analyseSelection();
          break;
        default:
          // Movement and selection live in the table; it listens for itself.
          return;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [platform, query, runMenu, analyseSelection, keyOverrides, master]);

  // The Preferences window asking for what only this window holds.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void getBackend().then((backend) => {
      if (!live) return;
      stop = backend.onPreferencesReset((what) => {
        if (what === "columns") {
          cols.reset();
        } else {
          setTreeWidth(clampWidth(305, bounds()));
          setSubWidth(DEFAULT_SUB_WIDTH);
          setSubTreeWidth(DEFAULT_SUB_TREE_WIDTH);
        }
      });
    });
    return () => {
      live = false;
      stop?.();
    };
  }, [cols, bounds]);

  const refreshDevices = useCallback(() => {
    void (async () => {
      const backend = await getBackend();
      setDevices(await backend.listDevices());
    })();
  }, []);

  const ejectDeviceFromTree = useCallback(async (node: TreeNode) => {
    const path = devicePath(node.id);
    if (!path || ejectingDeviceRef.current || syncing || exportRunning) return;
    ejectingDeviceRef.current = true;
    setEjectingDeviceId(node.id);
    try {
      const backend = await getBackend();
      await backend.ejectDevice(path);
      // Leave the device view before removing the row. This also keeps the
      // disconnect watcher from reporting an intentional eject as a loss.
      setSelectedNode((current) => current?.id === node.id
        ? tree.find((item) => item.kind === "allTracks") ?? tree[0] ?? null
        : current);
      setDevices((current) => current.filter((device) => device.path !== path));
      report(`${node.name} safely ejected.`);
    } catch (error) {
      refuse(error instanceof Error ? error.message : `${node.name} could not be ejected.`);
    } finally {
      ejectingDeviceRef.current = false;
      setEjectingDeviceId(null);
    }
  }, [syncing, exportRunning, tree, report, refuse]);

  // A stick renamed while the panel is open moves to another mount point on
  // macOS, so the node that was selected names a path that no longer
  // exists. The medium is still the same volume, so the selection follows it
  // to its new name rather than dropping back to the track list, and the
  // status bar says what happened. Checked when the window regains focus —
  // the rename happened in the Finder, so focus is the moment it can have
  // changed — and never on a timer.
  useEffect(() => {
    let live = true;
    const onFocus = () => {
      void getBackend()
        .then((backend) => backend.listDevices())
        .then((volumes) => {
          if (live) setDevices(volumes);
        })
        .catch(() => {
          // A failed listing leaves the last one standing.
        });
    };
    window.addEventListener("focus", onFocus);
    return () => {
      live = false;
      window.removeEventListener("focus", onFocus);
    };
  }, []);
  const previousDevices = useRef<readonly Device[]>([]);
  useEffect(() => {
    const before = previousDevices.current;
    previousDevices.current = devices;
    const path = selectedNode ? devicePath(selectedNode.id) : null;
    if (path === null || devices.some((device) => device.path === path)) return;
    // Only a stick this session has seen can be said to have moved or gone;
    // a device id restored from a previous run just falls back quietly.
    if (!before.some((device) => device.path === path)) return;
    const moved = renamedDevice(devices, path, before);
    if (moved) {
      setSelectedNode({ id: deviceId(moved.device), name: moved.device.name, kind: "device", depth: 0 });
      report(`${moved.oldName} is now ${moved.device.name}.`);
    } else {
      report(`${selectedNode?.name ?? "The device"} is no longer connected.`);
    }
    // Only the device list changing can move a selected device; the node
    // itself is what is being corrected here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [devices]);

  // Refresh connected devices without starting an export.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void (async () => {
      const backend = await getBackend();
      if (!live) return;
      stop = backend.onDevicesChanged(() => {
        void (async () => {
          const after = await backend.listDevices().catch(() => null);
          if (after === null || !live) return;
          setDevices(after);
        })();
      });
    })();
    return () => {
      live = false;
      stop?.();
    };
  }, []);

  // Devices join the tree as nodes so the Devices section renders through the
  // same path as every other section, and the Explorer's folders after them.
  const explorer = useExplorer();
  // View › Layout in Preferences decides whether All Tracks heads the
  // playlists and whether the Explorer is there at all; the rail dims a
  // section with nothing in it, so a hidden Explorer reads as empty.
  const treeNodes = useMemo(
    () => [
      ...(viewPrefs.allTracks ? tree : tree.filter((node) => node.kind !== "allTracks")),
      ...deviceLibraries.nodes,
      ...(viewPrefs.explorer ? explorer.nodes : []),
    ],
    [tree, deviceLibraries.nodes, explorer.nodes, viewPrefs.allTracks, viewPrefs.explorer],
  );
  // A lazy row was opened: a stick reads its libraries, a folder on disk
  // its subfolders.
  const expandNode = useCallback(
    (node: TreeNode) => (node.kind === "device" ? deviceLibraries.expand(node) : explorer.expand(node)),
    [deviceLibraries, explorer],
  );

  // A stick's own playlists, edited one library at a time as rekordbox's
  // Devices tree does. Nothing here touches the collection.
  const deviceBusy = syncing || exportRunning || ejectingDeviceId !== null;
  // `said` is the note once it is done, from how many entries changed.
  const editDevice = useCallback(
    async (node: TreeNode, edit: DevicePlaylistEdit, said: (changed: number) => string) => {
      const ref = parseDeviceNodeId(node.id);
      if (!ref) return;
      if (deviceBusy) {
        refuse(t("Wait for the device to finish before changing its playlists."));
        return;
      }
      try {
        const result = await deviceLibraries.edit(ref.path, ref.format, edit);
        if (result.changed > 0) report(said(result.changed));
      } catch (e) {
        refuse(e instanceof Error ? e.message : t("The device library could not be changed."));
      }
    },
    [deviceLibraries, deviceBusy, report, refuse, t],
  );
  const createOnDevice = useCallback(
    (parent: TreeNode, folder: boolean) => {
      const at = deviceParentFor(parent);
      if (at === null) return;
      // rekordbox's own names for a new one [OBS 7.2.14, Winrig 2026-10-08],
      // in the interface's language as rekordbox's are.
      const name = folder ? t("Untitled Folder") : t("Untitled Playlist");
      void editDevice(parent, { kind: "create", parent: at, name, folder }, () => t("Created {name}.", { name }));
    },
    [editDevice, t],
  );
  const renameOnDevice = useCallback(
    (node: TreeNode, name: string) => {
      const ref = parseDeviceNodeId(node.id);
      if (ref) void editDevice(node, { kind: "rename", id: ref.id, name }, () => t("Renamed to {name}.", { name }));
    },
    [editDevice, t],
  );
  // Asked first, in rekordbox's own words [OBS 7.2.14, `rekordbox-19`]: a
  // stick's playlists have no undo here. The tracks stay on the stick.
  const deleteOnDevice = useCallback(
    (node: TreeNode) => {
      const ref = parseDeviceNodeId(node.id);
      if (!ref) return;
      void (async () => {
        const ask = node.kind === "deviceFolder" ? DEVICE_ASKS.deleteFolder : DEVICE_ASKS.deletePlaylist;
        // OK and Cancel, as rekordbox's own box has.
        if (!(await confirmRemoval(`${t(ask)}\n\n'${node.name}'`))) return;
        // The selection stays in the Devices tree, on the library's
        // Playlists heading, rather than leaving the section.
        if (selectedNode?.id === node.id) {
          const heading = deviceNodeId({ ...ref, role: "playlists", id: "0" });
          setSelectedNode(treeNodes.find((n) => n.id === heading) ?? null);
        }
        await editDevice(node, { kind: "delete", id: ref.id }, () => t("Deleted {name}.", { name: node.name }));
      })();
    },
    [editDevice, selectedNode, treeNodes, t, confirmRemoval],
  );
  // Tracks of the selected stick library, into one of its playlists or out
  // of the one open.
  const selectedDeviceRef = useMemo(
    () => (selectedNode && isDeviceLibraryKind(selectedNode.kind) ? parseDeviceNodeId(selectedNode.id) : null),
    [selectedNode],
  );
  // A stick's library open in the browser goes with the stick when it is
  // unplugged or ejected; the tree falls back as it does for the stick's row.
  // Only a stick this session has listed can be said to have gone: at
  // startup the list is still empty.
  const listedSticks = useRef<ReadonlySet<string>>(new Set());
  useEffect(() => {
    const before = listedSticks.current;
    listedSticks.current = new Set(devices.map((device) => device.path));
    if (selectedDeviceRef && before.has(selectedDeviceRef.path) && !listedSticks.current.has(selectedDeviceRef.path)) {
      setSelectedNode(tree.find((item) => item.kind === "allTracks") ?? tree[0] ?? null);
    }
    // Only the device list going is a reason; the selection is what changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [devices]);
  const deviceMenu = useMemo(() => {
    if (!selectedNode || !selectedDeviceRef) return undefined;
    const inPlaylist = selectedNode.kind === "devicePlaylist";
    return {
      playlists: devicePlaylistsOf(treeNodes, selectedDeviceRef.path, selectedDeviceRef.format),
      inPlaylist,
      // Greyed while the stick is synced, exported or ejected, as the tree's
      // own edits are, rather than refused on the click.
      busy: deviceBusy,
      onAdd: (playlist: string, ids: readonly string[]) => {
        const target = devicePlaylistsOf(treeNodes, selectedDeviceRef.path, selectedDeviceRef.format).find((p) => p.id === playlist);
        const name = target?.name ?? "";
        void editDevice(selectedNode, { kind: "add", playlist, tracks: [...ids] }, () => t("Added to {name}.", { name }));
      },
      // rekordbox's question names no count [OBS 7.2.14, `rekordbox-13`];
      // the note after it counts the entries that went. A track listed
      // twice is one row id here, so both copies are selected and go
      // together, as in the collection's playlists.
      onRemove: (ids: readonly string[]) => {
        if (!inPlaylist || ids.length === 0) return;
        void (async () => {
          if (!(await confirmRemoval(t(DEVICE_ASKS.removeTracks)))) return;
          await editDevice(selectedNode, { kind: "remove", playlist: selectedDeviceRef.id, tracks: [...ids] }, (changed) =>
            changed === 1 ? t("Removed {count} track.", { count: changed }) : t("Removed {count} tracks.", { count: changed }));
        })();
      },
    };
  }, [selectedNode, selectedDeviceRef, treeNodes, editDevice, deviceBusy, t, confirmRemoval]);
  const selectedDevice = useMemo(
    () => devices.find((device) => deviceId(device) === selectedNode?.id) ?? null,
    [devices, selectedNode],
  );

  /**
   * Writes a playlist to a stick and says how it went, in the note and as
   * the returned summary; throws what went wrong, having said it. Shared by
   * the device panel and AppleScript's `export`.
   */
  const writeToDevice = useCallback(
    async (playlistId: string, device: { name: string; path: string }): Promise<string> => {
      const name = tree.find((n) => n.id === playlistId)?.name ?? "the playlist";
      setSyncing(true);
      report(`Writing ${name} to ${device.name}…`);
      try {
        const backend = await getBackend();
        const written = await backend.exportPlaylist(playlistId, device.path, stickDefaults, deleteUnlistedMusic, compatibilityFormat);
        const said = exportSummary(device.name, written);
        report(said);
        setDevices(await backend.listDevices());
        return said;
      } catch (e) {
        const said = e && typeof e === "object" && "kind" in e && e.kind === "cancelled" || e instanceof Error && e.message === "Export stopped."
          ? "Export stopped."
          : e instanceof Error ? e.message : "That export could not be written.";
        if (said === "Export stopped.") report(said);
        else refuse(said);
        throw new Error(said, { cause: e });
      } finally {
        setSyncing(false);
      }
    },
    [tree, report, refuse, stickDefaults, deleteUnlistedMusic, compatibilityFormat],
  );

  const syncToDevice = useCallback(
    async (playlistId: string) => {
      if (!selectedDevice) return;
      // Said in the note already; nothing else to do with it here.
      await writeToDevice(playlistId, selectedDevice).catch(() => undefined);
    },
    [selectedDevice, writeToDevice],
  );

  // AppleScript. The backend answers a script's reads itself; what it asks
  // of the window comes here and runs through the same callbacks the
  // controls use. See `src/lib/scripting.ts`.
  const scriptHandlers = useEventCallback((): Readonly<Record<string, ScriptHandler>> => ({
    "deck.load": async ({ deck, row }) => {
      const id: DeckId = deck === "b" ? "b" : "a";
      if (deckCount(layout) < deckNumber(id)) {
        throw new Error(`Deck ${deckNumber(id)} is not in this layout. Choose a layout with ${deckNumber(id)} players from the View menu.`);
      }
      const track = row as RowDto;
      loadTrack(id, track);
      await whenLoaded(id, track.id);
    },
    "deck.play": ({ deck }) => setPlaying(deck === "b" ? "b" : "a", true),
    "deck.pause": ({ deck }) => setPlaying(deck === "b" ? "b" : "a", false),
    "link.set": async ({ on }) => {
      const status = await setLinkOn(on === true);
      if (on === true && !status.on) throw new Error(status.problem ?? "LINK could not be turned on.");
    },
    "preferences.set": ({ path, value }) => {
      const next = withSetting(prefs.preferences, String(path), value);
      const [pane, field] = String(path).split(".") as [PreferencePane, string];
      prefs.update(pane, { [field]: (next[pane] as unknown as Record<string, unknown>)[field] });
      return next;
    },
    export: ({ playlist, device }) => {
      const to = device as { name: string; path: string };
      return writeToDevice(String(playlist), to);
    },
  }));
  useEffect(() => {
    let stop: (() => void) | undefined;
    let live = true;
    void getBackend().then((backend) => {
      if (live) stop = backend.serveScripts((request) => answer(scriptHandlers(), request));
    });
    return () => {
      live = false;
      stop?.();
    };
  }, [scriptHandlers]);
  // A script reads the preferences from the backend's copy, kept current here.
  useEffect(() => {
    void getBackend().then((backend) => backend.mirrorPreferences(prefs.preferences)).catch(() => undefined);
  }, [prefs.preferences]);

  const exportPlaylistFile = useCallback((node: TreeNode, format: "m3u8" | "txt") => {
    void (async () => {
      const backend = await getBackend();
      report(`Choosing where to write ${node.name}…`);
      try {
        const written = await backend.exportPlaylistFile(node.id, node.name, format);
        if (written === null) {
          setNote(null);
          return;
        }
        report(`Wrote ${node.name} as ${format}: ${written} track${written === 1 ? "" : "s"}.`);
      } catch (e) {
        refuse(e instanceof Error ? e.message : "That file could not be written.");
      }
    })();
  }, [report, refuse]);

  // Export Playlist / Export Folder › a stick: the tree menu lists the
  // connected sticks, so the export goes straight to the one chosen.
  const exportPlaylist = useCallback((node: TreeNode, path: string) => {
    const device = devices.find((d) => d.path === path) ?? { name: path, path };
    // Said in the note already; nothing else to do with it here.
    void writeToDevice(node.id, device).catch(() => undefined);
  }, [devices, writeToDevice]);

  // The top of the current view, kept only to write the next start's opening
  // screen. The library itself still lives entirely in Rust.
  const [screen, setScreen] = useState<{ rows: RowDto[]; count: number }>({
    rows: restored.rows,
    count: restored.count,
  });
  const onFirstRows = useCallback((rows: RowDto[], count: number) => {
    setScreen({ rows, count });
  }, []);

  useEffect(() => {
    if (sessionReady) clearWaveformPreviewCache();
  }, [sessionReady]);

  useEffect(() => {
    if (!sessionReady || screen.rows.length === 0) return;
    void getBackend().then(backend => backend.rememberScreenAssets(
      screen.rows.map(row => row.id),
      waveformKindOf(viewPrefs.waveformColor, false),
    ));
  }, [sessionReady, screen.rows, viewPrefs.waveformColor]);

  // Written on every change rather than on exit: a window that is force-quit,
  // or a machine that loses power, still comes back where it was. It is a few
  // hundred bytes to localStorage, not something worth batching.
  useEffect(() => {
    if (!sessionReady) return;
    saveSession({
      treeWidth,
      selectedNodeId: selectedNode?.id ?? null,
      treeExpansion,
      sort: sortState,
      infoOpen,
      subOpen,
      filterOpen,
      // Only once the real library is up: storing the seed back over itself
      // would keep the first run's rows alive forever.
      tree: [...tree].slice(0, SEEDED_NODES),
      rows: screen.rows.slice(0, SEEDED_ROWS),
      count: screen.count,
      layout,
      subWidth,
      subTreeWidth,
      trafficLight,
      waveformZoom,
      dualControl: dual,
    });
  }, [sessionReady, treeWidth, selectedNode, treeExpansion, sortState, infoOpen, subOpen, filterOpen, tree, screen, layout, subWidth, subTreeWidth, trafficLight, waveformZoom, dual]);

  // The last screen, handed to the table until the backend answers. Dropped as
  // soon as the library is up, so a stale row cannot outlive its replacement —
  // and dropped when the library has failed, since a window that says the
  // library could not be opened must not go on showing last week's tracks
  // under that message (seen on a machine with no rekordbox at all).
  const seed = useMemo(
    () =>
      summary === null && loadError === null && missingLibrary === null && restored.rows.length > 0
        ? { count: restored.count, rows: restored.rows }
        : undefined,
    [summary, loadError, missingLibrary, restored.count, restored.rows],
  );

  const selectionText =
    selectedCount > 1 ? `Selected: ${selectedCount} Tracks` : selectedCount === 1 ? "Selected: 1 Track" : "";

  const tip = useTooltip();
  const openViewSettings = useCallback(() => openPreferences("view"), [openPreferences]);
  const ejectA = useCallback(() => setPlayerTrack(null), []);
  const ejectB = useCallback(() => setPlayerTrackB(null), []);
  const masterA = useCallback(() => setSyncMaster("a"), [setSyncMaster]);
  const masterB = useCallback(() => setSyncMaster("b"), [setSyncMaster]);
  const exportDeckTrack = useCallback((device: string, id: string) => exportTrackTo(device, [id]), [exportTrackTo]);
  const showInformation = useCallback((row: RowDto) => { setPlayerTrack(row); setInfoOpen(true); }, []);
  const toggleFilter = useCallback(() => setFilterOpen(was => !was), []);
  const filterBar = useMemo(() => <TrackFilter state={filterState} onChange={setFilterState}
    values={filterValues} masterBpmX100={masterBpmX100} />, [filterState, filterValues, masterBpmX100]);
  const subTree = useMemo(() => ({
    dragging: draggedTracks !== null, onDropTracks: addDraggedTo,
    onDropFiles: readOnly ? undefined : importDroppedFilesTo,
    onExport: exportPlaylist, exportDevices: menuDevices, onExportFile: exportPlaylistFile, onCreatePlaylist: createPlaylistIn,
    onCreateFolder: createFolderIn, onDeleteNode: deleteNode, onRenameNode: renameNode,
    onMoveNode: readOnly ? undefined : moveNode, onExpand: expandNode,
    showCounts: viewPrefs.playlistCounts, onOpenSync: openSyncManager,
    onCreateSmartPlaylist: createSmartPlaylistIn, onEditSmartPlaylist: editSmartPlaylist,
    onAddArtwork: addPlaylistArtwork, onAddToShortcut: addToShortcut, onSortItems: sortItems,
    onEjectDevice: (node: TreeNode) => { void ejectDeviceFromTree(node); },
    ejectingDeviceId, deviceBusy: syncing || exportRunning || ejectingDeviceId !== null, readOnly,
  }), [
    draggedTracks, addDraggedTo, readOnly, importDroppedFilesTo, exportPlaylist, menuDevices, exportPlaylistFile,
    createPlaylistIn, createFolderIn, deleteNode, renameNode, moveNode, expandNode,
    viewPrefs.playlistCounts, openSyncManager, createSmartPlaylistIn, editSmartPlaylist,
    addPlaylistArtwork, addToShortcut, sortItems, ejectDeviceFromTree, ejectingDeviceId, syncing,
    exportRunning,
  ]);
  const subList = useMemo(() => ({
    onDragTracks: setDraggedTracks, onDragError: refuse, players: deckCount(layout), onLoadTrack: loadTrack,
    onShowInformation: showInformation, onShowInFinder: revealTrack, onRate: rateTrack,
    onComment: commentTrack, onResetPlayCount: resetPlayCount, onConvertMemoryCues: convertMemoryCues,
    onRemoveFromCollection: removeFromCollection, onImportToCollection: importToCollection,
    onAutoRelocate: autoRelocate, onRelocate: relocate,
    onAnalysisLock: analysisLock, onAddToPlaylist: addToPlaylist, onAddToTagList: addToTagList,
    onRemoveFromTagList: removeFromTagList, onReloadTag: reloadTag, onExportTrack: exportTrackTo,
    playlists: menuPlaylists, devices: menuDevices, onEditField: editTrackField,
    onEditBlocked: readOnly ? explainEditLock : undefined, onFocusedRow: deckLoaders.a,
    onSelectedRow: setSelectedRow, onSelectedTracks: reportSubSelection, pendingEdits, readOnly,
    dragging: draggedTracks !== null,
    onDropTracks: addDraggedTo, onRemoveTracksFromPlaylist: removeTracksFromPlaylist,
    onRemoveTracksFromHistory: removeTracksFromHistory, onReorderPlaylist: reorderPlaylist,
    onDropFilesIntoPlaylist: importDroppedFilesTo, onAnalyseTracks: analyseTracks,
  }), [
    layout, loadTrack, deckLoaders, showInformation, revealTrack, rateTrack, commentTrack, resetPlayCount,
    convertMemoryCues, removeFromCollection, importToCollection, autoRelocate, relocate, analysisLock, addToPlaylist,
    addToTagList, removeFromTagList, reloadTag, exportTrackTo, menuPlaylists, menuDevices,
    editTrackField, readOnly, explainEditLock, pendingEdits, draggedTracks, addDraggedTo,
    removeTracksFromPlaylist, removeTracksFromHistory, reorderPlaylist, importDroppedFilesTo,
    analyseTracks, refuse, reportSubSelection,
  ]);
  return (
    <PreferencesProvider value={prefs}>
    <MasterOutputConnection mode={viewPrefs.vuMeter} />
    <ConfirmHost />
    <div className={styles.window} data-platform={platform.linux ? "linux" : platform.mac ? "mac" : "windows"}>
      <div
        className={styles.titleBar}
        data-testid="title-bar"
        onMouseDown={startWindowDrag}
        onDoubleClick={toggleWindowMaximise}
      >
        {/* What the app is costing, in the corner: its own component, so a
            reading does not re-render the window around it. */}
        <AppCost className={styles.cost} />
        <span className={styles.appName}>rbxport</span>
      </div>
      <ConnectedTopBar
        clock={clock}
        onOpenSettings={openViewSettings}
        layout={layout}
        onLayoutChange={setLayout}
      />
      {/* Full Browser draws no deck at all, and no gutter under one. */}
      {deckCount(layout) > 0 ? (
        <div
          className={styles.decks}
          data-mixer={deckCount(layout) > 1 ? "" : undefined}
        >
          {/* One transport column for the pair, as rekordbox draws it: deck A
              down from the top, deck B up from the bottom, and the mixer strip
              beside it. Each deck still owns its own transport — it is
              portalled into the half of the column that belongs to it. */}
          {deckCount(layout) > 1 ? (
            <div className={styles.deckRail}>
              <div className={styles.transportSlot} ref={setTransportA} />
              {/* DUAL CONTROL, on the centre line where the capture has it:
                  it links what the two decks are showing rather than what
                  they are playing, so it belongs between them. */}
              <button
                type="button"
                className={styles.dual}
                aria-label="Dual control"
                aria-pressed={dual}
                title={tip("Link the waveform controls and beat jump across both decks.")}
                data-on={dual || undefined}
                onClick={() => setDual((was) => !was)}
              >
                <LayoutDualIcon className={styles.dualGlyph} />
              </button>
              <div className={styles.transportSlot} ref={setTransportB} />
            </div>
          ) : null}
          {/* Only with two decks. The one-player layout has no mixer and no
              crossfader, which is what rekordbox does — and what keeps a deck
              nobody has touched a fader for playing at the level of its file. */}
          {deckCount(layout) > 1 ? <MixerStrip /> : null}
          <Player
            track={playerTrack}
            onEject={ejectA}
            onError={setPlayerError}
            onAnalyse={analyseOne}
            onExportTrack={exportDeckTrack}
            devices={menuDevices}
            onExportLoop={exportLoop}
            simple={!isFullDeck(layout)}
            dragging={draggedTracks !== null}
            onDropTrack={loadDroppedInto.a}
            onLoadSelected={loadSelectedInto.a}
            selectedTrackId={selectedRow?.id ?? null}
            transportSlot={deckCount(layout) > 1 ? transportA : null}
            dual={deckCount(layout) > 1}
            publishZoom={deckCount(layout) > 1 ? publishZoom.a : undefined}
            bars={deckCount(layout) > 1 && dual ? dualBars : waveformZoom.a}
            onBars={deckCount(layout) > 1 && dual ? setLinkedZoom : setZoomA}
            {...(deckCount(layout) > 1 ? linked : {})}
            publishSync={publishSync.a}
            {...(deckCount(layout) > 1 ? { peerSync: peerSync.a } : {})}
            isMaster={syncMaster === "a"}
            onMaster={masterA}
            synced={synced.a && syncMaster !== "a"}
            onSyncToggle={deckCount(layout) > 1 ? toggleSync.a : undefined}
            leaderBpmX100={syncMaster === "a" ? null : leaderBpmX100}
            onPlayingBpm={reportPlayingBpm.a}
            publishGridFollow={publishGridFollow.a}
            onGridNudge={gridNudged.a}
            onKeyShift={reportKeyShift.a}
            readOnly={readOnly}
          />
          {deckCount(layout) > 1 ? (
            <Player
              deck="b"
              track={playerTrackB}
              onEject={ejectB}
              onAnalyse={analyseOne}
              onExportTrack={exportDeckTrack}
              devices={menuDevices}
              onExportLoop={exportLoop}
              onError={setPlayerError}
              simple={!isFullDeck(layout)}
              dragging={draggedTracks !== null}
              onDropTrack={loadDroppedInto.b}
              onLoadSelected={loadSelectedInto.b}
              transportSlot={transportB}
              flipped
              dual
              publishZoom={publishZoom.b}
              bars={dual ? dualBars : waveformZoom.b}
              onBars={dual ? setLinkedZoom : setZoomB}
              {...linked}
              publishSync={publishSync.b}
              peerSync={peerSync.b}
              isMaster={syncMaster === "b"}
              onMaster={masterB}
              synced={synced.b && syncMaster !== "b"}
              onSyncToggle={toggleSync.b}
              leaderBpmX100={syncMaster === "b" ? null : leaderBpmX100}
              onPlayingBpm={reportPlayingBpm.b}
              publishGridFollow={publishGridFollow.b}
              onGridNudge={gridNudged.b}
              onKeyShift={reportKeyShift.b}
              readOnly={readOnly}
            />
          ) : null}
          {/* Over the line between the decks, where the capture floats it.
              After the decks, so it paints over both. */}
          {deckCount(layout) > 1 ? (
            <div className={styles.zoomSlot}>
              <DualZoom onZoom={zoomBoth} />
            </div>
          ) : null}
          <div className={styles.playerGutter} aria-hidden />
        </div>
      ) : null}
      <div
        data-testid="body"
        className={styles.body}
        ref={bodyRef}
        style={{ ["--tree-w" as string]: `${treeWidth}px` }}
        data-info={infoOpen ? "" : undefined}
        data-sub={subOpen ? "" : undefined}
      >
        <TreeView
          nodes={treeNodes}
          selectedId={selectedNode?.id ?? null}
          onSelect={setSelectedNode}
          dragging={draggedTracks !== null}
          onDropTracks={addDraggedTo}
          onDropFiles={readOnly ? undefined : importDroppedFilesTo}
          onExport={exportPlaylist}
          exportDevices={menuDevices}
          onExportFile={exportPlaylistFile}
          onCreatePlaylist={createPlaylistIn}
          onCreateFolder={createFolderIn}
          onDeleteNode={deleteNode}
          onRenameNode={renameNode}
          onMoveNode={readOnly ? undefined : moveNode}
          readOnly={readOnly}
          onExpand={expandNode}
          initialExpansion={restored.treeExpansion}
          onExpansionChange={setTreeExpansion}
          showCounts={viewPrefs.playlistCounts}
          onOpenSync={openSyncManager}
          onCreateSmartPlaylist={createSmartPlaylistIn}
          onEditSmartPlaylist={editSmartPlaylist}
          onAddArtwork={addPlaylistArtwork}
          onAddToShortcut={addToShortcut}
          onSortItems={sortItems}
          railShortcuts={railShortcuts}
          onOpenShortcut={openShortcut}
          onDeleteShortcut={deleteShortcut}
          onEjectDevice={(node) => { void ejectDeviceFromTree(node); }}
          ejectingDeviceId={ejectingDeviceId}
          deviceBusy={deviceBusy}
          onDeviceCreate={createOnDevice}
          onDeviceRename={renameOnDevice}
          onDeviceDelete={deleteOnDevice}
        />
        <div
          className={styles.splitter}
          onPointerDown={onSplitterDown}
          onPointerMove={onSplitterMove}
          onPointerUp={onSplitterUp}
          role="separator"
          aria-orientation="vertical"
          aria-label="Resize the library tree"
          aria-valuenow={treeWidth}
        />
        {selectedDevice ? (
          <Suspense fallback={null}><DevicePanel
            device={selectedDevice}
            playlists={tree}
            onSync={syncToDevice}
            onRefresh={refreshDevices}
            onError={refuse}
            busy={syncing}
          /></Suspense>
        ) : (
        <TrackTable
          // The cached screen may mount before the backend exists. Remount
          // once restoration completes so a cancelled bootstrap view cannot
          // leave the real source permanently in its loading state.
          key={sessionReady ? "library-ready" : "library-loading"}
          spec={spec}
          onSortChange={handleSort}
          onSelectionChange={setSelectedCount}
          onSelectedTracks={reportMainSelection}
          onAnalyse={analyseSelection}
          onShowInformation={showInformation}
          onShowInFinder={revealTrack}
          onRemoveFromPlaylist={removeFromPlaylist}
          onRemoveFromHistory={removeFromHistory}
          onResetPlayCount={resetPlayCount}
          onConvertMemoryCues={convertMemoryCues}
          onRemoveFromCollection={removeFromCollection}
          onAutoRelocate={autoRelocate}
          onRelocate={relocate}
          onImportToCollection={importToCollection}
          onAnalysisLock={analysisLock}
          onAddToPlaylist={addToPlaylist}
          onAddToTagList={addToTagList}
          onRemoveFromTagList={removeFromTagList}
          onReloadTag={reloadTag}
          onExportTrack={exportTrackTo}
          playlists={menuPlaylists}
          devices={menuDevices}
          deviceMenu={deviceMenu}
          readOnly={readOnly}
          trafficLight={activeTrafficLight}
          onTrafficLight={setTrafficLight}
          trafficKey={trafficKey}
          onFocusedRow={deckLoaders.a}
          onSelectedRow={setSelectedRow}
          onDragTracks={setDraggedTracks}
          dragging={draggedTracks !== null}
          onDropTracks={selectedNode?.kind === "playlist" ? addDraggedTo : undefined}
          onDragError={refuse}
          onDropFiles={importDroppedFilesIntoOpen}
          players={deckCount(layout)}
          onLoadTrack={loadTrack}
          onRate={rateTrack}
          onComment={commentTrack}
          // The sub-browser's list is left out on purpose: it has a source and
          // a sort of its own, which this gate does not describe.
          onReorder={canReorder ? reorderPlaylistTracks : undefined}
          onEditField={editTrackField}
          onEditBlocked={readOnly ? explainEditLock : undefined}
          libraryGeneration={libraryGeneration}
          pendingEdits={pendingEdits}
          pendingRows={jobList
            .filter((job) => job.target !== null && job.target === selectedNode?.id)
            .reduce((sum, job) => sum + job.pendingRows, 0)}
          title={selectedNode?.name ?? "Collection"}
          query={query}
          searchField={searchField}
          onSearchFieldChange={setSearchField}
          onQueryChange={setQuery}
          searchRef={searchRef}
          columns={cols.columns}
          onColumnMove={cols.move}
          onColumnResize={cols.resize}
          onColumnToggle={cols.toggle}
          onColumnAutoSize={cols.autoSize}
          onColumnAutoSizeAll={cols.autoSizeEvery}
          seed={seed}
          onFirstRows={onFirstRows}
          filterOpen={filterOpen}
          onToggleFilter={toggleFilter}
          filterBar={filterBar}
        />
        )}
        {subOpen ? (
          <SubBrowser
            nodes={tree}
            libraryGeneration={libraryGeneration}
            width={subWidth}
            treeWidth={subTreeWidth}
            onWidthChange={setSubWidth}
            onTreeWidthChange={setSubTreeWidth}
            // Its tree and list take part in everything the main pair does:
            // a drag from either list lands on either tree, and its rows
            // load decks and take edits the same way.
            tree={subTree}
            list={subList}
          />
        ) : null}
        {infoOpen ? (
          <InfoPanel
            // The browser's selection, as rekordbox's Information Window
            // follows it; the deck's track only when nothing is selected —
            // never for a multiple selection, which the panel shows as one.
            track={infoSelection.length > 1 ? null : selectedRow ?? playerTrack}
            selection={infoSelection}
            readOnly={readOnly}
            libraryGeneration={libraryGeneration}
            onRate={rateTracks}
            onComment={commentTracks}
            onEdit={runEdit}
          />
        ) : null}
        <RightRail
          className={styles.rightRail}
          infoOpen={infoOpen}
          onToggleInfo={() => setInfoOpen((open) => !open)}
          subOpen={subOpen}
          onToggleSub={() => setSubOpen((open) => !open)}
        />
      </div>
      <Suspense fallback={null}>
      {updater.open ? (
        <UpdateManager
          state={updater.state}
          onCheck={() => updater.check(true)}
          onRetry={updater.retry}
          onRestart={updater.restart}
          onClose={updater.dismiss}
        />
      ) : null}
      {duplicatePrompt ? (
        <DuplicateTracksDialog playlist={duplicatePrompt.playlist} duplicates={duplicatePrompt.duplicates}
          total={duplicatePrompt.total} onAnswer={duplicatePrompt.settle} />
      ) : null}
      {missingLibrary !== null ? (
        <NewLibraryDialog key={missingLibrary.kind} problem={missingLibrary}
          onCreate={async () => {
            await (await getBackend()).createLibrary();
            // The ready event that follows loads it like any other start.
            setMissingLibrary(null);
          }}
          // Closed by the ready event when the default folder has a library,
          // or asked again by the problem event when it is empty.
          onUseDefault={async () => (await getBackend()).useDefaultLibrary()}
          onConfirm={async (message, labels) => (await getBackend()).confirm(message, labels)}
          onQuit={() => { void getBackend().then(backend => backend.closeWindow()); }} />
      ) : null}
      {missingFilesOpen ? (
        <MissingFileManager
          readOnly={readOnly}
          search={relocateSearch}
          searching={searching}
          onWrote={(said) => { void afterWrite(said); }}
          onFailed={refuse}
          onClose={() => setMissingFilesOpen(false)}
        />
      ) : null}
      {analysisSelection !== null ? (
        <AnalysisDialog count={analysisSelection.length} initialMode={analysisPrefs.mode}
          initialFirstBeatCue={analysisPrefs.firstBeatCue}
          onCancel={() => setAnalysisSelection(null)}
          onConfirm={settings => {
            if (readOnly) { refuse(ANALYSIS_REFUSED); return; }
            analysis.add(analysisSelection, settings);
            setAnalysisSelection(null);
          }} />
      ) : null}
      {autoAnalysis !== null ? (
        <AnalysisDialog auto count={autoAnalysis.tracks.length} initialMode={analysisPrefs.mode}
          initialFirstBeatCue={analysisPrefs.firstBeatCue}
          onCancel={() => setAutoAnalysis(null)}
          onConfirm={settings => void startAutoAnalysis(autoAnalysis, settings)} />
      ) : null}
      {settingsOpen !== null ? (
        <ConnectedPreferences
          summary={summary}
          limiter={limiter.limiter}
          onLimiterChange={limiter.set}
          initialPane={settingsOpen}
          onResetColumns={cols.reset}
          onResetLayout={() => {
            setTreeWidth(clampWidth(305, bounds()));
            setSubWidth(DEFAULT_SUB_WIDTH);
            setSubTreeWidth(DEFAULT_SUB_TREE_WIDTH);
          }}
          onClose={() => setSettingsOpen(null)}
        />
      ) : null}
      {syncOpen ? (
        <SyncManager onClose={() => setSyncOpen(false)} onSynced={refreshDevices} />
      ) : null}
      {reportOpen ? <ReportBug onClose={() => setReportOpen(false)} /> : null}
      {smartEditor ? (
        <SmartPlaylistEditor
          title={smartEditor.mode === "create" ? "Create New Intelligent Playlist" : "Edit the Intelligent Playlist"}
          name={smartEditor.name}
          rule={smartEditor.rule}
          myTags={smartTags}
          onSave={saveSmartPlaylist}
          onCancel={() => setSmartEditor(null)}
        />
      ) : null}

      </Suspense>
      {/* The LINK strip: present from the moment a player or mixer is heard,
          and the whole LINK interface from then on. It draws nothing at all
          before that, so the row it sits in collapses. */}
      <div className={styles.linkStrip}>
        <LinkDeckStrip
          peers={linkPeers}
          link={link}
          busy={linkBusy}
          onToggle={toggleLink}
          dragging={draggedTracks !== null}
          onDropToPlayer={loadDroppedOnLink}
          onSetMaster={setLinkMaster}
          onNudgeMaster={nudgeLinkMaster}
          onTakeMasterTempo={takeLinkMasterTempo}
        />
      </div>

      <StatusBar
        jobs={jobList}
        onStopJob={stopJob}
        exports={(exportRunning ? exportBatch : []).map(job => ({
          ...job, name: devices.find(device => device.path === job.path)?.name ?? job.path.split(/[\\/]/).filter(Boolean).at(-1) ?? job.path,
        }))}
        onReportBug={openReport}
        onSupport={SHOW_MAIN_SUPPORT ? openSupport : undefined}
        onOpenLog={openLog}
        updateNotice={updateNotice ? (
          <UpdateReadyNotice
            state={updateNotice}
            onRestart={updater.restart}
            onWhatsNew={openWhatsNew}
            onDismiss={() => setUpdateNoticeVisible(false)}
          />
        ) : null}
        backupActivity={backupJob.error || backupJob.text}
        backupProgress={backupJob.progress.running ? backupJob.progress : undefined}
        version={version}
        analysisProgress={analysis.running ? {
          completed: analysis.state.done + analysis.state.failed.length,
          total: analysis.total,
          waiting: analysis.waiting,
        } : undefined}
        activity={
          note !== null && !note.failed
                ? note.text
                : (summary ? "" : "Loading the library…")
        }
        // Everything that went wrong, in one place and in red: the deck's
        // refusals, a library that would not open, and a write the library
        // turned down.
        error={playerError ?? loadError ?? [...exportJobs.values()].find(job => job.state === "failed")?.title ?? (note?.failed === true ? note.text : null)}
        onOpenProtection={!playerError && !loadError && note?.failed && note.text === refusal(true) && advancedPrefs.protectLibrary
          ? () => openPreferences("libraryProtection") : undefined}
        onCancelAnalysis={analysis.running ? analysis.cancel : undefined}
        analysisFailures={analysis.state.failed.length}
        selection={selectionText}
        readOnly={readOnly}
        protectedLibrary={advancedPrefs.protectLibrary}
        onExplainReadOnly={() => { setPlayerError(null); explainEditLock(); }}
        onDisableReadOnly={() => {
          if (advancedPrefs.protectLibrary) {
            explainEditLock();
            return;
          }
          void getBackend().then(backend => backend.disableReadOnly()).then(() => {
            setSummary(current => current ? { ...current, readOnly: false } : current);
            setNote({ text: "Read-only mode disabled for this session.", failed: false });
          }).catch(error => refuse(error instanceof Error ? error.message : String(error)));
        }}
      />
    </div>
    </PreferencesProvider>
  );
}
