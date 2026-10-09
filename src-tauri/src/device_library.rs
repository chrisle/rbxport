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
    #[serde(rename = "add")]
    Add { playlist: String, tracks: Vec<String> },
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
    blocking("device_playlist_edit", move || apply_edit(&path, &format, edit, rbl_db::is_rekordbox_running())).await
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
        DeviceEditDto::Add { playlist, tracks } => Edit::Add { playlist: id_from(&playlist)?, tracks: track_ids(&mount, format, &tracks)? },
        DeviceEditDto::Remove { playlist, tracks } => Edit::Remove { playlist: id_from(&playlist)?, tracks: track_ids(&mount, format, &tracks)? },
    };
    let applied = device::apply(&mount, format, &edit).map_err(|e| device_error(&e))?;
    tracing::info!(format = format_name(format), changed = applied.changed, "device playlist edit");
    Ok(DeviceEditResultDto { id: applied.id.to_string(), changed: u32::try_from(applied.changed).unwrap_or(u32::MAX) })
}

/// What Import Playlist did with one of a stick's playlists.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceImportResultDto {
    /// The collection playlist made; `None` when nothing was imported
    /// because none of the playlist's tracks is in the collection.
    pub id: Option<String>,
    /// The name it was given, or the device's name when nothing was made.
    pub name: String,
    /// The name differs from the device's because the top level already
    /// had one by that name.
    pub renamed: bool,
    /// Entries added, a track listed twice counted twice.
    pub tracks: u32,
    /// Entries left out because their track is not in the collection.
    pub missing: u32,
}

/// Import Playlist over a stick's playlist: a playlist at the end of the
/// collection's top level holding the stick playlist's tracks, as
/// rekordbox's Devices tree does (see
/// [`rbl_db::write::Writer::import_device_playlist`]). Only the user's
/// library is written; the stick is read.
///
/// rekordbox copies a track that is not in the collection to the computer
/// and imports it [static, rekordbox 7.2.11 macOS:
/// `importFromDeviceManager::start` @0x100191868 and
/// `import_from_d_ask_file_copy` @0x100191d44 ask, then
/// `importFromDeviceThread::import_from_d_file_copy` @0x10018fdc0 copies to
/// `PioneerDJ/Imported from Device`]. This does not copy files: such a
/// track is left out and counted in `missing`, as rekordbox's older device
/// import (`importDevicePlaylistLegacy` @0x1016689bc) leaves it out, and
/// when that is every track nothing is made.
#[tauri::command]
pub async fn device_playlist_import<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    path: String,
    format: String,
    id: String,
) -> AppResult<DeviceImportResultDto> {
    let reading = Arc::clone(&state);
    let plan = blocking("device_playlist_import", move || {
        let mount = connected(&path)?;
        let (format, id) = (format_from(&format)?, id_from(&id)?);
        let stick = StickTracks::read(&mount, format)?;
        let Some(node) = stick.library.node(id).filter(|n| !n.folder) else {
            return Err(AppError::new(ErrorKind::NotFound, "That playlist is no longer on the device."));
        };
        reading.read_db(|db| plan_import(db, &stick, node)).map_err(crate::commands::write_error)
    })
    .await?;
    let missing = u32::try_from(plan.missing).unwrap_or(u32::MAX);
    if plan.contents.is_empty() && plan.listed > 0 {
        return Ok(DeviceImportResultDto { id: None, name: plan.name, renamed: false, tracks: 0, missing });
    }
    let made: Arc<parking_lot::Mutex<Option<rbl_db::write::ImportedPlaylist>>> = Arc::default();
    let slot = Arc::clone(&made);
    let (name, contents) = (plan.name.clone(), plan.contents);
    crate::commands::edit(app, state, "device_playlist_import", crate::commands::Touched::Playlists, move |w| {
        *slot.lock() = Some(w.import_device_playlist(&name, &contents)?);
        Ok(())
    })
    .await?;
    let made = made.lock().take().ok_or_else(|| AppError::new(ErrorKind::Internal, "The playlist could not be imported."))?;
    tracing::info!(tracks = made.tracks, missing = plan.missing, "imported a device playlist");
    Ok(DeviceImportResultDto {
        id: Some(made.id),
        renamed: made.name != plan.name,
        name: made.name,
        tracks: u32::try_from(made.tracks).unwrap_or(u32::MAX),
        missing,
    })
}

/// One library of a stick, with what tells its tracks apart from the
/// collection's.
struct StickTracks {
    mount: PathBuf,
    library: device::Library,
    /// For the Device Library, whose rows do not say which desktop track
    /// they are: the stick's `exportLibrary.db` rows by file, which do.
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

/// What Import Playlist will write for one of a stick's playlists.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportPlan {
    /// The playlist's name on the stick.
    name: String,
    /// The collection's ids for its entries, in the stick's order, without
    /// the ones the collection does not have.
    contents: Vec<String>,
    /// Entries the stick lists.
    listed: usize,
    /// Entries left out.
    missing: usize,
}

/// Works out which collection track each entry of `node` is, in
/// rekordbox's order [static, rekordbox 7.2.11 macOS,
/// `importFromDeviceManager::start` @0x100191868]: first a collection track
/// at the same file (`rekordboxDBController::isCollectionSong`, the file on
/// the stick), then the track the entry was exported from, when the stick
/// names this library (`getMainTrackByDeviceTrack` @0x100464c00: the
/// row's `masterDbId` equal to this library's `DBID`, then its
/// `masterContentId`). Read-only.
///
/// The Device Library's rows carry no such ids, and how rekordbox tells
/// them apart is [UNKNOWN]; this takes the ids of the stick's `OneLibrary`
/// row for the same file [ASSUME: both libraries of one stick describe the
/// same files].
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
    let mut contents = Vec::with_capacity(node.tracks.len());
    for entry in &node.tracks {
        let Some(track) = stick.library.track(*entry) else { continue };
        let file = audio_path(&stick.mount, &track.path);
        let at_file: Option<String> = conn
            .prepare("SELECT ID FROM djmdContent WHERE FolderPath = ?1 AND rb_local_deleted = 0 ORDER BY ID LIMIT 1")?
            .query_map([file.to_string_lossy().as_ref()], |r| r.get(0))?
            .next()
            .transpose()?;
        if let Some(found) = at_file {
            contents.push(found);
            continue;
        }
        let (master_db, master_content) = match (track.master_db_id, track.master_content_id) {
            (0, 0) => stick.masters_by_path.get(&track.path).copied().unwrap_or((0, 0)),
            ids => ids,
        };
        if db_id != 0 && master_db == db_id && master_content != 0 {
            let content = master_content.to_string();
            if live(&content)? {
                contents.push(content);
            }
        }
    }
    let listed = node.tracks.len();
    Ok(ImportPlan { name: node.name.clone(), missing: listed - contents.len(), contents, listed })
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
        let rows = ids
            .iter()
            .enumerate()
            .filter_map(|(at, id)| library.track(*id).map(|t| device_row(&mount, t, at)))
            .collect();
        let view = DeviceView::open(rows, &parsed, field);
        let (view_id, len, generation) = handle.open_device_view(view);
        Ok(ViewHandleDto { view_id, len, gen: generation })
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
    }
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
        analysed: 0,
        date_added: row.date_added.clone(),
        release_date: String::new(),
        hot_cues: Vec::new(),
        memory_cues: Vec::new(),
        artwork_hue: 0,
        has_artwork: false,
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
        apply_edit(path, "deviceLibrary", DeviceEditDto::Add { playlist: made.id.clone(), tracks: rows }, false).unwrap();
        let found = libraries(dest.path());
        let opening = found[0].nodes.iter().find(|n| n.name == "Opening").unwrap();
        assert_eq!(opening.count, 3);
        assert!(!found[1].nodes.iter().any(|n| n.name == "Opening"), "OneLibrary is left as it was");
        let refused = apply_edit(path, "deviceLibrary", DeviceEditDto::Rename { id: made.id, name: "   ".into() }, false).unwrap_err();
        assert!(matches!(refused.kind, ErrorKind::Malformed));
        assert!(apply_edit(path, "elsewhere", DeviceEditDto::Delete { id: "1".into() }, false).is_err());
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
    /// collection's first four. Then, as an XDJ-AZ saves a TAG LIST, a
    /// playlist the player wrote: `TAG LIST` with the fourth, the second and
    /// the fourth track again.
    struct Rig {
        dir: tempfile::TempDir,
        _src: tempfile::TempDir,
        stick: tempfile::TempDir,
        location: rbl_db::LibraryLocation,
    }

    const TAG_LIST: i64 = 9001;

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let (src, stick) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let tracks: Vec<rbl_export::SourceTrack> = (0..4_usize)
            .map(|i| {
                let path = src.path().join(format!("t{i}.mp3"));
                std::fs::write(&path, vec![u8::try_from(i).unwrap(); 512]).unwrap();
                rbl_export::SourceTrack {
                    id: rbl_db::fixture::track_id(i).parse().unwrap(),
                    source_path: path,
                    title: format!("Track {i}"),
                    artist: "TRIODE".into(),
                    ..Default::default()
                }
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
    }

    fn ids(indices: &[usize]) -> Vec<String> {
        indices.iter().map(|&i| rbl_db::fixture::track_id(i)).collect()
    }

    #[test]
    fn a_tag_list_the_player_saved_finds_its_tracks_in_the_collection() {
        let rig = rig();
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        assert_eq!(plan, ImportPlan { name: "TAG LIST".into(), contents: ids(&[3, 1, 3]), listed: 3, missing: 0 });
        // And it lands as rekordbox's Import Playlist leaves it: at the end
        // of the top level (the fixture has Playlist 0-2 at Seq 0-2), in
        // the player's order, the repeated track twice.
        let made = rig.writer().import_device_playlist(&plan.name, &plan.contents).unwrap();
        let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (seq, parent): (i64, String) = db
            .connection()
            .query_row("SELECT Seq, ParentID FROM djmdPlaylist WHERE ID = ?1", [&made.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((made.name.as_str(), seq, parent.as_str()), ("TAG LIST", 3, "root"));
        let entries: Vec<String> = db
            .connection()
            .prepare("SELECT ContentID FROM djmdSongPlaylist WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo")
            .unwrap()
            .query_map([&made.id], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(entries, ids(&[3, 1, 3]));
    }

    #[test]
    fn the_device_library_finds_its_tracks_through_the_sticks_onelibrary() {
        let rig = rig();
        assert_eq!(rig.plan(Format::DeviceLibrary, "Friday").contents, ids(&[0, 1]));
    }

    #[test]
    fn a_track_from_another_library_or_gone_from_this_one_is_left_out() {
        let rig = rig();
        let one = rig.stick.path().join("PIONEER/rekordbox/exportLibrary.db");
        let conn = rbl_onelibrary::playlists::open_read_write(&one).unwrap();
        conn.execute("UPDATE content SET masterDbId = 42 WHERE masterContentId = ?1", [rbl_db::fixture::track_id(1)]).unwrap();
        drop(conn);
        assert_eq!(rig.plan(Format::OneLibrary, "TAG LIST"), ImportPlan { name: "TAG LIST".into(), contents: ids(&[3, 3]), listed: 3, missing: 1 });
        rig.writer().delete_track(&rbl_db::fixture::track_id(3)).unwrap();
        let plan = rig.plan(Format::OneLibrary, "TAG LIST");
        assert_eq!((plan.contents.len(), plan.listed, plan.missing), (0, 3, 3), "nothing to import");
    }

    #[test]
    fn a_collection_track_at_the_sticks_own_file_comes_first() {
        // rekordbox looks for the file before the exported-from id.
        let rig = rig();
        let library = device::read(rig.stick.path(), Format::OneLibrary).unwrap();
        let master: u64 = rbl_db::fixture::track_id(1).parse().unwrap();
        let on_stick = library.tracks.iter().find(|t| t.master_content_id == master).unwrap();
        let file = audio_path(rig.stick.path(), &on_stick.path);
        rbl_db::fixture::point_at_audio(&rig.location, 20, &file.to_string_lossy(), 300).unwrap();
        assert_eq!(rig.plan(Format::OneLibrary, "TAG LIST").contents, ids(&[3, 20, 3]));
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
