//! Application state: the loaded library and the open views.
//!
//! The library is loaded once and shared immutably. Views are `Vec<Row>` held
//! here rather than sent to the frontend, which is what keeps IPC payloads to a
//! window of rows regardless of collection size.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use rbl_index::folder::FolderView;
use rbl_index::{
    BpmFilter, Library, RelatedCriterion, SortColumn, TrackFilter, TrackSource, View, ViewSpec, COLOR_NAMES,
};

use crate::dto::{cue_colour_css, RowCueDto, RowDto, TrackFilterDto, TrackSourceDto, ViewSpecDto};
use crate::error::{AppError, AppResult, ErrorKind};

/// Views are dropped oldest-first past this many, so a user clicking through
/// playlists cannot grow memory without bound.
const MAX_VIEWS: usize = 16;

pub struct AppState {
    pub(crate) edit_gate: parking_lot::ReentrantMutex<()>,
    pub(crate) edit_history: parking_lot::Mutex<EditHistory>,
    pub(crate) analysis_write: parking_lot::Mutex<()>,
    pub(crate) backup_progress: parking_lot::Mutex<crate::backups::BackupProgress>,
    pub(crate) backup_sizes: parking_lot::Mutex<crate::backup_sizes::SizeCache>,
    inner: RwLock<Inner>,
    /// Stable local home for recovery journals and the destination setting.
    backup_dir: std::path::PathBuf,
    backup_destination: RwLock<std::path::PathBuf>,
    /// A read-only handle to the database for point reads, opened on first
    /// use. Opening costs 50 ms on the reference library — the `SQLCipher`
    /// key derivation — and a point read under 1 ms, so the handle is kept
    /// rather than reopened per selection. A `Mutex`, not `RwLock`, because a
    /// `rusqlite::Connection` is `Send` and not `Sync`.
    reader: parking_lot::Mutex<Option<rbl_db::Library>>,
    /// The Missing File Manager's last scan, paged out to it a screenful at a
    /// time. See [`crate::relocate::MissingScan`].
    pub(crate) missing_scan: parking_lot::Mutex<Option<crate::relocate::MissingScan>>,
}

/// One reversible library operation. Grid edits keep their own history because
/// they also rewrite analysis files; text fields keep `WebKit`'s native history.
#[derive(Clone)]
pub(crate) enum LibraryEdit {
    DeletePlaylist(rbl_db::write::PlaylistDeletion),
    RenamePlaylist(rbl_db::write::PlaylistRename),
    MovePlaylist(rbl_db::write::PlaylistMove),
    RemovePlaylistTracks(rbl_db::write::PlaylistTrackRemoval),
    Track(Vec<rbl_db::write::TrackEdit>),
    TrackTags(rbl_db::write::TrackTagEdit),
}

impl LibraryEdit {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Self::DeletePlaylist(edit) => edit.is_empty(),
            Self::RemovePlaylistTracks(edit) => edit.is_empty(),
            Self::Track(edits) => edits.is_empty() || edits.iter().all(rbl_db::write::TrackEdit::is_empty),
            Self::TrackTags(edit) => edit.is_empty(),
            Self::RenamePlaylist(_) | Self::MovePlaylist(_) => false,
        }
    }
}

#[derive(Clone)]
pub(crate) struct HistoryEntry {
    pub(crate) edit: LibraryEdit,
    pub(crate) label: &'static str,
}

/// Bounded session history for reversible library edits. The payloads retain
/// exact database values or row ids, rather than reconstructing state from the
/// interface when an action is undone.
#[derive(Default)]
pub(crate) struct EditHistory {
    pub(crate) undo: Vec<HistoryEntry>,
    pub(crate) redo: Vec<HistoryEntry>,
}

impl EditHistory {
    const LIMIT: usize = 50;

    pub(crate) fn record(&mut self, edit: LibraryEdit, label: &'static str) {
        self.undo.push(HistoryEntry { edit, label });
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(crate) fn clear_redo(&mut self) {
        self.redo.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/// A library view, and whether it lists the Tag List: the one source a Tag
/// List edit makes stale.
struct OpenView {
    view: Arc<View>,
    tag_list: bool,
}

#[derive(Default)]
struct Inner {
    library: Option<Arc<Library>>,
    /// Where the library is, decided once when it is loaded. Every later open
    /// — the reader, the writer, a reload — goes through this rather than
    /// detecting again, which is also what lets a test point the whole shell
    /// at a fixture.
    location: Option<rbl_db::LibraryLocation>,
    read_only: bool,
    db_version: Option<i64>,
    load_ms: u64,
    views: HashMap<u32, OpenView>,
    /// The Explorer's views, under the same ids and the same eviction.
    folders: HashMap<u32, Arc<FolderView>>,
    /// The Devices tree's views of a stick's own library, likewise.
    devices: HashMap<u32, Arc<rbl_index::device::DeviceView>>,
    /// Insertion order, for eviction.
    view_order: Vec<u32>,
    next_view_id: u32,
    /// Bumped when the library is reloaded; invalidates cached pages.
    generation: u32,
    /// The LINK session, while one is running.
    link: Option<crate::link::Session>,
    /// The passive network watcher, running from startup: who is on the
    /// network, so the shell can offer LINK when a player appears.
    watcher: Option<rbl_link::Watcher>,
    /// Why the last load failed, until one succeeds. Kept rather than only
    /// sent as an event: with no library at all the load fails in
    /// milliseconds, before the window has subscribed to anything.
    library_problem: Option<crate::dto::LibraryProblemDto>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// State for the installed library, with backups under the app's own
    /// data directory.
    pub fn new() -> Self {
        Self::with_backups(default_backup_dir())
    }

    /// State that stores manual backups under `backup_dir`. The location is
    /// not chosen here: it arrives with the library, in [`Self::set_library`].
    pub fn with_backups(backup_dir: impl Into<std::path::PathBuf>) -> Self {
        let backup_dir = backup_dir.into();
        let backup_destination = rbl_backup::default_destination(&backup_dir);
        Self {
            edit_gate: parking_lot::ReentrantMutex::new(()),
            edit_history: parking_lot::Mutex::new(EditHistory::default()),
            analysis_write: parking_lot::Mutex::new(()),
            backup_progress: parking_lot::Mutex::new(crate::backups::BackupProgress::default()),
            backup_sizes: parking_lot::Mutex::new(crate::backup_sizes::SizeCache::default()),
            inner: RwLock::new(Inner { next_view_id: 1, generation: 1, ..Inner::default() }),
            backup_dir,
            backup_destination: RwLock::new(backup_destination),
            reader: parking_lot::Mutex::new(None),
            missing_scan: parking_lot::Mutex::new(None),
        }
    }

    /// Stable local recovery directory, independent of the backup destination.
    pub fn backup_dir(&self) -> &std::path::Path {
        &self.backup_dir
    }

    pub fn backup_destination(&self) -> std::path::PathBuf {
        self.backup_destination.read().clone()
    }

    /// Persist the destination before publishing it to the running app.
    /// Recovery journals stay local even when snapshots go to a removable drive.
    pub fn set_backup_destination(&self, directory: &std::path::Path) -> AppResult<String> {
        let _gate = self.edit_gate.lock();
        let progress = self.backup_progress.lock();
        if progress.running {
            return Err(AppError::internal("Wait for the current backup to finish before changing its folder."));
        }
        let directory = directory.canonicalize().map_err(|e| AppError::internal(format!("The backup folder could not be opened: {e}")))?;
        if let Ok(location) = self.location() {
            crate::backups::validate_destination(&directory, &location)?;
        }
        let check = || -> std::io::Result<()> {
            let probe = tempfile::Builder::new().prefix(".rbxport-folder-check-").tempfile_in(&directory)?;
            probe.as_file().sync_all()?;
            probe.close()?;
            crate::durable::sync_dir(&directory)
        };
        check().map_err(|e| AppError::internal(format!("The backup folder is not writable: {e}")))?;
        crate::durable::create_dir_all(&self.backup_dir).map_err(|e| AppError::internal(e.to_string()))?;
        let bytes = serde_json::to_vec(&directory).map_err(|e| AppError::internal(e.to_string()))?;
        crate::durable::write(&self.backup_dir.join(rbl_backup::DESTINATION_FILE), &bytes)
            .map_err(|e| AppError::internal(format!("The backup folder setting could not be saved: {e}")))?;
        self.backup_destination.write().clone_from(&directory);
        Ok(directory.to_string_lossy().into_owned())
    }

    /// Lets go of the read-only handle, for when the file underneath it is
    /// about to be replaced. The next read opens a fresh one.
    pub fn drop_reader(&self) {
        *self.reader.lock() = None;
    }

    /// Prevent point reads from reopening the file while a restore swaps it.
    pub fn with_closed_reader<T>(&self, work: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        let mut reader = self.reader.lock();
        *reader = None;
        work()
    }

    /// Where the loaded library is.
    pub fn location(&self) -> AppResult<rbl_db::LibraryLocation> {
        self.inner
            .read()
            .location
            .clone()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "The library has not finished loading yet."))
    }

    /// A fresh read-only handle to the loaded library.
    ///
    /// Blocking — call from `spawn_blocking`, never from a command body.
    pub fn open_read_only(&self) -> Result<rbl_db::Library, rbl_db::DbError> {
        if crate::backups::pending(self.backup_dir()) {
            return Err(rbl_db::DbError::WriteRefused("A library restore is unfinished. Restart the app to recover it before reading.".into()));
        }
        let location = self
            .location()
            .map_err(|e| rbl_db::DbError::NotInstalled(e.message))?;
        rbl_db::Library::open(location, rbl_db::OpenMode::ReadOnly)
    }

    /// Opens the library for writing, runs one edit against it, and closes
    /// it. Refused while rekordbox is running.
    ///
    /// Opened per edit rather than held: holding it would keep the database
    /// open read-write for the life of the app, and rekordbox launching
    /// behind us must be able to take the file back. Database backups are
    /// made only by an explicit request, never automatically on edits.
    ///
    /// Blocking — call from `spawn_blocking`, never from a command body.
    pub fn write<T>(
        &self,
        edit: impl FnOnce(&mut rbl_db::write::Writer) -> Result<T, rbl_db::DbError>,
    ) -> Result<T, rbl_db::DbError> {
        self.write_then(edit, |_, result| Ok(result))
    }

    /// Keep the encrypted connection and edit gate through the refresh, so
    /// a small edit neither pays for another key derivation nor races a writer.
    pub fn write_then<T, U>(
        &self,
        edit: impl FnOnce(&mut rbl_db::write::Writer) -> Result<T, rbl_db::DbError>,
        refresh: impl FnOnce(&rbl_db::Library, T) -> Result<U, rbl_db::DbError>,
    ) -> Result<U, rbl_db::DbError> {
        let started = std::time::Instant::now();
        let location = self
            .location()
            .map_err(|e| rbl_db::DbError::NotInstalled(e.message))?;
        let _gate = self.edit_gate.lock();
        if crate::backups::pending(self.backup_dir()) {
            return Err(rbl_db::DbError::WriteRefused("A library restore is unfinished. Restart the app to recover it before editing.".into()));
        }
        // Grid/analysis edits hold this mutex while intentionally publishing
        // their own journal. Otherwise recover before another edit can change
        // the USN used to decide an interrupted operation's outcome.
        if let Some(_files) = self.analysis_write.try_lock() {
            crate::file_journal::recover(self.backup_dir(), &location)
                .map_err(|e| rbl_db::DbError::WriteRefused(e.to_string()))?;
        }
        let gate_ms = started.elapsed().as_millis();
        let mut writer = rbl_db::write::Writer::open(location.clone(), self.backup_dir.clone())?;
        writer.disable_automatic_backups();
        let open_ms = started.elapsed().as_millis();
        let result = edit(&mut writer);
        let edit_ms = started.elapsed().as_millis();
        let result = result.and_then(|value| refresh(writer.library(), value));
        tracing::debug!(gate_ms, open_ms = open_ms - gate_ms,
            edit_ms = edit_ms - open_ms, refresh_ms = started.elapsed().as_millis() - edit_ms,
            "library write phases");
        result
    }

    /// Runs one read against the database, opening the handle if needed.
    ///
    /// A read that fails drops the handle, so the next call opens a fresh one:
    /// the file can be replaced underneath us by a library restore, and a
    /// handle to the old inode would answer with stale rows forever.
    ///
    /// Blocking — call from `spawn_blocking`, never from a command body.
    pub fn read_db<T>(
        &self,
        f: impl FnOnce(&rbl_db::Library) -> Result<T, rbl_db::DbError>,
    ) -> Result<T, rbl_db::DbError> {
        let mut slot = self.reader.lock();
        if slot.is_none() {
            *slot = Some(self.open_read_only()?);
        }
        let Some(db) = slot.as_ref() else {
            return Err(rbl_db::DbError::Open("no reader".to_owned()));
        };
        let outcome = f(db);
        if outcome.is_err() {
            *slot = None;
        }
        outcome
    }

    pub fn set_library(
        &self,
        library: Library,
        read_only: bool,
        db_version: Option<i64>,
        load_ms: u64,
        location: rbl_db::LibraryLocation,
    ) {
        // The reader is a handle to wherever the previous library was. Dropped
        // before `inner` is taken: `read_db` holds the reader while it opens,
        // and opening reads `inner`, so taking them the other way round here
        // would be a deadlock waiting for a reload during a point read.
        *self.reader.lock() = None;
        let mut inner = self.inner.write();
        inner.library = Some(Arc::new(library));
        inner.library_problem = None;
        inner.location = Some(location);
        inner.read_only = read_only;
        inner.db_version = db_version;
        inner.load_ms = load_ms;
        inner.views.clear();
        inner.folders.clear();
        inner.view_order.clear();
        inner.generation = inner.generation.wrapping_add(1).max(1);
        // A reload follows every write, including a new analysis: the
        // players must not be served what was parsed before it.
        if let Some(session) = inner.link.as_ref() {
            session.analysis_changed();
        }
    }

    /// Why the library is not loaded, when a load has failed.
    pub fn library_problem(&self) -> Option<crate::dto::LibraryProblemDto> {
        self.inner.read().library_problem.clone()
    }

    pub(crate) fn set_library_problem(&self, problem: Option<crate::dto::LibraryProblemDto>) {
        self.inner.write().library_problem = problem;
    }

    /// Drops every open view and bumps the generation.
    ///
    /// Used after a playlist edit: the track columns are untouched, but a view
    /// over a playlist whose membership changed is stale, and so are the pages
    /// the frontend has cached against the old generation.
    pub fn invalidate_views(&self) -> u32 {
        let mut inner = self.inner.write();
        inner.views.clear();
        inner.folders.clear();
        inner.view_order.clear();
        inner.generation = inner.generation.wrapping_add(1).max(1);
        inner.generation
    }

    /// Drops the views over the Tag List and keeps every other. No row or
    /// column of any other view shows Tag List membership, so they stay open
    /// under the same generation and the frontend keeps the pages it has: a
    /// player tagging a track does not reload the playlist on screen.
    pub fn invalidate_tag_list_views(&self) -> u32 {
        let mut inner = self.inner.write();
        let Inner { views, folders, view_order, generation, .. } = &mut *inner;
        views.retain(|_, open| !open.tag_list);
        view_order.retain(|id| views.contains_key(id) || folders.contains_key(id));
        *generation
    }

    /// Publish metadata without changing row identities or analysis files.
    /// Call under the edit gate, using the connection that committed the edit.
    pub fn refresh_metadata(&self, db: &rbl_db::Library, ids: &[String], histories: bool) -> Result<u32, rbl_db::DbError> {
        let mut inner = self.inner.write();
        let current = inner.library.as_ref().ok_or_else(|| rbl_db::DbError::Open("No library loaded".into()))?;
        // Build privately so a failed read never publishes a partial update.
        let mut next = (**current).clone();
        rbl_index::reload_metadata(db, &mut next, ids)?;
        if histories { next.set_histories(rbl_index::reload_histories(db, &next)?); }
        inner.library = Some(Arc::new(next));
        inner.views.clear();
        inner.folders.clear();
        inner.view_order.clear();
        inner.generation = inner.generation.wrapping_add(1).max(1);
        Ok(inner.generation)
    }

    /// Starts or replaces the LINK session. Returns the previous one, if
    /// any, so the caller can drop it — which unbinds its ports and joins
    /// its threads — outside the lock.
    pub fn set_link(&self, session: Option<crate::link::Session>) -> Option<crate::link::Session> {
        std::mem::replace(&mut self.inner.write().link, session)
    }

    pub fn link_running(&self) -> bool {
        self.inner.read().link.is_some()
    }

    /// Installs the network watcher, replacing any previous one.
    pub fn set_watcher(&self, watcher: Option<rbl_link::Watcher>) -> Option<rbl_link::Watcher> {
        std::mem::replace(&mut self.inner.write().watcher, watcher)
    }

    /// Whether the watcher is running.
    pub fn watching(&self) -> bool {
        self.inner.read().watcher.is_some()
    }

    /// The peers the watcher has heard.
    pub fn link_peers(&self) -> Vec<rbl_link::Player> {
        self.inner.read().watcher.as_ref().map(rbl_link::Watcher::peers).unwrap_or_default()
    }

    /// The LINK session as the window shows it, or `None` when LINK is off.
    pub fn link_status(&self) -> Option<crate::link::LinkStatusDto> {
        // One read lock at a time: a second, nested read would wait behind a
        // writer queued between them and never get it.
        let library = self.library().ok();
        self.inner.read().link.as_ref().map(|session| session.status(library.as_deref()))
    }

    /// Tells a CDJ on the link to load a track from our library.
    pub fn link_load_track(&self, player_number: u8, track_id: u32) -> Result<(), String> {
        match self.inner.read().link.as_ref() {
            Some(session) => session.load_track(player_number, track_id),
            None => Err("LINK is off, so no player can be told to load a track.".to_owned()),
        }
    }

    /// Becomes or resigns the network's tempo master.
    pub fn link_set_master(&self, on: bool) {
        if let Some(session) = self.inner.read().link.as_ref() {
            session.set_master(on);
        }
    }

    /// Nudges the master tempo by `delta_bpm`.
    pub fn link_nudge_master(&self, delta_bpm: f64) {
        if let Some(session) = self.inner.read().link.as_ref() {
            session.nudge_master(delta_bpm);
        }
    }

    /// Takes the current master player's tempo; `false` when none is master
    /// or LINK is off.
    pub fn link_take_master_tempo(&self) -> bool {
        self.inner.read().link.as_ref().is_some_and(crate::link::Session::take_master_tempo)
    }


    pub fn library(&self) -> AppResult<Arc<Library>> {
        self.inner
            .read()
            .library
            .clone()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "The library has not finished loading yet."))
    }

    /// Where analysis files live for the loaded library, or nowhere useful
    /// before it has loaded.
    pub fn share_root(&self) -> std::path::PathBuf {
        self.inner.read().location.as_ref().map(|l| l.share_root.clone()).unwrap_or_default()
    }

    pub fn summary(&self) -> (bool, Option<i64>, u64, u32) {
        let inner = self.inner.read();
        (inner.read_only, inner.db_version, inner.load_ms, inner.generation)
    }

    /// Opens a view and returns `(view_id, len, generation)`.
    pub fn open_view(&self, spec: &ViewSpec) -> AppResult<(u32, u32, u32)> {
        self.open_view_scoped(spec, rbl_index::SearchField::All)
    }

    pub fn open_view_scoped(&self, spec: &ViewSpec, field: rbl_index::SearchField) -> AppResult<(u32, u32, u32)> {
        let library = self.library()?;
        let view = library.open_view_scoped(spec, field);
        let len = u32::try_from(view.len()).unwrap_or(u32::MAX);

        let mut inner = self.inner.write();
        let tag_list = matches!(spec.source, TrackSource::TagList);
        let id = inner.register(Registered::Library(OpenView { view: Arc::new(view), tag_list }));
        let generation = inner.generation;
        Ok((id, len, generation))
    }

    /// Opens the Explorer's view of one folder; the same handle shape.
    pub fn open_folder_view(&self, view: FolderView) -> (u32, u32, u32) {
        let len = u32::try_from(view.len()).unwrap_or(u32::MAX);
        let mut inner = self.inner.write();
        let id = inner.register(Registered::Folder(Arc::new(view)));
        (id, len, inner.generation)
    }

    /// Opens a view of a library on a stick; the same handle shape.
    pub fn open_device_view(&self, view: rbl_index::device::DeviceView) -> (u32, u32, u32) {
        let len = u32::try_from(view.len()).unwrap_or(u32::MAX);
        let mut inner = self.inner.write();
        let id = inner.register(Registered::Device(Arc::new(view)));
        (id, len, inner.generation)
    }

    /// The device view behind an id, or `None` for any other kind of id.
    pub fn device_view(&self, view_id: u32) -> Option<Arc<rbl_index::device::DeviceView>> {
        self.inner.read().devices.get(&view_id).cloned()
    }

    /// The folder view behind an id, or `None` when the id is a library view
    /// or nothing at all.
    pub fn folder_view(&self, view_id: u32) -> Option<Arc<FolderView>> {
        self.inner.read().folders.get(&view_id).cloned()
    }

    pub fn view(&self, view_id: u32) -> AppResult<Arc<View>> {
        self.inner.read().views.get(&view_id).map(|open| Arc::clone(&open.view)).ok_or_else(|| {
            AppError::new(ErrorKind::NotFound, "That list is no longer open. Reselect it to continue.")
                .with_detail(format!("view {view_id} was evicted or never existed"))
        })
    }
}

/// Where manual backups of the installed library go, and the recovery
/// state RBXport Restore shares.
fn default_backup_dir() -> std::path::PathBuf {
    rbl_backup::state_dir()
}

/// A view of either kind, on its way into the table.
enum Registered {
    Library(OpenView),
    Folder(Arc<FolderView>),
    Device(Arc<rbl_index::device::DeviceView>),
}

impl Inner {
    /// Hands out the next id and evicts the oldest view past the cap, whichever
    /// kind it is.
    fn register(&mut self, view: Registered) -> u32 {
        let id = self.next_view_id;
        self.next_view_id = self.next_view_id.wrapping_add(1).max(1);
        match view {
            Registered::Library(view) => {
                self.views.insert(id, view);
            }
            Registered::Folder(view) => {
                self.folders.insert(id, view);
            }
            Registered::Device(view) => {
                self.devices.insert(id, view);
            }
        }
        self.view_order.push(id);
        while self.view_order.len() > MAX_VIEWS {
            let oldest = self.view_order.remove(0);
            self.views.remove(&oldest);
            self.folders.remove(&oldest);
            self.devices.remove(&oldest);
        }
        id
    }
}

/// Translates a wire sort name. Unknown names fall back to track order rather
/// than failing the whole request.
pub fn sort_from_wire(name: &str) -> SortColumn {
    match name {
        "title" => SortColumn::Title,
        "artist" => SortColumn::Artist,
        "album" => SortColumn::Album,
        "genre" => SortColumn::Genre,
        "label" => SortColumn::Label,
        "comment" => SortColumn::Comment,
        "key" => SortColumn::Key,
        "keyCamelot" => SortColumn::KeyCamelot,
        "bpm" => SortColumn::Bpm,
        "duration" => SortColumn::Duration,
        "rating" => SortColumn::Rating,
        "djPlayCount" => SortColumn::PlayCount,
        "dateAdded" => SortColumn::DateAdded,
        "releaseDate" => SortColumn::ReleaseDate,
        "size" => SortColumn::Size,
        "year" => SortColumn::Year,
        "sampleRate" => SortColumn::SampleRate,
        "bitrate" => SortColumn::Bitrate,
        "color" => SortColumn::Color,
        "fileName" => SortColumn::FileName,
        "location" => SortColumn::Location,
        "composer" => SortColumn::Composer,
        "albumArtist" => SortColumn::AlbumArtist,
        "remixer" => SortColumn::Remixer,
        "originalArtist" => SortColumn::OriginalArtist,
        "mixName" => SortColumn::MixName,
        "discNo" => SortColumn::DiscNo,
        "trackNumber" => SortColumn::TrackNumber,
        "fileType" => SortColumn::FileType,
        "bitDepth" => SortColumn::BitDepth,
        "lyricist" => SortColumn::Lyricist,
        "dateCreated" => SortColumn::DateCreated,
        "publishTrackInfo" => SortColumn::PublishTrackInfo,
        "message" => SortColumn::Message,
        _ => SortColumn::TrackNo,
    }
}

pub fn spec_from_wire(library: &Library, dto: &ViewSpecDto) -> ViewSpec {
    // An id that names nothing falls back to the collection rather than
    // erroring: a tree node can outlive what it points at, and a window of the
    // whole library is a better answer to that than a red bar.
    let source = match &dto.source {
        // A folder never reaches the index: `open_view` opens one through
        // `explorer::open_folder` before translating. The collection is what
        // the sort and query here would apply to if it ever did.
        TrackSourceDto::Collection | TrackSourceDto::Folder { .. } | TrackSourceDto::Device { .. } => TrackSource::Collection,
        TrackSourceDto::History { id } => id
            .parse::<u64>()
            .ok()
            .and_then(|numeric| library.histories().index_of(numeric))
            .map_or(TrackSource::Collection, TrackSource::History),
        // An intelligent playlist arrives under the same wire kind as an
        // ordinary one — the tree node is the only thing that knows which it
        // is, and the index does too, so the shell decides here.
        TrackSourceDto::Playlist { id } => id
            .parse::<u64>()
            .ok()
            .and_then(|numeric| {
                let playlists = library.playlists();
                let index = playlists.index_of(numeric)?;
                Some(if playlists.is_smart(index) {
                    TrackSource::SmartPlaylist(index)
                } else {
                    TrackSource::Playlist(index)
                })
            })
            .unwrap_or(TrackSource::Collection),
        TrackSourceDto::PlaylistFolder { id } => id
            .parse::<u64>()
            .ok()
            .and_then(|numeric| {
                let playlists = library.playlists();
                let index = playlists.index_of(numeric)?;
                playlists.is_folder(index).then_some(TrackSource::PlaylistFolder(index))
            })
            .unwrap_or(TrackSource::Collection),
        TrackSourceDto::TagList => TrackSource::TagList,
        TrackSourceDto::Related { track, criterion } => TrackSource::Related {
            // No such track, or none: past the end, which relates to nothing.
            track: library.row_of(track).unwrap_or(u32::MAX),
            criterion: match criterion.as_str() {
                "genreRecent" => RelatedCriterion::SameGenreRecent,
                "artist" => RelatedCriterion::SameArtist,
                "suggestion" => RelatedCriterion::Suggestion,
                _ => RelatedCriterion::BpmAndKey,
            },
        },
    };
    ViewSpec {
        source,
        sort: sort_from_wire(&dto.sort),
        descending: dto.descending,
        query: dto.query.clone(),
        filter: filter_from_wire(library, &dto.filter),
    }
}

/// Translates the filter bar's picks, resolving names to ids.
///
/// A key name the library does not hold resolves to no id, so a ticked key
/// column with only unknown names matches nothing — which is what the list
/// would show for it. A colour name outside the eight is dropped the same way.
pub fn filter_from_wire(library: &Library, dto: &TrackFilterDto) -> TrackFilter {
    TrackFilter {
        bpm: dto.bpm.as_ref().map(|b| BpmFilter {
            values: b.values.clone(),
            tolerance_pct: b.tolerance_pct.min(6),
            master_bpm_x100: b.master_bpm_x100.filter(|&bpm| bpm > 0),
        }),
        keys: dto.keys.as_ref().map(|names| library.key_ids_named(names)),
        ratings: dto.ratings.clone(),
        colors: dto.colors.as_ref().map(|names| {
            names
                .iter()
                .filter_map(|name| {
                    COLOR_NAMES
                        .iter()
                        .position(|&c| c == name)
                        .and_then(|at| u8::try_from(at + 1).ok())
                })
                .collect()
        }),
    }
}

/// A row's hot cues in slot order, A to P, which is the order the badges are
/// painted in so a later slot covers an earlier one where two share a
/// position.
///
/// `[OBS]` from `player-1p-hotcue@1x.png`: "Love To Give" carries D at
/// 162486 ms (index 21, green) and H at 162485 ms (index 33, yellow), and the
/// capture shows yellow there. Position order would have painted D last and
/// shown green; both cues were created 2025-10-15, a year before the capture,
/// so D was not added afterwards. The index keeps cues in position order, so
/// this is a sort of at most sixteen.
fn hot_cues_in_slot_order(library: &Library, row: rbl_index::Row) -> Vec<RowCueDto> {
    let mut hot: Vec<(u8, RowCueDto)> = library
        .cues_of(row)
        .iter()
        .filter_map(|cue| {
            let letter = cue.hot_letter()?;
            Some((cue.kind, RowCueDto(letter, cue.position_ms, cue_colour_css(cue.colour))))
        })
        .collect();
    hot.sort_by_key(|(kind, _)| *kind);
    hot.into_iter().map(|(_, dto)| dto).collect()
}

/// Builds the wire rows for a window. `position` is the row's 1-based place in
/// the view, which is what the `#` column shows.
pub fn rows_to_dto(library: &Library, rows: &[rbl_index::Row], first_position: usize) -> Vec<RowDto> {
    rows.iter()
        .enumerate()
        .map(|(offset, &row)| {
            let index = row as usize;
            RowDto {
                id: library.ids.get(index).copied().unwrap_or(0).to_string(),
                track_no: u32::try_from(first_position + offset + 1).unwrap_or(u32::MAX),
                title: library.title.get(index).to_owned(),
                artist: library.artist_name(row).to_owned(),
                album: library.album_name(row).to_owned(),
                genre: library.genre_name(row).to_owned(),
                label: library.label_name(row).to_owned(),
                comment: library.comment.get(index).to_owned(),
                bpm_x100: library.bpm_x100.get(index).copied().unwrap_or(0),
                key: library.key_name(row).to_owned(),
                duration_sec: library.length_sec.get(index).copied().unwrap_or(0),
                rating: library.rating.get(index).copied().unwrap_or(0),
                analysed: library.analysed.get(index).copied().unwrap_or(0),
                date_added: library.date_added.get(index).to_owned(),
                release_date: library.release_date.get(index).to_owned(),
                hot_cues: hot_cues_in_slot_order(library, row),
                memory_cues: library.cues_of(row).iter().filter(|cue| cue.is_memory()).map(|cue| cue.position_ms).collect(),
                // Stable per track so the placeholder tint does not flicker on scroll.
                has_artwork: !library.artwork_path.get(index).is_empty(),
                artwork_hue: u16::try_from(
                    library.ids.get(index).copied().unwrap_or(0) % 360,
                )
                .unwrap_or(0),
                file_name: library.file_name.get(index).to_owned(),
                missing: crate::relocate::is_missing(library.folder_path.get(index)),
                extra: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use rbl_index::testing::{library_from, TestTrack};
    use rbl_index::{Cue, SortColumn};

    use super::{rows_to_dto, sort_from_wire};

    fn cue(kind: u8, position_ms: u32, colour: u8) -> Cue {
        Cue { position_ms, kind, colour, ..Cue::default() }
    }

    #[test]
    fn a_row_carries_hot_badges_and_memory_positions_separately() {
        let library = library_from(&[TestTrack {
            id: 7,
            title: "Take Me Home",
            bpm_x100: 12_800,
            cues: vec![cue(1, 46, 21), cue(0, 46, 0), cue(6, 24, 18), cue(2, 165_046, 41), cue(0, 1000, 0), cue(3, 2000, 0)],
            ..TestTrack::default()
        }]);
        let rows = rows_to_dto(&library, &[0], 0);
        let json = serde_json::to_value(&rows[0]).unwrap();
        assert_eq!(json["memoryCues"], serde_json::json!([46, 1000]));
        // In slot order, letters from `Kind` and rekordbox's resolved colour;
        // a hot cue with no colour carries none, so it is drawn in the default
        // green rather than the palette's black at index 0.
        assert_eq!(
            json["hotCues"],
            serde_json::json!([["A", 46, "#3CEB50"], ["B", 165_046, "#E02823"], ["C", 2000, null], ["E", 24, "#10B176"]])
        );
    }

    #[test]
    fn the_dj_play_count_wire_key_uses_the_numeric_index() {
        assert_eq!(sort_from_wire("djPlayCount"), SortColumn::PlayCount);
    }

    #[test]
    fn every_sortable_browser_column_has_its_own_wire_key() {
        // The frontend's column keys, which are what it sends as the sort.
        let wire = [
            ("title", SortColumn::Title), ("artist", SortColumn::Artist), ("album", SortColumn::Album),
            ("genre", SortColumn::Genre), ("label", SortColumn::Label), ("comment", SortColumn::Comment),
            ("key", SortColumn::Key), ("keyCamelot", SortColumn::KeyCamelot), ("bpm", SortColumn::Bpm),
            ("duration", SortColumn::Duration), ("rating", SortColumn::Rating),
            ("djPlayCount", SortColumn::PlayCount), ("dateAdded", SortColumn::DateAdded),
            ("releaseDate", SortColumn::ReleaseDate), ("size", SortColumn::Size), ("year", SortColumn::Year),
            ("sampleRate", SortColumn::SampleRate), ("bitrate", SortColumn::Bitrate),
            ("color", SortColumn::Color), ("fileName", SortColumn::FileName),
            ("location", SortColumn::Location), ("composer", SortColumn::Composer),
            ("albumArtist", SortColumn::AlbumArtist), ("remixer", SortColumn::Remixer),
            ("originalArtist", SortColumn::OriginalArtist), ("mixName", SortColumn::MixName),
            ("discNo", SortColumn::DiscNo), ("trackNumber", SortColumn::TrackNumber),
            ("fileType", SortColumn::FileType), ("bitDepth", SortColumn::BitDepth),
            ("lyricist", SortColumn::Lyricist), ("dateCreated", SortColumn::DateCreated),
            ("publishTrackInfo", SortColumn::PublishTrackInfo), ("message", SortColumn::Message),
        ];
        for (name, column) in wire {
            assert_eq!(sort_from_wire(name), column, "{name}");
        }
        // Every index column but the view's own order is reachable.
        let reached: std::collections::HashSet<_> = wire.iter().map(|&(_, column)| column).collect();
        for column in SortColumn::ALL {
            assert!(column == SortColumn::TrackNo || reached.contains(&column), "{column:?} has no wire key");
        }
        assert_eq!(sort_from_wire("hotCue"), SortColumn::TrackNo, "Hot Cue is not sortable");
        assert_eq!(sort_from_wire("trackNo"), SortColumn::TrackNo);
    }

    #[test]
    fn a_full_page_of_rows_with_every_hot_cue_set_stays_under_the_response_cap() {
        // rekordbox 7 allows sixteen hot cues a track; a page is 64 rows. This
        // is the worst case the row tuples were sized for.
        let kinds: Vec<u8> = (1..=17).filter(|&k| k != 4).collect();
        assert_eq!(kinds.len(), 16);
        let tracks: Vec<TestTrack> = (0..64)
            .map(|i| TestTrack {
                id: i + 1,
                title: "Something In The Air (Extended Mix) — a long enough title",
                artist: "22Bullets & Pascal Letoublon ft. MER",
                album: "Something In The Air",
                comment: "8A - C - 128 and a comment of ordinary length",
                bpm_x100: 12_800,
                length_sec: 300,
                cues: kinds
                    .iter()
                    .enumerate()
                    .map(|(n, &k)| cue(k, 20_000 * u32::try_from(n).unwrap(), 21))
                    .chain((0..10).map(|n| cue(0, n * 25_000, 0)))
                    .collect(),
                ..TestTrack::default()
            })
            .collect();
        let library = library_from(&tracks);
        let rows: Vec<u32> = (0..64).collect();
        let bytes = serde_json::to_vec(&rows_to_dto(&library, &rows, 0)).unwrap();
        assert!(bytes.len() < 64 * 1024, "{} bytes", bytes.len());
    }
}

#[cfg(test)]
mod edit_refresh_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::commands::{refresh_after_edit, Touched};

    #[test]
    fn small_edits_refresh_and_persist() {
        let subscriber = tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).without_time().with_test_writer().finish();
        let _logging = tracing::subscriber::set_default(subscriber);
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location.clone());
        crate::backups::create(&state).unwrap();
        let track = rbl_db::fixture::track_id(1);
        let original = state.library().unwrap();
        let row = original.row_of(&track).unwrap() as usize;
        let before = original.play_count[row];
        state.write_then(|w| w.record_play(&track), |db, _| {
            refresh_after_edit(&state, db, Touched::Histories(vec![track.clone()]))
        }).unwrap();
        assert_eq!(state.library().unwrap().play_count[row], before + 1);
        assert_eq!(original.play_count[row], before, "existing readers keep a consistent snapshot");
        let history = {
            let lib = state.library().unwrap();
            let histories = lib.histories();
            let index = histories.members.iter().rposition(|members| members.contains(&u32::try_from(row).unwrap_or(0))).unwrap();
            histories.ids[index].to_string()
        };
        state.write_then(|w| w.remove_from_history(&history, std::slice::from_ref(&track)), |db, _| {
            refresh_after_edit(&state, db, Touched::Histories(Vec::new()))
        }).unwrap();
        let current = state.library().unwrap();
        let histories = current.histories();
        let index = histories.ids.iter().position(|id| id.to_string() == history).unwrap();
        assert!(!histories.members[index].contains(&u32::try_from(row).unwrap_or(0)));
        drop(histories);
        state.write_then(|w| w.set_field(&track, rbl_db::write::TrackField::PlayCount, "0"), |db, _| {
            refresh_after_edit(&state, db, Touched::Metadata(vec![track.clone()]))
        }).unwrap();
        assert_eq!(state.library().unwrap().play_count[row], 0);
        let reopened = state.open_read_only().unwrap();
        let (persisted, _) = rbl_index::load(&reopened).unwrap();
        assert_eq!(persisted.play_count[row], 0);
        let generation = state.summary().3;
        assert!(state.write_then(|w| w.set_rating(&track, 6), |db, _| {
            refresh_after_edit(&state, db, Touched::Metadata(vec![track.clone()]))
        }).is_err());
        assert_eq!(state.summary().3, generation);
    }
}
