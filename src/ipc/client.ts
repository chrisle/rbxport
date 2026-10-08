/**
 * The only module that talks to Tauri.
 *
 * Views import the typed functions here; they never call `invoke` themselves,
 * so the IPC surface stays auditable and the mock can stand in wholesale.
 */
import { detectPlatform } from "@/lib/shortcuts";
import type {
  AnalysisResult, AudioDevices, Backend, Backup, BackupProgress, BackupSizes, Cue, DeckEvent, Device, DeviceSettings, DeviceSyncState,
  Diagnostics, Duplicates, GridState, Limiter, PreferencesRequest, SmartRule, SyncDeviceReport, SyncProgress, UpdateCheck,
  UpdateProgress, UpdateReady, XmlImportReport,
  ExportProgress, ExportReport, ExplorerChildren, ExplorerRoot, FilterValues, Phrase, ImportReport,
  DatabaseDrive, EditHistoryState, ItunesLibrary, LibraryProblem, LibrarySummary, LinkPeerSeen, Meters,
  LinkStatus, MissingExportFile, MissingTracks, PreviewState, ReferenceStickSettings, RelocateReport, RowDto, ScriptRequest, Tick,
  TreeNode, ViewHandle,
  TrackDetails, TrackLookups,
} from "./types";

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Native file sources allow one track drag to land in Finder or in our webview. */
export function nativeTrackDragging(): boolean {
  return isTauri && detectPlatform().mac;
}

/** Resolves when the native drag finishes, including cancellation. */
export async function dragTracksToDesktop(ids: readonly string[]): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    await invoke("drag_tracks", { ids: [...ids] });
  } catch (error) {
    throw error instanceof Error ? error : new Error(String(error));
  }
}

/** Resolve a DOM file drop without intercepting the webview's internal drags. */
export async function droppedFilePaths(files: File[]): Promise<string[]> {
  if (files.length === 0) throw new Error("No files were dropped.");
  const paths = files.map((file) => (file as File & { path?: string }).path);
  if (paths.every((path): path is string => Boolean(path))) return paths;
  if (!isTauri) throw new Error("Drop files in the desktop app to import them.");
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<string[]>("dropped_file_paths", { names: files.map((file) => file.name) });
  } catch (error) {
    throw error instanceof Error ? error : new Error(String(error));
  }
}

export interface NativeFileDrop {
  paths: string[];
  /** Drop position in CSS pixels, relative to the webview. */
  x: number;
  y: number;
}

/**
 * Receives native OS file drops on platforms whose webview does not expose
 * filesystem paths through DOM `File` objects (notably Linux WebKitGTK).
 */
export function subscribeNativeFileDrops(listener: (drop: NativeFileDrop) => void): () => void {
  if (!isTauri) return () => {};
  let live = true;
  let stop: (() => void) | undefined;
  void Promise.all([
    import("@tauri-apps/api/webview"),
    import("@tauri-apps/api/window"),
  ]).then(async ([{ getCurrentWebview }, { getCurrentWindow }]) => {
    const scale = await getCurrentWindow().scaleFactor();
    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop" || event.payload.paths.length === 0) return;
      listener({
        paths: event.payload.paths,
        x: event.payload.position.x / scale,
        y: event.payload.position.y / scale,
      });
    });
    if (live) stop = unlisten;
    else unlisten();
  });
  return () => {
    live = false;
    stop?.();
  };
}

/** Keep the native Edit menu in sync with the focused editor's history. */
export async function setHistoryMenu(undo: string | null, redo: string | null): Promise<void> {
  if (!isTauri) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_history_menu", { undo, redo });
}

/**
 * Subscribes to a backend event, returning its own unsubscribe.
 *
 * The event module is imported lazily, so a caller can unsubscribe before the
 * import lands; that case has to be handled or the listener outlives its
 * component.
 */
function subscribe<T>(event: string, listener: (payload: T) => void): () => void {
  let live = true;
  let stop: (() => void) | undefined;
  void import("@tauri-apps/api/event").then(async ({ listen }) => {
    const unlisten = await listen<T>(event, (e) => listener(e.payload));
    if (live) stop = unlisten;
    else unlisten();
  });
  return () => {
    live = false;
    stop?.();
  };
}

async function realBackend(): Promise<Backend> {
  const { invoke } = await import("@tauri-apps/api/core");
  return {
    rekordboxBrowseSettings: () => invoke<string | null>("rekordbox_browse_settings"),
    librarySummary: () => invoke<LibrarySummary>("library_summary"),
    disableReadOnly: () => invoke<void>("disable_read_only"),
    rememberScreenAssets: (ids, waveformKind) => invoke<void>("remember_screen_assets", { ids, waveformKind }),
    playlistTree: () => invoke<TreeNode[]>("playlist_tree"),
    openView: (spec) => invoke<ViewHandle>("open_view", { spec }),
    fetchRows: (viewId, offset, len, extraColumns) => invoke<RowDto[]>("fetch_rows", { viewId, offset, len, extraColumns }),
    viewIdsInRange: (viewId, from, to) => invoke<string[]>("view_ids_in_range", { viewId, from, to }),
    trackWaveform: async (trackId, kind, window) => {
      // Raw bytes rather than a JSON number array: the three-band detail tag
      // is 158 KB on a five-minute track and JSON would multiply that.
      const bytes = await invoke<ArrayBuffer | number[] | Uint8Array>("track_waveform", {
        trackId,
        kind,
        from: window?.from,
        len: window?.len,
      });
      if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
      return bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes);
    },
    trackPcmWaveform: async (trackId, fromMs, toMs, columns) => {
      const bytes = await invoke<ArrayBuffer | number[] | Uint8Array>("track_pcm_waveform", {
        trackId, fromMs, toMs, columns,
      });
      if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
      return bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes);
    },
    analyseTrack: (trackId, mode = "rbxport", settings) => invoke<AnalysisResult>("analyse_track", { trackId, mode, settings }),
    trackBeats: async (trackId) => {
      // Raw bytes rather than a JSON array of objects: a long mix has tens of
      // thousands of beats, and `{"timeMs":123,"downbeat":true}` each is an
      // order of magnitude more to send and to parse.
      const bytes = await invoke<ArrayBuffer | number[] | Uint8Array>("track_beats", {
        track: trackId,
      });
      if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
      return bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes);
    },
    gridState: (trackId) => invoke<GridState>("grid_state", { track: trackId }),
    trackCues: (trackId) => invoke<Cue[]>("track_cues", { track: trackId }),
    trackPhrases: (trackId) => invoke<Phrase[]>("track_phrases", { track: trackId }),
    editPhrase: (trackId, beat, action) => invoke<boolean>("edit_phrase", { trackId, beat, action }),
    trackVocals: async (trackId) => {
      const bytes = await invoke<ArrayBuffer | number[] | Uint8Array>("track_vocals", {
        track: trackId,
      });
      if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
      return bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes);
    },
    importFiles: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: true,
        directory: false,
        title: "Add music to the library",
        filters: [
          {
            name: "Audio",
            extensions: ["mp3", "m4a", "aac", "flac", "wav", "aiff", "aif", "ogg", "opus"],
          },
        ],
      });
      // Cancelling is a normal outcome, not an error.
      if (!Array.isArray(picked) || picked.length === 0) return null;
      return invoke<ImportReport>("import_files", { paths: picked });
    },
    importFolder: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      // A folder picker: the native panel cannot offer files and folders at
      // once, so choosing a folder is its own action alongside Import. The
      // backend walks each folder recursively for the audio it recognises.
      const picked = await open({
        multiple: true,
        directory: true,
        title: "Add a folder of music to the library",
      });
      // Cancelling is a normal outcome, not an error.
      if (!Array.isArray(picked) || picked.length === 0) return null;
      return invoke<ImportReport>("import_files", { paths: picked });
    },
    importPaths: (paths) => invoke<ImportReport>("import_files", { paths }),
    exportLoopWav: async (track, title, inMs, outMs) => {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const picked = await save({
        title: "Export the loop as WAV",
        defaultPath: `${title} loop.wav`,
        filters: [{ name: "WAV", extensions: ["wav"] }],
      });
      if (typeof picked !== "string") return null;
      return invoke<number>("export_loop_wav", { track, inMs, outMs, path: picked });
    },
    exportPlaylistFile: async (playlistId, name, format) => {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const picked = await save({
        title: `Export ${name} as ${format}`,
        defaultPath: `${name}.${format}`,
        filters: [{ name: format === "txt" ? "Text" : "M3U playlist", extensions: [format] }],
      });
      if (typeof picked !== "string") return null;
      return invoke<number>("export_playlist_file", { playlist: playlistId, path: picked, format });
    },
    importXml: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Choose a rekordbox XML collection",
        filters: [{ name: "rekordbox XML", extensions: ["xml"] }],
      });
      if (typeof picked !== "string") return null;
      return invoke<XmlImportReport>("import_xml", { path: picked });
    },
    importItunes: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Choose the iTunes or Music Library.xml",
        filters: [{ name: "iTunes Library XML", extensions: ["xml"] }],
      });
      if (typeof picked !== "string") return null;
      return invoke<XmlImportReport>("import_itunes", { path: picked });
    },
    itunesDefaultLibrary: () => invoke<ItunesLibrary | null>("itunes_default_library"),
    chooseItunesLibrary: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Choose the iTunes or Music Library.xml",
        filters: [{ name: "iTunes Library XML", extensions: ["xml"] }],
      });
      if (typeof picked !== "string") return null;
      return invoke<ItunesLibrary>("itunes_library_at", { path: picked });
    },
    importItunesSelected: (path, ids) => invoke<XmlImportReport>("import_itunes_selected", { path, ids: [...ids] }),
    exportXml: async () => {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const picked = await save({
        title: "Export the collection as rekordbox XML",
        defaultPath: "rekordbox.xml",
        filters: [{ name: "rekordbox XML", extensions: ["xml"] }],
      });
      if (typeof picked !== "string") return null;
      return invoke<number>("export_xml", { path: picked });
    },
    exportPlaylist: (playlistId, destination, defaults, deleteUnlistedMusic, compatibilityFormat) =>
      invoke<ExportReport>("export_playlist", {
        playlist: playlistId,
        destination,
        defaults: defaults ?? null,
        deleteUnlistedMusic: deleteUnlistedMusic ?? false,
        compatibilityFormat: compatibilityFormat ?? null,
      }),
    exportTracksToDevice: (tracks, destination, defaults, compatibilityFormat) =>
      invoke<ExportReport>("export_tracks_to_device", { tracks, destination, defaults: defaults ?? null, compatibilityFormat: compatibilityFormat ?? null }),
    referenceStickSettings: () => invoke<ReferenceStickSettings>("reference_stick_settings"),
    listDevices: () => invoke<Device[]>("list_devices"),
    onExportProgress: (listener) => subscribe<ExportProgress>("export:progress", listener),
    onImportProgress: (listener) => subscribe<ExportProgress>("import:progress", listener),
    exportProgress: () => invoke<ExportProgress[]>("export_progress"),
    cancelExport: (path) => invoke<void>("cancel_export", { path }),
    listBackups: () => invoke<Backup[]>("list_backups"),
    backupDirectory: () => invoke<string>("backup_directory"),
    openBackupDirectory: () => invoke<void>("open_backup_directory"),
    backupSizes: (refresh = false) => invoke<BackupSizes>("backup_sizes", { refresh }),
    startBackup: () => invoke<void>("start_backup"),
    cancelBackup: () => invoke<void>("cancel_backup"),
    backupProgress: () => invoke<BackupProgress>("backup_progress"),
    openUrl: (url) => invoke<void>("open_url", { url }),
    backUpLibrary: () => invoke<string>("back_up_library"),
    setBackupDirectory: (directory) => invoke<string>("set_backup_directory", { directory }),
    deleteBackup: (path) => invoke<void>("delete_backup", { path }),
    confirm: async (message, labels) => {
      const { ask } = await import("@tauri-apps/plugin-dialog");
      return ask(message, { kind: "warning", ...(labels ? { okLabel: labels.yes, cancelLabel: labels.no } : {}) });
    },
    deckLoad: (deck, trackId, loadId) => invoke<void>("deck_load", { deck, track: trackId, loadId }),
    deckUnload: (deck) => invoke<void>("deck_unload", { deck }),
    deckPlay: (deck) => invoke<void>("deck_play", { deck }),
    deckPlayAfter: (deck, delayMs) => invoke<void>("deck_play_after", { deck, delayMs }),
    deckPause: (deck) => invoke<void>("deck_pause", { deck }),
    deckSeek: (deck, positionMs) => invoke<void>("deck_seek", { deck, positionMs }),
    deckMove: (deck, byMs) => invoke<void>("deck_move", { deck, byMs }),
    previewPlay: (track, positionMs) => invoke<void>("preview_play", { track, positionMs }),
    previewStop: () => invoke<void>("preview_stop"),
    previewState: () => invoke<PreviewState>("preview_state"),
    deckSetLoop: (deck, inMs, outMs) => invoke<void>("deck_set_loop", { deck, inMs, outMs }),
    deckLoopActive: (deck, on) => invoke<void>("deck_loop_active", { deck, on }),
    deckClearLoop: (deck) => invoke<void>("deck_clear_loop", { deck }),
    deckScrubBegin: (deck) => invoke<void>("deck_scrub_begin", { deck }),
    deckScrubTo: (deck, positionMs) => invoke<void>("deck_scrub_to", { deck, positionMs }),
    deckScrubEnd: (deck) => invoke<void>("deck_scrub_end", { deck }),
    setMasterLevel: (level) => invoke<void>("set_master_level", { level }),
    audioDevices: () => invoke<AudioDevices>("audio_devices"),
    setAudioDevice: (device) => invoke<void>("set_audio_device", { device }),
    checkForUpdate: () => invoke<UpdateCheck>("check_for_update"),
    readyUpdate: () => invoke<UpdateReady | null>("ready_update"),
    downloadUpdate: () => invoke<UpdateReady>("download_update"),
    restartToUpdate: () => invoke<void>("restart_to_update"),
    onUpdateProgress: (listener) => subscribe<UpdateProgress>("update:progress", listener),
    masterLimiter: () => invoke<Limiter>("master_limiter"),
    setMasterLimiter: (limiter) => invoke<Limiter>("set_master_limiter", { limiter }),
    deckTempo: (deck, tempo) => invoke<void>("deck_tempo", { deck, tempo }),
    deckMasterTempo: (deck, on) => invoke<void>("deck_master_tempo", { deck, on }),
    deckMetronome: (deck, on) => invoke<void>("deck_metronome", { deck, on }),
    setMetronomeGrid: (deck, beats) => invoke<void>("deck_metronome_grid", { deck, beats }),
    deckKeyShift: (deck, semitones) => invoke<void>("deck_key_shift", { deck, semitones }),
    setMetronome: (sound, volume) => invoke<void>("set_metronome", { sound, volume }),
    setAudioConfig: (sampleRate, bufferFrames) =>
      invoke<void>("set_audio_config", { sampleRate, bufferFrames }),
    setChannelBand: (deck, band, position) =>
      invoke<void>("set_channel_band", { deck, band, position }),
    setChannelKill: (deck, band, killed) =>
      invoke<void>("set_channel_kill", { deck, band, killed }),
    setChannelTrim: (deck, trim) => invoke<void>("set_channel_trim", { deck, trim }),
    setCrossfade: (position) => invoke<void>("set_crossfade", { position }),
    setEqCurve: (isolator) => invoke<void>("set_eq_curve", { isolator }),
    appDiagnostics: () => invoke<Diagnostics>("app_diagnostics"),
    appVersion: () => invoke<string>("app_version"),
    openLog: () => invoke<void>("open_log"),
    revealTrack: (trackId) => invoke<void>("reveal_track", { track: trackId }),
    deckState: () => invoke<Tick>("deck_state"),
    onDeckTick: (listener) => subscribe<Tick>("deck:tick", listener),
    onMeters: (listener) => subscribe<Meters>("deck:meters", listener),
    onDeckEvent: (listener) => {
      // Loaded and failed are the same shape and the same subscription; the
      // message is what tells them apart.
      const stopLoaded = subscribe<DeckEvent>("deck:loaded", listener);
      const stopError = subscribe<DeckEvent>("deck:error", listener);
      return () => {
        stopLoaded();
        stopError();
      };
    },
    onDeckReset: (listener) => subscribe<void>("deck:reset", listener),
    onLibraryReady: (listener) => subscribe("library:ready", () => listener()),
    onLibraryProblem: (listener) => subscribe<LibraryProblem>("library:problem", listener),
    libraryProblem: () => invoke<LibraryProblem | null>("library_problem"),
    createLibrary: () => invoke<void>("create_library"),
    useDefaultLibrary: () => invoke<void>("use_default_library"),
    databaseDrives: () => invoke<DatabaseDrive[]>("database_drives"),
    switchLibrary: (masterDb) => invoke<void>("switch_library", { masterDb }),
    onCuesChanged: (listener) => subscribe<string>("cues:changed", listener),
    onGridChanged: (listener) => subscribe<string>("grid:changed", listener),
    onAnalysisChanged: (listener) => subscribe<string>("analysis:changed", listener),
    reloadLibrary: () => invoke<number>("reload_library"),
    onMenu: (listener) => subscribe<string>("menu", listener),
    setMenuLabels: (labels) => invoke<void>("set_menu_labels", { labels }),
    serveScripts: (handle) => {
      // Ready only once the listener is in: a request sent before then would
      // reach nobody and wait out its timeout.
      let live = true;
      let stop: (() => void) | undefined;
      void import("@tauri-apps/api/event").then(async ({ listen }) => {
        const unlisten = await listen<ScriptRequest>("script:request", (e) => {
          void handle(e.payload).then((reply) =>
            invoke<void>("script_reply", { id: e.payload.id, value: reply.value ?? null, error: reply.error ?? null }),
          );
        });
        if (!live) {
          unlisten();
          return;
        }
        stop = unlisten;
        await invoke<void>("script_ready");
      });
      return () => {
        live = false;
        stop?.();
      };
    },
    mirrorPreferences: (preferences) => invoke<void>("script_preferences", { preferences }),
    onDevicesChanged: (listener) => subscribe("devices:changed", () => listener()),
    linkStatus: () => invoke<LinkStatus>("link_status"),
    linkPeers: () => invoke<LinkPeerSeen[]>("link_peers"),
    onLinkPeers: (listener) => subscribe<LinkPeerSeen[]>("link:peers", listener),
    startLinkExport: (iface, settings, keySort) => invoke<LinkStatus>("start_link_export", {
      interface: iface ?? null,
      deviceSettings: settings ?? null,
      alphabeticalKeys: keySort === "alphabetical",
    }),
    stopLinkExport: () => invoke<LinkStatus>("stop_link_export"),
    loadTrackOnLink: (playerNumber, trackId) => invoke<void>("link_load_track", { playerNumber, trackId }),
    setLinkMaster: (on) => invoke<LinkStatus>("link_set_master", { on }),
    nudgeLinkMaster: (deltaBpm) => invoke<LinkStatus>("link_nudge_master", { deltaBpm }),
    takeLinkMasterTempo: () => invoke<LinkStatus>("link_take_master_tempo"),
    onLinkStatus: (listener) => subscribe<LinkStatus>("link:status", listener),
    missingTracks: (limit) => invoke<MissingTracks>("missing_tracks", { limit }),
    findDuplicates: (limit) => invoke<Duplicates>("find_duplicates", { limit }),
    relocateTrack: async (trackId) => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        title: "Choose the file for this track",
        filters: [
          {
            name: "Audio",
            extensions: ["mp3", "m4a", "aiff", "aif", "wav", "flac", "aac", "ogg"],
          },
        ],
      });
      // Cancelling is a normal outcome, not an error.
      if (typeof picked !== "string") return null;
      await invoke<number>("relocate_track", { track: trackId, path: picked });
      return picked;
    },
    autoRelocate: (folders) => invoke<RelocateReport>("auto_relocate", { folders }),
    pickImage: async (title) => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        title,
        filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png"] }],
      });
      return typeof picked === "string" ? picked : null;
    },
    pickFolder: async (title) => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ multiple: false, directory: true, title });
      // Cancelling is a normal outcome, not an error.
      return typeof picked === "string" ? picked : null;
    },
    openPreferences: async (pane) => {
      await invoke<void>("open_preferences", { pane });
      return true;
    },
    onPreferencesReset: (listener) => subscribe<PreferencesRequest>("preferences:reset", listener),
    requestPreferencesReset: async (what) => {
      const { emit } = await import("@tauri-apps/api/event");
      await emit("preferences:reset", what);
    },
    closeWindow: async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    },
    openReportWindow: async () => { await invoke("open_report_window"); return true; },
    reportAttachment: () => invoke<string>("report_attachment"),
    openReportAttachment: (attachment) => invoke<void>("open_report_attachment", { attachment }),
    openSyncWindow: async () => {
      await invoke<void>("open_sync_window");
      return true;
    },
    syncDevices: (playlists, destinations, defaults, automatic, ejectAfterSync, deleteUnlistedMusic, compatibilityFormat) =>
      invoke<SyncDeviceReport[]>("sync_devices", {
        playlists,
        destinations,
        defaults: defaults ?? null,
        automatic: automatic ?? false,
        ejectAfterSync: ejectAfterSync ?? false,
        deleteUnlistedMusic: deleteUnlistedMusic ?? false,
        compatibilityFormat: compatibilityFormat ?? null,
      }),
    validateExportFiles: (playlists) => invoke<MissingExportFile[]>("validate_export_files", { playlists }),
    importUsb: (path, cues, history, settings) => invoke("import_usb", { path, cues, history, settings }),
    ejectDevice: (path) => invoke<void>("eject_device", { path }),
    deviceSyncState: (path) => invoke<DeviceSyncState>("device_sync_state", { path }),
    smartRule: (playlist) => invoke<SmartRule>("smart_rule", { playlist }),
    onSyncProgress: (listener) => subscribe<SyncProgress>("sync:progress", listener),
    deviceSettings: (path) => invoke<DeviceSettings>("device_settings", { path }),
    writeDeviceDefaults: (path, defaults) => invoke<DeviceSettings>("write_device_defaults", { path, defaults }),
    saveDeviceSettings: (path, settings) =>
      invoke<DeviceSettings>("save_device_settings", { path, settings }),
    ensureDeviceLibrary: (path, defaults) =>
      invoke<DeviceSettings>("ensure_device_library", { path, defaults: defaults ?? null }),
    explorerRoots: () => invoke<ExplorerRoot[]>("explorer_roots"),
    explorerChildren: (path) => invoke<ExplorerChildren>("explorer_children", { path }),
    onLibraryChanged: (listener) => {
      // Tauri's listen resolves asynchronously; unsubscribing before it does
      // has to still work, so the flag is checked when it lands.
      let live = true;
      let stop: (() => void) | undefined;
      void import("@tauri-apps/api/event").then(async ({ listen }) => {
        const unlisten = await listen<number>("library:changed", (e) => listener(e.payload));
        if (live) stop = unlisten;
        else unlisten();
      });
      return () => {
        live = false;
        stop?.();
      };
    },
    onTagListChanged: (listener) => subscribe("tag-list:changed", () => listener()),
    onEditHistory: (listener) => subscribe<EditHistoryState>("edit-history:changed", listener),
    edits: {
      createPlaylist: (name, parent) => invoke<number>("create_playlist", { name, parent }),
      createSmartPlaylist: (name, parent, rule) => invoke<number>("create_smart_playlist", { name, parent, rule }),
      setSmartRule: (playlist, rule) => invoke<number>("set_smart_rule", { playlist, rule }),
      createFolder: (name, parent) => invoke<number>("create_folder", { name, parent }),
      renamePlaylist: (id, name) => invoke<EditHistoryState>("rename_playlist", { id, name }),
      movePlaylist: (id, parent, index) => invoke<EditHistoryState>("move_playlist", { id, parent, index }),
      deletePlaylist: (id) => invoke<EditHistoryState>("delete_playlist", { id }),
      undoEdit: () => invoke<EditHistoryState>("undo_edit"),
      redoEdit: () => invoke<EditHistoryState>("redo_edit"),
      addTracksToPlaylist: (playlist, tracks) =>
        invoke<number>("add_tracks_to_playlist", { playlist, tracks }),
      reloadTags: (tracks) => invoke<number>("reload_tags", { tracks }),
      addToTagList: (tracks) => invoke<number>("add_to_tag_list", { tracks }),
      removeFromTagList: (tracks) => invoke<number>("remove_from_tag_list", { tracks }),
      clearTagList: () => invoke<number>("clear_tag_list"),
      removeTracksFromPlaylist: (playlist, tracks) =>
        invoke<EditHistoryState>("remove_tracks_from_playlist", { playlist, tracks }),
      resetPlayCount: (tracks) => invoke<EditHistoryState>("reset_play_count", { tracks }),
      recordPlay: (track) => invoke<number>("record_play", { track }),
      removeFromHistory: (history, tracks) => invoke<number>("remove_from_history", { history, tracks }),
      removeFromCollection: (tracks) => invoke<number>("remove_from_collection", { tracks }),
      reorderPlaylist: (playlist, tracks) =>
        invoke<number>("reorder_playlist", { playlist, tracks }),
      setTrackRating: (track, stars) => invoke<EditHistoryState>("set_track_rating", { track, stars }),
      setTrackComment: (track, comment) => invoke<EditHistoryState>("set_track_comment", { track, comment }),
      setTrackColor: (track, color) => invoke<EditHistoryState>("set_track_color", { track, color }),
      addCue: (track, kind, positionMs) => invoke<string>("add_cue", { track, kind, positionMs }),
      addLoop: (track, kind, inMs, outMs, beats) =>
        invoke<string>("add_loop", { track, kind, inMs, outMs, beats: beats ?? null }),
      moveCue: (cue, positionMs) => invoke<void>("move_cue", { cue, positionMs }),
      setCueColour: (cue, colour) => invoke<void>("set_cue_colour", { cue, colour }),
      deleteCue: (cue) => invoke<void>("delete_cue", { cue }),
      gridEdit: (track, edit, options) =>
        invoke<GridState>("grid_edit", {
          track, edit, fromMs: options?.fromMs ?? null, deck: options?.deck ?? null, options,
        }),
      gridUndo: (track, deck) => invoke<GridState>("grid_undo", { track, deck: deck ?? null }),
      gridRedo: (track, deck) => invoke<GridState>("grid_redo", { track, deck: deck ?? null }),
      gridLock: (track, on) => invoke<GridState>("grid_lock", { track, on }),
      convertMemoryCuesToHot: (track) => invoke<number>("convert_memory_cues_to_hot", { track }),
      setTrackField: (track, field, value) =>
        invoke<EditHistoryState>("set_track_field", { track, field, value }),
      addArtwork: (track, image) => invoke<EditHistoryState>("add_artwork", { track, image }),
      addPlaylistArtwork: (playlist, image) => invoke<number>("add_playlist_artwork", { playlist, image }),
      setMyTags: (track, tags) => invoke<EditHistoryState>("set_my_tags", { track, tags }),
      clearArtwork: (track) => invoke<EditHistoryState>("clear_artwork", { track }),
    },
    filterValues: (spec) => invoke<FilterValues>("filter_values", { spec }),
    trackDetails: (trackId) => invoke<TrackDetails>("track_details", { track: trackId }),
    trackLookups: () => invoke<TrackLookups>("track_lookups"),
  };
}

let backendPromise: Promise<Backend> | null = null;

export function getBackend(): Promise<Backend> {
  backendPromise ??= isTauri
    ? realBackend()
    : import("./backend-mock").then((m) => m.createMockBackend());
  return backendPromise;
}

/** Test seam: lets e2e and unit tests substitute a backend. */
export function __setBackend(b: Backend | null): void {
  backendPromise = b ? Promise.resolve(b) : null;
}
