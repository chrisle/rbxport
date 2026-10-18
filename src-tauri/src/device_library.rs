//! The Devices tree's libraries: a stick's own playlists, browsed and
//! edited where they are, as rekordbox's Devices tree does.
//!
//! Every read is of the stick, on a blocking thread, when asked; nothing here
//! touches the user's library. The edits go through
//! [`rbl_export::device_library`], which changes one of the stick's two
//! libraries at a time and publishes the file through the export journal.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rbl_export::device_library::{self as device, Edit, Format};
use rbl_index::device::{DeviceRow, DeviceView};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{blocking, MAX_ROWS};
use crate::dto::{RowDto, ViewHandleDto, ViewSpecDto};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::state::{sort_from_wire, AppState};

/// One library on a stick, for the tree.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLibraryDto {
    /// `deviceLibrary` or `oneLibrary`.
    pub format: &'static str,
    /// Tracks in the library, for All Tracks.
    pub tracks: u32,
    /// Playlists and folders in tree order.
    pub nodes: Vec<DeviceNodeDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceNodeDto {
    pub id: String,
    /// `"0"` for the top level.
    pub parent_id: String,
    pub name: String,
    pub folder: bool,
    /// Under the Playlists heading: 0 for the top level.
    pub depth: u32,
    /// Tracks in a playlist; children of a folder.
    pub count: u32,
}

/// An edit to a stick's playlists, as the interface sends it. Tracks are
/// the rows' own ids: `file:` and the track's path on the stick.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DeviceEditDto {
    #[serde(rename = "create")]
    Create { parent: String, name: String, folder: bool },
    #[serde(rename = "rename")]
    Rename { id: String, name: String },
    #[serde(rename = "delete")]
    Delete { id: String },
    /// `duplicates` is the answer to rekordbox's question about tracks the
    /// playlist already holds: true for "Add", false for "Skip".
    #[serde(rename = "add")]
    Add {
        playlist: String,
        tracks: Vec<String>,
        #[serde(default)]
        duplicates: bool,
    },
    #[serde(rename = "remove")]
    Remove { playlist: String, tracks: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEditResultDto {
    /// The playlist or folder the edit was about; the new one for a create.
    pub id: String,
    /// What changed; 0 when the edit had nothing to do.
    pub changed: u32,
}

/// What a stick's library names beside one of its tracks' audio, kept so the
/// track's waveforms, beat grid and artwork are read from the stick, as
/// rekordbox's Devices tree shows them (#319).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceAssets {
    /// The stick's mount point; the paths below are relative to it.
    pub mount: PathBuf,
    /// The audio the library names, volume-relative, as it spells it.
    pub audio: String,
    /// The `.DAT` the library names, volume-relative; empty for none.
    pub analysis: String,
    /// The artwork image the library names, volume-relative; empty for none.
    pub artwork: String,
}

impl DeviceAssets {
    /// The track's `.DAT` on the stick, or `None` when the library names
    /// none or the file is not there. The path comes from the stick's
    /// database, so it must stay under the stick's root.
    pub fn analysis_dat(&self) -> Option<PathBuf> {
        Self::on_stick(&self.mount, &self.analysis)
    }

    /// The track's artwork on the stick, under the same rule.
    pub fn artwork_file(&self) -> Option<PathBuf> {
        Self::on_stick(&self.mount, &self.artwork)
    }

    /// The track's audio on the stick, under the same rule.
    pub fn audio_file(&self) -> Option<PathBuf> {
        Self::on_stick(&self.mount, &self.audio)
    }

    /// The file `relative` names under `mount`: the path as spelled when it
    /// is there, otherwise the one the stick's own file system would open
    /// for it, whatever its case (#334). The lookup by directory listing
    /// runs only when the spelling misses, so a stick whose names match
    /// costs nothing more.
    fn on_stick(mount: &Path, relative: &str) -> Option<PathBuf> {
        if relative.is_empty() {
            return None;
        }
        // `resolve_under` only joins (#271), so whether the file is there is
        // asked here: a miss is what sends the lookup to the listings.
        let there = |relative: &str| crate::protocol::resolve_under(mount, relative).filter(|path| plainly_under(mount, path));
        there(relative).or_else(|| {
            let found = device::StickFiles::new(mount).locate(relative);
            let within = found.strip_prefix(mount).ok()?.to_str()?.to_owned();
            there(&within)
        })
    }
}

/// Whether `path` is there below `mount` with no link on the way down.
///
/// A stick is not the user's own data: HFS+, APFS and ext4 sticks can hold
/// symbolic links, and one named where the stick's library expects artwork,
/// a `.DAT` or a track could otherwise hand any file the app can read to the
/// webview, or have Import Playlist copy it into the Music folder. Each part
/// is looked at with `symlink_metadata`, as the backups do, rather than by
/// resolving the whole path: asking a volume for a file's final path fails
/// on some Windows volumes (#271). On Windows a junction counts as a link
/// too.
// perf-ok: one `lstat` per part (four or five for a stick file), only for a
// stick's own track.
fn plainly_under(mount: &Path, path: &Path) -> bool {
    let Ok(below) = path.strip_prefix(mount) else { return false };
    let mut at = mount.to_path_buf();
    let mut any = false;
    for part in below.components() {
        let std::path::Component::Normal(part) = part else { return false };
        at.push(part);
        any = true;
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if !meta.file_type().is_symlink() => {}
            _ => return false,
        }
    }
    any
}

/// The file a deck, the preview or the waveform should open for `track`,
/// given the path its id names. That path is the stick's library spelling
/// joined to the mount; a stick track whose file is not there under that
/// spelling, as on a case-sensitive Linux mount (#334), is looked up as the
/// stick's own file system would find it. Anything else is left as it is,
/// so its own missing-file handling still applies.
pub(crate) fn openable(state: &AppState, track: &str, path: PathBuf) -> PathBuf {
    let Some(assets) = state.device_assets(track) else { return path };
    // perf-ok: one `stat` of the file about to be opened, as rekordbox's own
    // load does; the listings only when that misses.
    if path.exists() {
        return path;
    }
    assets.audio_file().unwrap_or(path)
}

const fn format_name(format: Format) -> &'static str {
    match format {
        Format::DeviceLibrary => "deviceLibrary",
        Format::OneLibrary => "oneLibrary",
    }
}

fn format_from(name: &str) -> AppResult<Format> {
    match name {
        "deviceLibrary" => Ok(Format::DeviceLibrary),
        "oneLibrary" => Ok(Format::OneLibrary),
        other => Err(AppError::new(ErrorKind::Malformed, "That is not a library on the device.").with_detail(format!("format {other:?}"))),
    }
}

fn connected(path: &str) -> AppResult<PathBuf> {
    let mount = PathBuf::from(path);
    if !mount.is_dir() {
        return Err(AppError::new(ErrorKind::NotFound, "That device is no longer connected."));
    }
    Ok(mount)
}

/// What a failed read or write of the stick says. A refused edit is the
/// user's to fix, not a bug.
fn device_error(e: &rbl_export::ExportError) -> AppError {
    match e {
        rbl_export::ExportError::Conflict(message) => AppError::new(ErrorKind::Malformed, message.clone()),
        rbl_export::ExportError::DeviceGone => AppError::new(ErrorKind::NotFound, "That device is no longer connected."),
        other => AppError::new(ErrorKind::Internal, "The device library could not be read or written.").with_detail(other.to_string()),
    }
}

fn id_from(text: &str) -> AppResult<u32> {
    text.parse().map_err(|_| AppError::new(ErrorKind::Malformed, "That playlist is no longer on the device.").with_detail(format!("id {text:?}")))
}

/// The libraries on the stick at `path`, Device Library first. A stick with
/// neither answers with none.
#[tauri::command]
pub async fn device_libraries(path: String) -> AppResult<Vec<DeviceLibraryDto>> {
    blocking("device_libraries", move || {
        let mount = connected(&path)?;
        device::formats(&mount)
            .into_iter()
            .map(|format| {
                let library = device::read(&mount, format).map_err(|e| device_error(&e))?;
                Ok(library_dto(&library))
            })
            .collect()
    })
    .await
}

fn library_dto(library: &device::Library) -> DeviceLibraryDto {
    let count = |n: &device::Node| -> u32 {
        let count = if n.folder { library.nodes.iter().filter(|c| c.parent == n.id).count() } else { n.tracks.len() };
        u32::try_from(count).unwrap_or(u32::MAX)
    };
    DeviceLibraryDto {
        format: format_name(library.format),
        tracks: u32::try_from(library.tracks.len()).unwrap_or(u32::MAX),
        nodes: library
            .nodes
            .iter()
            .map(|n| DeviceNodeDto {
                id: n.id.to_string(),
                parent_id: n.parent.to_string(),
                name: n.name.clone(),
                folder: n.folder,
                depth: u32::try_from(library.depth(n.id)).unwrap_or(0),
                count: count(n),
            })
            .collect(),
    }
}

/// Applies one edit to one library on the stick at `path`.
#[tauri::command]
pub async fn device_playlist_edit(path: String, format: String, edit: DeviceEditDto) -> AppResult<DeviceEditResultDto> {
    blocking("device_playlist_edit", move || apply_edit(&path, &format, edit, rbl_db::is_rekordbox_app_running())).await
}

/// Maps collection ids to their rows in one device library. Missing rows are
/// `None`; callers can export those tracks before asking for the mapping again.
#[tauri::command]
pub async fn device_playlist_collection_tracks(
    state: State<'_, Arc<AppState>>,
    path: String,
    format: String,
    tracks: Vec<String>,
) -> AppResult<Vec<Option<String>>> {
    let state = Arc::clone(&state);
    blocking("device_playlist_collection_tracks", move || {
        let mount = connected(&path)?;
        let format = format_from(&format)?;
        let library = device::read(&mount, format).map_err(|e| device_error(&e))?;
        let db_id = state
            .read_db(|db| rbl_db::export_info::db_id(db.connection()))
            .map_err(crate::commands::write_error)?;
        Ok(collection_track_rows(&library, db_id, &tracks))
    })
    .await
}

fn collection_track_rows(library: &device::Library, db_id: u64, tracks: &[String]) -> Vec<Option<String>> {
    let mut rows = std::collections::HashMap::new();
    for track in &library.tracks {
        if track.master_db_id == db_id && track.master_content_id != 0 {
            rows.entry(track.master_content_id).or_insert_with(|| track.id.to_string());
        }
    }
    tracks.iter().map(|id| id.parse::<u64>().ok().and_then(|id| rows.get(&id).cloned())).collect()
}

/// The rows of `tracks` whose track `playlist` already holds, each once, so
/// the interface can ask rekordbox's Add or Skip question before adding.
/// Only reads the stick.
#[tauri::command]
pub async fn device_playlist_duplicates(path: String, format: String, playlist: String, tracks: Vec<String>) -> AppResult<Vec<String>> {
    blocking("device_playlist_duplicates", move || duplicates_of(&path, &format, &playlist, &tracks)).await
}

fn duplicates_of(path: &str, format: &str, playlist: &str, rows: &[String]) -> AppResult<Vec<String>> {
    let mount = connected(path)?;
    let format = format_from(format)?;
    let library = device::read(&mount, format).map_err(|e| device_error(&e))?;
    let playlist_id = id_from(playlist)?;
    if library.node(playlist_id).is_none_or(|node| node.folder) {
        return Err(AppError::new(ErrorKind::NotFound, "That playlist is no longer on the device."));
    }
    let ids = track_ids_in(&library, &mount, rows)?;
    let held: std::collections::BTreeSet<u32> = library.held(playlist_id, &ids).into_iter().collect();
    let mut named = std::collections::BTreeSet::new();
    Ok(rows.iter().zip(&ids).filter(|(_, id)| held.contains(id) && named.insert(**id)).map(|(row, _)| row.clone()).collect())
}

/// Why an edit to a stick must be refused, if it must.
///
/// rekordbox holds a connected stick's databases open and writes them on its
/// own (a Devices tree edit, a sync, an export), so this refuses whenever it
/// runs, as the USB export does. The interface greys the edits too, but from
/// a snapshot that can be out of date, and the unsafe-writes override that
/// lets the user's own library be written clears it; neither may decide for
/// a stick. Kept apart from [`apply_edit`] so the rule is tested without a
/// running rekordbox.
pub fn edit_refusal(rekordbox_running: bool) -> Option<AppError> {
    rekordbox_running.then(|| AppError::new(ErrorKind::ReadOnly, "Quit rekordbox before changing this USB's playlists so only one application writes its libraries."))
}

fn apply_edit(path: &str, format: &str, edit: DeviceEditDto, rekordbox_running: bool) -> AppResult<DeviceEditResultDto> {
    if let Some(refused) = edit_refusal(rekordbox_running) {
        return Err(refused);
    }
    let mount = connected(path)?;
    let format = format_from(format)?;
    let edit = match edit {
        DeviceEditDto::Create { parent, name, folder } => Edit::Create { parent: id_from(&parent)?, name, folder },
        DeviceEditDto::Rename { id, name } => Edit::Rename { id: id_from(&id)?, name },
        DeviceEditDto::Delete { id } => Edit::Delete { id: id_from(&id)? },
        DeviceEditDto::Add { playlist, tracks, duplicates } => {
            Edit::Add { playlist: id_from(&playlist)?, tracks: track_ids(&mount, format, &tracks)?, duplicates }
        }
        DeviceEditDto::Remove { playlist, tracks } => Edit::Remove { playlist: id_from(&playlist)?, tracks: track_ids(&mount, format, &tracks)? },
    };
    let applied = device::apply(&mount, format, &edit).map_err(|e| device_error(&e))?;
    tracing::info!(format = format_name(format), changed = applied.changed, "device playlist edit");
    Ok(DeviceEditResultDto { id: applied.id.to_string(), changed: u32::try_from(applied.changed).unwrap_or(u32::MAX) })
}

/// What Import Playlist does with a stick track the collection does not
/// have: the user's choice in Preferences, or the answer to the question
/// rekordbox asks (`importFromDeviceCopySettingWindow`, OK or Cancel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MissingTracks {
    /// Copy it to `PioneerDJ/Imported from Device` and import it, as
    /// rekordbox's OK does.
    Copy,
    /// Leave it out and import the rest.
    Matched,
    /// Import nothing, as rekordbox's Cancel does.
    Cancel,
}

/// What Import Playlist did with one of a stick's playlists.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceImportResultDto {
    /// The collection playlist made; `None` when nothing was imported.
    pub id: Option<String>,
    /// The name it was given, or the device's name when nothing was made.
    pub name: String,
    /// The name differs from the device's because the top level already
    /// had one by that name.
    pub renamed: bool,
    /// Entries added, a track listed twice counted twice.
    pub tracks: u32,
    /// Entries whose track the collection did not have.
    pub missing: u32,
    /// Files copied to the computer and added to the collection.
    pub copied: u32,
    /// Of those, the tracks given the device's analysis (BPM, key, the
    /// analysis files and cues) and the ones it could not be given to.
    pub analysed: u32,
    pub analysis_failed: u32,
    /// Nothing was imported because a track was missing and the choice was
    /// to cancel.
    pub cancelled: bool,
}

/// What Import Playlist would do with one of a stick's playlists, for the
/// question rekordbox asks before it copies. Only reads.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceImportPlanDto {
    pub name: String,
    /// Entries the stick lists.
    pub listed: u32,
    /// Entries whose track the collection does not have.
    pub missing: u32,
    /// Files to copy for them, each once, and their size.
    pub files: u32,
    pub bytes: u64,
    /// Where they would be copied.
    pub destination: String,
    /// Free space there, when it can be read.
    pub free_bytes: Option<u64>,
}

/// The plan for Import Playlist over a stick's playlist, read-only: how many
/// of its tracks the collection lacks and what copying them takes, for the
/// question rekordbox asks (`import_from_d_ask_file_copy`, 7.2.19
/// @0x1001dc94c, counts the files and sums their sizes for
/// `showCopySetting` @0x1001d860c).
#[tauri::command]
pub async fn device_playlist_import_plan(
    state: State<'_, Arc<AppState>>,
    path: String,
    format: String,
    id: String,
    destination: Option<String>,
) -> AppResult<DeviceImportPlanDto> {
    let reading = Arc::clone(&state);
    blocking("device_playlist_import_plan", move || {
        let plan = read_plan(&reading, &path, &format, &id)?;
        let destination = copy_destination(destination.as_deref())?;
        let copies = plan.copies();
        Ok(DeviceImportPlanDto {
            name: plan.name.clone(),
            listed: count(plan.entries.len()),
            missing: count(plan.missing()),
            files: count(copies.len()),
            bytes: copies.iter().map(|c| c.bytes).sum(),
            free_bytes: rbl_export::device_import::free_space(&destination).ok(),
            destination: destination.to_string_lossy().into_owned(),
        })
    })
    .await
}

/// Import Playlist over a stick's playlist: a playlist at the end of the
/// collection's top level holding the stick playlist's tracks, as
/// rekordbox's Devices tree does (see
/// [`rbl_db::write::Writer::import_device_playlist_entries`]).
///
/// A track the collection does not have is handled as `missing` says.
/// rekordbox asks, and on OK copies each such file to
/// `<Music>/PioneerDJ/Imported from Device` and imports it with the rest
/// [static, rekordbox 7.2.19: `importFromDeviceManager::start` @0x1001dc470,
/// `import_from_d_ask_file_copy` @0x1001dc94c, `importFromDeviceThread::
/// import_from_d_file_copy` @0x1001daa68]; on Cancel it imports nothing.
/// Its Change… picks another folder for the copies
/// (`importFromDeviceCopySettingWindow::buttonClicked` @0x1001d802c,
/// `FileChooser::browseForDirectory`): `destination`, when given.
///
/// Each track copied is given what the stick knows of it: its BPM and key,
/// the stick's analysis files (waveforms, beat grid, cues) and its cues,
/// as rekordbox's import thread does for each track it copies [static,
/// rekordbox 7.2.19 `importFromDeviceThread::run` @0x1001d9618] (see
/// [`carry_stick_analysis`]). Tracks the collection already had are left
/// as they are, as rekordbox leaves them.
///
/// The stick is only read. The copies are made first, each checked against
/// the stick's file and never over an existing file (see
/// [`rbl_export::device_import`]); then the new tracks and the playlist are
/// written in one transaction. If that write fails, the copies this import
/// made are removed again.
#[tauri::command]
pub async fn device_playlist_import<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    path: String,
    format: String,
    id: String,
    missing: MissingTracks,
    destination: Option<String>,
) -> AppResult<DeviceImportResultDto> {
    let reading = Arc::clone(&state);
    let plan = blocking("device_playlist_import", move || read_plan(&reading, &path, &format, &id)).await?;
    let none = |cancelled: bool| DeviceImportResultDto {
        id: None,
        name: plan.name.clone(),
        renamed: false,
        tracks: 0,
        missing: count(plan.missing()),
        copied: 0,
        analysed: 0,
        analysis_failed: 0,
        cancelled,
    };
    let to_copy = plan.copies();
    let (entries, copies, carried) = match missing {
        _ if to_copy.is_empty() => (plan.matched_entries(), None, Vec::new()),
        MissingTracks::Cancel => return Ok(none(true)),
        MissingTracks::Matched => {
            let entries = plan.matched_entries();
            if entries.is_empty() {
                return Ok(none(false));
            }
            (entries, None, Vec::new())
        }
        MissingTracks::Copy => {
            let location = state.location()?;
            if location.is_real_install && rbl_db::is_rekordbox_running() {
                return Err(AppError::new(ErrorKind::ReadOnly, "rekordbox is running. Quit it before making changes."));
            }
            let (entries, copies, carried) = blocking("device_playlist_import_copy", {
                let plan = plan.clone();
                move || copy_missing(&plan, &to_copy, &copy_destination(destination.as_deref())?)
            })
            .await?;
            (entries, Some(copies), carried)
        }
    };

    let made: Arc<parking_lot::Mutex<Option<rbl_db::write::ImportedPlaylist>>> = Arc::default();
    let slot = Arc::clone(&made);
    let name = plan.name.clone();
    let written = crate::commands::edit(app.clone(), state.clone(), "device_playlist_import", crate::commands::Touched::Playlists, move |w| {
        *slot.lock() = Some(w.import_device_playlist_entries(&name, &entries)?);
        Ok(())
    })
    .await;
    let made = written.and_then(|_| {
        made.lock().take().ok_or_else(|| AppError::new(ErrorKind::Internal, "The playlist could not be imported."))
    });
    let made = match made {
        Ok(made) => made,
        Err(error) => {
            if let Some(copies) = copies {
                let rolled_back = blocking("device_playlist_import_roll_back", move || {
                    copies.roll_back().map_err(|e| AppError::new(ErrorKind::Internal, "The music files could not be copied.").with_detail(e.to_string()))
                })
                .await;
                if let Err(left) = rolled_back {
                    tracing::warn!(error = %left, "could not remove the copies of a failed device import");
                }
            }
            return Err(error);
        }
    };
    let (mut analysed, mut analysis_failed) = (0_usize, 0_usize);
    if !made.added.is_empty() {
        let added = made.added.clone();
        let mount = plan.mount.clone();
        let writing = Arc::clone(&state);
        (analysed, analysis_failed) = blocking("device_playlist_import_analysis", move || {
            Ok(carry_all(&writing, &mount, &added, &carried))
        })
        .await?;
        // New tracks: the collection changed as well as the tree.
        crate::commands::reload(app, Arc::clone(&state)).await?;
    }
    tracing::info!(tracks = made.tracks, missing = plan.missing(), copied = made.added.len(), analysed, analysis_failed, "imported a device playlist");
    Ok(DeviceImportResultDto {
        id: Some(made.id),
        renamed: made.name != plan.name,
        name: made.name,
        tracks: count(made.tracks),
        missing: count(plan.missing()),
        copied: count(made.added.len()),
        analysed: count(analysed),
        analysis_failed: count(analysis_failed),
        cancelled: false,
    })
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `<Music>/PioneerDJ/Imported from Device`.
fn music_destination() -> AppResult<PathBuf> {
    dirs::audio_dir()
        .map(|music| rbl_export::device_import::destination(&music))
        .ok_or_else(|| AppError::new(ErrorKind::NotFound, "Your Music folder could not be found."))
}

/// Where the copies go: the folder picked with Change…, or
/// `<Music>/PioneerDJ/Imported from Device`. rekordbox only takes a folder
/// that exists (`buttonClicked` @0x1001d8194 checks `File::exists` on the
/// chooser's result), so a picked folder must be a folder already there.
fn copy_destination(chosen: Option<&str>) -> AppResult<PathBuf> {
    match chosen.map(str::trim).filter(|c| !c.is_empty()) {
        None => music_destination(),
        Some(chosen) => {
            let path = PathBuf::from(chosen);
            if path.is_absolute() && path.is_dir() {
                Ok(path)
            } else {
                Err(AppError::new(ErrorKind::NotFound, "That folder could not be found.").with_detail(chosen.to_owned()))
            }
        }
    }
}

/// Reads the stick and plans the import against the collection.
fn read_plan(state: &AppState, path: &str, format: &str, id: &str) -> AppResult<ImportPlan> {
    let mount = connected(path)?;
    let (format, id) = (format_from(format)?, id_from(id)?);
    let stick = StickTracks::read(&mount, format)?;
    let Some(node) = stick.library.node(id).filter(|n| !n.folder) else {
        return Err(AppError::new(ErrorKind::NotFound, "That playlist is no longer on the device."));
    };
    state.read_db(|db| plan_import(db, &stick, node)).map_err(crate::commands::write_error)
}

/// Copies every file in `to_copy` under `destination` (`<Music>/PioneerDJ/
/// Imported from Device`), and gives the plan's entries with each missing track replaced by
/// its copy, and each copy with the stick file it came from. On any failure
/// the copies made so far are removed.
fn copy_missing(
    plan: &ImportPlan,
    to_copy: &[MissingFile],
    destination: &Path,
) -> AppResult<CopiedEntries> {
    let bytes: u64 = to_copy.iter().map(|c| c.bytes).sum();
    if rbl_export::device_import::free_space(destination).is_ok_and(|free| free < bytes) {
        return Err(AppError::new(ErrorKind::ReadOnly, "There is not enough free space to copy the music files."));
    }
    let mut copies = rbl_export::device_import::Copies::new();
    let mut copy_of: std::collections::HashMap<PathBuf, PathBuf> = std::collections::HashMap::new();
    for file in to_copy {
        // Only ever a file of the stick: a library path that climbs out of
        // it (`..`), or a link on the stick that leads elsewhere, is not
        // copied. Walked part by part rather than canonicalized, which fails
        // on WinFsp-style mounts.
        if !plainly_under(&plan.mount, &file.source) {
            if let Err(left) = copies.roll_back() {
                tracing::warn!(error = %left, "could not remove the copies of a failed device import");
            }
            return Err(AppError::new(ErrorKind::NotFound, "The music files could not be copied.")
                .with_detail(format!("{} is not a file on the device", file.source.display())));
        }
        let wanted = rbl_export::device_import::candidate(destination, &file.artist, &file.album, &file.name);
        // A copy an earlier import made of this very file is used again,
        // rather than a second copy and a second collection track.
        let earlier = rbl_export::device_import::Copies::existing_copy(&file.source, &wanted).ok().flatten();
        if let Some(at) = earlier {
            copy_of.insert(file.source.clone(), at);
            continue;
        }
        match copies.copy(&file.source, &wanted) {
            Ok(at) => {
                copy_of.insert(file.source.clone(), at);
            }
            Err(error) => {
                let detail = format!("{}: {error}", file.source.display());
                if let Err(left) = copies.roll_back() {
                    tracing::warn!(error = %left, "could not remove the copies of a failed device import");
                }
                return Err(AppError::new(ErrorKind::Internal, "The music files could not be copied.").with_detail(detail));
            }
        }
    }
    let entries = plan
        .entries
        .iter()
        .filter_map(|entry| match entry {
            PlanEntry::Matched(content) => Some(rbl_db::write::DeviceEntry::Track(content.clone())),
            PlanEntry::Missing(file) => copy_of.get(&file.source).cloned().map(rbl_db::write::DeviceEntry::File),
        })
        .collect();
    let carried = to_copy.iter().filter_map(|file| copy_of.get(&file.source).map(|copy| (copy.clone(), file.clone()))).collect();
    Ok((entries, copies, carried))
}

/// [`carry_stick_analysis`] for every track an import added from a copy,
/// `added` as the writer gives it and `carried` as [`copy_missing`] does.
/// Says how many were given the stick's analysis and how many could not be;
/// one that fails is left as the bare file the import added.
fn carry_all(state: &AppState, mount: &Path, added: &[(String, PathBuf)], carried: &[(PathBuf, MissingFile)]) -> (usize, usize) {
    let mut counts = (0, 0);
    for (content, file) in added {
        let Some((_, stick)) = carried.iter().find(|(copy, _)| copy == file) else { continue };
        match carry_stick_analysis(state, mount, content, file, stick) {
            Ok(()) => counts.0 += 1,
            Err(error) => {
                counts.1 += 1;
                tracing::warn!(track = %content, error = %error, "could not give an imported device track its analysis");
            }
        }
    }
    counts
}

/// Gives `content`, a track Import Playlist just added from `copy` (a copy
/// of `stick`'s file), what the stick knows of it, as rekordbox does for
/// each track it copies [static, rekordbox 7.2.19 macOS arm64,
/// `importFromDeviceThread::run` @0x1001d9618]: `addTrackFromDevice` adds
/// the track from the device's row (its BPM and key), and
/// `import_device_analysis` @0x1001db460 copies the stick's `.DAT`, `.EXT`
/// and `.2EX` into the share tree and gives them the new file's path
/// (`CAnalyzerIF::initPathAndDeviceCue`), with the device's cues
/// (`getDeviceContentCueInfo`).
///
/// The files go where rekordbox keeps a track's analysis (the UUID-derived
/// path, see [`rbl_db::write::analysis_data_path`]), each `PPTH` the share
/// tree's `?/<file name>` ([`rbl_anlz::share_tree_ppth`]), every other
/// section byte for byte as on the stick. They and the row are one
/// journalled edit ([`crate::file_journal`]), so a failure or a crash
/// leaves neither. The cues are the `.EXT`'s extended list, which carries
/// their colours and comments [ASSUME: the stick's analysis cues are the
/// ones its database lists; an export writes both from the same cues].
/// The stick is only read, and only below its mount point.
fn carry_stick_analysis(state: &AppState, mount: &Path, content: &str, copy: &Path, stick: &MissingFile) -> AppResult<()> {
    let ppth = rbl_anlz::share_tree_ppth(&copy.to_string_lossy());
    let read = |relative: &Path| -> AppResult<Option<rbl_anlz::Anlz>> {
        let at = audio_path(mount, &relative.to_string_lossy());
        let at = if at.exists() { at } else { device::StickFiles::new(mount).locate(&relative.to_string_lossy()) };
        if !at.is_file() || !plainly_under(mount, &at) {
            return Ok(None);
        }
        rbl_anlz::Anlz::read(&at)
            .map(Some)
            .map_err(|e| AppError::new(ErrorKind::Malformed, "The device's analysis could not be read.").with_detail(format!("{}: {e}", at.display())))
    };
    let dat_relative = Path::new(&stick.analysis);
    let dat = if stick.analysis.trim().is_empty() { None } else { read(dat_relative)? };

    let _gate = state.edit_gate.lock();
    let _files = state.analysis_write.lock();
    let location = state.location()?;
    crate::file_journal::recover(state.backup_dir(), &location)?;
    let key = Some(stick.key.as_str()).filter(|k| !k.is_empty());
    let Some(dat) = dat else {
        // Nothing analysed on the stick: the row's BPM and key only.
        let analysis = rbl_db::write::DeviceAnalysis { bpm_x100: stick.bpm_x100, key, analysis_data_path: None, cues: &[] };
        state.write(|w| w.import_device_analysis(content, &analysis)).map_err(crate::commands::write_error)?;
        return Ok(());
    };
    let ext = read(&dat_relative.with_extension("EXT"))?;
    let two_ex = read(&dat_relative.with_extension("2EX"))?;
    let cues = ext.as_ref().map(rbl_anlz::Anlz::cue_entries).unwrap_or_default();

    let relative = state.write(|w| w.analysis_data_path_for(content)).map_err(crate::commands::write_error)?;
    let target = rbl_anlz::resolve(&location.share_root, &relative);
    let mut files = vec![(target.clone(), dat.with_path(&ppth))];
    for (extension, anlz) in [("EXT", &ext), ("2EX", &two_ex)] {
        if let Some(anlz) = anlz {
            files.push((rbl_anlz::sibling(&target, extension), anlz.with_path(&ppth)));
        }
    }
    let bpm_after = if stick.bpm_x100 > 0 {
        stick.bpm_x100
    } else {
        state
            .read_db(|db| Ok(db.connection().query_row("SELECT COALESCE(BPM,0) FROM djmdContent WHERE ID=?1", [content], |r| r.get::<_, u32>(0))?))
            .map_err(crate::commands::write_error)?
    };
    let journal = crate::file_journal::FileJournal::prepare(state.backup_dir(), &location, content, bpm_after, Some(relative.clone()), true, &files)?;
    if let Err(e) = journal.publish() {
        journal.rollback()?;
        return Err(e);
    }
    let analysis = rbl_db::write::DeviceAnalysis { bpm_x100: stick.bpm_x100, key, analysis_data_path: Some(&relative), cues: &cues };
    if let Err(e) = state.write(|w| w.import_device_analysis(content, &analysis)) {
        journal.reconcile(&location)?;
        return Err(crate::commands::write_error(e));
    }
    journal.commit()
}

/// What [`copy_missing`] gives: the entries to write, the copies it made,
/// and each copy with the stick file it came from.
type CopiedEntries = (Vec<rbl_db::write::DeviceEntry>, rbl_export::device_import::Copies, Vec<(PathBuf, MissingFile)>);

/// One library of a stick, with what tells its tracks apart from the
/// collection's.
struct StickTracks {
    mount: PathBuf,
    library: device::Library,
    /// For a Device Library track whose row does not say which desktop
    /// track it is (one written before rows carried it): the stick's
    /// `exportLibrary.db` rows by file, which do.
    masters_by_path: std::collections::HashMap<String, (u64, u64)>,
}

impl StickTracks {
    fn read(mount: &Path, format: Format) -> AppResult<Self> {
        let library = device::read(mount, format).map_err(|e| device_error(&e))?;
        let mut masters_by_path = std::collections::HashMap::new();
        if format == Format::DeviceLibrary && device::formats(mount).contains(&Format::OneLibrary) {
            let one = device::read(mount, Format::OneLibrary).map_err(|e| device_error(&e))?;
            masters_by_path = one.tracks.iter().map(|t| (t.path.clone(), (t.master_db_id, t.master_content_id))).collect();
        }
        Ok(Self { mount: mount.to_path_buf(), library, masters_by_path })
    }
}

/// A stick file to copy for a track the collection does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MissingFile {
    /// The file on the stick.
    source: PathBuf,
    /// Its file name, artist and album, for where the copy goes.
    name: String,
    artist: String,
    album: String,
    bytes: u64,
    /// What the stick knows of the track, for the collection track the copy
    /// becomes: its BPM x100, its key by name, and its analysis `.DAT`,
    /// volume-relative (empty for none).
    bpm_x100: u32,
    key: String,
    analysis: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlanEntry {
    /// The collection's track.
    Matched(String),
    Missing(MissingFile),
}

/// What Import Playlist will write for one of a stick's playlists.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportPlan {
    /// Where the stick is mounted.
    mount: PathBuf,
    /// The playlist's name on the stick.
    name: String,
    /// Its entries, in the stick's order.
    entries: Vec<PlanEntry>,
}

impl ImportPlan {
    /// The collection's ids for the entries it has, in the stick's order.
    fn contents(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                PlanEntry::Matched(content) => Some(content.clone()),
                PlanEntry::Missing(_) => None,
            })
            .collect()
    }

    fn matched_entries(&self) -> Vec<rbl_db::write::DeviceEntry> {
        self.contents().into_iter().map(rbl_db::write::DeviceEntry::Track).collect()
    }

    fn missing(&self) -> usize {
        self.entries.iter().filter(|e| matches!(e, PlanEntry::Missing(_))).count()
    }

    /// The files to copy, each once, in the order the playlist first
    /// lists them.
    fn copies(&self) -> Vec<MissingFile> {
        let mut seen = std::collections::HashSet::new();
        self.entries
            .iter()
            .filter_map(|e| match e {
                PlanEntry::Missing(file) if seen.insert(file.source.clone()) => Some(file.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Works out which collection track each entry of `node` is, in
/// rekordbox's order [static, rekordbox 7.2.19,
/// `importFromDeviceManager::start` @0x1001dc470; 7.2.11 macOS
/// `getMainTrackByDeviceTrack` @0x100464c00]: first a collection track at
/// the same file (`rekordboxDBController::isCollectionSong`, the file on the
/// stick), then the track the entry was exported from, when the stick names
/// this library (the row's `masterDbId` equal to this library's `DBID`,
/// then its `masterContentId`). Read-only.
///
/// A Device Library row written without those ids takes the ids of the
/// stick's `OneLibrary` row for the same file [ASSUME: both libraries of
/// one stick describe the same files].
fn plan_import(db: &rbl_db::Library, stick: &StickTracks, node: &device::Node) -> Result<ImportPlan, rbl_db::DbError> {
    let conn = db.connection();
    let db_id = rbl_db::export_info::db_id(conn)?;
    let live = |content: &str| -> Result<bool, rbl_db::DbError> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM djmdContent WHERE ID = ?1 AND rb_local_deleted = 0",
            [content],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    };
    let mut entries = Vec::with_capacity(node.tracks.len());
    for entry in &node.tracks {
        let Some(track) = stick.library.track(*entry) else { continue };
        let file = audio_path(&stick.mount, &track.path);
        let at_file: Option<String> = conn
            .prepare("SELECT ID FROM djmdContent WHERE FolderPath = ?1 AND rb_local_deleted = 0 ORDER BY ID LIMIT 1")?
            .query_map([file.to_string_lossy().as_ref()], |r| r.get(0))?
            .next()
            .transpose()?;
        if let Some(found) = at_file {
            entries.push(PlanEntry::Matched(found));
            continue;
        }
        let (master_db, master_content) = match (track.master_db_id, track.master_content_id) {
            (0, _) | (_, 0) => stick.masters_by_path.get(&track.path).copied().unwrap_or((0, 0)),
            ids => ids,
        };
        if db_id != 0 && master_db == db_id && master_content != 0 {
            let content = master_content.to_string();
            if live(&content)? {
                entries.push(PlanEntry::Matched(content));
                continue;
            }
        }
        // A file the stick's own file system finds under another spelling
        // (#334) is the one to copy.
        // perf-ok: a `stat` or two per missing track, for the size the
        // question shows; the stick's listings only when the spelling misses.
        let source = if file.exists() { file } else { device::StickFiles::new(&stick.mount).locate(&track.path) };
        // A link on the stick is never copied (see `copy_missing`), so its
        // target's size is not asked for either.
        let bytes = if plainly_under(&stick.mount, &source) { std::fs::metadata(&source).map_or(0, |m| m.len()) } else { 0 };
        let name = Path::new(&track.path.replace('\\', "/"))
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        entries.push(PlanEntry::Missing(MissingFile {
            source,
            name,
            artist: track.artist.clone(),
            album: track.album.clone(),
            bytes,
            bpm_x100: track.bpm_x100,
            key: track.key.clone(),
            analysis: track.analysis_path.clone(),
        }));
    }
    Ok(ImportPlan { mount: stick.mount.clone(), name: node.name.clone(), entries })
}

/// Where a track of the stick plays from: its mount point and the library's
/// volume-relative path. The row id and the edit both come from here, so
/// the two always agree.
fn audio_path(mount: &Path, relative: &str) -> PathBuf {
    mount.join(relative.trim_start_matches(['/', '\\']))
}

/// The stick's track ids behind rows' `file:` ids, in the rows' order.
/// A row that is not a track of this library is refused.
fn track_ids(mount: &Path, format: Format, rows: &[String]) -> AppResult<Vec<u32>> {
    let library = device::read(mount, format).map_err(|e| device_error(&e))?;
    track_ids_in(&library, mount, rows)
}

fn track_ids_in(library: &device::Library, mount: &Path, rows: &[String]) -> AppResult<Vec<u32>> {
    let by_path: std::collections::HashMap<PathBuf, u32> =
        library.tracks.iter().map(|t| (audio_path(mount, &t.path), t.id)).collect();
    rows.iter()
        .map(|row| {
            row.strip_prefix(rbl_index::folder::LOOSE_PREFIX)
                .and_then(|path| by_path.get(Path::new(path)).copied())
                .ok_or_else(|| {
                    AppError::new(ErrorKind::Malformed, "Only tracks of this library on the device can go into its playlists.")
                        .with_detail(format!("row {row:?}"))
                })
        })
        .collect()
}

/// Opens a library on a stick as a view: one playlist's tracks in its
/// order, or with playlist `0`, every track.
pub async fn open_view(state: &State<'_, Arc<AppState>>, path: String, format: String, playlist: String, spec: &ViewSpecDto) -> AppResult<ViewHandleDto> {
    let handle = Arc::clone(state);
    let parsed = rbl_index::ViewSpec {
        source: rbl_index::TrackSource::Collection,
        sort: sort_from_wire(&spec.sort),
        descending: spec.descending,
        query: spec.query.clone(),
        filter: rbl_index::TrackFilter::default(),
    };
    let field = spec.search_field;
    blocking("open_view", move || {
        let mount = connected(&path)?;
        let format = format_from(&format)?;
        let playlist = id_from(&playlist)?;
        let library = device::read(&mount, format).map_err(|e| device_error(&e))?;
        let ids: Vec<u32> = if playlist == 0 {
            library.tracks.iter().map(|t| t.id).collect()
        } else {
            library.node(playlist).map(|n| n.tracks.clone()).unwrap_or_default()
        };
        let rows: Vec<DeviceRow> = ids
            .iter()
            .enumerate()
            .filter_map(|(at, id)| library.track(*id).map(|t| device_row(&mount, t, at)))
            .collect();
        handle.remember_device_assets(&mount, library.tracks.iter().map(|t| assets_of(&mount, t)));
        let view = DeviceView::open(rows, &parsed, field);
        let (view_id, len, generation) = handle.open_device_view(view);
        Ok(ViewHandleDto { view_id, len, gen: generation, totals: None })
    })
    .await
}

fn device_row(mount: &Path, track: &device::Track, at: usize) -> DeviceRow {
    DeviceRow {
        id: track.id,
        position: u32::try_from(at + 1).unwrap_or(u32::MAX),
        path: audio_path(mount, &track.path),
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        genre: track.genre.clone(),
        label: track.label.clone(),
        key: track.key.clone(),
        comment: track.comment.clone(),
        date_added: track.date_added.clone(),
        bpm_x100: track.bpm_x100,
        duration_sec: track.duration_sec,
        rating: track.rating,
        color: track.color,
        analysed: !track.analysis_path.is_empty(),
        has_artwork: !track.artwork_path.is_empty(),
    }
}

/// A stick track's row id with what its library names beside the audio.
fn assets_of(mount: &Path, track: &device::Track) -> (String, DeviceAssets) {
    (
        rbl_index::folder::loose_id(&audio_path(mount, &track.path)),
        DeviceAssets {
            mount: mount.to_path_buf(),
            audio: track.path.clone(),
            analysis: track.analysis_path.clone(),
            artwork: track.artwork_path.clone(),
        },
    )
}

/// A window of a device view's rows. Each is a track the deck can play from
/// its file, as an Explorer file is.
pub fn fetch_rows(view: &DeviceView, offset: u32, len: u32) -> AppResult<Vec<RowDto>> {
    if len > MAX_ROWS {
        return Err(AppError::new(ErrorKind::Malformed, "Too many rows requested at once.")
            .with_detail(format!("len {len} exceeds the {MAX_ROWS}-row cap")));
    }
    Ok(view.window(offset as usize, len as usize).iter().map(row_dto).collect())
}

/// The row ids between two positions of a device view, for a shift-click.
pub fn ids_in_range(view: &DeviceView, from: u32, to: u32) -> Vec<String> {
    let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
    let (lo, hi) = (lo as usize, (hi as usize).min(view.len().saturating_sub(1)));
    view.rows.get(lo..=hi).unwrap_or(&[]).iter().map(|r| rbl_index::folder::loose_id(&r.path)).collect()
}

fn row_dto(row: &DeviceRow) -> RowDto {
    RowDto {
        id: rbl_index::folder::loose_id(&row.path),
        track_no: row.position,
        title: row.title.clone(),
        artist: row.artist.clone(),
        album: row.album.clone(),
        genre: row.genre.clone(),
        label: row.label.clone(),
        comment: row.comment.clone(),
        bpm_x100: row.bpm_x100,
        key: row.key.clone(),
        duration_sec: row.duration_sec,
        rating: row.rating,
        analysed: u8::from(row.analysed),
        date_added: row.date_added.clone(),
        release_date: String::new(),
        hot_cues: Vec::new(),
        memory_cues: Vec::new(),
        memory_cue_colours: Vec::new(),
        artwork_hue: 0,
        has_artwork: row.has_artwork,
        file_name: row.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        extra: None,
        // A stick's own row: its file is on the stick, not a collection
        // file that rekordbox could mark missing.
        missing: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A stick written by the export pipeline: a folder with a list, and a
    /// list at the top level.
    fn stick() -> (tempfile::TempDir, tempfile::TempDir) {
        let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let tracks: Vec<rbl_export::SourceTrack> = (1..=3_u8)
            .map(|i| {
                let path = src.path().join(format!("t{i}.mp3"));
                std::fs::write(&path, vec![i; 512]).unwrap();
                rbl_export::SourceTrack { id: u64::from(i), source_path: path, title: format!("Track {i}"), artist: "TRIODE".into(), ..Default::default() }
            })
            .collect();
        let playlists = vec![
            rbl_export::SourcePlaylist { id: 10, name: "Sets".into(), folder: true, ..Default::default() },
            rbl_export::SourcePlaylist { id: 11, name: "Friday".into(), parent_id: 10, track_indices: vec![0, 1], ..Default::default() },
            rbl_export::SourcePlaylist { id: 12, name: "Warm Up".into(), track_indices: vec![2], ..Default::default() },
        ];
        rbl_export::export(dest.path(), &tracks, &playlists).unwrap();
        (src, dest)
    }

    fn libraries(mount: &Path) -> Vec<DeviceLibraryDto> {
        device::formats(mount).into_iter().map(|f| library_dto(&device::read(mount, f).unwrap())).collect()
    }

    #[test]
    fn a_stick_lists_both_libraries_with_their_trees() {
        let (_src, dest) = stick();
        let found = libraries(dest.path());
        assert_eq!(found.iter().map(|l| l.format).collect::<Vec<_>>(), ["deviceLibrary", "oneLibrary"]);
        let tree: Vec<(&str, u32, u32, bool)> = found[0].nodes.iter().map(|n| (n.name.as_str(), n.depth, n.count, n.folder)).collect();
        assert_eq!(tree, [("Sets", 0, 1, true), ("Friday", 1, 2, false), ("Warm Up", 0, 1, false)]);
        assert_eq!(found[0].tracks, 3);
    }

    #[test]
    fn rows_name_tracks_by_their_file_and_edits_find_them_again() {
        let (_src, dest) = stick();
        let mount = dest.path();
        let library = device::read(mount, Format::OneLibrary).unwrap();
        let rows: Vec<RowDto> = library.tracks.iter().enumerate().map(|(at, t)| row_dto(&device_row(mount, t, at))).collect();
        assert!(rows.iter().all(|r| r.id.starts_with("file:") && Path::new(&r.id["file:".len()..]).is_file()));
        let ids = track_ids(mount, Format::OneLibrary, &[rows[2].id.clone(), rows[0].id.clone()]).unwrap();
        assert_eq!(ids, vec![library.tracks[2].id, library.tracks[0].id]);
        assert!(track_ids(mount, Format::OneLibrary, &["file:/elsewhere/x.mp3".into()]).is_err());
        assert!(track_ids(mount, Format::OneLibrary, &["7".into()]).is_err(), "a library track id is not a device track");
    }

    #[test]
    fn collection_track_ids_map_only_to_rows_owned_by_this_library() {
        let (_src, dest) = stick();
        let mut library = device::read(dest.path(), Format::OneLibrary).unwrap();
        library.tracks[0].master_db_id = 77;
        library.tracks[0].master_content_id = 101;
        library.tracks[1].master_db_id = 77;
        library.tracks[1].master_content_id = 102;

        assert_eq!(collection_track_rows(&library, 77, &["102".into(), "999".into(), "not-an-id".into()]),
            [Some(library.tracks[1].id.to_string()), None, None]);
        assert_eq!(collection_track_rows(&library, 78, &["101".into()]), [None]);
    }

    #[test]
    fn an_edit_from_the_interface_changes_the_library_it_names() {
        let (_src, dest) = stick();
        let path = dest.path().to_str().unwrap();
        let made = apply_edit(path, "deviceLibrary", DeviceEditDto::Create { parent: "0".into(), name: "Opening".into(), folder: false }, false).unwrap();
        assert_eq!(made.changed, 1);
        let rows: Vec<String> = device::read(dest.path(), Format::DeviceLibrary)
            .unwrap()
            .tracks
            .iter()
            .map(|t| rbl_index::folder::loose_id(&audio_path(dest.path(), &t.path)))
            .collect();
        apply_edit(path, "deviceLibrary", DeviceEditDto::Add { playlist: made.id.clone(), tracks: rows, duplicates: false }, false).unwrap();
        let found = libraries(dest.path());
        let opening = found[0].nodes.iter().find(|n| n.name == "Opening").unwrap();
        assert_eq!(opening.count, 3);
        assert!(!found[1].nodes.iter().any(|n| n.name == "Opening"), "OneLibrary is left as it was");
        let refused = apply_edit(path, "deviceLibrary", DeviceEditDto::Rename { id: made.id, name: "   ".into() }, false).unwrap_err();
        assert!(matches!(refused.kind, ErrorKind::Malformed));
        assert!(apply_edit(path, "elsewhere", DeviceEditDto::Delete { id: "1".into() }, false).is_err());
    }

    #[test]
    fn the_interface_learns_which_rows_a_playlist_holds_and_adds_them_again_only_when_told() {
        let (_src, dest) = stick();
        let path = dest.path().to_str().unwrap();
        for format in ["deviceLibrary", "oneLibrary"] {
            let library = device::read(dest.path(), format_from(format).unwrap()).unwrap();
            let row = |id: u32| rbl_index::folder::loose_id(&audio_path(dest.path(), &library.track(id).unwrap().path));
            let friday = library.nodes.iter().find(|n| n.name == "Friday").unwrap();
            let (inside, outside) = (friday.tracks[0], library.tracks.iter().map(|t| t.id).find(|t| !friday.tracks.contains(t)).unwrap());
            let picked = vec![row(outside), row(inside), row(inside)];
            let held = duplicates_of(path, format, &friday.id.to_string(), &picked).unwrap();
            assert_eq!(held, vec![row(inside)], "{format}: each held row once, the new one not at all");

            let add = |duplicates| {
                apply_edit(path, format, DeviceEditDto::Add { playlist: friday.id.to_string(), tracks: vec![row(outside), row(inside)], duplicates }, false).unwrap()
            };
            assert_eq!(add(false).changed, 1, "{format}: Skip adds only the new track");
            assert_eq!(add(true).changed, 2, "{format}: Add puts both on the end again");
            let after = device::read(dest.path(), format_from(format).unwrap()).unwrap();
            let mut expected = friday.tracks.clone();
            expected.extend([outside, outside, inside]);
            assert_eq!(after.node(friday.id).unwrap().tracks, expected, "{format}");
        }
        // An add the interface sends without the answer skips, as before.
        let edit: DeviceEditDto = serde_json::from_str(r#"{"kind":"add","playlist":"1","tracks":[]}"#).unwrap();
        assert!(matches!(edit, DeviceEditDto::Add { duplicates: false, .. }));
        let edit: DeviceEditDto = serde_json::from_str(r#"{"kind":"add","playlist":"1","tracks":[],"duplicates":true}"#).unwrap();
        assert!(matches!(edit, DeviceEditDto::Add { duplicates: true, .. }));
        assert!(duplicates_of(path, "deviceLibrary", "1", &["file:/elsewhere/x.mp3".into()]).is_err());
        assert!(duplicates_of(path, "deviceLibrary", "999", &[]).is_err(), "a missing target is refused before an export starts");
    }

    #[test]
    fn an_edit_is_refused_while_rekordbox_runs_and_leaves_the_stick_alone() {
        assert!(edit_refusal(false).is_none());
        // The export's own refusal, in the same words but for the edit.
        let refusal = edit_refusal(true).unwrap();
        assert!(matches!(refusal.kind, ErrorKind::ReadOnly));
        assert_eq!(refusal.message, "Quit rekordbox before changing this USB's playlists so only one application writes its libraries.");
        let (_src, dest) = stick();
        let path = dest.path().to_str().unwrap();
        let db = dest.path().join("PIONEER/rekordbox");
        let files = || ["export.pdb", "exportLibrary.db"].map(|f| std::fs::read(db.join(f)).unwrap());
        let before = files();
        for format in ["deviceLibrary", "oneLibrary"] {
            let refused = apply_edit(path, format, DeviceEditDto::Create { parent: "0".into(), name: "Opening".into(), folder: false }, true).unwrap_err();
            assert!(matches!(refused.kind, ErrorKind::ReadOnly));
            assert_eq!(refused.message, refusal.message);
        }
        assert!(files() == before, "a refused edit writes nothing");
        assert!(!dest.path().join(".rbxport-publication").exists());
    }

    /// A collection, and a stick exported from it whose `OneLibrary` names it
    /// as rekordbox's export does (`masterDbId` its `DBID`,
    /// `masterContentId` the track's id). The stick's tracks 1-4 are the
    /// collection's first four, real WAV files so a copy can be imported.
    /// Then, as an XDJ-AZ saves a TAG LIST, a playlist the player wrote:
    /// `TAG LIST` with the fourth, the second and the fourth track again.
    struct Rig {
        dir: tempfile::TempDir,
        _src: tempfile::TempDir,
        stick: tempfile::TempDir,
        location: rbl_db::LibraryLocation,
    }

    const TAG_LIST: i64 = 9001;

    /// A minimal but genuine WAV, `samples` long, so the tag reader opens it.
    fn wav(samples: u32) -> Vec<u8> {
        let data_len = samples * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&44_100_u32.to_le_bytes());
        out.extend_from_slice(&88_200_u32.to_le_bytes());
        out.extend_from_slice(&2_u16.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(44 + data_len as usize, 0);
        out
    }

    fn rig() -> Rig {
        rig_with(|_, _| {})
    }

    /// [`rig`] with each stick track as `edit` leaves it before the export.
    fn rig_with(edit: impl Fn(usize, &mut rbl_export::SourceTrack)) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let (src, stick) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let tracks: Vec<rbl_export::SourceTrack> = (0..4_usize)
            .map(|i| {
                let path = src.path().join(format!("t{i}.wav"));
                std::fs::write(&path, wav(4410 * (u32::try_from(i).unwrap() + 1))).unwrap();
                let mut track = rbl_export::SourceTrack {
                    id: rbl_db::fixture::track_id(i).parse().unwrap(),
                    source_path: path,
                    title: format!("Track {i}"),
                    artist: "TRIODE".into(),
                    ..Default::default()
                };
                edit(i, &mut track);
                track
            })
            .collect();
        let playlists = vec![rbl_export::SourcePlaylist { id: 11, name: "Friday".into(), track_indices: vec![0, 1], ..Default::default() }];
        rbl_export::export(stick.path(), &tracks, &playlists).unwrap();
        let one = stick.path().join("PIONEER/rekordbox/exportLibrary.db");
        let conn = rbl_onelibrary::playlists::open_read_write(&one).unwrap();
        conn.execute("UPDATE content SET masterDbId = 1000000001", []).unwrap();
        drop(conn);
        let library = device::read(stick.path(), Format::OneLibrary).unwrap();
        let stick_id = |i: usize| -> i64 {
            let master: u64 = rbl_db::fixture::track_id(i).parse().unwrap();
            i64::from(library.tracks.iter().find(|t| t.master_content_id == master).unwrap().id)
        };
        let change = rbl_onelibrary::playlists::Change {
            create: vec![rbl_onelibrary::playlists::PlaylistRow { id: TAG_LIST, parent: 0, name: "TAG LIST".into(), attribute: 0, sequence: 5 }],
            contents: vec![(TAG_LIST, vec![stick_id(3), stick_id(1), stick_id(3)])],
            ..Default::default()
        };
        rbl_onelibrary::playlists::apply(&one, &change).unwrap();
        Rig { dir, _src: src, stick, location }
    }

    impl Rig {
        fn plan(&self, format: Format, name: &str) -> ImportPlan {
            let stick = StickTracks::read(self.stick.path(), format).unwrap();
            let node = stick.library.nodes.iter().find(|n| n.name == name).unwrap().clone();
            let db = rbl_db::Library::open(self.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
            plan_import(&db, &stick, &node).unwrap()
        }

        fn writer(&self) -> rbl_db::write::Writer {
            rbl_db::write::Writer::open(self.location.clone(), self.dir.path().join("backups")).unwrap()
        }

        /// Makes the stick say track `i` came from another library, so the
        /// collection does not have it.
        fn came_from_elsewhere(&self, i: usize) {
            let one = self.stick.path().join("PIONEER/rekordbox/exportLibrary.db");
            let conn = rbl_onelibrary::playlists::open_read_write(&one).unwrap();
            conn.execute("UPDATE content SET masterDbId = 42 WHERE masterContentId = ?1", [rbl_db::fixture::track_id(i)]).unwrap();
        }

        fn entries(&self, playlist: &str) -> Vec<String> {
            let db = rbl_db::Library::open(self.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
            let mut stmt = db
                .connection()
                .prepare("SELECT ContentID FROM djmdSongPlaylist WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo")
                .unwrap();
            stmt.query_map([playlist], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        }

        fn sticks_file(&self, i: usize) -> PathBuf {
            let library = device::read(self.stick.path(), Format::OneLibrary).unwrap();
            let master: u64 = rbl_db::fixture::track_id(i).parse().unwrap();
            audio_path(self.stick.path(), &library.tracks.iter().find(|t| t.master_content_id == master).unwrap().path)
        }
    }

    fn ids(indices: &[usize]) -> Vec<String> {
        indices.iter().map(|&i| rbl_db::fixture::track_id(i)).collect()
    }

    /// Every file under `dir`, relative to it.
    fn files_under(dir: &Path) -> Vec<String> {
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else { return };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else {
                    out.push(path.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn a_tag_list_the_player_saved_finds_its_tracks_in_the_collection() {
        let rig = rig();
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        assert_eq!((plan.name.as_str(), plan.contents(), plan.missing()), ("TAG LIST", ids(&[3, 1, 3]), 0));
        assert_eq!(plan.copies(), []);
        // And it lands as rekordbox's Import Playlist leaves it: at the end
        // of the top level (the fixture has Playlist 0-2 at Seq 0-2), in
        // the player's order, the repeated track twice.
        let made = rig.writer().import_device_playlist_entries(&plan.name, &plan.matched_entries()).unwrap();
        let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (seq, parent): (i64, String) = db
            .connection()
            .query_row("SELECT Seq, ParentID FROM djmdPlaylist WHERE ID = ?1", [&made.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((made.name.as_str(), seq, parent.as_str()), ("TAG LIST", 3, "root"));
        assert_eq!(rig.entries(&made.id), ids(&[3, 1, 3]));
    }

    #[test]
    fn the_device_library_finds_its_tracks_through_the_sticks_onelibrary() {
        // This export left export.pdb's MASTERDBID blank, so the stick's
        // OneLibrary row for the same file says where the track came from.
        let rig = rig();
        assert_eq!(rig.plan(Format::DeviceLibrary, "Friday").contents(), ids(&[0, 1]));
    }

    #[test]
    fn a_track_from_another_library_or_gone_from_this_one_is_missing() {
        let rig = rig();
        rig.came_from_elsewhere(1);
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        assert_eq!((plan.contents(), plan.missing()), (ids(&[3, 3]), 1));
        let copies = plan.copies();
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].source, rig.sticks_file(1));
        assert_eq!((copies[0].artist.as_str(), copies[0].bytes), ("TRIODE", 44 + 4410 * 2 * 2));
        rig.writer().delete_track(&rbl_db::fixture::track_id(3)).unwrap();
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        assert_eq!((plan.contents().len(), plan.entries.len(), plan.missing()), (0, 3, 3));
        // Track 3 is listed twice but copied once.
        assert_eq!(plan.copies().len(), 2);
    }

    #[test]
    fn a_collection_track_at_the_sticks_own_file_comes_first() {
        // rekordbox looks for the file before the exported-from id.
        let rig = rig();
        let file = rig.sticks_file(1);
        rbl_db::fixture::point_at_audio(&rig.location, 20, &file.to_string_lossy(), 300).unwrap();
        assert_eq!(rig.plan(Format::OneLibrary, "TAG LIST").contents(), ids(&[3, 20, 3]));
    }

    #[test]
    fn copying_the_missing_tracks_imports_the_whole_playlist() {
        // rekordbox's OK: the tracks the collection lacks are copied to
        // PioneerDJ/Imported from Device/Contents/<Artist>/<Album>/ and the
        // playlist is imported complete, in the device's order.
        let rig = rig();
        rig.came_from_elsewhere(3);
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        assert_eq!(plan.missing(), 2);
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        let stick_file = rig.sticks_file(3);
        let stick_bytes = std::fs::read(&stick_file).unwrap();
        let (entries, copies, _) = copy_missing(&plan, &plan.copies(), &destination).unwrap();
        let copy = destination.join("Contents/TRIODE/UnknownAlbum").join(stick_file.file_name().unwrap());
        assert_eq!(copies.files(), std::slice::from_ref(&copy), "the track listed twice is copied once");
        assert_eq!(std::fs::read(&copy).unwrap(), stick_bytes);
        let made = rig.writer().import_device_playlist_entries(&plan.name, &entries).unwrap();
        assert_eq!(made.added.len(), 1);
        let new_track = made.added[0].0.clone();
        assert_eq!(made.added[0].1, copy);
        assert_eq!(rig.entries(&made.id), vec![new_track.clone(), rbl_db::fixture::track_id(1), new_track]);
        assert_eq!(std::fs::read(&stick_file).unwrap(), stick_bytes, "the stick is only read");
    }

    #[test]
    fn importing_the_same_playlist_again_uses_the_earlier_copies() {
        // rekordbox's second import of the playlist asked nothing and made no
        // copy: "P186 (1)" held the two tracks the first import made [OBS
        // rekordbox 7.2.14, chris-win11, #198].
        let rig = rig();
        rig.came_from_elsewhere(3);
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        let (entries, _, _) = copy_missing(&plan, &plan.copies(), &destination).unwrap();
        let first = rig.writer().import_device_playlist_entries(&plan.name, &entries).unwrap();
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        let (entries, copies, _) = copy_missing(&plan, &plan.copies(), &destination).unwrap();
        assert_eq!(copies.files(), Vec::<PathBuf>::new(), "no second copy");
        let second = rig.writer().import_device_playlist_entries(&plan.name, &entries).unwrap();
        assert_eq!(second.name, "TAG LIST (1)");
        assert_eq!(second.added, vec![]);
        assert_eq!(rig.entries(&second.id), rig.entries(&first.id));
        assert_eq!(files_under(music.path()).len(), 1);
    }

    #[test]
    fn a_copy_never_replaces_a_file_already_in_the_music_folder() {
        let rig = rig();
        rig.came_from_elsewhere(1);
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        let name = rig.sticks_file(1).file_name().unwrap().to_owned();
        let theirs = destination.join("Contents/TRIODE/UnknownAlbum").join(&name);
        std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
        std::fs::write(&theirs, b"the user's own file").unwrap();
        let (_, copies, _) = copy_missing(&plan, &plan.copies(), &destination).unwrap();
        assert_eq!(std::fs::read(&theirs).unwrap(), b"the user's own file");
        let made = copies.files()[0].clone();
        assert_ne!(made, theirs);
        assert!(made.file_name().unwrap().to_string_lossy().contains("-1."), "{made:?}");
    }

    /// An app state over `rig`'s collection, for the steps that write
    /// through it.
    fn state_over(rig: &Rig) -> AppState {
        let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(rig.dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, rig.location.clone());
        state
    }

    /// Every file under `dir` with its bytes.
    fn contents_under(dir: &Path) -> Vec<(String, Vec<u8>)> {
        files_under(dir).into_iter().map(|f| { let bytes = std::fs::read(dir.join(&f)).unwrap(); (f, bytes) }).collect()
    }

    /// A stick track 0 that rekordbox analysed: 128 BPM in F minor, a grid,
    /// waveforms, a hot cue A and a memory loop.
    fn analysed_on_the_stick(i: usize, track: &mut rbl_export::SourceTrack) {
        if i != 0 {
            return;
        }
        track.bpm_x100 = 12_800;
        track.key = "Fm".into();
        let beats = [rbl_anlz::Beat { beat_number: 1, tempo_x100: 12_800, time_ms: 25 }, rbl_anlz::Beat { beat_number: 2, tempo_x100: 12_800, time_ms: 494 }];
        let mut dat = rbl_anlz::write::AnlzBuilder::new();
        dat.path("/Contents/TRIODE/UnknownAlbum/t0.wav").beat_grid(&beats).waveform_preview(b"PWAV", &[7; 400]).cue_lists(false);
        let mut ext = rbl_anlz::write::AnlzBuilder::new();
        ext.path("/Contents/TRIODE/UnknownAlbum/t0.wav").waveform_preview(b"PWV3", &[9; 30]).cue_lists(true);
        track.analysis = vec![("DAT".into(), dat.finish()), ("EXT".into(), ext.finish())];
        let cue = |kind: u8, time_ms: u32, loop_time_ms: Option<u32>| rbl_anlz::cues::ExportCue {
            kind,
            time_ms,
            loop_time_ms,
            color_id: 0,
            color_code: u8::from(kind != 0),
            comment: String::new(),
            active_loop: false,
            loop_numerator: 0,
            loop_denominator: 0,
        };
        track.cues = Some(vec![cue(1, 25, None), cue(0, 494, Some(2369))]);
    }

    #[test]
    fn a_copied_track_brings_the_sticks_bpm_key_waveform_grid_and_cues() {
        // #568: the copy came in as a bare file, its BPM, waveform, cues and
        // key lost. rekordbox gives each track it copies the device's row,
        // analysis files and cues [static, rekordbox 7.2.19
        // importFromDeviceThread::run @0x1001d9618].
        for format in [Format::OneLibrary, Format::DeviceLibrary] {
            copied_track_brings_the_sticks_analysis(format);
        }
    }

    fn copied_track_brings_the_sticks_analysis(format: Format) {
        let rig = rig_with(analysed_on_the_stick);
        rig.came_from_elsewhere(0);
        rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadWrite)
            .unwrap()
            .connection()
            .execute("INSERT INTO djmdKey (ID, ScaleName, Seq, created_at, updated_at) VALUES ('12', 'Fm', 7, '2026-01-01', '2026-01-01')", [])
            .unwrap();
        let state = state_over(&rig);
        let stick_before = contents_under(rig.stick.path());
        let stick_dat = {
            let library = device::read(rig.stick.path(), format).unwrap();
            let track = library.tracks.iter().find(|t| t.title == "Track 0").unwrap();
            rbl_anlz::Anlz::read(&audio_path(rig.stick.path(), &track.analysis_path)).unwrap()
        };
        let plan = rig.plan(format, "Friday");
        assert_eq!(plan.missing(), 1, "{format:?}");
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        let (entries, _, carried) = copy_missing(&plan, &plan.copies(), &destination).unwrap();
        let made = state.write(|w| w.import_device_playlist_entries(&plan.name, &entries)).unwrap();
        assert_eq!(carry_all(&state, rig.stick.path(), &made.added, &carried), (1, 0));

        let (content, copy) = made.added[0].clone();
        let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (bpm, key, path, analysed): (i64, String, String, i64) = db
            .connection()
            .query_row("SELECT BPM, KeyID, AnalysisDataPath, Analysed FROM djmdContent WHERE ID = ?1", [&content], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!((bpm, key.as_str(), analysed), (12_800, "12", rbl_db::write::ANALYSED_FULL), "{format:?}");
        let dat_path = rbl_anlz::resolve(&rig.location.share_root, &path);
        let dat = rbl_anlz::Anlz::read(&dat_path).unwrap();
        let ext = rbl_anlz::Anlz::read(&rbl_anlz::sibling(&dat_path, "EXT")).unwrap();
        let named = rbl_anlz::share_tree_ppth(&copy.to_string_lossy());
        assert_eq!((dat.path(), ext.path()), (Some(named.clone()), Some(named)), "the share tree's ?/<file name>");
        assert_eq!(dat.beat_grid(), stick_dat.beat_grid());
        assert_eq!(dat.section(b"PWAV"), stick_dat.section(b"PWAV"), "the waveform as the stick has it");
        assert_eq!(ext.section(b"PWV3").map(|s| s.payload.len()), Some(30));
        let mut stmt = db
            .connection()
            .prepare("SELECT InMsec, OutMsec, Kind FROM djmdCue WHERE ContentID = ?1 AND rb_local_deleted = 0 ORDER BY InMsec")
            .unwrap();
        let cues: Vec<(i64, Option<i64>, i64)> = stmt.query_map([&content], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(cues, vec![(25, None, 1), (494, Some(2369), 0)]);
        assert_eq!(contents_under(rig.stick.path()), stick_before, "the stick is only read");
        assert!(files_under(&rig.dir.path().join("backups/analysis-journal")).is_empty(), "the journal is closed");
    }

    #[test]
    fn a_copied_track_the_stick_never_analysed_still_takes_its_bpm() {
        let rig = rig_with(|i, track| if i == 1 { track.bpm_x100 = 9_950 });
        rig.came_from_elsewhere(1);
        let state = state_over(&rig);
        let plan = rig.plan(Format::OneLibrary, "Friday");
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        let (entries, _, carried) = copy_missing(&plan, &plan.copies(), &destination).unwrap();
        let made = state.write(|w| w.import_device_playlist_entries(&plan.name, &entries)).unwrap();
        assert_eq!(carry_all(&state, rig.stick.path(), &made.added, &carried), (1, 0));
        let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (bpm, path): (i64, Option<String>) = db
            .connection()
            .query_row("SELECT BPM, AnalysisDataPath FROM djmdContent WHERE ID = ?1", [&made.added[0].0], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(bpm, 9_950);
        assert!(path.unwrap_or_default().is_empty(), "no analysis files, no path");
    }

    #[test]
    fn a_picked_folder_takes_the_copies_and_one_that_is_not_there_is_refused() {
        // rekordbox's Change… takes a folder that exists
        // (importFromDeviceCopySettingWindow::buttonClicked @0x1001d802c).
        let picked = tempfile::tempdir().unwrap();
        let at = copy_destination(Some(&picked.path().to_string_lossy())).unwrap();
        assert_eq!(at, picked.path());
        assert!(copy_destination(Some(&picked.path().join("not there").to_string_lossy())).is_err());
        assert!(copy_destination(Some("relative/folder")).is_err());
        let rig = rig();
        rig.came_from_elsewhere(3);
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        let (_, copies, _) = copy_missing(&plan, &plan.copies(), &at).unwrap();
        let name = rig.sticks_file(3).file_name().unwrap().to_owned();
        assert_eq!(copies.files(), [picked.path().join("Contents/TRIODE/UnknownAlbum").join(name)]);
    }

    #[test]
    fn a_failed_import_leaves_neither_tracks_nor_copies() {
        // The copies are made, then the write refuses (a matched track was
        // deleted in between): what the command does, roll the copies back.
        let rig = rig();
        rig.came_from_elsewhere(3);
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        let (entries, copies, _) = copy_missing(&plan, &plan.copies(), &destination).unwrap();
        let mut writer = rig.writer();
        writer.delete_track(&rbl_db::fixture::track_id(1)).unwrap();
        let count = |w: &rbl_db::write::Writer, sql: &str| -> i64 { w.library().connection().query_row(sql, [], |r| r.get(0)).unwrap() };
        let before = (count(&writer, "SELECT COUNT(*) FROM djmdContent"), count(&writer, "SELECT COUNT(*) FROM djmdPlaylist"));
        assert!(writer.import_device_playlist_entries(&plan.name, &entries).is_err());
        copies.roll_back().unwrap();
        assert_eq!((count(&writer, "SELECT COUNT(*) FROM djmdContent"), count(&writer, "SELECT COUNT(*) FROM djmdPlaylist")), before);
        assert_eq!(files_under(music.path()), Vec::<String>::new());
    }

    #[test]
    fn a_path_that_climbs_out_of_the_stick_is_not_copied() {
        let rig = rig();
        rig.came_from_elsewhere(1);
        let mut plan = rig.plan(Format::OneLibrary, "TAG LIST");
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.wav");
        std::fs::write(&secret, wav(10)).unwrap();
        for entry in &mut plan.entries {
            if let PlanEntry::Missing(file) = entry {
                file.source = rig.stick.path().join("..").join(outside.path().file_name().unwrap()).join("secret.wav");
            }
        }
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        assert!(copy_missing(&plan, &plan.copies(), &destination).is_err());
        assert_eq!(files_under(music.path()), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_on_the_stick_to_a_file_elsewhere_is_not_copied() {
        // #248 review: a stick (ext4, APFS, HFS+) can hold a symbolic link
        // where its library names a track. Import Playlist must not copy
        // the file it leads to into the Music folder or the collection.
        let rig = rig();
        rig.came_from_elsewhere(1);
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.wav");
        std::fs::write(&secret, wav(10)).unwrap();
        let tracks = || -> i64 {
            let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
            db.connection().query_row("SELECT COUNT(*) FROM djmdContent", [], |r| r.get(0)).unwrap()
        };
        let before = tracks();
        let on_stick = rig.sticks_file(1);
        std::fs::remove_file(&on_stick).unwrap();
        std::os::unix::fs::symlink(&secret, &on_stick).unwrap();

        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        let copies = plan.copies();
        assert_eq!((copies.len(), copies[0].bytes), (1, 0), "the linked file's size is not asked for");
        let music = tempfile::tempdir().unwrap();
        let destination = rbl_export::device_import::destination(music.path());
        assert!(copy_missing(&plan, &copies, &destination).is_err());
        assert_eq!(files_under(music.path()), Vec::<String>::new());
        assert_eq!(tracks(), before, "nothing added to the collection");
    }

    #[test]
    fn only_a_plain_file_below_the_mount_counts_as_the_sticks() {
        // The check is made without canonicalizing, which fails on
        // WinFsp-style mounts, so it must see `..`, other volumes and
        // missing files by itself.
        let dir = tempfile::tempdir().unwrap();
        let (stick, outside) = (dir.path().join("STICK"), dir.path().join("outside"));
        std::fs::create_dir_all(stick.join("Contents/A")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(stick.join("Contents/A/t.wav"), b"wav").unwrap();
        std::fs::write(outside.join("secret.wav"), b"secret").unwrap();

        assert!(plainly_under(&stick, &stick.join("Contents/A/t.wav")));
        assert!(!plainly_under(&stick, &stick.join("Contents/A/../../../outside/secret.wav")), "climbs out");
        assert!(!plainly_under(&stick, &stick.join("Contents/../Contents/A/t.wav")), "any `..` is refused");
        assert!(!plainly_under(&stick, &outside.join("secret.wav")), "not under the mount");
        assert!(!plainly_under(&stick, &stick.join("Contents/A/gone.wav")), "not there");
        assert!(!plainly_under(&stick, &stick), "the mount itself is not a file of it");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, stick.join("Contents/B")).unwrap();
            std::os::unix::fs::symlink(outside.join("secret.wav"), stick.join("Contents/A/u.wav")).unwrap();
            assert!(!plainly_under(&stick, &stick.join("Contents/B/secret.wav")), "a linked folder on the way");
            assert!(!plainly_under(&stick, &stick.join("Contents/A/u.wav")), "a linked file");
        }
    }

    #[test]
    fn a_sticks_tracks_draw_their_waveform_and_artwork_from_the_stick() {
        // #319: a Devices list showed no preview, no artwork and an empty
        // deck, because its rows said "not analysed, no artwork" and nothing
        // read the stick's own ANLZ files or artwork.
        let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let image = src.path().join("cover.jpg");
        let jpeg = b"\xff\xd8sleeve\xff\xd9".to_vec();
        std::fs::write(&image, &jpeg).unwrap();
        let preview: Vec<u8> = (0..400_u16).map(|i| u8::try_from(i % 30 + 2).unwrap()).collect();
        let mut tracks: Vec<rbl_export::SourceTrack> = (1..=2_u8)
            .map(|i| {
                let path = src.path().join(format!("t{i}.mp3"));
                std::fs::write(&path, vec![i; 512]).unwrap();
                rbl_export::SourceTrack { id: u64::from(i), source_path: path, title: format!("Track {i}"), ..Default::default() }
            })
            .collect();
        tracks[0].analysis = vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/t1.mp3").waveform_preview(b"PWAV", &preview).finish())];
        tracks[0].artwork = Some(image);
        rbl_export::export(dest.path(), &tracks, &[]).unwrap();
        let mount = dest.path();
        // As the page's `encodeURIComponent` sends a `file:` id.
        let encoded = |id: &str| -> String { id.bytes().map(|b| if b.is_ascii_alphanumeric() { char::from(b).to_string() } else { format!("%{b:02X}") }).collect() };
        let artwork = |state: &Arc<AppState>, id: &str| {
            let request = tauri::http::Request::builder().uri(format!("rbl://localhost/artwork/{}", encoded(id))).body(Vec::new()).unwrap();
            crate::protocol::handle(state, &request)
        };

        for format in [Format::DeviceLibrary, Format::OneLibrary] {
            let library = device::read(mount, format).unwrap();
            let state = Arc::new(AppState::new());
            state.remember_device_assets(mount, library.tracks.iter().map(|t| assets_of(mount, t)));
            let rows: Vec<RowDto> = library.tracks.iter().enumerate().map(|(at, t)| row_dto(&device_row(mount, t, at))).collect();
            let (full, bare) = (rows.iter().find(|r| r.title == "Track 1").unwrap(), rows.iter().find(|r| r.title == "Track 2").unwrap());
            assert_eq!((full.analysed, full.has_artwork), (1, true), "{format:?}");
            assert_eq!((bare.analysed, bare.has_artwork), (0, false), "{format:?}");

            let dat = state.device_assets(&full.id).unwrap().analysis_dat().unwrap();
            assert!(dat.starts_with(mount) && dat.is_file(), "{format:?}: {}", dat.display());
            assert_eq!(crate::commands::waveform_of(&dat, "preview", None, None), preview, "{format:?}");
            assert_eq!(state.device_assets(&bare.id).unwrap().analysis_dat(), None, "{format:?}: a stick track without analysis draws nothing");
            assert_eq!(state.device_assets("12345"), None, "{format:?}: a library id falls through to the library");

            // The artwork, through the `rbl://` handler, with no library open.
            let response = artwork(&state, &full.id);
            assert_eq!(response.status(), tauri::http::StatusCode::OK, "{format:?}");
            assert_eq!(response.body(), &jpeg, "{format:?}");
            assert_eq!(artwork(&state, &bare.id).status(), tauri::http::StatusCode::NOT_FOUND, "{format:?}");
        }
    }

    /// #334: the library names `/Contents/...` and `/PIONEER/USBANLZ/...`,
    /// but the stick spells those folders in lower case, which a
    /// case-sensitive Linux mount does not take for the same names. Rows
    /// keep the library's spelling, joined with no file I/O; the deck, the
    /// preview, the waveform and the artwork find the files on opening.
    /// (On a case-insensitive temp dir, as macOS's and Windows's usually
    /// are, the spelling is found directly; Linux CI exercises the fallback.)
    #[test]
    fn a_stick_track_opens_when_the_stick_spells_its_folders_in_another_case() {
        let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let image = src.path().join("cover.jpg");
        std::fs::write(&image, b"\xff\xd8sleeve\xff\xd9").unwrap();
        let mut tracks: Vec<rbl_export::SourceTrack> = (1..=2_u8)
            .map(|i| {
                let path = src.path().join(format!("t{i}.mp3"));
                std::fs::write(&path, vec![i; 512]).unwrap();
                rbl_export::SourceTrack { id: u64::from(i), source_path: path, title: format!("Track {i}"), ..Default::default() }
            })
            .collect();
        tracks[0].analysis = vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/t1.mp3").finish())];
        tracks[0].artwork = Some(image);
        rbl_export::export(dest.path(), &tracks, &[]).unwrap();
        let mount = dest.path();
        let respell = |dir: &Path, name: &str| {
            let lower = name.to_lowercase();
            std::fs::rename(dir.join(name), dir.join(format!("{lower}.issue-334"))).unwrap();
            std::fs::rename(dir.join(format!("{lower}.issue-334")), dir.join(&lower)).unwrap();
        };
        respell(mount, "Contents");
        let pioneer = mount.join("PIONEER");
        for entry in std::fs::read_dir(&pioneer).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            if name != "rekordbox" && name != name.to_lowercase() {
                respell(&pioneer, &name);
            }
        }

        for format in [Format::DeviceLibrary, Format::OneLibrary] {
            let library = device::read(mount, format).unwrap();
            assert!(library.tracks.iter().all(|t| t.path.starts_with("/Contents/")), "{:?}", library.tracks);
            let state = AppState::new();
            state.remember_device_assets(mount, library.tracks.iter().map(|t| assets_of(mount, t)));
            let rows: Vec<RowDto> = library.tracks.iter().enumerate().map(|(at, t)| row_dto(&device_row(mount, t, at))).collect();
            for (row, track) in rows.iter().zip(&library.tracks) {
                let spelled = mount.join(track.path.trim_start_matches('/'));
                assert_eq!(row.id, rbl_index::folder::loose_id(&spelled), "{format:?}: the row keeps the library's spelling");
                let opened = openable(&state, &row.id, spelled);
                assert_eq!(std::fs::read(&opened).unwrap().len(), 512, "{format:?}: {}", opened.display());
            }
            let full = state.device_assets(&rows[0].id).unwrap();
            assert!(full.analysis_dat().is_some_and(|dat| dat.is_file()), "{format:?}: {full:?}");
            assert!(full.artwork_file().is_some_and(|art| art.is_file()), "{format:?}: {full:?}");
            assert_eq!(track_ids(mount, format, &[rows[1].id.clone()]).unwrap(), vec![library.tracks[1].id], "{format:?}");
        }
        // Anything that is not a stick's track is handed back as it came.
        let elsewhere = mount.join("contents/elsewhere.mp3");
        assert_eq!(openable(&AppState::new(), &rbl_index::folder::loose_id(&elsewhere), elsewhere.clone()), elsewhere);
    }

    #[test]
    fn a_sticks_track_draws_every_palettes_waveforms_from_the_stick() {
        // #372: a track loaded from a USB's library showed no waveform. The
        // deck asks for the palette's overview and detail (3Band by default:
        // PWV6/PWV7 in the .2EX), not the .DAT's PWAV that the #319 test
        // checks, so each kind is pinned here as the deck and the browser ask
        // for it, from the files an export puts on the stick.
        let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        // Seeds of 2 and up: the export raises a preview column lower than
        // that (#278), which would not compare.
        let bytes = |len: usize, seed: u8| -> Vec<u8> { (0..len).map(|i| u8::try_from(i % 29).unwrap() + seed).collect() };
        let (pwav, pwv3, pwv4, pwv5, pwv6, pwv7) = (bytes(400, 2), bytes(300, 2), bytes(1_200 * 6, 3), bytes(300 * 2, 4), bytes(1_200 * 3, 5), bytes(300 * 3, 6));
        let path = src.path().join("t1.mp3");
        std::fs::write(&path, vec![1; 512]).unwrap();
        let track = rbl_export::SourceTrack {
            id: 1,
            source_path: path,
            title: "Track 1".into(),
            analysis: vec![
                ("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/t1.mp3").waveform_preview(b"PWAV", &pwav).finish()),
                (
                    "EXT".into(),
                    rbl_anlz::AnlzBuilder::new()
                        .path("/t1.mp3")
                        .waveform_scroll(b"PWV3", 1, &pwv3)
                        .waveform_scroll(b"PWV4", 6, &pwv4)
                        .waveform_scroll(b"PWV5", 2, &pwv5)
                        .finish(),
                ),
                ("2EX".into(), rbl_anlz::AnlzBuilder::new().path("/t1.mp3").waveform_scroll(b"PWV6", 3, &pwv6).waveform_scroll(b"PWV7", 3, &pwv7).finish()),
            ],
            ..Default::default()
        };
        rbl_export::export(dest.path(), &[track], &[]).unwrap();
        let mount = dest.path();

        for format in [Format::DeviceLibrary, Format::OneLibrary] {
            let library = device::read(mount, format).unwrap();
            let state = Arc::new(AppState::new());
            state.remember_device_assets(mount, library.tracks.iter().map(|t| assets_of(mount, t)));
            let row = row_dto(&device_row(mount, &library.tracks[0], 0));
            assert_eq!(row.analysed, 1, "{format:?}: the deck draws only an analysed row's waveform");
            let dat = state.device_assets(&row.id).unwrap().analysis_dat().unwrap();
            // Each palette's pair, as `waveformKindOf` names them.
            for (kind, want) in [
                ("bands", &pwv6),
                ("bandsDetail", &pwv7),
                ("colour", &pwv4),
                ("colourDetail", &pwv5),
                ("mono", &pwav),
                ("monoDetail", &pwv3),
            ] {
                assert_eq!(&crate::commands::waveform_of(&dat, kind, None, None), want, "{format:?} {kind}");
            }
        }
    }

    #[test]
    fn a_sticks_track_without_a_2ex_has_no_bands_but_keeps_its_dat_preview() {
        // #372: a stick exported before 3Band data existed carries only the
        // .DAT and .EXT. The 3Band kinds are then empty, which is what sends
        // the deck's overview to the .DAT's PWAV, as rekordbox's does; that
        // PWAV, and the .EXT's colour tags, must still come from the stick.
        let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let bytes = |len: usize, seed: u8| -> Vec<u8> { (0..len).map(|i| u8::try_from(i % 29).unwrap() + seed).collect() };
        let (pwav, pwv4) = (bytes(400, 2), bytes(1_200 * 6, 3));
        let path = src.path().join("t1.mp3");
        std::fs::write(&path, vec![1; 512]).unwrap();
        let track = rbl_export::SourceTrack {
            id: 1,
            source_path: path,
            title: "Track 1".into(),
            analysis: vec![
                ("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/t1.mp3").waveform_preview(b"PWAV", &pwav).finish()),
                ("EXT".into(), rbl_anlz::AnlzBuilder::new().path("/t1.mp3").waveform_scroll(b"PWV4", 6, &pwv4).finish()),
            ],
            ..Default::default()
        };
        rbl_export::export(dest.path(), &[track], &[]).unwrap();
        let mount = dest.path();

        for format in [Format::DeviceLibrary, Format::OneLibrary] {
            let library = device::read(mount, format).unwrap();
            let state = Arc::new(AppState::new());
            state.remember_device_assets(mount, library.tracks.iter().map(|t| assets_of(mount, t)));
            let row = row_dto(&device_row(mount, &library.tracks[0], 0));
            assert_eq!(row.analysed, 1, "{format:?}");
            let dat = state.device_assets(&row.id).unwrap().analysis_dat().unwrap();
            assert!(!rbl_anlz::sibling(&dat, "2EX").exists(), "{format:?}: the export wrote no .2EX");
            for kind in ["bands", "bandsDetail"] {
                assert!(crate::commands::waveform_of(&dat, kind, None, None).is_empty(), "{format:?} {kind}");
            }
            assert_eq!(crate::commands::waveform_of(&dat, "mono", None, None), pwav, "{format:?}");
            assert_eq!(crate::commands::waveform_of(&dat, "colour", None, None), pwv4, "{format:?}");
        }
    }

    #[test]
    fn a_stick_path_that_climbs_out_is_not_read() {
        let stick = crate::test_stick::stick();
        std::fs::create_dir_all(stick.path().join("PIONEER")).unwrap();
        let assets = DeviceAssets { mount: stick.path().join("PIONEER"), audio: String::new(), analysis: "/../outside.DAT".into(), artwork: "../x.jpg".into() };
        std::fs::write(stick.path().join("outside.DAT"), b"x").unwrap();
        assert_eq!((assets.analysis_dat(), assets.artwork_file()), (None, None));
    }

    #[test]
    fn a_stick_file_that_is_not_there_is_not_named() {
        // The join no longer asks the file system (#271), so a missing file
        // must still come back as nothing rather than as a path to open.
        let stick = crate::test_stick::stick();
        std::fs::create_dir_all(stick.path().join("PIONEER/Artwork/00001")).unwrap();
        std::fs::write(stick.path().join("PIONEER/Artwork/00001/a1.jpg"), b"jpeg").unwrap();
        let assets = DeviceAssets {
            mount: stick.path().to_path_buf(),
            audio: "/Contents/missing.mp3".into(),
            analysis: "/PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT".into(),
            artwork: "/PIONEER/Artwork/00001/a1.jpg".into(),
        };
        assert_eq!((assets.audio_file(), assets.analysis_dat()), (None, None));
        assert_eq!(assets.artwork_file(), Some(stick.path().join("PIONEER/Artwork/00001/a1.jpg")));
    }

    #[cfg(unix)]
    #[test]
    fn a_stick_file_reached_through_a_link_is_not_named() {
        // A stick's library is untrusted: a link where it names a `.DAT` or
        // artwork, or a linked folder on the way, must not lead out of the
        // stick to another file (#271 review).
        let dir = tempfile::tempdir().unwrap();
        let (stick, outside) = (dir.path().join("STICK"), dir.path().join("outside"));
        std::fs::create_dir_all(stick.join("PIONEER/Artwork/00001")).unwrap();
        std::fs::create_dir_all(stick.join("PIONEER/USBANLZ")).unwrap();
        std::fs::create_dir_all(outside.join("00000001")).unwrap();
        std::fs::write(outside.join("00000001/ANLZ0000.DAT"), b"dat").unwrap();
        std::fs::write(outside.join("secret"), b"secret").unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), stick.join("PIONEER/Artwork/00001/a1.jpg")).unwrap();
        std::os::unix::fs::symlink(&outside, stick.join("PIONEER/USBANLZ/P000")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), stick.join("Contents.mp3")).unwrap();

        let assets = DeviceAssets {
            mount: stick.clone(),
            audio: "/Contents.mp3".into(),
            analysis: "/PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT".into(),
            artwork: "/PIONEER/Artwork/00001/a1.jpg".into(),
        };
        assert_eq!((assets.audio_file(), assets.analysis_dat(), assets.artwork_file()), (None, None, None));
        // The case-insensitive fallback (#334) does not get round it either.
        let shouted = DeviceAssets { analysis: assets.analysis.to_uppercase(), artwork: assets.artwork.to_uppercase(), ..assets };
        assert_eq!((shouted.analysis_dat(), shouted.artwork_file()), (None, None));
    }

    #[test]
    fn a_reread_or_ejected_stick_forgets_its_earlier_tracks() {
        let (first, other) = (Path::new("/Volumes/STICK"), Path::new("/Volumes/OTHER"));
        let at = |mount: &Path, analysis: &str| DeviceAssets { mount: mount.to_path_buf(), audio: String::new(), analysis: analysis.into(), artwork: String::new() };
        let state = AppState::new();
        state.remember_device_assets(first, [("file:/Volumes/STICK/a.mp3".to_string(), at(first, "/a.DAT"))]);
        state.remember_device_assets(other, [("file:/Volumes/OTHER/o.mp3".to_string(), at(other, "/o.DAT"))]);

        // A different stick read at the same mount replaces the first one's tracks.
        state.remember_device_assets(first, [("file:/Volumes/STICK/b.mp3".to_string(), at(first, "/b.DAT"))]);
        assert_eq!(state.device_assets("file:/Volumes/STICK/a.mp3"), None);
        assert_eq!(state.device_assets("file:/Volumes/STICK/b.mp3"), Some(at(first, "/b.DAT")));
        assert_eq!(state.device_assets("file:/Volumes/OTHER/o.mp3"), Some(at(other, "/o.DAT")), "another mount is untouched");

        state.forget_device_assets(first);
        assert_eq!(state.device_assets("file:/Volumes/STICK/b.mp3"), None);
        assert_eq!(state.device_assets("file:/Volumes/OTHER/o.mp3"), Some(at(other, "/o.DAT")));
    }

    #[test]
    fn a_view_orders_a_playlist_and_pages_it() {
        let (_src, dest) = stick();
        let library = device::read(dest.path(), Format::DeviceLibrary).unwrap();
        let friday = library.nodes.iter().find(|n| n.name == "Friday").unwrap();
        let rows: Vec<DeviceRow> = friday.tracks.iter().enumerate().map(|(at, id)| device_row(dest.path(), library.track(*id).unwrap(), at)).collect();
        let spec = rbl_index::ViewSpec {
            source: rbl_index::TrackSource::Collection,
            sort: rbl_index::SortColumn::Title,
            descending: true,
            query: String::new(),
            filter: rbl_index::TrackFilter::default(),
        };
        let view = DeviceView::open(rows, &spec, rbl_index::SearchField::All);
        let page = fetch_rows(&view, 0, 10).unwrap();
        assert_eq!(page.iter().map(|r| (r.title.as_str(), r.track_no)).collect::<Vec<_>>(), [("Track 2", 2), ("Track 1", 1)]);
        assert_eq!(ids_in_range(&view, 1, 0), vec![page[0].id.clone(), page[1].id.clone()]);
        assert!(fetch_rows(&view, 0, MAX_ROWS + 1).is_err());
    }
}
