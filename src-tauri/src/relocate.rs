//! Missing files: the `[!]` a track whose file is gone shows in the
//! Collection, the Missing File Manager's list, Auto Relocate and Delete.
//!
//! [OBS rekordbox 7.2.14, Windows, chris-win11 2026-10-08, issue #201]
//! File › Display All Missing Files opens the Missing File Manager: every
//! track whose file is not there (36,444 of the rig's 38,733), with a
//! "N Track" count and Auto Relocate, Relocate, Delete and OK. The browser
//! checks the working path of each row it draws
//! (`BrowseBasicView::updateMissingStatus` @0x100321010 in 7.2.19 macOS,
//! see `rbl_db::track_path`); this reads the same path, the index's
//! resolved `folder_path`.
//!
//! Auto Relocate ([`auto_relocate`]) is rekordbox's
//! (`MissingFileTable::relocateSelectedFiles` with `SearchMode` 2, the
//! Auto Relocate button: `MissingFileComponent::buttonClicked` @0x1012a8dac;
//! the per-track search is its `$_0` @0x1012a9b54). [OBS rekordbox 7.2.19
//! macOS arm64, static, issue #201] For each missing track, in order:
//!
//! 1. The file the track's `rb_file_id` names on the volume its old path was
//!    on ([`by_file_id`]): a file moved anywhere on the same drive keeps that
//!    number, so it is found wherever it went. macOS only, APFS and HFS+.
//! 2. Otherwise the track's `FileNameL`, matched exactly, in the folders
//!    below, each walked through every subfolder ([`Walks`]); the first
//!    folder holding a file of that name ends the search, its first such file
//!    the answer. The folders, in order: the folder of the file the run last
//!    relocated a track to, then the Specified user folders that exist (when
//!    that box is ticked, `isSelectedAutoRelocateUserFolder`, default off),
//!    the Music folder, Movies/Videos and the Desktop where ticked (defaults
//!    on; bytes 0x1037bbacc..acf read by `SettingIF::isSelectedAutoRelocate*Folder`).
//!
//! A file found that is already a track in the collection is not taken
//! (`DatabaseIF::isCollectionSong`); rekordbox lists such files under "Track
//! exists in the collection" when it is done.
//!
//! Relocate over several tracks is rekordbox's too
//! ([`relocate_by_location`]): once one file has been chosen, the rest can
//! be looked for "using the location of this track".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use rbl_index::Library;

use crate::commands::{announce_history, blocking, permanent_edit, reload, write_error, wrote_outside_history, MAX_ROWS};
use crate::dto::{MissingTrackDto, MissingTracksDto};
use crate::error::AppResult;
use crate::commands::Touched;
use crate::state::AppState;

/// Whether a track's file is gone: it has a path and nothing is there.
///
/// An empty path is a track that never had a file, not one that lost it.
pub fn is_missing(path: &str) -> bool {
    // perf-ok: one `stat` per row drawn, which is what rekordbox's own
    // browser does; whole-library scans run under `blocking`.
    !path.is_empty() && !Path::new(path).exists()
}

/// Whether a file the library points at is gone.
pub fn is_missing_path(path: &Path) -> bool {
    // perf-ok: one `stat`, as rekordbox's own load does before it opens.
    !path.as_os_str().is_empty() && !path.exists()
}

/// rekordbox's `kPlayerOperateErrorLoadMissingFile` (global 0x1056c6b00,
/// set @0x1004d4004 in 7.2.19): the status bar's word when a deck is asked to
/// load a track whose file is gone.
pub const LOAD_MISSING_FILE: &str = "Load error. The file could not be found.";

/// The rows of every missing track, in the index's order.
fn scan(library: &Library) -> Vec<u32> {
    (0..library.len())
        .filter(|&index| is_missing(library.folder_path.get(index)))
        .filter_map(|index| u32::try_from(index).ok())
        .collect()
}

/// The Missing File Manager's last scan of a library.
///
/// The list can run to tens of thousands of tracks, more than one response
/// may carry, so it is paged out from here; it belongs to the library it was
/// taken of and is thrown away once that is reloaded.
pub struct MissingScan {
    library: Arc<Library>,
    rows: Arc<Vec<u32>>,
}

/// The missing rows of `library`: a fresh scan when asked for or when the
/// last one was of another library, the last one otherwise.
fn missing_rows(state: &AppState, library: &Arc<Library>, rescan: bool) -> Arc<Vec<u32>> {
    let mut held = state.missing_scan.lock();
    if !rescan {
        if let Some(scan) = held.as_ref().filter(|scan| Arc::ptr_eq(&scan.library, library)) {
            return Arc::clone(&scan.rows);
        }
    }
    let rows = Arc::new(scan(library));
    *held = Some(MissingScan { library: Arc::clone(library), rows: Arc::clone(&rows) });
    rows
}

fn missing_dto(library: &Library, row: u32) -> MissingTrackDto {
    let index = row as usize;
    MissingTrackDto {
        id: library.ids.get(index).copied().unwrap_or(0).to_string(),
        title: library.title.get(index).to_owned(),
        artist: library.artist_name(row).to_owned(),
        album: library.album_name(row).to_owned(),
        path: library.folder_path.get(index).to_owned(),
    }
}

/// A page of the Missing File Manager's list.
///
/// `rescan` checks every track's file again, as opening the manager does;
/// otherwise the page comes from the last scan.
#[tauri::command]
pub async fn missing_tracks(
    state: State<'_, Arc<AppState>>,
    offset: u32,
    limit: u32,
    rescan: bool,
) -> AppResult<MissingTracksDto> {
    let library = state.library()?;
    let state = Arc::clone(&state);
    let wanted = limit.min(MAX_ROWS) as usize;
    blocking("missing_tracks", move || {
        let rows = missing_rows(&state, &library, rescan);
        let tracks = rows
            .iter()
            .skip(offset as usize)
            .take(wanted)
            .map(|&row| missing_dto(&library, row))
            .collect();
        Ok(MissingTracksDto { total: u32::try_from(rows.len()).unwrap_or(u32::MAX), tracks })
    })
    .await
}

/// The rows an action over missing files applies to: the ones named that are
/// still missing, or with none named every missing track, checked afresh.
fn chosen(state: &AppState, library: &Arc<Library>, tracks: Option<&[String]>) -> Vec<u32> {
    match tracks {
        None => missing_rows(state, library, true).as_ref().clone(),
        Some(ids) => ids
            .iter()
            .filter_map(|id| library.row_of(id))
            .filter(|&row| is_missing(library.folder_path.get(row as usize)))
            .collect(),
    }
}

/// What an automatic relocate did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateReportDto {
    pub relocated: u32,
    /// Missing tracks for which nothing was found.
    pub unresolved: u32,
    /// Files found that are already tracks in the collection, so were not
    /// taken: rekordbox's "Track exists in the collection" list, its first
    /// [`IN_COLLECTION_LISTED`] paths.
    pub in_collection: Vec<String>,
    /// How many such files there were in all.
    pub in_collection_total: u32,
}

/// [OBS rekordbox 7.2.19 static, `relocateSelectedFiles` @0x1012a6e64] The
/// "Track exists in the collection" list shows the first 1000 paths, then
/// "and more...".
const IN_COLLECTION_LISTED: usize = 1000;

/// How many directory entries one run looks at in all, so a search folder
/// that is the root of a large disk ends rather than runs for an hour.
/// [ASSUME] rekordbox has no bound, only its progress window's Cancel; rbx's
/// search has no Cancel, so it has this instead.
const MAX_ENTRIES: usize = 2_000_000;

/// The search folders, each walked once, on first use, and kept for the run.
///
/// [OBS rekordbox 7.2.19 static] Each folder is searched with
/// `juce::RangedDirectoryIterator (folder, true, "*", findFiles,
/// FollowSymlinks::yes)` (`$_0`'s inner lambda @0x1012aa64c), stopping at
/// the first file whose `getFileName()` equals the track's `FileNameL`
/// (`juce::operator==`, so case and Unicode form count). That iterator goes
/// depth first: a folder's subfolder is searched as soon as it is met, before
/// the folder's later entries; hidden folders are searched; links to folders
/// are followed. So the file a folder gives for a name is the first of that
/// name in that order, which is what [`walk`] records.
pub struct Walks {
    walked: HashMap<PathBuf, HashMap<String, PathBuf>>,
    budget: usize,
}

impl Walks {
    pub fn new() -> Self {
        Self::with_budget(MAX_ENTRIES)
    }

    fn with_budget(budget: usize) -> Self {
        Self { walked: HashMap::new(), budget }
    }

    /// The first file called `name` under `folder`, in the iterator's order.
    pub fn find(&mut self, folder: &Path, name: &str) -> Option<PathBuf> {
        if !self.walked.contains_key(folder) {
            let found = walk(folder, &mut self.budget);
            self.walked.insert(folder.to_path_buf(), found);
        }
        self.walked.get(folder).and_then(|found| found.get(name)).cloned()
    }
}

impl Default for Walks {
    fn default() -> Self {
        Self::new()
    }
}

/// Every file under `folder`, by name, the first met in a depth-first walk
/// winning. A link to a folder is followed once: a link back up the tree
/// would otherwise be walked forever.
fn walk(folder: &Path, budget: &mut usize) -> HashMap<String, PathBuf> {
    use std::collections::HashSet;
    let mut found: HashMap<String, PathBuf> = HashMap::new();
    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let Some(entries) = open_dir(folder, &mut seen) else { return found };
    let mut stack = vec![entries];
    while let Some(entries) = stack.last_mut() {
        let Some(entry) = entries.next() else {
            stack.pop();
            continue;
        };
        let Ok(entry) = entry else { continue };
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let path = entry.path();
        let Ok(mut kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            // perf-ok: a `stat` per link only, under `blocking`.
            match std::fs::metadata(&path) {
                Ok(target) => kind = target.file_type(),
                Err(_) => continue,
            }
        }
        if kind.is_dir() {
            if let Some(inner) = open_dir(&path, &mut seen) {
                stack.push(inner);
            }
        } else if kind.is_file() {
            found.entry(entry.file_name().to_string_lossy().into_owned()).or_insert(path);
        }
    }
    found
}

/// A folder's entries, unless it has been walked already in this walk.
fn open_dir(dir: &Path, seen: &mut std::collections::HashSet<(u64, u64)>) -> Option<std::fs::ReadDir> {
    // perf-ok: a plain function, run under `blocking` by the command below.
    let meta = std::fs::metadata(dir).ok()?;
    if !first_visit(&meta, seen) {
        return None;
    }
    std::fs::read_dir(dir).ok()
}

/// Whether a folder has not been met before in this walk, by its device and
/// inode.
#[cfg(unix)]
fn first_visit(meta: &std::fs::Metadata, seen: &mut std::collections::HashSet<(u64, u64)>) -> bool {
    use std::os::unix::fs::MetadataExt;
    seen.insert((meta.dev(), meta.ino()))
}

/// [ASSUME] Windows folder links are rare in music folders and `std` gives
/// no stable file index, so every folder counts as new there.
#[cfg(not(unix))]
fn first_visit(_meta: &std::fs::Metadata, _seen: &mut std::collections::HashSet<(u64, u64)>) -> bool {
    true
}

/// The file a track's `rb_file_id` names on the volume its old path was on,
/// or `None`.
///
/// [OBS rekordbox 7.2.19 macOS arm64, static] `common::getFilePathFromID`
/// @0x101e42610: nothing for id 0; climbs from the old path's folder to the
/// nearest one that exists, and gives up if that is `/Volumes` (the drive is
/// not there); asks that folder's file system type and goes on only for
/// `apfs` or `hfs`; then resolves `/.vol/<NSFileSystemNumber>/<id>`.
/// `$_0` @0x1012a9bac..0x1012a9c78 then refuses an answer still under
/// `/.vol/` or containing `/.Trash/`, or without a playable extension
/// (`hasFileExtension`). [UNKNOWN] what rekordbox for Windows does with the
/// id; rbx looks it up on macOS only.
fn by_file_id(old: &str, file_id: u64, playable: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let found = volfs::path_of(Path::new(old), file_id)?;
    let text = found.to_string_lossy();
    if text.starts_with("/.vol/") || text.contains("/.Trash/") {
        return None;
    }
    // perf-ok: one `stat`, under `blocking`.
    (playable(&found) && found.is_file()).then_some(found)
}

/// Whether a file found by its id may be taken: its extension is one
/// rekordbox plays, or the missing track's own.
///
/// [UNKNOWN] the exact list `$_0` checks (a string built at start-up at
/// 0x105510d68, plus `rbsc`); a moved file keeps its own extension, so the
/// track's is accepted too.
fn playable_like(track: &str) -> impl Fn(&Path) -> bool + '_ {
    let own = Path::new(track).extension().map(|e| e.to_string_lossy().to_lowercase());
    move |found: &Path| {
        let Some(ext) = found.extension().map(|e| e.to_string_lossy().to_lowercase()) else { return false };
        own.as_deref() == Some(ext.as_str()) || rbl_db::import::AUDIO_EXTENSIONS.contains(&ext.as_str())
    }
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "statfs and F_GETPATH are only reachable through libc; buffers are sized for the calls and the descriptor is owned by a File"
)]
mod volfs {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::AsRawFd;
    use std::path::{Path, PathBuf};

    pub(super) fn path_of(old: &Path, file_id: u64) -> Option<PathBuf> {
        if file_id == 0 {
            return None;
        }
        // perf-ok: a `stat` per folder climbed, under `blocking`.
        let mut dir = old.parent()?;
        while !dir.exists() {
            dir = dir.parent()?;
        }
        if dir == Path::new("/Volumes") {
            return None;
        }
        let kind = fs_type(dir)?;
        if kind != "apfs" && kind != "hfs" {
            return None;
        }
        let device = std::fs::metadata(dir).ok()?.dev();
        // `%lld/%lld` of two `long long`s, as rekordbox formats them.
        let id = i64::from_ne_bytes(file_id.to_ne_bytes());
        let vol = PathBuf::from(format!("/.vol/{device}/{id}"));
        real_path(&vol)
    }

    /// `f_fstypename` of the file system `dir` is on.
    fn fs_type(dir: &Path) -> Option<String> {
        let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut info: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is a NUL-terminated path alive for the call and `info`
        // is a writable `statfs`.
        if unsafe { libc::statfs(c.as_ptr(), &raw mut info) } != 0 {
            return None;
        }
        // SAFETY: the kernel NUL-terminates `f_fstypename` within its array.
        let name = unsafe { CStr::from_ptr(info.f_fstypename.as_ptr()) };
        Some(name.to_string_lossy().into_owned())
    }

    /// Where a `/.vol` path really is: `F_GETPATH` on the opened file, as
    /// `NSURL URLByResolvingSymlinksInPath` finds it for rekordbox.
    /// [ASSUME] Foundation also drops a leading `/private`; music is not
    /// kept there, so that is not copied.
    fn real_path(vol: &Path) -> Option<PathBuf> {
        let file = std::fs::File::open(vol).ok()?;
        let mut buf = vec![0_u8; libc::PATH_MAX as usize];
        // SAFETY: the descriptor is open for the call and `buf` holds
        // MAXPATHLEN bytes, as F_GETPATH requires.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) } == -1 {
            return None;
        }
        let path = CStr::from_bytes_until_nul(&buf).ok()?;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes())))
    }
}

#[cfg(not(target_os = "macos"))]
mod volfs {
    use std::path::{Path, PathBuf};

    pub(super) fn path_of(_old: &Path, _file_id: u64) -> Option<PathBuf> {
        None
    }
}

/// The file name a library row's path ends in.
fn file_name(path: &str) -> Option<String> {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Preferences › Advanced › Database › Auto Relocate Search Folders.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateSearch {
    /// Specified user folders: empty unless that box is ticked.
    pub folders: Vec<String>,
    pub music: bool,
    pub video: bool,
    pub desktop: bool,
}

/// The folders Auto Relocate walks, in rekordbox's order: the user's own
/// that exist, then Music, Movies/Videos and the Desktop where ticked.
fn search_roots(
    search: &RelocateSearch,
    music: Option<PathBuf>,
    video: Option<PathBuf>,
    desktop: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = search
        .folders
        .iter()
        .map(PathBuf::from)
        // perf-ok: one `stat` per configured folder, under `blocking`.
        .filter(|folder| folder.exists())
        .collect();
    let system = [(search.music, music), (search.video, video), (search.desktop, desktop)];
    for (ticked, folder) in system {
        if let Some(folder) = folder.filter(|_| ticked) {
            if !roots.contains(&folder) {
                roots.push(folder);
            }
        }
    }
    roots
}

/// A missing track Auto Relocate looks for.
#[derive(Debug, Clone)]
pub struct Wanted {
    pub id: String,
    /// The path the library has for it.
    pub path: String,
    /// Its `FileNameL`, the name searched for.
    pub name: String,
    /// Its `rb_file_id`, 0 for none.
    pub file_id: u64,
}

/// What a search decided, before anything is written.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Track id and the file it is to point at, in the order searched.
    pub moves: Vec<(String, PathBuf)>,
    /// Files found that already belong to a track.
    pub in_collection: Vec<PathBuf>,
    /// Tracks for which no file was found.
    pub unresolved: u32,
}

/// Searches for each wanted track in turn, as rekordbox's `$_0` does.
///
/// `by_id` is the file-id look ([`by_file_id`]); `taken` says whether a file
/// is already a track in the collection. A file this run has already given
/// to one track is not given to another.
pub fn plan(
    wanted: &[Wanted],
    roots: &[PathBuf],
    walks: &mut Walks,
    by_id: impl Fn(&Wanted) -> Option<PathBuf>,
    taken: impl Fn(&Path) -> bool,
) -> Plan {
    let mut out = Plan::default();
    let mut claimed: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    // [OBS static] the folder of the file last relocated to is searched
    // first (`$_0` @0x1012a9cb0..0x1012a9d08, `SearchMode` 2); it is set by
    // each successful relocate (the inner lambda @0x1012aa5d4..0x1012aa61c).
    let mut last: Option<PathBuf> = None;
    for track in wanted {
        let found = by_id(track).or_else(|| {
            let first = last.as_ref().and_then(|file| file.parent()).map(Path::to_path_buf);
            first.iter().chain(roots).find_map(|root| walks.find(root, &track.name))
        });
        let Some(found) = found else {
            out.unresolved += 1;
            continue;
        };
        if taken(&found) || claimed.contains(&found) {
            if !out.in_collection.contains(&found) {
                out.in_collection.push(found);
            }
            continue;
        }
        claimed.insert(found.clone());
        last = Some(found.clone());
        out.moves.push((track.id.clone(), found));
    }
    out
}

/// Points missing tracks at their moved files, as rekordbox's Auto Relocate
/// finds them: the tracks named, or every missing track when none are.
///
/// Nothing changes unless this is asked for; a track is only ever pointed
/// at a file that is there and belongs to no other track.
#[tauri::command]
pub async fn auto_relocate<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    search: RelocateSearch,
    tracks: Option<Vec<String>>,
) -> AppResult<RelocateReportDto> {
    let paths = app.path();
    let (music, video, desktop) = (paths.audio_dir().ok(), paths.video_dir().ok(), paths.desktop_dir().ok());
    let library = state.library()?;
    let state_for_reload = Arc::clone(&state);
    let writing = Arc::clone(&state);
    // `blocking` is `spawn_blocking` with a name: the walk and the writes
    // happen off the async thread.
    let report = blocking("auto_relocate", move || {
        // The missing tracks first, from the index: the walk is the slow
        // part, and a library with nothing missing need not walk at all.
        let rows = chosen(&writing, &library, tracks.as_deref());
        if rows.is_empty() {
            return Ok(RelocateReportDto::default());
        }
        let wanted: Vec<Wanted> = rows
            .into_iter()
            .filter_map(|row| {
                let index = row as usize;
                let id = library.ids.get(index).copied().unwrap_or(0).to_string();
                let path = library.folder_path.get(index).to_owned();
                let stored = library.file_name.get(index);
                let name = if stored.is_empty() { file_name(&path)? } else { stored.to_owned() };
                // An id that cannot be read is a track without one: the
                // folder search still runs.
                let file_id = writing.read_db(|db| db.file_id(&id)).unwrap_or(0);
                Some(Wanted { id, path, name, file_id })
            })
            .collect();

        let roots = search_roots(&search, music, video, desktop);
        let mut walks = Walks::new();
        let plan = plan(
            &wanted,
            &roots,
            &mut walks,
            |track| by_file_id(&track.path, track.file_id, playable_like(&track.path)),
            |file| library.row_for_path(file).is_some(),
        );

        let in_collection_total = u32::try_from(plan.in_collection.len()).unwrap_or(u32::MAX);
        let in_collection = plan
            .in_collection
            .iter()
            .take(IN_COLLECTION_LISTED)
            .map(|file| file.to_string_lossy().into_owned())
            .collect();
        let unresolved = plan.unresolved;
        let relocated = if plan.moves.is_empty() {
            0
        } else {
            let _gate = writing.edit_gate.lock();
            let relocated = writing
                .write(|writer| {
                    for (id, path) in &plan.moves {
                        writer.relocate(id, path)?;
                    }
                    Ok(u32::try_from(plan.moves.len()).unwrap_or(u32::MAX))
                })
                .map_err(write_error)?;
            wrote_outside_history(&writing, false);
            relocated
        };
        Ok(RelocateReportDto { relocated, unresolved, in_collection, in_collection_total })
    })
    .await?;

    // Only reload if anything moved; a search that found nothing has not
    // changed the library.
    if report.relocated > 0 {
        reload(app.clone(), Arc::clone(&state_for_reload)).await?;
        announce_history(&app, &state_for_reload);
    }
    Ok(report)
}

/// The Missing File Manager's Delete: the missing tracks named, or every
/// missing track when none are, leave the collection and every playlist, as
/// Remove from Collection does. Returns how many went.
///
/// A track whose file has come back since the list was drawn is left alone.
#[tauri::command]
pub async fn remove_missing_tracks<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Option<Vec<String>>,
) -> AppResult<u32> {
    let library = state.library()?;
    let checking = Arc::clone(&state);
    let ids: Vec<String> = blocking("remove_missing_tracks_scan", move || {
        Ok(chosen(&checking, &library, tracks.as_deref())
            .into_iter()
            .map(|row| library.ids.get(row as usize).copied().unwrap_or(0).to_string())
            .collect())
    })
    .await?;
    if ids.is_empty() {
        return Ok(0);
    }
    let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    permanent_edit(app, state, "remove_missing_tracks", Touched::Tracks, move |w| {
        for id in &ids {
            w.delete_track(id)?;
        }
        Ok(())
    })
    .await?;
    Ok(count)
}

/// The tracks a Relocate over a selection asks about, in the order given:
/// the ones named whose file is missing. Paged by the caller, so a page
/// stays inside one response.
///
/// [OBS rekordbox 7.2.19 static] `MissingFileTable::relocateSelectedFiles`
/// @0x1012a6964..0x1012a69b0 passes over a selected track whose file is
/// there (`File::existsAsFile` on its working path).
#[tauri::command]
pub async fn relocation_targets(state: State<'_, Arc<AppState>>, tracks: Vec<String>) -> AppResult<Vec<MissingTrackDto>> {
    let library = state.library()?;
    blocking("relocation_targets", move || {
        Ok(tracks
            .iter()
            .take(MAX_ROWS as usize)
            .filter_map(|id| library.row_of(id))
            .filter(|&row| is_missing(library.folder_path.get(row as usize)))
            .map(|row| missing_dto(&library, row))
            .collect())
    })
    .await
}

/// Whether two strings are equal ignoring case, char by char, as
/// `juce::String::equalsIgnoreCase` compares them.
fn same_char(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

fn starts_with_ignoring_case(text: &[char], prefix: &[char]) -> bool {
    text.len() >= prefix.len() && text.iter().zip(prefix).all(|(&a, &b)| same_char(a, b))
}

/// `text` with every occurrence of `from` replaced by `to`, ignoring case:
/// `juce::String::replace(from, to, true)`.
fn replace_ignoring_case(text: &str, from: &str, to: &str) -> String {
    let text: Vec<char> = text.chars().collect();
    let from: Vec<char> = from.chars().collect();
    if from.is_empty() {
        return text.into_iter().collect();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        if starts_with_ignoring_case(&text[at..], &from) {
            out.push_str(to);
            at += from.len();
        } else {
            out.push(text[at]);
            at += 1;
        }
    }
    out
}

/// Where a missing track's file would be if it moved the way the one just
/// relocated did, or `None`.
///
/// [OBS rekordbox 7.2.19 static, `relocateSelectedFiles` `$_2` @0x1012aa9d8]
/// First `to` + the track's file name (`File::getChildFile(FileNameL)`);
/// failing that, when `from` and `to` differ and the track's path (with `\`
/// as `/`) starts with `from`, the path with `from` replaced by `to`. `from`
/// and `to` are the old and new folders with the trailing folder names they
/// share taken off. Only a file that is there counts.
fn moved_like(path: &str, file_name: &str, from: &str, to: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let child = Path::new(to).join(file_name);
    if exists(&child) {
        return Some(child);
    }
    let same = from.chars().count() == to.chars().count() && from.chars().zip(to.chars()).all(|(a, b)| same_char(a, b));
    if same {
        return None;
    }
    let path = path.replace('\\', "/");
    let chars: Vec<char> = path.chars().collect();
    let prefix: Vec<char> = from.chars().collect();
    if !starts_with_ignoring_case(&chars, &prefix) {
        return None;
    }
    let moved = PathBuf::from(replace_ignoring_case(&path, from, to));
    exists(&moved).then_some(moved)
}

/// A path built with `/` in Windows' own separators, as `juce::File` keeps
/// it, so the library stores what rekordbox would.
fn native(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().replace('/', "\\"))
    } else {
        path
    }
}

/// Relocate's "find other missing file using the location of this track":
/// each named track still missing whose file turns up where
/// [`moved_like`] looks, and is not already another track in the
/// collection, is pointed at it. Returns how many were.
///
/// [OBS rekordbox 7.2.19 static] `$_2` skips a found file the collection
/// already holds (`DatabaseIF::isCollectionSong`) and relocates the rest
/// (`DatabaseIF::relocateTrack`); the count is what "rekordbox found N
/// files." reports.
#[tauri::command]
pub async fn relocate_by_location<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    from: String,
    to: String,
) -> AppResult<u32> {
    let library = state.library()?;
    let writing = Arc::clone(&state);
    let state_for_reload = Arc::clone(&state);
    let found = blocking("relocate_by_location", move || {
        let moves: Vec<(String, PathBuf)> = tracks
            .iter()
            .filter_map(|id| library.row_of(id).map(|row| (id, row)))
            .filter_map(|(id, row)| {
                let path = library.folder_path.get(row as usize);
                if !is_missing(path) {
                    return None;
                }
                let name = file_name(path)?;
                // perf-ok: a `stat` or two per track, under `blocking`.
                let moved = native(moved_like(path, &name, &from, &to, Path::is_file)?);
                library.row_for_path(&moved).is_none().then(|| (id.clone(), moved))
            })
            .collect();
        if moves.is_empty() {
            return Ok(0);
        }
        let _gate = writing.edit_gate.lock();
        let found = writing
            .write(|writer| {
                for (id, path) in &moves {
                    writer.relocate(id, path)?;
                }
                Ok(u32::try_from(moves.len()).unwrap_or(u32::MAX))
            })
            .map_err(write_error)?;
        wrote_outside_history(&writing, false);
        Ok(found)
    })
    .await?;
    if found > 0 {
        reload(app.clone(), Arc::clone(&state_for_reload)).await?;
        announce_history(&app, &state_for_reload);
    }
    Ok(found)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        // perf-ok: a test's fixture, not a command.
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    fn wanted(id: &str, name: &str) -> Wanted {
        Wanted { id: id.to_owned(), path: format!("/gone/{name}"), name: name.to_owned(), file_id: 0 }
    }

    fn no_id(_: &Wanted) -> Option<PathBuf> {
        None
    }

    fn untaken(_: &Path) -> bool {
        false
    }

    #[test]
    fn a_file_moved_anywhere_below_a_search_folder_is_found() {
        // The reporter's case (#201, 2026-10-10): the file went elsewhere in
        // the same tree. rekordbox's iterator has no depth limit.
        let dir = tempfile::tempdir().unwrap();
        let mut deep = dir.path().join("Music");
        for level in 0..24 {
            deep = deep.join(format!("level {level}"));
        }
        touch(&deep.join("Track.mp3"));
        let plan = plan(&[wanted("1", "Track.mp3")], &[dir.path().join("Music")], &mut Walks::new(), no_id, untaken);
        assert_eq!(plan.moves, [("1".to_owned(), deep.join("Track.mp3"))]);
        assert_eq!(plan.unresolved, 0);
    }

    #[test]
    fn the_name_must_match_exactly() {
        // `juce::operator==`: case counts.
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a/track.MP3"));
        let plan = plan(&[wanted("1", "track.mp3")], &[dir.path().to_path_buf()], &mut Walks::new(), no_id, untaken);
        assert_eq!(plan.moves, []);
        assert_eq!(plan.unresolved, 1);
    }

    #[test]
    fn hidden_folders_are_searched_and_folder_links_followed_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        touch(&root.join(".hidden/In Hidden.mp3"));
        touch(&dir.path().join("elsewhere/Through Link.mp3"));
        std::fs::create_dir_all(&root).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("elsewhere"), root.join("link")).unwrap();
            // A link back up the tree must not be walked forever.
            std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        }
        let mut walks = Walks::new();
        assert_eq!(walks.find(&root, "In Hidden.mp3"), Some(root.join(".hidden/In Hidden.mp3")));
        #[cfg(unix)]
        assert_eq!(walks.find(&root, "Through Link.mp3"), Some(root.join("link/Through Link.mp3")));
    }

    #[test]
    fn the_first_folder_with_the_name_ends_the_search() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("mine");
        let music = dir.path().join("Music");
        touch(&mine.join("deep/down/Song.mp3"));
        touch(&music.join("Song.mp3"));
        let plan = plan(&[wanted("1", "Song.mp3")], &[mine.clone(), music.clone()], &mut Walks::new(), no_id, untaken);
        assert_eq!(plan.moves, [("1".to_owned(), mine.join("deep/down/Song.mp3"))]);
    }

    #[test]
    fn the_folder_of_the_last_file_found_is_searched_first() {
        // [OBS static] `$_0` puts the parent of the file it last relocated to
        // ahead of the search folders (`SearchMode` 2).
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("mine");
        let music = dir.path().join("Music");
        touch(&mine.join("B.mp3"));
        touch(&music.join("Moved/A.mp3"));
        touch(&music.join("Moved/B.mp3"));
        let tracks = [wanted("1", "A.mp3"), wanted("2", "B.mp3")];
        let plan = plan(&tracks, &[mine.clone(), music.clone()], &mut Walks::new(), no_id, untaken);
        assert_eq!(
            plan.moves,
            [("1".to_owned(), music.join("Moved/A.mp3")), ("2".to_owned(), music.join("Moved/B.mp3"))]
        );
        // Taken alone, B is found in the earlier folder.
        let alone = super::plan(&tracks[1..], &[mine.clone(), music], &mut Walks::new(), no_id, untaken);
        assert_eq!(alone.moves, [("2".to_owned(), mine.join("B.mp3"))]);
    }

    #[test]
    fn the_file_id_is_tried_before_any_folder() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        touch(&music.join("Song.mp3"));
        let by_id_file = dir.path().join("Other Drive Folder/Song.mp3");
        touch(&by_id_file);
        let target = by_id_file.clone();
        let plan = plan(&[wanted("1", "Song.mp3")], &[music], &mut Walks::new(), move |_| Some(target.clone()), untaken);
        assert_eq!(plan.moves, [("1".to_owned(), by_id_file)]);
    }

    #[test]
    fn a_file_that_is_already_a_track_is_listed_not_taken() {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("Music");
        touch(&music.join("Owned.mp3"));
        touch(&music.join("Shared.mp3"));
        let owned = music.join("Owned.mp3");
        let tracks = [wanted("1", "Owned.mp3"), wanted("2", "Shared.mp3"), wanted("3", "Shared.mp3")];
        let plan = plan(&tracks, std::slice::from_ref(&music), &mut Walks::new(), no_id, |file| file == owned);
        // One file never ends up behind two tracks.
        assert_eq!(plan.moves, [("2".to_owned(), music.join("Shared.mp3"))]);
        assert_eq!(plan.in_collection, [music.join("Owned.mp3"), music.join("Shared.mp3")]);
        assert_eq!(plan.unresolved, 0);
    }

    #[test]
    fn a_walk_stops_at_its_budget() {
        let dir = tempfile::tempdir().unwrap();
        for at in 0..10 {
            touch(&dir.path().join(format!("{at}.mp3")));
        }
        let mut walks = Walks::with_budget(3);
        let found = (0..10).filter(|at| walks.find(dir.path(), &format!("{at}.mp3")).is_some()).count();
        assert_eq!(found, 3);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_file_moved_on_the_same_volume_is_found_by_its_number() {
        // [OBS static] `common::getFilePathFromID`: `/.vol/<device>/<id>` on
        // the old path's APFS or HFS+ volume. The temporary folder is on the
        // system's APFS volume.
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Crate/Track.mp3");
        touch(&old);
        let id = rbl_db::file_id::of(&old);
        let new = dir.path().join("Elsewhere/Deeper/Track.mp3");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::rename(&old, &new).unwrap();
        // The old folder is gone too: the climb finds the nearest one left.
        std::fs::remove_dir(dir.path().join("Crate")).unwrap();

        let old = old.to_string_lossy().into_owned();
        let found = by_file_id(&old, id, playable_like(&old)).unwrap();
        assert_eq!(found.canonicalize().unwrap(), new.canonicalize().unwrap());
        // No number, or a file rekordbox would not play: not found.
        assert_eq!(by_file_id(&old, 0, playable_like(&old)), None);
        assert_eq!(by_file_id(&old, id, |_: &Path| false), None);
    }

    #[test]
    fn a_found_file_must_be_playable_or_the_tracks_own_kind() {
        let playable = playable_like("/gone/clip.MP4");
        assert!(playable(Path::new("/x/song.mp3")));
        assert!(playable(Path::new("/x/song.AIFF")));
        assert!(playable(Path::new("/x/clip.mp4")));
        assert!(!playable(Path::new("/x/notes.txt")));
        assert!(!playable(Path::new("/x/no-extension")));
    }

    #[test]
    fn a_track_is_missing_only_when_it_has_a_path_and_nothing_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("here.mp3");
        std::fs::write(&here, b"x").unwrap();
        assert!(!is_missing(here.to_str().unwrap()));
        assert!(is_missing(dir.path().join("gone.mp3").to_str().unwrap()));
        // No path is a track that never had a file, which is not this.
        assert!(!is_missing(""));
    }

    #[test]
    fn the_scan_lists_the_missing_tracks_in_index_order() {
        use rbl_index::testing::{library_from, TestTrack};
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("here.mp3");
        std::fs::write(&here, b"x").unwrap();
        let here: &'static str = Box::leak(here.to_string_lossy().into_owned().into_boxed_str());
        let gone: &'static str = Box::leak(dir.path().join("gone.mp3").to_string_lossy().into_owned().into_boxed_str());
        let also: &'static str = Box::leak(dir.path().join("also.mp3").to_string_lossy().into_owned().into_boxed_str());
        let library = library_from(&[
            TestTrack { id: 1, title: "Gone", path: gone, ..TestTrack::default() },
            TestTrack { id: 2, title: "Here", path: here, ..TestTrack::default() },
            TestTrack { id: 3, title: "Never had one", path: "", ..TestTrack::default() },
            TestTrack { id: 4, title: "Also gone", album: "Lost", path: also, ..TestTrack::default() },
        ]);
        let rows = scan(&library);
        let listed: Vec<MissingTrackDto> = rows.iter().map(|&row| missing_dto(&library, row)).collect();
        let titles: Vec<&str> = listed.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["Gone", "Also gone"]);
        assert_eq!(listed[1].album, "Lost");
        assert_eq!(listed[1].path, also);
    }

    #[test]
    fn a_file_name_is_the_last_segment_of_a_row_path() {
        assert_eq!(file_name("/Users/x/Music/Track.aiff").as_deref(), Some("Track.aiff"));
        assert_eq!(file_name(""), None);
    }
    #[test]
    fn the_search_folders_come_in_rekordboxs_order() {
        let dir = tempfile::tempdir().unwrap();
        let mine = dir.path().join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        let gone = dir.path().join("gone");
        let music = Some(PathBuf::from("/m/Music"));
        let video = Some(PathBuf::from("/m/Movies"));
        let desktop = Some(PathBuf::from("/m/Desktop"));
        let every = RelocateSearch {
            folders: vec![gone.to_string_lossy().into_owned(), mine.to_string_lossy().into_owned()],
            music: true,
            video: true,
            desktop: true,
        };
        assert_eq!(
            search_roots(&every, music.clone(), video.clone(), desktop.clone()),
            [mine.clone(), PathBuf::from("/m/Music"), PathBuf::from("/m/Movies"), PathBuf::from("/m/Desktop")]
        );
        let only_desktop = RelocateSearch { desktop: true, ..RelocateSearch::default() };
        assert_eq!(search_roots(&only_desktop, music, video, desktop), [PathBuf::from("/m/Desktop")]);
        assert_eq!(search_roots(&RelocateSearch::default(), None, None, None), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_track_moved_like_the_relocated_one_is_found() {
        let there = |paths: &'static [&'static str]| move |p: &Path| paths.iter().any(|q| Path::new(q) == p);
        // The same file name in the new folder comes first.
        assert_eq!(
            moved_like("/old/A/x.mp3", "x.mp3", "/old", "/new", there(&["/new/x.mp3", "/new/A/x.mp3"])),
            Some(PathBuf::from("/new/x.mp3"))
        );
        // Then the old folder swapped for the new, ignoring case.
        assert_eq!(
            moved_like("/OLD/A/x.mp3", "x.mp3", "/old", "/new", there(&["/new/A/x.mp3"])),
            Some(PathBuf::from("/new/A/x.mp3"))
        );
        // A Windows path is compared with its separators as `/`.
        assert_eq!(
            moved_like("C:\\Music\\A\\x.mp3", "x.mp3", "C:/Music", "D:/Music", there(&["D:/Music/A/x.mp3"])),
            Some(PathBuf::from("D:/Music/A/x.mp3"))
        );
        // Nothing there, or a path outside the old folder: not found.
        assert_eq!(moved_like("/old/A/x.mp3", "x.mp3", "/old", "/new", there(&[])), None);
        assert_eq!(moved_like("/else/A/x.mp3", "x.mp3", "/old", "/new", there(&["/new/A/x.mp3"])), None);
        // The same folder both ways is only the child-file look.
        assert_eq!(moved_like("/old/A/x.mp3", "x.mp3", "/old", "/OLD", there(&["/OLD/A/x.mp3"])), None);
    }

    #[test]
    fn replacing_ignores_case_and_takes_every_occurrence() {
        assert_eq!(replace_ignoring_case("/Old/a/old/b", "/old", "/new"), "/new/a/new/b");
        assert_eq!(replace_ignoring_case("abc", "", "x"), "abc");
    }
}
