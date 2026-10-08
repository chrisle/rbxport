/**
 * IPC contract.
 *
 * These types will be generated from the Rust DTOs by specta once `rbl-app`
 * lands; until then they are hand-written and the mock backend is checked
 * against the Rust view tests. Field names are camelCase to match
 * `#[serde(rename_all = "camelCase")]` on the Rust side.
 */

/** One row of the track table. Kept flat and small: ~200 bytes of JSON. */
export interface RowDto {
  id: string;
  trackNo: number;
  title: string;
  artist: string;
  album: string;
  genre: string;
  label: string;
  comment: string;
  /** BPM x100, as rekordbox stores it. */
  bpmX100: number;
  key: string;
  durationSec: number;
  rating: number;
  /** 0 = not analysed, 1 = analysed. */
  analysed: number;
  dateAdded: string;
  releaseDate: string;
  /**
   * The track's hot cues, in slot order A to P, for the badges on the row's
   * preview waveform. Tuples rather than objects so a page of 64 rows with
   * every slot set stays inside the 64 KB response cap.
   */
  hotCues: RowCue[];
  /** Memory-cue and memory-loop start positions, in milliseconds. */
  memoryCues?: number[];
  artworkHue: number;
  /** Whether the backend can serve artwork for this track. */
  hasArtwork: boolean;
  /**
   * The file's own name, for the Explorer's File Name column.
   *
   * Optional only so a row built before the column existed still type-checks;
   * the backend always sends it.
   */
  fileName?: string;
  /** Extra database fields requested for visible browser columns. */
  extra?: Record<string, string | number | boolean>;
}

export type TrackSource =
  | { kind: "collection" }
  | { kind: "playlist"; id: string }
  | { kind: "playlistFolder"; id: string }
  | { kind: "history"; id: string }
  /** A folder on disk, for the Explorer. An empty path is the heading, which lists nothing. */
  | { kind: "folder"; path: string }
  /** Related Tracks: what goes with `track` under a criterion. An empty track lists nothing. */
  | { kind: "related"; track: string; criterion: RelatedCriterion }
  /** The Tag List, in its own order. */
  | { kind: "tagList" };

/** The Related Tracks section's criteria: rekordbox's own three. */
export type RelatedCriterion = "bpmKey" | "genreRecent" | "artist" | "suggestion";

/**
 * The browser columns the backend can order by: every column rekordbox
 * 7.2.11's own list sorts (its `BrowseHeaderManager::isSortableColumn`),
 * except Hot Cue, whose rekordbox sort is by a field this column does not
 * show. Each name is the column's own key.
 */
export type SortColumn =
  | "trackNo" | "title" | "artist" | "album" | "genre" | "label"
  | "comment" | "bpm" | "key" | "duration" | "rating" | "djPlayCount"
  | "dateAdded" | "releaseDate" | "size" | "year" | "sampleRate" | "bitrate"
  | "color" | "fileName" | "location" | "composer" | "albumArtist" | "remixer"
  | "originalArtist" | "mixName" | "discNo" | "trackNumber" | "fileType"
  | "bitDepth" | "lyricist" | "dateCreated" | "publishTrackInfo" | "message";

/**
 * What the backend sorts by. The columns, plus the key round the Camelot
 * wheel: the Key column sorts by what it shows, and with the alphanumeric
 * display that is `1A` to `12B`, not the classic names' alphabet.
 */
export type SortKey = SortColumn | "keyCamelot";

export interface ViewSpec {
  source: TrackSource;
  sort: SortKey;
  descending: boolean;
  /** Free-text search, matched the way the Rust index folds it. */
  query: string;
  searchField?: import("@/lib/search").TrackSearchField;
  /**
   * The track filter bar's picks, applied by Rust after the search. Absent
   * means no filter; the sub-browser never sends one.
   */
  filter?: TrackFilter;
}

export interface ViewHandle {
  viewId: number;
  /** Row count for this view. */
  len: number;
  /** Bumped whenever the underlying library changes; stale pages are dropped. */
  gen: number;
}

export interface TreeNode {
  id: string;
  name: string;
  kind:
    | "collection" | "histories" | "folder" | "playlist" | "history" | "allTracks" | "device"
    /** An intelligent playlist: a rule, whose tracks are whatever it admits when opened. */
    | "smartPlaylist"
    /** The Explorer heading, and a folder on disk under it. */
    | "explorer" | "directory"
    /** The Related Tracks heading, and a criterion under it. */
    | "related" | "relatedCriterion"
    /** rekordbox's Tag List: its one temporary list, kept in the library. */
    | "tagList"
    /** A line of information in the tree, not a place: nothing opens when it is clicked. */
    | "note";
  depth: number;
  /** Undefined for leaves. */
  expanded?: boolean;
  childCount?: number;
  /**
   * Its children are read when it is opened, not before, so it is a branch
   * whether or not anything sits under it yet. The Explorer's folders.
   */
  lazy?: true;
}

/** Why the library did not load at startup. */
export type LibraryProblem =
  /** No library configured anywhere; one can be made at `masterDb`. */
  | { kind: "missing"; masterDb: string }
  /**
   * A library is configured at `masterDb`, not the default folder, and is
   * not there, most often because its drive is not connected: rekordbox's
   * "Cannot find Master Database" question. Nothing is made in its place;
   * `useDefaultLibrary` switches to `defaultMasterDb`'s folder instead.
   */
  | { kind: "unavailable"; masterDb: string; defaultMasterDb: string }
  /** A library, or something in its place, that would not open. */
  | { kind: "failed"; message: string };

/** One entry of Database management's drive list. */
export interface DatabaseDrive {
  /** The drive's name: its volume label, as rekordbox shows it. */
  name: string;
  /** The library's `master.db` on that drive. */
  masterDb: string;
  /** Whether it is the library open now. */
  current: boolean;
}

export interface LibrarySummary {
  trackCount: number;
  playlistCount: number;
  /** True when rekordbox is running, i.e. writes are refused. */
  readOnly: boolean;
  /** rekordbox's own DB schema version, e.g. 6000. */
  dbVersion: number | null;
}

/** What the app is costing, for the title bar's readout. */
export interface Diagnostics {
  audioLoad: number;
  audioXruns: number;
  /** Percent of one core, as the OS accounts it. Over 100 on several cores. */
  cpu: number;
  memoryMb: number;
  /** `null` where the platform will not say. */
  threads: number | null;
  openFiles: number | null;
  /**
   * Always `null` on macOS: per-process GPU is behind `powermetrics`, which
   * needs root. Reported rather than dropped, so the readout can say it is
   * unavailable instead of implying the app uses none.
   */
  gpu: number | null;
}

/** Every command returns this shape on failure. */
export interface AppErrorDto {
  kind: "readOnly" | "notFound" | "malformed" | "cancelled" | "internal";
  message: string;
  detail?: string;
}

/** Which waveform to fetch for a track. */
/**
 * Which waveform tag to read, one pair per palette of View › Color:
 *
 * - `bands` / `bandsDetail`: the three-band `PWV6` / `PWV7`, three bytes a
 *   column, which 3Band draws — every track checked in the reference
 *   library has them.
 * - `mono` / `monoDetail`: `PWAV` / `PWV3`, one byte a column (five bits of
 *   height, three of whiteness), which BLUE draws.
 * - `colour` / `colourDetail`: `PWV4` / `PWV5`, six and two bytes a column,
 *   which RGB draws.
 */
export type WaveformKind = "bands" | "bandsDetail" | "mono" | "monoDetail" | "colour" | "colourDetail";


export interface Backend {
  /** The installed rekordbox browseSetting.xml, when present. Read-only. */
  rekordboxBrowseSettings(): Promise<string | null>;
  librarySummary(): Promise<LibrarySummary>;
  /** Session-only escape hatch, available only when the native process opted in. */
  disableReadOnly(): Promise<void>;
  rememberScreenAssets(ids: string[], waveformKind: WaveformKind): Promise<void>;
  playlistTree(): Promise<TreeNode[]>;
  openView(spec: ViewSpec): Promise<ViewHandle>;
  fetchRows(viewId: number, offset: number, len: number, extraColumns?: readonly string[]): Promise<RowDto[]>;
  /** Ids between two row indices inclusive; used for shift-click across unfetched rows. */
  viewIdsInRange(viewId: number, from: number, to: number): Promise<string[]>;
  /**
   * Raw waveform bytes for a track, or an empty array when it has no analysis.
   * Sent as bytes rather than JSON: a colour waveform is several kilobytes of
   * numbers and JSON would multiply that and cost a parse on the UI thread.
   */
  trackWaveform(
    trackId: string,
    kind: WaveformKind,
    /** Window into the tag, in entries. The detail tag is far past the cap. */
    window?: { from: number; len: number },
  ): Promise<Uint8Array>;
  /**
   * A decoded stereo PCM peak envelope for a short window around the
   * playhead. Each point is left min/max then right min/max as i16 values.
   */
  trackPcmWaveform(trackId: string, fromMs: number, toMs: number, columns: number): Promise<Uint8Array>;

  /**
   * Editing. Plain edits return the library generation; reversible edits
   * return that generation together with the shared undo/redo state.
   *
   * All of these are refused while Rekordbox is running — it holds the
   * database — and the refusal arrives as an `AppError` of kind `readOnly`.
   */
  edits: Edits;

  /**
   * Called when the library changes underneath us — after an edit, or when
   * Rekordbox itself writes. The argument is the new generation, which
   * invalidates every cached page.
   *
   * Returns an unsubscribe function.
   */
  onLibraryChanged(listener: (generation: number) => void): () => void;
  /**
   * Called when the Tag List changes, from this window or from a player
   * over LINK. No other list shows Tag List membership, so the generation
   * stays as it was and only a view of the Tag List has to reopen.
   *
   * Returns an unsubscribe function.
   */
  onTagListChanged(listener: () => void): () => void;
  /** The shared library undo stack changed, including its native-menu labels. */
  onEditHistory(listener: (history: EditHistoryState) => void): () => void;
  /**
   * Re-reads the library on request. What the analysis queue asks for once
   * it has drained: the rows drawn from each answer are then replaced by
   * the library's own, in one reload rather than one per track.
   */
  reloadLibrary(): Promise<number>;
  /**
   * A track's analysis files were rewritten, so a deck showing it redraws
   * its waveform. Returns its own unsubscribe.
   */
  onAnalysisChanged(listener: (trackId: string) => void): () => void;

  /**
   * Fires when the backend has finished loading the library.
   *
   * The load runs on its own thread and takes as long as the collection is
   * big, so the first thing the interface asks for can easily arrive before
   * there is anything to answer with. Without this the window sat on
   * "Loading…" forever, because the failed first attempt was never retried.
   *
   * Returns an unsubscribe function.
   */
  onLibraryReady(listener: () => void): () => void;

  /** Fires when the library could not be loaded, with the reason. */
  onLibraryProblem(listener: (problem: LibraryProblem) => void): () => void;

  /**
   * Why the library did not load, or null while it is loading or loaded.
   * Asked as well as listened for: with no library at all the backend gives
   * up before the window has subscribed.
   */
  libraryProblem(): Promise<LibraryProblem | null>;

  /**
   * Makes a new, empty library where rekordbox keeps one and loads it;
   * `onLibraryReady` fires when it is up. Only when `libraryProblem` said
   * `missing`; a library that has appeared since is loaded, not replaced.
   */
  createLibrary(): Promise<void>;

  /**
   * The Yes of "Cannot find Master Database", once confirmed: sets
   * rekordbox's library location to the default folder and loads what is
   * there, or reports `missing` when there is nothing to load.
   */
  useDefaultLibrary(): Promise<void>;

  /**
   * Database management's drive list: the default drive when it holds a
   * library, and every connected drive holding a rekordbox library.
   */
  databaseDrives(): Promise<DatabaseDrive[]>;

  /**
   * Switches to the library on a drive from `databaseDrives`, as choosing it
   * in rekordbox's Database management does, and starts the app again on
   * it. Rejects with the reason when it cannot.
   */
  switchLibrary(masterDb: string): Promise<void>;

  /**
   * Fires after a cue edit with the id of the track whose cues changed.
   * A deck showing that track refetches its cues; nothing else has to move.
   */
  onCuesChanged(listener: (trackId: string) => void): () => void;

  /**
   * Fires after a grid edit, undo or redo with the id of the track whose
   * grid changed. A deck showing that track refetches its beats and its
   * grid state. A tempo change also announces `onLibraryChanged`, because
   * the row's BPM column changed with it.
   */
  onGridChanged(listener: (trackId: string) => void): () => void;

  /**
   * Tracks whose file has gone. The count is exact; the list is a first page,
   * because a library can lose thousands when a drive is unplugged and a list
   * that long is neither useful nor small enough for the IPC cap.
   */
  /**
   * Analyses one track: tempo, beat grid, key and waveforms.
   *
   * Slow — a decode and a DSP pass — so callers run these one at a time.
   */
  analyseTrack(trackId: string, mode?: "rekordbox" | "rbxport", settings?: AnalysisSettings): Promise<AnalysisResult>;

  /**
   * A track's whole beat grid, as raw bytes: seven per beat, a little-endian
   * `u32` of milliseconds, the beat's number in its bar, and a little-endian
   * `u16` of the tempo there x100.
   *
   * Whole and once per track rather than a window at a time. Windowing it
   * still re-read and re-parsed the entire analysis file on every fetch, which
   * is the expensive part; `parseBeatGrid` turns these bytes into typed arrays
   * and `beatsIn` slices the window being drawn.
   */
  trackBeats(trackId: string): Promise<Uint8Array>;

  /**
   * The GRID panel's view of a track's grid: its tempo, how many beats,
   * whether there is anything to undo or redo, and whether it is locked.
   * Rejects as `notFound` for a track with no grid.
   */
  gridState(trackId: string): Promise<GridState>;

  /** A track's cue points, ordered by position. */
  trackCues(trackId: string): Promise<Cue[]>;

  /**
   * Asks for files and adds them to the library.
   *
   * Resolves to what happened, or `null` if the picker was cancelled. Reports
   * per file rather than failing the batch: a folder with two unreadable
   * tracks should import the rest.
   */
  importFiles(): Promise<ImportReport | null>;
  /**
   * Add music from a folder: picks one or more directories and imports every
   * supported audio file under them, recursively through subfolders.
   *
   * Resolves to what happened, or `null` if the picker was cancelled. Reports
   * per file like {@link importFiles}, so a stray unreadable track in the tree
   * does not sink the rest.
   */
  importFolder(): Promise<ImportReport | null>;
  /** Adds these files to the library: the Explorer's Import To Collection. */
  importPaths(paths: string[]): Promise<ImportReport>;
  /**
   * Export Loop As WAV: asks where, then writes the loop's stretch of the
   * track as a WAV. Resolves to the frames written, or null when cancelled.
   */
  exportLoopWav(track: string, title: string, inMs: number, outMs: number): Promise<number | null>;
  /**
   * Export a playlist to a file, where the platform's save dialog says:
   * `m3u8`, or rekordbox's tab-separated `txt`. Resolves to how many tracks,
   * or null when cancelled.
   */
  exportPlaylistFile(playlistId: string, name: string, format: "m3u8" | "txt"): Promise<number | null>;
  /**
   * Imports a rekordbox XML collection chosen in the platform's file
   * dialog: its files into the library, its playlists, and the cues of each
   * track that landed. Null when the dialog is cancelled.
   */
  importXml(): Promise<XmlImportReport | null>;
  /** Asks for Music.app's Library.xml and imports its tracks and playlists. */
  importItunes(): Promise<XmlImportReport | null>;
  /**
   * The iTunes / Music library at its usual place, for the Sync Manager's
   * iTunes column. Null when no shared `Library.xml` is found, so the column
   * can offer {@link chooseItunesLibrary} instead.
   */
  itunesDefaultLibrary(): Promise<ItunesLibrary | null>;
  /**
   * Picks an iTunes / Music `Library.xml` in the file dialog and reads its
   * playlist tree. Null when the dialog is cancelled.
   */
  chooseItunesLibrary(): Promise<ItunesLibrary | null>;
  /**
   * Imports the ticked iTunes playlists — `itunes:<index>` ids from an
   * {@link ItunesLibrary} tree — into the library, folders above them kept.
   */
  importItunesSelected(path: string, ids: readonly string[]): Promise<XmlImportReport>;
  /**
   * Writes the collection as rekordbox's XML where the platform's save
   * dialog says; resolves to how many tracks, or null when cancelled.
   */
  exportXml(): Promise<number | null>;

  /**
   * Writes a playlist to `destination`, asking for one when none is given.
   *
   * Resolves to what was written, or `null` if the picker was cancelled. A
   * destination that already holds one of our exports is synced rather than
   * rewritten.
   */
  exportPlaylist(
    playlistId: string,
    destination?: string,
    /** What a stick with no settings of its own is given; see `StickDefaults`. */
    defaults?: StickDefaults,
    /** Remove RBXport-exported music outside the playlists being synced. */
    deleteUnlistedMusic?: boolean,
    /** Convert incompatible USB copies; undefined preserves the source format. */
    compatibilityFormat?: "wav" | "aiff" | "mp3",
  ): Promise<ExportReport | null>;

  /**
   * Export Track: puts tracks on a stick on their own, in no playlist,
   * beside what the stick already holds. A later sync keeps them unless
   * deleteUnlistedMusic is enabled.
   */
  exportTracksToDevice(tracks: string[], destination: string, defaults?: StickDefaults, compatibilityFormat?: "wav" | "aiff" | "mp3"): Promise<ExportReport>;

  /**
   * rekordbox's reference browse categories and sort options: what a
   * fresh export writes when the Preferences window has not changed them.
   */
  referenceStickSettings(): Promise<ReferenceStickSettings>;

  /** A track's phrase structure, empty when it has no `PSSI` tag. */
  trackPhrases(trackId: string): Promise<Phrase[]>;
  /**
   * PHRASE EDIT: `cut` splits the phrase under `beat` (1-based, on the
   * track's grid), `clear` takes it out. Resolves to whether anything
   * changed; `onAnalysisChanged` says so as well.
   */
  editPhrase(trackId: string, beat: number, action: "cut" | "clear"): Promise<boolean>;

  /**
   * Per-column vocal presence, empty when the track has no `PVDI` tag.
   *
   * Raw bytes rather than JSON: this is one value per overview column.
   */
  trackVocals(trackId: string): Promise<Uint8Array>;

  /** Opens an https address in the person's browser. */
  openUrl(url: string): Promise<void>;
  /** The backups this app has taken, newest first. */
  listBackups(): Promise<Backup[]>;
  /** The configured directory, including before the first backup exists. */
  backupDirectory(): Promise<string>;
  /** Opens the configured backup folder in the system file manager. */
  openBackupDirectory(): Promise<void>;
  /** Logical sizes of the current library contents included in a backup. */
  backupSizes(refresh?: boolean): Promise<BackupSizes>;
  startBackup(): Promise<void>;
  cancelBackup(): Promise<void>;
  backupProgress(): Promise<BackupProgress>;
  /** Copies the library aside now; resolves to where the copy went. */
  backUpLibrary(): Promise<string>;
  /** Persist the default folder used for future backups. Existing files stay where they are. */
  setBackupDirectory(directory: string): Promise<string>;
  deleteBackup(path: string): Promise<void>;
  /** Called after each track of an export, while one runs. Returns its own unsubscribe. */
  onExportProgress(listener: (progress: ExportProgress) => void): () => void;
  exportProgress(): Promise<ExportProgress[]>;
  cancelExport(path: string): Promise<void>;
  /** A yes-or-no question in the platform's own dialog; false when dismissed. */
  confirm(message: string, labels?: { yes: string; no: string }): Promise<boolean>;
  /** The volumes an export could be written to, and what is on each. */
  listDevices(): Promise<Device[]>;
  /**
   * Called when a volume is mounted or unmounted, so the panel can refresh
   * without waiting for focus or a click. Returns its own unsubscribe.
   */
  onDevicesChanged(listener: () => void): () => void;

  /**
   * Native menu clicks, as the item's id.
   *
   * One subscription rather than one per item, so adding a menu item does not
   * mean adding another listener. Returns its own unsubscribe.
   */
  onMenu(listener: (id: string) => void): () => void;

  /** Rebuilds the native application menu with the active UI translations. */
  setMenuLabels(labels: Readonly<Record<string, string>>): Promise<void>;

  /**
   * AppleScript's requests to the window: each is run with `handle` and its
   * reply sent back, and the backend is told once they can be heard.
   * Returns its own unsubscribe.
   */
  serveScripts(handle: (request: ScriptRequest) => Promise<ScriptReply>): () => void;
  /**
   * Keeps the backend's copy of the preferences current, for scripts to
   * read: the whole set, as `src/lib/preferences.ts` stores it.
   */
  mirrorPreferences(preferences: object): Promise<void>;

  /** LINK as it stands: on or off, on what, and who is listening. */
  linkStatus(): Promise<LinkStatus>;
  /** Players and mixers heard on the network, whether or not LINK is on. */
  linkPeers(): Promise<LinkPeerSeen[]>;
  /** Called as the set of devices heard on the network changes (from start). */
  onLinkPeers(listener: (peers: LinkPeerSeen[]) => void): () => void;
  /**
   * Turns LINK on: the app announces itself as `rekordbox` on the named
   * interface (the first one when none is given) and serves the library to
   * every player that asks. Refused, with the reason, while rekordbox runs.
   */
  startLinkExport(iface?: string, keyDisplay?: KeyDisplay, keySort?: "alphabetical" | "musical"): Promise<LinkStatus>;
  stopLinkExport(): Promise<LinkStatus>;
  /** Tells a CDJ on the link to load a specific track from our library. */
  loadTrackOnLink(playerNumber: number, trackId: string): Promise<void>;
  /** Becomes the network's tempo master, or resigns; returns fresh status. */
  setLinkMaster(on: boolean): Promise<LinkStatus>;
  /** Nudges the master tempo by `deltaBpm` (rekordbox's −/+ is ±1). */
  nudgeLinkMaster(deltaBpm: number): Promise<LinkStatus>;
  /** Takes the current master player's tempo as the master tempo (⟳). */
  takeLinkMasterTempo(): Promise<LinkStatus>;
  /** Called as LINK turns on or off and as the players change. */
  onLinkStatus(listener: (status: LinkStatus) => void): () => void;

  /**
   * The decks.
   *
   * Playback is in Rust — see `crates/rbl-deck`. The audio device is not
   * opened until one of these is called, so a window nobody has played
   * anything in holds no device at all.
   *
   * Position does not come back from any of these: it arrives on `onDeckTick`
   * ten times a second and the interface extrapolates between ticks.
   */
  deckLoad(deck: DeckId, trackId: string, loadId: number): Promise<void>;
  deckUnload(deck: DeckId): Promise<void>;
  deckPlay(deck: DeckId): Promise<void>;
  /**
   * Starts a deck after `delayMs` of silence, counted by the audio callback:
   * quantized play on a synced deck, held for the master's next beat.
   */
  deckPlayAfter(deck: DeckId, delayMs: number): Promise<void>;
  deckPause(deck: DeckId): Promise<void>;
  deckSeek(deck: DeckId, positionMs: number): Promise<void>;
  /**
   * Sets a loop between two points and turns it on. A head already past
   * the out point goes back to the in point. The deck rounds at the out
   * point itself, on the frame, with nothing faded at the seam.
   */
  deckSetLoop(deck: DeckId, inMs: number, outMs: number): Promise<void>;
  /** RELOOP (on): back in from the in point. EXIT (off): out, the range kept. */
  deckLoopActive(deck: DeckId, on: boolean): Promise<void>;
  deckClearLoop(deck: DeckId): Promise<void>;
  /**
   * Dragging the waveform like a record.
   *
   * Between `deckScrubBegin` and `deckScrubEnd` the deck reads a decoded
   * window at the drag's own rate — forwards, backwards, and silent when the
   * pointer stops — rather than seeking. A seek per pointer move gives the
   * right place at the wrong speed: a burst of normal-speed audio each time.
   */
  deckScrubBegin(deck: DeckId): Promise<void>;
  deckScrubTo(deck: DeckId, positionMs: number): Promise<void>;
  deckScrubEnd(deck: DeckId): Promise<void>;
  /** The master output level, 0 to +2 dB. It arrives back on the next tick. */
  setMasterLevel(level: number): Promise<void>;
  /**
   * The outputs the audio could go to, and which is in use.
   *
   * Read on each call rather than cached: an interface is plugged in while the
   * app is open more often than not.
   */
  audioDevices(): Promise<AudioDevices>;
  /** Choose one, or `null` for the system's own. Takes effect on the next play. */
  setAudioDevice(device: string | null): Promise<void>;
  /**
   * Asks the download server for the newest version.
   *
   * `version` is null when this build is the newest; otherwise `changes`
   * holds the release notes between the two, newest first, and the update is
   * held for `downloadUpdate`. `ready` says when that version was already
   * downloaded this run, so there is nothing to fetch again.
   */
  checkForUpdate(): Promise<UpdateCheck>;
  /** A completed download held by this process, without asking the network. */
  readyUpdate(): Promise<UpdateReady | null>;
  /**
   * Downloads what the last check found and puts it in place: swapped on
   * disk where the app can be while it runs (macOS, an AppImage), so the
   * next launch is the new version; staged for the quit on Windows. Resolves
   * to what happened.
   */
  downloadUpdate(): Promise<UpdateReady>;
  /**
   * Runs the downloaded update now: a restart into one that is in place, or
   * the staged installer, which brings the app back itself. Resolves only
   * if that failed — a success ends the process.
   */
  restartToUpdate(): Promise<void>;
  /** The download's progress, about ten times a second while it runs. */
  onUpdateProgress(listener: (progress: UpdateProgress) => void): () => void;
  /** The master limiter as it stands. */
  masterLimiter(): Promise<Limiter>;
  /** Sets the master limiter; what comes back is what the engine could set. */
  setMasterLimiter(limiter: Limiter): Promise<Limiter>;
  /**
   * How fast a deck plays, as a multiple of the file's own speed.
   *
   * A ratio rather than a BPM: what BPM that comes to depends on the track,
   * and the deck does not need to know the track's to play it faster.
   */
  deckTempo(deck: DeckId, tempo: number): Promise<void>;
  /** Master Tempo: whether the pitch is held while the speed changes. */
  deckMasterTempo(deck: DeckId, on: boolean): Promise<void>;
  /** A click on every beat of the deck's grid while it plays. */
  deckMetronome(deck: DeckId, on: boolean): Promise<void>;
  /** The key, in semitones from the track's own; −12 to 12. */
  deckKeyShift(deck: DeckId, semitones: number): Promise<void>;
  /** Preferences › Audio › Metronome: which click (1 to 3) and how loud. */
  setMetronome(sound: 1 | 2 | 3, volume: "small" | "middle" | "large"): Promise<void>;
  /**
   * Preferences › Audio › Sample Rate and Buffer size, asked of the device
   * the next time a deck plays. Either may be left to the device.
   */
  setAudioConfig(sampleRate: number | null, bufferFrames: number | null): Promise<void>;
  /**
   * One deck's channel strip.
   *
   * Knob positions rather than decibels: what a position means is the mixer's
   * to decide and it changes with the EQ / ISOLATOR switch, so the interface
   * never has to know the curve. 0.5 is centre and 0.5 is unity.
   */
  setChannelBand(deck: DeckId, band: EqBand, position: number): Promise<void>;
  /** Kill a band: the knob at the bottom of whichever curve is in use. */
  setChannelKill(deck: DeckId, band: EqBand, killed: boolean): Promise<void>;
  /** The deck's gain, 0 to 2 — up to +6 dB, as a mixer's trim gives. */
  setChannelTrim(deck: DeckId, trim: number): Promise<void>;
  /** The crossfader: 0 is deck A alone, 1 is deck B alone, 0.5 is both. */
  setCrossfade(position: number): Promise<void>;
  /** EQ or ISOLATOR — the bottom of each band's travel, and nothing else. */
  setEqCurve(isolator: boolean): Promise<void>;
  /** Both decks now, to anchor the interface when it starts. */
  deckState(): Promise<Tick>;
  /**
   * The browser's preview player: a click on a row's waveform plays the track
   * from there without loading it onto a deck, and pauses the decks, as
   * rekordbox does outside PERFORMANCE mode. Its own player, so it has no
   * tick: the interface asks `previewState` while it plays.
   */
  previewPlay(trackId: string, positionMs: number): Promise<void>;
  previewStop(): Promise<void>;
  previewState(): Promise<PreviewState>;
  /** Both decks, ten times a second, and only while something is playing. */
  onDeckTick(listener: (tick: Tick) => void): () => void;
  /**
   * The master's meters, thirty times a second.
   *
   * Its own event because it is wanted three times as often as the decks and
   * is a twentieth of the size. The peaks are cleared as they are read, so
   * each one is the loudest sample since the last.
   */
  onMeters(listener: (meters: Meters) => void): () => void;
  /** A deck has finished loading a track, or could not. */
  onDeckEvent(listener: (event: DeckEvent) => void): () => void;
  /** The audio engine was replaced after its device or stream format changed. */
  onDeckReset(listener: () => void): () => void;

  /** A reading of this process, sampled on demand. */
  appDiagnostics(): Promise<Diagnostics>;
  /** The version this build carries, for About; no network is asked. */
  appVersion(): Promise<string>;
  /** Opens the current application log with the OS default handler. */
  openLog(): Promise<void>;

  /** Shows a track's file in the Finder. */
  revealTrack(trackId: string): Promise<void>;

  missingTracks(limit: number): Promise<MissingTracks>;
  /** Tracks that share a title and an artist, the first `limit` groups listed. */
  findDuplicates(limit: number): Promise<Duplicates>;

  /**
   * Asks the user for a file and points a track at it.
   *
   * Resolves to the chosen path, or `null` if they cancelled. Outside Tauri
   * there is no picker, so it resolves to `null` immediately.
   */
  relocateTrack(trackId: string): Promise<string | null>;

  /**
   * Points every missing track at a file of the same name found under one
   * of `folders`, searched in order. A track whose name is found nowhere
   * is left missing.
   */
  autoRelocate(folders: string[]): Promise<RelocateReport>;

  /** Opens a folder picker; null when it is cancelled. */
  pickFolder(title: string): Promise<string | null>;
  /** The platform's file dialog for one JPEG or PNG; null when cancelled. */
  pickImage(title: string): Promise<string | null>;

  /**
   * Opens the Preferences window on `pane`, or turns the open one to it.
   * False where there are no windows — a browser — and the shell draws the
   * Preferences over itself instead.
   */
  openPreferences(pane: string): Promise<boolean>;

  /**
   * The Preferences window asking the main one for something only it holds:
   * the browser's columns or the panes' widths put back, or a check for
   * updates, whose window belongs to the main one. Returns its own
   * unsubscribe.
   */
  onPreferencesReset(listener: (what: PreferencesRequest) => void): () => void;
  requestPreferencesReset(what: PreferencesRequest): Promise<void>;

  /** Closes the window this runs in; nothing in a browser. */
  closeWindow(): Promise<void>;

  /**
   * Opens the Sync Manager window, or brings the open one to the front.
   * False where there are no windows — a browser — and the shell draws the
   * manager over itself instead.
   */
  openSyncWindow(): Promise<boolean>;
  openReportWindow(): Promise<boolean>;
  reportAttachment(): Promise<string>;
  openReportAttachment(attachment: string): Promise<void>;

  /**
   * Writes the same playlists to every destination, one after another, and
   * says how each fared. A stick that fails does not stop the rest: its
   * entry carries the error and the others their reports.
   */
  syncDevices(
    playlists: string[],
    destinations: string[],
    /** What a stick with no settings of its own is given; see `StickDefaults`. */
    defaults: StickDefaults | undefined,
    /**
     * Automatic synchronization: recorded on each stick, so that when it is
     * next plugged in the same playlists are written to it again unasked.
     */
    automatic?: boolean,
    ejectAfterSync?: boolean,
    /** Remove RBXport-exported music outside the playlists being synced. */
    deleteUnlistedMusic?: boolean,
    /** Convert incompatible USB copies; undefined preserves the source format. */
    compatibilityFormat?: "wav" | "aiff" | "mp3",
  ): Promise<SyncDeviceReport[]>;

  /** Missing source audio in the exact playlists selected for USB export. */
  validateExportFiles(playlists: string[]): Promise<MissingExportFile[]>;

  /** Safely eject a mounted USB device; fails if it is in use. */
  ejectDevice(path: string): Promise<void>;

  /** What a stick was last synced with, and what it holds now. */
  deviceSyncState(path: string): Promise<DeviceSyncState>;
  importUsb(path: string, cues: boolean, history: boolean, settings: boolean): Promise<{ tracks: number; histories: number; settings: number; skipped: number; warnings?: string[] }>;

  /**
   * An intelligent playlist's rule, for the editor; an empty "all" for a
   * rule that does not parse. Rejects a rule that nests groups.
   */
  smartRule(playlist: string): Promise<SmartRule>;

  /**
   * Each stick as a sync reaches it and leaves it, so a run over several
   * sticks can say which one it is on. Returns its own unsubscribe.
   */
  onSyncProgress(listener: (progress: SyncProgress) => void): () => void;

  /**
   * The values the track filter bar can offer for a list: which whole BPMs
   * and which keys it holds, counted over the source and query alone so a
   * picked value never hides the others. Rust tallies them in one pass; the
   * frontend never scans rows for this.
   */
  filterValues(spec: ViewSpec): Promise<FilterValues>;

  /**
   * What a selected device's six tabs show: the display settings a player
   * reads from `DEVSETTING.DAT`, and the name, browse categories, sort
   * options, sub-column and colour comments in `exportLibrary.db`. A stick
   * that holds none of it still answers, with the defaults and the presence
   * flags clear.
   */
  deviceSettings(path: string): Promise<DeviceSettings>;
  /**
   * Gives a stick that holds an export but no DEVSETTING.DAT the DJ System
   * defaults, as rekordbox does when its device panel opens; a stick that
   * has one keeps it. Resolves to what the stick now holds.
   */
  writeDeviceDefaults(path: string, defaults: StickDefaults): Promise<DeviceSettings>;
  /** Writes them back and resolves to what the stick now holds. */
  saveDeviceSettings(path: string, settings: DeviceSettings): Promise<DeviceSettings>;
  /**
   * Gives a stick with no database the folders rekordbox creates on connect
   * (an empty database from `defaults`), and reads its settings back. A
   * stick that has one is only read.
   */
  ensureDeviceLibrary(path: string, defaults: StickDefaults | undefined): Promise<DeviceSettings>;

  /**
   * The Explorer.
   *
   * Where it starts, and one folder's subfolders when that folder is opened.
   * Nothing is read ahead: a volume costs one directory read of its top
   * level until somebody opens something under it. A folder that cannot be
   * read answers with no children rather than an error.
   */
  explorerRoots(): Promise<ExplorerRoot[]>;
  explorerChildren(path: string): Promise<ExplorerChildren>;

  /**
   * One track's full record: what the information panel's Summary and Info
   * tabs show and the row DTO does not carry.
   *
   * A point read by id, not a widening of the index — the twenty-odd columns
   * are wanted for one track at a time. About 1 KB.
   */
  trackDetails(trackId: string): Promise<TrackDetails>;

  /** What the Info tab's Key and Genre dropdowns offer: what the library holds. */
  trackLookups(): Promise<TrackLookups>;
}

/** Which deck. Two, named rather than indexed, as the mixer is. */
export type DeckId = "a" | "b";

/** The browser's preview player. */
export interface PreviewState {
  /** The track it holds, or null before anything was previewed. */
  track: string | null;
  playing: boolean;
  positionMs: number;
  durationMs: number;
}

/** Something an AppleScript asks of the window; see `src/lib/scripting.ts`. */
export interface ScriptRequest {
  id: number;
  action: string;
  args: Record<string, unknown>;
}

/** What goes back to the script: the value, or why it could not be done. */
export type ScriptReply = { value: unknown; error?: undefined } | { value?: undefined; error: string };

/** One deck in a tick. */
/** One output the audio could go to. */
export interface AudioDevice {
  /** What to store and open by: stable across runs and reboots. */
  id: string;
  /** What to show. */
  name: string;
}

export interface AudioDevices {
  devices: AudioDevice[];
  /** The system's own choice, so the list can say which that is. */
  default: string | null;
  /** What this app has been told to use, or `null` for the system's. */
  chosen: string | null;
}

/** What the Preferences window can ask the main window to do. */
export type PreferencesRequest = "columns" | "layout";

/** One release's section of the published release notes. */
export interface UpdateChange {
  version: string;
  /** `2026-09-10`, when the heading carries one. */
  date: string | null;
  /** The section's markdown, its heading included. */
  body: string;
}

/** What a check for updates found. */
export interface UpdateCheck {
  currentVersion: string;
  /** The version on offer, or null when this build is the newest. */
  version: string | null;
  /** RFC 3339, when the feed says when it was published. */
  date: string | null;
  /** The release notes between the two versions, newest first. */
  changes: UpdateChange[];
  /** Set when the version on offer is already downloaded this run. */
  ready: UpdateReady | null;
}

/** A downloaded update, and whether it is already in the app's place. */
export interface UpdateReady {
  version: string;
  /**
   * true when the next launch runs it as things stand; false when an
   * installer still has to run, at the quit or from Restart Now.
   */
  installed: boolean;
}

export interface UpdateProgress {
  downloaded: number;
  /** null when the server did not say how big the file is. */
  total: number | null;
}

/**
 * The master limiter, which keeps two decks summed from clipping.
 *
 * Both directions: what is sent, and what the engine says it set — it clamps
 * the numbers to what it can do, and the interface shows that.
 */
export interface Limiter {
  /** Input gain in dB, −24 to +24. */
  inputGainDb: number;
  enabled: boolean;
  /** dBFS, −12 to 0. */
  ceilingDb: number;
  /** Milliseconds, 10 to 1000. */
  releaseMs: number;
}

/** The three bands of a channel strip, high to low as the strip is drawn. */
export type EqBand = "high" | "mid" | "low";

export interface DeckTick {
  /** The engine's frame counter, in device-rate frames. */
  frames: number;
  /** The track's length in the same frames, or 0 when it is not known. */
  totalFrames: number;
  /** Bumped on every load and seek; a new one means snap, not slide. */
  generation: number;
  playing: boolean;
  loaded: boolean;
  /** The load request whose audio is installed; zero means no track. */
  loadId?: number;
  /** A multiple of the file's own speed: 1 is the track as recorded. */
  tempo: number;
  /** Whether the pitch is held while that speed changes. */
  masterTempo: boolean;
  /** Semitones from the track's own key: a CDJ's key shift. */
  keyShift: number;
  /** Output frames until a started deck sounds: a play held for the beat. */
  startInFrames: number;
  /** The loop's in and out points in the same frames; both 0 for none. */
  loopInFrames: number;
  loopOutFrames: number;
  /** Whether the deck is inside the loop: RELOOP on, EXIT off. */
  looping: boolean;
}

/** Both decks at one instant. About 200 bytes, well inside the event cap. */
export interface Tick {
  a: DeckTick;
  b: DeckTick;
  /** The device's rate, which is what every frame count here is in. */
  sampleRate: number;
  /** The loudest sample the device was given last callback, per channel. */
  peakLeft: number;
  peakRight: number;
  /** The master level, 0 to +2 dB. */
  master: number;
  /** How far the limiter turned the sum down since the last tick, in dB. */
  reduction: number;
  /**
   * Whether this build can shift a key without moving the tempo. Only the
   * Rubber Band backend holds a pitch to within a cent; without it the
   * semitone buttons are drawn greyed rather than offer a wrong key.
   */
  shiftsKey: boolean;
}

/** The master's meters and level, on their own faster beat. */
export interface Meters {
  /** Unweighted 400 ms RMS amplitudes from the audio callback. */
  rmsLeft?: number;
  rmsRight?: number;
  peakLeft: number;
  peakRight: number;
  master: number;
  /** How far the limiter turned the sum down since the last tick, in dB. */
  reduction: number;
}

/** A deck finishing a load, or failing one. */
export interface DeckEvent {
  deck: DeckId;
  /** The request this completion belongs to, so superseded loads are ignored. */
  loadId: number;
  totalFrames: number;
  sampleRate: number;
  /** Set when the load failed, and says why. */
  message: string | null;
}

/**
 * One cue point.
 *
 * The colour is what rekordbox paints for the cue's `ColorTableIndex`, from
 * the palette; a memory cue carries its named colour or `null` for No Color.
 */
export interface Cue {
  /** Saved cue note; older backends may omit it. */
  comment?: string;
  /**
   * `djmdCue.ID`, which `moveCue` and `deleteCue` take. Empty for a cue the
   * backend cannot edit — one whose id is not a number under 2^32, of which
   * the reference library has none — and the interface offers no ✕ for it.
   */
  id: string;
  positionMs: number;
  /** Where a loop ends, or 0 for a plain cue. */
  outMs: number;
  /** `A` to `P` for a hot cue, empty for a memory cue. */
  letter: string;
  memory: boolean;
  /** `#RRGGBB`, or `null` where unmeasured. */
  colour: string | null;
}

/** Which slot a new cue goes in: a memory cue, or a hot cue by its letter. */
export type CueKind = "memory" | { hot: string };

/** A hot cue on a track-list row: letter, position in ms, drawn colour. */
export type RowCue = readonly [letter: string, positionMs: number, colour: string | null];

/** A volume an export could be written to. */
export interface Device {
  name: string;
  /** Where it is mounted; this is what an export is written to. */
  path: string;
  totalBytes: number;
  freeBytes: number;
  fileSystem?: string;
  /** Whether the OS calls it removable. External SSDs often say no. */
  removable: boolean;
  /**
   * Names the medium across a rename. On macOS a rename moves the mount
   * point, so `path` goes stale while this stays the same.
   */
  volumeId: string;
  /** What is already on it, null when it holds no export. */
  export: DeviceExport | null;
}

/** What a device already holds. */
export interface DeviceExport {
  tracks: number;
  playlists: number;
  /** True when we wrote it, which is what makes the next export a sync. */
  ours: boolean;
  /** When our own export last ran; empty when this is not one of ours. */
  written: string;
}

/** What an export wrote. */
export interface ExportReport {
  tracks: number;
  playlists: number;
  bytesCopied: number;
  analysisFiles: number;
  /** Tracks already on the stick, unchanged, that did not need copying again. */
  reused: number;
  /** Tracks taken off the stick because the playlist no longer holds them. */
  removed: number;
  /** Playlists newly added to this USB during the export. */
  playlistsAdded: number;
  /** Playlists removed from this USB during the export. */
  playlistsRemoved: number;
  /** Tracks left out because their audio was missing or unreadable. */
  skipped: string[];
  /** Whether the export read back correctly with the independent parser. */
  verified: boolean;
}

/** What one destination got out of a sync: its report, or why it got none. */
export interface SyncDeviceReport {
  ejected?: boolean;
  ejectError?: string;
  /** The mount point it was written to. */
  path: string;
  report?: ExportReport;
  error?: string;
}

export interface MissingExportFile {
  title: string;
  path: string;
}

/** One playlist a stick was last synced with. */
export interface SyncPlaylist {
  /** The tree's node id for the playlist, so it can be ticked again. */
  libraryId: string;
  name: string;
}

/** What a stick was last synced with, and what it holds now. */
export interface DeviceSyncState {
  /** Our last export's playlists; empty when the stick is not ours. */
  selected: SyncPlaylist[];
  /** The playlist names in its export, folders left out; empty without one. */
  onDevice: string[];
  libraries?: { name: string; nodes: { id: string; parentId: string; name: string; folder: boolean }[] }[];
  /**
   * The stick asks to be synced again when it is plugged in — its sync
   * record's Automatic synchronization, and the record is this library's.
   */
  automatic: boolean;
}

/** One step of a sync: a stick being written, then done or failed. */
export interface SyncProgress {
  path: string;
  state: "writing" | "ejecting" | "done" | "failed";
}

/**
 * One phrase of a track's structure, from the `PSSI` analysis tag.
 *
 * `label` is what rekordbox prints in the phrase bar — "INTRO 2", "CHORUS 1",
 * "UP 1" — and `kind` is the raw numeric value it came from, kept so a label
 * we do not recognise can still be traced back.
 */
export interface Phrase {
  /** Beat the phrase starts on, 1-based. */
  beat: number;
  /**
   * Milliseconds from the start, resolved against the beat grid.
   *
   * Absent when the grid does not reach the phrase, which happens on a track
   * whose analysis is older than its length. The strip falls back to the beat.
   */
  timeMs: number | null;
  kind: number;
  label: string;
}

/** One beat of the grid. */
export interface Beat {
  timeMs: number;
  /** The first beat of a bar, drawn heavier than the rest. */
  downbeat: boolean;
}

/** A device heard on the network before LINK is on. */
export interface LinkPeerSeen {
  number: number;
  name: string;
  /** `player`, `mixer`, `rekordbox` or `device`. */
  kind: string;
  address: string;
}

/** A network interface LINK can run on. */
export interface LinkInterface {
  /** The OS name: `en0`, `Ethernet 2`. */
  name: string;
  address: string;
  adapter?: string | null;
  connection?: "wired" | "wireless" | null;
}

/** A player on the link, and what it has loaded from us. */
export interface LinkPlayer {
  number: number;
  name: string;
  /** `player`, `mixer`, `rekordbox` or `device`. */
  kind: string;
  address: string;
  loaded: { id: string; title: string; artist: string } | null;
  playing: boolean;
  master: boolean;
  /** The player has SYNC on. */
  sync: boolean;
  /** The player is sitting at its cue point (play state Cued or Cuing). */
  cued: boolean;
  /** The mixer's Link Cue button. Always false; the backend cannot reach it yet. */
  linkCue: boolean;
  /** The player has mounted the library, so a track can be sent to it. */
  mounted: boolean;
}

export interface LinkStatus {
  on: boolean;
  /** Why it could not be turned on, when it could not. */
  problem: string | null;
  /** What it runs on, while on. */
  interface: LinkInterface | null;
  players: LinkPlayer[];
  /** What it could run on, for the picker. */
  interfaces: LinkInterface[];
  /** We are the network's tempo master, driving the tempo the players sync to. */
  master: boolean;
  /** The master tempo we would drive, in BPM; shown whether or not we are master. */
  masterBpm: number;
  /**
   * Where the join is while on: `waiting` (nothing announced until a player
   * or mixer is heard, as rekordbox does), `joining` (probing for a device
   * number), `up`, or `down` (with `problem`). `off` while off.
   */
  state: "off" | "waiting" | "joining" | "up" | "down";
  /** The device number the join settled on — 17, or 18 when another rekordbox holds 17. */
  number: number | null;
}

/** One backup of the library. */
export interface BackupSizes {
  updatedAt: number;
  trackCount: number;
  artwork: number;
  vocals: number;
  database: number;
  waveforms: number;
  cues: number;
  beatGrids: number;
  phrases: number;
  other: number;
}

export interface BackupProgress {
  running: boolean;
  /** Periodically sampled section / relative file path; cleared when the job finishes. */
  currentItem?: string | null;
  phase: "" | "preparing" | "copying" | "compressing" | "validating" | "complete" | "failed" | "stopping" | "cancelled";
  copiedBytes: number;
  totalBytes: number;
  error: string | null;
  path: string | null;
}

export interface Backup {
  createdAt: number;
  includesAnalysis: boolean;
  includesArtwork?: boolean;
  path: string;
  /** The file's name, which carries when it was taken. */
  name: string;
  bytes: number;
}

/** Per-device progress; done counts tracks processed before the current one. */
export interface ExportProgress {
  path: string;
  state: "preparing" | "checking" | "copying" | "database" | "verifying" | "publishing" | "ejecting" | "done" | "failed" | "cancelled";
  done: number;
  total: number;
  title: string;
}

/** Results selected for replacement, and timing options for this batch. */
export interface AnalysisSettings {
  bpmGrid: boolean;
  key: boolean;
  highPrecision: boolean;
  minBpm: number;
  maxBpm: number;
  /** Add a memory cue on the new grid's first beat unless one is there. */
  firstBeatCue: boolean;
}

/** What analysing one track found, now written to the library. */
export interface AnalysisResult {
  trackId: string;
  /** Key-only analysis preserves the existing analysed marker. */
  analysed?: number;
  bpmX100: number;
  key: string;
  beats: number;
  /** Peak sample magnitude, 0 to 1. */
  peak: number;
  durationSec: number;
  /** How long the analysis took, for the progress readout. */
  elapsedMs: number;
  /** Where the analysis files went, share-relative. */
  analysisPath: string;
}

/** What an import batch did. */
export interface ImportReport {
  imported: number;
  /** One line per file that was not imported, saying why. */
  skipped: string[];
  /** The tracks that landed, so they can be queued for analysis. */
  tracks: { id: string; title: string }[];
  /** Files that were already in the library, with their existing track ids. */
  existing: { id: string; title: string }[];
}

/** What importing a rekordbox XML collection did. */
/**
 * An iTunes / Music library read for the Sync Manager's iTunes column: where
 * its XML is, and its playlist tree to tick from. Nothing is imported yet.
 */
export interface ItunesLibrary {
  path: string;
  /** Folders and playlists only, ids `itunes:<index>`, top level at depth 1. */
  tree: TreeNode[];
}

export interface XmlImportReport {
  imported: number;
  /** Tracks whose file was already in the library, reused as they are. */
  existing: number;
  skipped: string[];
  playlists: number;
  cues: number;
  tracks: { id: string; title: string }[];
}

/** What an automatic relocate did. */
export interface RelocateReport {
  relocated: number;
  /** Missing tracks whose file name was found in none of the folders. */
  unresolved: number;
}

/** A track whose audio file is no longer where the library says. */
export interface MissingTrack {
  id: string;
  title: string;
  artist: string;
  /** Where the library still expects it. */
  path: string;
}

export interface DuplicateTrack {
  id: string;
  path: string;
  durationSec: number;
  /** Whether the file is where the library says. */
  present: boolean;
}

export interface DuplicateGroup {
  title: string;
  artist: string;
  tracks: DuplicateTrack[];
}

export interface Duplicates {
  /** Every group, not just the ones listed. */
  groups: number;
  /** Copies beyond the first, over every group. */
  extra: number;
  shown: DuplicateGroup[];
}

export interface MissingTracks {
  /** Every missing track, not just the ones listed. */
  total: number;
  tracks: MissingTrack[];
}

/**
 * An intelligent playlist's rule, flat as rekordbox's editor has it: one
 * group of conditions, all of them or any of them, in rekordbox's own
 * vocabulary (see `SmartCondition`).
 */
export interface SmartRule {
  logic: "all" | "any";
  conditions: SmartCondition[];
}

/** One line of a rule: rekordbox's internal property name (`artist`, `name` for the title, `stockDate` for the date added…) and operator number (1 is … 11 ends with). */
export interface SmartCondition {
  property: string;
  operator: string;
  left: string;
  right: string;
  /** `day`, `week`, `month` or `year` for "is in the last"; empty otherwise. */
  unit: string;
}

/** Shared library history after an edit, undo, or redo. */
export interface EditHistoryState {
  generation: number;
  canUndo: boolean;
  canRedo: boolean;
  undoLabel: string | null;
  redoLabel: string | null;
}

export interface Edits {
  createPlaylist(name: string, parent: string): Promise<number>;
  /** Create New Intelligent Playlist: a rule under `parent`. */
  createSmartPlaylist(name: string, parent: string, rule: SmartRule): Promise<number>;
  /** Replaces an intelligent playlist's rule. */
  setSmartRule(playlist: string, rule: SmartRule): Promise<number>;
  createFolder(name: string, parent: string): Promise<number>;
  renamePlaylist(id: string, name: string): Promise<EditHistoryState>;
  /**
   * Moves a playlist or folder under `parent`.
   *
   * `index` is the place to take among that parent's children, counted once
   * the node has been lifted out of wherever it was. Omitted, it is appended.
   */
  movePlaylist(id: string, parent: string, index?: number): Promise<EditHistoryState>;
  deletePlaylist(id: string): Promise<EditHistoryState>;
  undoEdit(): Promise<EditHistoryState>;
  redoEdit(): Promise<EditHistoryState>;
  addTracksToPlaylist(playlist: string, tracks: string[]): Promise<number>;
  /** Reload Tag: the files' tags read again over the rows. */
  reloadTags(tracks: string[]): Promise<number>;
  /** The Tag List: tracks go on the end, come off, or it is emptied. */
  addToTagList(tracks: string[]): Promise<number>;
  removeFromTagList(tracks: string[]): Promise<number>;
  clearTagList(): Promise<number>;
  removeTracksFromPlaylist(playlist: string, tracks: string[]): Promise<EditHistoryState>;
  /** Reset DJ Play Count: back to zero on each track. */
  resetPlayCount(tracks: string[]): Promise<EditHistoryState>;
  /** A play: the track goes on today's history session and its count goes up. */
  recordPlay(track: string): Promise<number>;
  /** Remove from History: the tracks' plays leave the session. */
  removeFromHistory(history: string, tracks: string[]): Promise<number>;
  /** Remove from Collection: the tracks leave the library and every playlist. The files stay. */
  removeFromCollection(tracks: string[]): Promise<number>;
  reorderPlaylist(playlist: string, tracks: string[]): Promise<number>;
  setTrackRating(track: string, stars: number): Promise<EditHistoryState>;
  setTrackComment(track: string, comment: string): Promise<EditHistoryState>;
  setTrackColor(track: string, color: string | null): Promise<EditHistoryState>;
  /**
   * One of the Info tab's editable fields, by wire name. The backend keeps
   * the list of what may be written; a name it does not know is refused as
   * `readOnly` rather than mapped onto a guess.
   */
  setTrackField(track: string, field: TrackField, value: string): Promise<EditHistoryState>;
  /** Sets the My Tags on a track to exactly these ids. */
  setMyTags(track: string, tags: string[]): Promise<EditHistoryState>;
  /** Add Artwork: the image is filed in the share tree and the track points at it. */
  addArtwork(track: string, image: string): Promise<EditHistoryState>;
  /** Add Artwork on a playlist or folder, from the tree menu. */
  addPlaylistArtwork(playlist: string, image: string): Promise<number>;
  /** Delete Artwork: the track points at no image; the file stays. */
  clearArtwork(track: string): Promise<EditHistoryState>;

  /**
   * Cues. Unlike the edits above these do not return a generation: a cue
   * edit changes one track's cues and nothing else, so the backend re-reads
   * only that track and says so through `onCuesChanged` rather than
   * reloading the library and dropping every cached page.
   */
  /** Adds a cue and resolves to its id. */
  addCue(track: string, kind: CueKind, positionMs: number): Promise<string>;
  /**
   * Adds a loop: a cue with an out point. `beats` is the loop's length in
   * beats when known; left out, In and Out are all that is recorded, which
   * is what most of the library's loops do.
   */
  addLoop(track: string, kind: CueKind, inMs: number, outMs: number, beats?: number): Promise<string>;
  moveCue(cue: string, positionMs: number): Promise<void>;
  /** Changes a cue's palette entry; null resets it to its default. */
  setCueColour(cue: string, colour: number | null): Promise<void>;
  deleteCue(cue: string): Promise<void>;
  /**
   * Convert Memory Cues to Hot Cues: each memory cue, by position, into the
   * next free slot from A. Resolves to how many were made.
   */
  convertMemoryCuesToHot(track: string): Promise<number>;

  /**
   * The beat grid. Like the cues, these change one track's analysis and say
   * so through `onGridChanged`; an edit that changes the tempo also writes
   * the row's BPM and reloads the library.
   *
   * `fromMs` applies the edit from the beat nearest that time on — the scope
   * point, or the playhead for the from-here buttons — and `deck` names the
   * deck the track is loaded on, so its metronome follows the new grid.
   * Every one resolves to the grid's state afterwards.
   */
  gridEdit(track: string, edit: GridEdit, options?: GridEditOptions): Promise<GridState>;
  gridUndo(track: string, deck?: DeckId): Promise<GridState>;
  gridRedo(track: string, deck?: DeckId): Promise<GridState>;
  /** Locks or unlocks the grid against editing. */
  gridLock(track: string, on: boolean): Promise<GridState>;
}

/** One change to a beat grid, as `rbl_anlz::grid::Edit` spells them. */
export type GridEdit =
  /** Shift every beat by `ms`; positive is later. */
  | { kind: "nudge"; ms: number }
  /** Twice the tempo: a beat between every pair. */
  | { kind: "double" }
  /** Half the tempo: every other beat dropped. */
  | { kind: "halve" }
  /** Make the beat nearest `timeMs` the downbeat. */
  | { kind: "downbeat"; timeMs: number }
  /** Re-space the grid at `bpmX100`, a beat held at `anchorMs`. */
  | { kind: "tempo"; bpmX100: number; anchorMs: number }
  /** Move the target beat by milliseconds, holding the first editable beat. */
  | { kind: "stretch"; byMs: number; timeMs: number }
  | { kind: "tap"; bpm: number; anchorMs: number }
  /** Move the grid so the beat nearest `timeMs` lands on it. */
  | { kind: "align"; timeMs: number };

export interface GridEditOptions {
  durationMs?: number;
  /** Apply from the beat nearest this time on; omitted, the whole grid. */
  fromMs?: number;
  /** Explicit confirmation before flattening a variable-tempo section. */
  allowDynamic?: boolean;
  /** Consecutive TAP updates in this run share one undo transaction. */
  transaction?: string;
  /** The deck the track is loaded on, whose metronome follows. */
  deck?: DeckId;
}

/** What the GRID panel shows and enables. */
export interface GridState {
  /** The grid's tempo x100, from its first beat. */
  bpmX100: number;
  beats: number;
  canUndo: boolean;
  canRedo: boolean;
  /** The next action in each history stack, when supplied by the backend. */
  undoLabel?: string | null;
  redoLabel?: string | null;
  locked: boolean;
}

/** One of the folders the Explorer starts from. */
export interface ExplorerRoot {
  /** What to show: `Music`, the user's name, `Macintosh HD`, a stick's name. */
  name: string;
  path: string;
}

/** The folders directly under one folder, by name. */
export interface ExplorerChildren {
  /** The first of them by name, up to the backend's cap. */
  names: string[];
  /** How many there were: more than `names` holds when the cap cut it. */
  total: number;
}

/** `ParentID` of a playlist or folder at the top of the tree. */
export const TREE_ROOT = "root";

/**
 * One track, in full — the information panel's record.
 *
 * Numbers the library leaves NULL arrive as 0 and text as empty, so nothing
 * here is optional; rekordbox's own Info tab prints 0 in an empty Year box.
 */
export interface TrackDetails {
  id: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  originalArtist: string;
  composer: string;
  remixer: string;
  lyricist: string;
  genre: string;
  label: string;
  key: string;
  comment: string;
  mixName: string;
  message: string;
  /** `"0"` or empty for none, `"1"` to `"8"` for rekordbox's eight colours. */
  color: string;
  rating: number;
  bpmX100: number;
  durationSec: number;
  year: number;
  trackNumber: number;
  discNumber: number;
  playCount: number;
  /** rekordbox's own code: 1 MP3, 4 M4A, 5 FLAC, 11 WAV, 12 AIFF. */
  fileType: number;
  fileSize: number;
  /** kbps. */
  bitrate: number;
  /** Hz. */
  sampleRate: number;
  bitDepth: number;
  /** `YYYY-MM-DD`. */
  dateCreated: string;
  releaseDate: string;
  /** The audio file's absolute path. */
  path: string;
  hotCueAutoLoad: boolean;
  publish: boolean;
  hasArtwork: boolean;
  /** The ids of the My Tags on the track. */
  myTags: string[];
}

export interface TrackLookups {
  keys: string[];
  genres: string[];
  /** The library's My Tags by category, with the ids the toggles set. */
  myTagCategories: { name: string; tags: { id: string; name: string }[] }[];
}

/** The fields the backend will write. Everything else on the Info tab is shown read-only. */
export type TrackField =
  | "title" | "artist" | "album" | "year" | "trackNumber" | "discNumber"
  | "originalArtist" | "composer" | "remixer" | "lyricist" | "playCount"
  | "genre" | "label" | "key" | "bpm";

/**
 * The BPM column of the track filter bar.
 *
 * `values` are whole BPMs picked from the list, empty for `All`. With values
 * picked, `tolerancePct` widens each into a band; with none, it is a band
 * around the master player's BPM — and with no master player it is inert.
 */
export interface BpmFilter {
  values: number[];
  /** 0 to 6, the `MASTER PLAYER ± n%` list. */
  tolerancePct: number;
  /** The master player's BPM x100, or `null` when no deck is loaded. */
  masterBpmX100: number | null;
}

/**
 * The track filter bar's picks: one entry per ticked column, combined with
 * AND. Keys and colours travel by name, which is what the bar shows.
 */
export interface TrackFilter {
  bpm?: BpmFilter;
  keys?: string[];
  ratings?: number[];
  colors?: string[];
}

/** A value the bar can offer, with how many tracks of the list carry it. */
export interface Counted<T> {
  value: T;
  count: number;
}

/** A My Tag category and the tags under it, by name. Drawn, not filtered on. */
export interface TagCategory {
  name: string;
  tags: string[];
}

export interface FilterValues {
  /** Whole BPMs present, ascending. */
  bpms: Counted<number>[];
  /** Keys present, in Camelot order. */
  keys: Counted<string>[];
  tags: TagCategory[];
}

/** One browse category or sort option on a stick: a row of `category` or `sort`. */
export interface MenuSlot {
  /** The row's own key, stable across exports. */
  id: number;
  /** Which menu item it is; `MENU_ITEM.*` in `src/lib/deviceSettings.ts`. */
  menuItem: number;
  /** As the player shows it: `ARTIST`, `DATE ADDED`. */
  name: string;
  /** Position among the visible rows, 1-based; 0 when hidden. */
  seq: number;
  visible: boolean;
}

export interface ColorName {
  /** 1 to 8, in rekordbox's order Pink to Purple. */
  id: number;
  name: string;
}

export type WaveformColor = "blue" | "rgb" | "3band";
export type WaveformPosition = "center" | "left";
export type OverviewWaveform = "half" | "full";
export type KeyDisplay = "classic" | "alphanumeric";

/** The reference rows a fresh stick's `exportLibrary.db` starts from. */
export interface ReferenceStickSettings {
  categories: MenuSlot[];
  sorts: MenuSlot[];
}

/**
 * What a stick gets on its first export: the Preferences window's DJ System
 * pane. `categories` and `sorts` replace the reference rows; `subColumn`
 * is the sort option shown beside the track name, or null for none.
 */
export interface StickDefaults {
  waveformColor: WaveformColor;
  waveformPosition: WaveformPosition;
  overviewWaveform: OverviewWaveform;
  keyDisplay: KeyDisplay;
  categories: MenuSlot[] | null;
  sorts: MenuSlot[] | null;
  subColumn: number | null;
}

/** Everything the device tabs read and write. A few kilobytes. */
export interface DeviceSettings {
  /** `export.pdb` is on the stick — "Device Library" in rekordbox's words. */
  hasDeviceLibrary: boolean;
  /** `exportLibrary.db` is on the stick — "OneLibrary". */
  hasOneLibrary: boolean;
  /** `DEVSETTING.DAT` was read; when false the four below are defaults. */
  hasDevSetting: boolean;
  waveformColor: WaveformColor;
  waveformPosition: WaveformPosition;
  overviewWaveform: OverviewWaveform;
  keyDisplay: KeyDisplay;
  /** The library rows were read; when false they are the reference rows and are not written. */
  hasLibrarySettings: boolean;
  deviceName: string;
  /** `property.backGroundColorType`, carried but not understood. */
  backgroundColorType: number;
  categories: MenuSlot[];
  sorts: MenuSlot[];
  /** `menuItem` of the sort option shown beside the track name, or null for Not Specified. */
  subColumn: number | null;
  colors: ColorName[];
}
