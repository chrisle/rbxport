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

/// What a stick's library names beside one of its tracks' audio, kept so the
/// track's waveforms, beat grid and artwork are read from the stick, as
/// rekordbox's Devices tree shows them (#319).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceAssets {
    /// The stick's mount point; both paths below are relative to it.
    pub mount: PathBuf,
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

    fn on_stick(mount: &Path, relative: &str) -> Option<PathBuf> {
        if relative.is_empty() {
            return None;
        }
        crate::protocol::resolve_under(mount, relative)
    }
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
        let rows: Vec<DeviceRow> = ids
            .iter()
            .enumerate()
            .filter_map(|(at, id)| library.track(*id).map(|t| device_row(&mount, t, at)))
            .collect();
        handle.remember_device_assets(library.tracks.iter().map(|t| assets_of(&mount, t)));
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
        analysed: !track.analysis_path.is_empty(),
        has_artwork: !track.artwork_path.is_empty(),
    }
}

/// A stick track's row id with what its library names beside the audio.
fn assets_of(mount: &Path, track: &device::Track) -> (String, DeviceAssets) {
    (
        rbl_index::folder::loose_id(&audio_path(mount, &track.path)),
        DeviceAssets { mount: mount.to_path_buf(), analysis: track.analysis_path.clone(), artwork: track.artwork_path.clone() },
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
            state.remember_device_assets(library.tracks.iter().map(|t| assets_of(mount, t)));
            let rows: Vec<RowDto> = library.tracks.iter().enumerate().map(|(at, t)| row_dto(&device_row(mount, t, at))).collect();
            let (full, bare) = (rows.iter().find(|r| r.title == "Track 1").unwrap(), rows.iter().find(|r| r.title == "Track 2").unwrap());
            assert_eq!((full.analysed, full.has_artwork), (1, true), "{format:?}");
            assert_eq!((bare.analysed, bare.has_artwork), (0, false), "{format:?}");

            let dat = state.device_assets(&full.id).unwrap().analysis_dat().unwrap();
            assert!(dat.starts_with(mount.canonicalize().unwrap()), "{format:?}: {}", dat.display());
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

    #[test]
    fn a_stick_path_that_climbs_out_is_not_read() {
        let stick = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(stick.path().join("PIONEER")).unwrap();
        let assets = DeviceAssets { mount: stick.path().join("PIONEER"), analysis: "/../outside.DAT".into(), artwork: "../x.jpg".into() };
        std::fs::write(stick.path().join("outside.DAT"), b"x").unwrap();
        assert_eq!((assets.analysis_dat(), assets.artwork_file()), (None, None));
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
