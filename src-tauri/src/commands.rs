//! Tauri commands.
//!
//! Each is a thin adapter. Anything that touches the index runs on a blocking
//! thread via [`blocking`], which serves two rules at once: the async runtime
//! is never blocked, and a panic inside a command surfaces as an `AppError`
//! instead of taking the process down.

use std::sync::Arc;

use rbl_deck::{Band, Curve};
use rbl_index::Library;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::link::LinkStatusDto;
use crate::dto::{
    cue_colour_css, AudioDeviceDto, AudioDevicesDto, CueDto, DeviceDto, DeviceExportDto, ExportReportDto,
    EditHistoryDto, FolderPlaylistDto, ImportReportDto, LibrarySummaryDto, LimiterDto, PhraseDto, RowDto, UnanalysedTrackDto, UnanalysedTracksDto,
    TreeNodeDto, ViewHandleDto, ViewSpecDto,
    BackupDto, CountedDto, DeviceSyncStateDto, DuplicateGroupDto, DuplicateTrackDto, DuplicatesDto,
    ExportProgressDto, FilterValuesDto, ItunesLibraryDto, MissingExportFileDto, SmartConditionDto, SmartRuleDto, SyncDeviceReportDto, SyncPlaylistDto, SyncProgressDto, TagCategoryDto,
    XmlImportReportDto,
};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::state::{rows_to_dto, spec_from_wire, AppState, EditHistory, LibraryEdit};

/// Rows per request. The frontend asks a page at a time; this bound is what
/// keeps a response inside the 64 KB cap.
pub const MAX_ROWS: u32 = 128;

/// Beats returned for one track. A four-minute track at 128 BPM has about 500
/// and a three-hour mix around 23,000; this bounds the response without
/// truncating any real grid.
const MAX_BEATS: usize = 65_536;

/// Phrases returned for one track. The longest song structure in the reference
/// library has 457 [OBS] — a two-hour DJ mix — and every ordinary track is
/// under fifty, so this bounds the response without truncating a real one.
const MAX_PHRASES: usize = 512;

/// Runs `f` on a blocking thread and converts a panic there into an `AppError`.
pub(crate) async fn blocking<T, F>(name: &'static str, f: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    // `run_command` catches an unwind inside the worker so the message names the
    // command; the JoinError arm is the backstop if the thread dies some other way.
    match tauri::async_runtime::spawn_blocking(move || {
        crate::error::run_command(name, std::panic::AssertUnwindSafe(f))
    })
    .await
    {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(command = name, error = %e, "command panicked");
            Err(AppError::internal(format!("{name} panicked: {e}")))
        }
    }
}

#[tauri::command]
pub async fn library_summary(state: State<'_, Arc<AppState>>) -> AppResult<LibrarySummaryDto> {
    let library = state.library()?;
    let (read_only, db_version, load_ms, _generation) = state.summary();
    let is_real_install = state.location()?.is_real_install;
    blocking("library_summary", move || {
        // The UI refreshes this while open; startup's process state is stale
        // as soon as rekordbox launches or exits. Fixtures keep their own gate.
        let read_only = if is_real_install {
            rbl_db::is_rekordbox_running() && !rbl_db::unsafe_writes_enabled()
        } else {
            read_only
        };
        Ok(LibrarySummaryDto {
            track_count: u32::try_from(library.len()).unwrap_or(u32::MAX),
            playlist_count: u32::try_from(library.playlists().len()).unwrap_or(u32::MAX),
            read_only,
            db_version,
            load_ms,
        })
    })
    .await
}

/// Removes the rekordbox-running write gate for this process. This deliberately
/// requires both an environment opt-in and an explicit gesture in the UI.
#[tauri::command]
pub fn disable_read_only() -> AppResult<()> {
    if rbl_db::enable_unsafe_writes() {
        Ok(())
    } else {
        Err(AppError::internal(format!(
            "Set {} before launching rbxport to enable this override.",
            rbl_db::UNSAFE_WRITES_ENV
        )))
    }
}

#[tauri::command]
pub async fn playlist_tree(state: State<'_, Arc<AppState>>) -> AppResult<Vec<TreeNodeDto>> {
    let library = state.library()?;
    blocking("playlist_tree", move || Ok(build_tree(&library))).await
}

fn build_tree(library: &Library) -> Vec<TreeNodeDto> {
    let playlists = library.playlists();
    let histories = library.histories();
    let mut nodes = vec![
        TreeNodeDto {
            id: "all".into(),
            name: "All Tracks".into(),
            kind: "allTracks",
            depth: 0,
            expanded: None,
            child_count: Some(u32::try_from(library.len()).unwrap_or(u32::MAX)),
        },
        TreeNodeDto {
            id: "playlists".into(),
            name: "Playlists".into(),
            kind: "collection",
            depth: 0,
            expanded: Some(true),
            child_count: Some(u32::try_from(playlists.len()).unwrap_or(u32::MAX)),
        },
    ];

    push_lists(&mut nodes, &playlists, ListStyle::PLAYLISTS, 2);

    // Histories only when there are some: an empty section is a heading that
    // leads nowhere, and the rail already dims what has nothing in it.
    if !histories.is_empty() {
        nodes.push(TreeNodeDto {
            id: "histories".into(),
            name: "Histories".into(),
            kind: "histories",
            depth: 0,
            // Open, with the years under it open and the months closed: the
            // rail shows one section at a time, so the sessions — 187 of them
            // in the reference library — open over nothing else.
            expanded: Some(true),
            child_count: Some(u32::try_from(histories.len()).unwrap_or(u32::MAX)),
        });
        // A year folder and a session are both "history": they are one
        // section, and what tells them apart in the tree is whether anything
        // sits under them.
        push_lists(&mut nodes, &histories, ListStyle::HISTORIES, 2);
    }
    nodes
}

/// What to call a list with children, and one without, and how to order them.
#[derive(Debug, Clone, Copy)]
struct ListStyle {
    folder: &'static str,
    leaf: &'static str,
    /// An intelligent playlist: a rule rather than a membership. Histories
    /// have none, so theirs is the leaf.
    smart: &'static str,
    /// Filed by date rather than by hand: folders are a year and a month,
    /// which rekordbox shows in calendar order under their month's name, not
    /// in the order they were made. Sessions keep their `Seq`, which is the
    /// order they were played in.
    calendar: bool,
}

impl ListStyle {
    const PLAYLISTS: Self =
        Self { folder: "folder", leaf: "playlist", smart: "smartPlaylist", calendar: false };
    const HISTORIES: Self =
        Self { folder: "history", leaf: "history", smart: "history", calendar: true };
}

/// A month folder's name as rekordbox shows it: `djmdHistory` stores the
/// month as its number.
fn month_name(number: &str) -> Option<&'static str> {
    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June",
        "July", "August", "September", "October", "November", "December",
    ];
    let month = number.parse::<usize>().ok()?;
    MONTHS.get(month.checked_sub(1)?).copied()
}

/// Flattens one list tree onto `nodes`, depth-first, in `Seq` order — or, for
/// a calendar, with the year and month folders in date order.
///
/// `open_to` is the depth below which branches arrive expanded: the tree opens
/// on the playlists and on the years, and closed on the months.
fn push_lists(
    nodes: &mut Vec<TreeNodeDto>,
    lists: &rbl_index::Playlists,
    style: ListStyle,
    open_to: u32,
) {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); lists.len()];
    let mut roots: Vec<usize> = Vec::new();
    for index in 0..lists.len() {
        match lists.parent.get(index).copied() {
            Some(parent) if parent != rbl_index::NO_ID && (parent as usize) < lists.len() => {
                if let Some(bucket) = children.get_mut(parent as usize) {
                    bucket.push(index);
                }
            }
            _ => roots.push(index),
        }
    }
    if style.calendar {
        // A year is "2026" and a month "9": the number is the date. A folder
        // named anything else sorts after the dated ones, in `Seq` order.
        let by_date = |index: &usize| -> (u64, u32) {
            if lists.is_folder(*index) {
                (lists.name(*index).parse::<u64>().unwrap_or(u64::MAX), 0)
            } else {
                (u64::MAX, lists.seq.get(*index).copied().unwrap_or(u32::MAX))
            }
        };
        roots.sort_by_key(by_date);
        for bucket in &mut children {
            bucket.sort_by_key(by_date);
        }
    }

    // Iterative, with a visited set: a corrupt parent cycle must not recurse
    // forever or blow the stack.
    let mut stack: Vec<(usize, u32)> = roots.iter().rev().map(|&i| (i, 1_u32)).collect();
    let mut visited = vec![false; lists.len()];
    while let Some((index, depth)) = stack.pop() {
        if visited.get(index).copied().unwrap_or(true) {
            continue;
        }
        if let Some(slot) = visited.get_mut(index) {
            *slot = true;
        }
        let under = children.get(index).map_or(0, Vec::len);
        let members = lists.members.get(index).map_or(0, Vec::len);
        // A folder by its attribute, or by what is under it: a history year
        // is a folder only in the second sense, an empty playlist folder only
        // in the first.
        let folder = lists.is_folder(index) || under > 0;
        let name = lists.name(index);
        // Depth 2 under the section's heading is the month, filed in a year.
        let name = match (style.calendar && folder && depth == 2, month_name(name)) {
            (true, Some(month)) => month.to_owned(),
            _ => name.to_owned(),
        };
        let smart = !folder && lists.is_smart(index);
        nodes.push(TreeNodeDto {
            id: lists.ids.get(index).copied().unwrap_or(0).to_string(),
            name,
            kind: if folder {
                style.folder
            } else if smart {
                style.smart
            } else {
                style.leaf
            },
            depth,
            expanded: if folder { Some(depth < open_to) } else { None },
            // An intelligent playlist's count is whatever its rule admits
            // today, which is not known until it is opened; the tree shows
            // none rather than evaluating every rule to draw itself.
            child_count: if smart {
                None
            } else {
                Some(u32::try_from(if folder { under } else { members }).unwrap_or(u32::MAX))
            },
        });
        if let Some(below) = children.get(index) {
            for &child in below.iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }
}

#[tauri::command]
pub async fn open_view(state: State<'_, Arc<AppState>>, spec: ViewSpecDto) -> AppResult<ViewHandleDto> {
    // A stick's own library is read from the stick, not from the index.
    if let crate::dto::TrackSourceDto::Device { path, format, playlist } = &spec.source {
        return crate::device_library::open_view(&state, path.clone(), format.clone(), playlist.clone(), &spec).await;
    }
    let library = state.library()?;
    // A folder is read from disk, not from the index, so it takes its own
    // path before the source is translated.
    if let crate::dto::TrackSourceDto::Folder { path } = &spec.source {
        return crate::explorer::open_folder(&state, path.clone(), &spec).await;
    }
    let parsed = spec_from_wire(&library, &spec);
    // Sorting and filtering happen here, so this is the one that must not run
    // on the async thread.
    let handle = Arc::clone(&state);
    blocking("open_view", move || {
        let (view_id, len, generation) = handle.open_view_scoped(&parsed, spec.search_field)?;
        Ok(ViewHandleDto { view_id, len, gen: generation })
    })
    .await
}

#[tauri::command]
pub async fn fetch_rows(
    state: State<'_, Arc<AppState>>,
    view_id: u32,
    offset: u32,
    len: u32,
    extra_columns: Option<Vec<String>>,
) -> AppResult<Vec<RowDto>> {
    if len > MAX_ROWS {
        return Err(
            AppError::new(ErrorKind::Malformed, "Too many rows requested at once.")
                .with_detail(format!("len {len} exceeds the {MAX_ROWS}-row cap")),
        );
    }
    if let Some(device) = state.device_view(view_id) {
        return crate::device_library::fetch_rows(&device, offset, len);
    }
    let library = state.library()?;
    let extra_columns = extra_columns.unwrap_or_default();
    let handle = Arc::clone(&state);
    if let Some(folder) = state.folder_view(view_id) {
        let mut rows = crate::explorer::fetch_rows(library, folder, offset, len).await?;
        if extra_columns.is_empty() {
            Ok(rows)
        } else {
            blocking("fetch_row_details", move || {
                enrich_rows(&handle, &mut rows, &extra_columns)?;
                Ok(rows)
            }).await
        }
    } else {
        let view = state.view(view_id)?;
        blocking("fetch_rows", move || {
            let offset = offset as usize;
            let window = view.window(offset, len as usize);
            let mut rows = rows_to_dto(&library, window, offset);
            for (position, row) in rows.iter_mut().enumerate() {
                row.track_no = view.track_no_at(offset.saturating_add(position));
            }
            if !extra_columns.is_empty() { enrich_rows(&handle, &mut rows, &extra_columns)?; }
            Ok(rows)
        })
        .await
    }
}

/// Adds only requested browser fields, keeping ordinary row pages small.
fn enrich_rows(state: &AppState, rows: &mut [RowDto], columns: &[String]) -> AppResult<()> {
    use serde_json::{json, Value};
    const FIELDS: &[&str] = &[
        "size", "discNo", "albumArtist", "composer", "lyricist", "fileType", "year",
        "mixName", "remixer", "originalArtist", "sampleRate", "bitrate", "bitDepth",
        "location", "dateCreated", "publishTrackInfo", "message", "color",
        "djPlayCount", "myTag", "trackNumber", "cloud",
    ];
    let wanted: Vec<&str> = columns.iter().map(String::as_str).filter(|column| FIELDS.contains(column)).collect();
    if wanted.is_empty() { return Ok(()); }
    state.read_db(|db| {
        // The location rekordbox shows: the stored path through the drive
        // substitution, or this machine's own cloud-shared track's local copy.
        let track_paths = wanted.contains(&"location").then(|| db.track_paths());
        for row in rows {
            if row.id.starts_with("file:") { continue; }
            let Some(details) = rbl_db::details::browser_details(db.connection(), &row.id)? else { continue };
            let cloud = details.path.starts_with("/contents_");
            let location = match &track_paths {
                Some(paths) => db.stored_path(&row.id)?.map_or_else(|| details.path.clone(), |stored| paths.location(&stored)),
                None => String::new(),
            };
            let mut values = serde_json::Map::new();
            for &column in &wanted {
                let value: Value = match column {
                    "size" => json!(details.file_size),
                    "discNo" => json!(details.disc_number),
                    "albumArtist" => json!(details.album_artist),
                    "composer" => json!(details.composer),
                    "lyricist" => json!(details.lyricist),
                    "fileType" => json!(details.file_type),
                    "year" => json!(details.year),
                    "mixName" => json!(details.mix_name),
                    "remixer" => json!(details.remixer),
                    "originalArtist" => json!(details.original_artist),
                    "sampleRate" => json!(details.sample_rate),
                    "bitrate" => json!(details.bitrate),
                    "bitDepth" => json!(details.bit_depth),
                    "location" => json!(location),
                    "dateCreated" => json!(details.date_created),
                    "publishTrackInfo" => json!(details.publish),
                    "message" => json!(details.message),
                    "color" => json!(details.color.parse::<u8>().unwrap_or(0)),
                    "djPlayCount" => json!(details.play_count),
                    "myTag" => json!(rbl_db::details::my_tag_names(db.connection(), &row.id).join(", ")),
                    "trackNumber" => json!(details.track_number),
                    "cloud" => json!(cloud),
                    _ => continue,
                };
                values.insert(column.to_owned(), value);
            }
            row.extra = Some(values);
        }
        Ok(())
    }).map_err(write_error)
}

#[tauri::command]
pub async fn view_ids_in_range(
    state: State<'_, Arc<AppState>>,
    view_id: u32,
    from: u32,
    to: u32,
) -> AppResult<Vec<String>> {
    if let Some(device) = state.device_view(view_id) {
        return Ok(crate::device_library::ids_in_range(&device, from, to));
    }
    let library = state.library()?;
    if let Some(folder) = state.folder_view(view_id) {
        return crate::explorer::ids_in_range(library, folder, from, to).await;
    }
    let view = state.view(view_id)?;
    blocking("view_ids_in_range", move || {
        Ok(library
            .ids_in_range(&view, from as usize, to as usize)
            .into_iter()
            .map(|id| id.to_string())
            .collect())
    })
    .await
}

/// Waveform bytes for a track, as raw bytes rather than JSON.
///
/// A colour waveform is a few kilobytes of numbers; sending it as a JSON array
/// would be several times larger and cost a parse on the UI thread. `tauri`
/// hands `Vec<u8>` to the webview as a binary response.
///
/// Returns an empty vector when the track has no analysis, which the UI draws
/// as a blank preview rather than an error.
#[tauri::command]
/// Waveform bytes for a track, as raw bytes rather than a JSON number array.
///
/// `from` and `len` window the tag, counted in entries. Absent means the whole
/// thing, which is only safe for the small tags: `PWV7` is 158 KB on a
/// five-minute track, far past the 64 KB response cap, so the detail view asks
/// for the span it is about to draw.
pub async fn track_waveform(
    state: State<'_, Arc<AppState>>,
    track_id: String,
    kind: String,
    from: Option<u32>,
    len: Option<u32>,
) -> AppResult<tauri::ipc::Response> {
    let library = match state.library() {
        Ok(library) => library,
        Err(error) => return crate::screen_cache::cached_waveform(&track_id, &kind)
            .map(tauri::ipc::Response::new)
            .ok_or(error),
    };
    let share = state.share_root();
    blocking("track_waveform", move || waveform_bytes(&library, &share, &track_id, &kind, from, len))
        .await
        .map(tauri::ipc::Response::new)
}

pub(crate) fn waveform_bytes(
    library: &Library,
    share: &std::path::Path,
    track_id: &str,
    kind: &str,
    from: Option<u32>,
    len: Option<u32>,
) -> AppResult<Vec<u8>> {
    let Ok(numeric) = track_id.parse::<u64>() else {
        return Err(AppError::new(ErrorKind::Malformed, "That track id is not valid.")
            .with_detail(format!("track_id {track_id:?}")));
    };
    // Through the id map, not a scan. `ids` is 38,681 long and a screenful
    // of rows asks once each, which is the reason `artwork_path_of` was
    // given the map in the first place; this call was still walking the
    // whole column for every row a scroll went past.
    let Some(row) = library.row_of_id(numeric).map(|row| row as usize) else {
        return Ok(Vec::new());
    };
    let analysis_path = library.analysis_path.get(row);
    if analysis_path.is_empty() {
        return Ok(Vec::new());
    }

        // The stored path names the .DAT; the colour waveforms live in the
        // .EXT sibling and the three-band ones in .2EX.
    let dat = rbl_anlz::resolve(share, analysis_path);
        // rekordbox 7 draws the three-band waveforms, and every one of the
        // first 300 tracks checked in the reference library has them. `PWV6`
        // is the 1,200-column overview and `PWV7` the full-resolution detail,
        // both three bytes per column: low, mid, high.
    let (file, tag, stride): (std::path::PathBuf, [u8; 4], usize) = match kind {
            "bands" => (rbl_anlz::sibling(&dat, "2EX"), *b"PWV6", 3),
            "bandsDetail" => (rbl_anlz::sibling(&dat, "2EX"), *b"PWV7", 3),
            // The RGB palette's pair, six and two bytes a column.
            "colourDetail" | "detail" => (rbl_anlz::sibling(&dat, "EXT"), *b"PWV5", 2),
            "colour" | "color" => (rbl_anlz::sibling(&dat, "EXT"), *b"PWV4", 6),
            // The BLUE palette's pair, one byte a column.
            "monoDetail" => (rbl_anlz::sibling(&dat, "EXT"), *b"PWV3", 1),
            _ => (dat, *b"PWAV", 1),
    };

    let Ok(anlz) = rbl_anlz::Anlz::read(&file) else {
        // Analysis missing on disk: draw nothing rather than fail the view.
        return Ok(Vec::new());
    };
    let whole = anlz.waveform(&tag).map(|(_, data)| data).unwrap_or_default();
    Ok(window_of(whole, stride, from, len))
}

/// A short, true stereo PCM waveform window. The response is decimated to
/// per-channel min/max pairs, which preserves attacks at display resolution
/// the way a DAW waveform view does without creating another analysis file.
#[tauri::command]
pub async fn track_pcm_waveform(
    state: State<'_, Arc<AppState>>,
    track_id: String,
    from_ms: f64,
    to_ms: f64,
    columns: u32,
) -> AppResult<tauri::ipc::Response> {
    // The deck's streamer gives us the same frame-accurate seek path that
    // playback uses, without sharing or disturbing the live deck decoder.
    const RATE: u32 = 44_100;
    let library = state.library()?;
    // Eight bytes per point stays below Tauri's 64 KB IPC response cap while
    // still leaving thousands of peak buckets in the closest view, even after
    // its two-second guard on both sides.
    let columns = columns.clamp(1, 7_500) as usize;
    let from_ms = from_ms.max(0.0);
    let to_ms = to_ms.max(from_ms);
    blocking("track_pcm_waveform", move || {
        let Some(path) = library.audio_path_of(&track_id).map(std::path::PathBuf::from) else {
            return Ok(Vec::new());
        };
        if !path.exists() || to_ms <= from_ms {
            return Ok(Vec::new());
        }
        let mut stream = rbl_deck::decode::Streamer::open(&path, RATE)
            .map_err(|e| AppError::new(ErrorKind::Malformed, "That file could not be decoded.").with_detail(e.to_string()))?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a waveform span in frames, already clamped non-negative")]
        let first = (from_ms * f64::from(RATE) / 1000.0).round().max(0.0) as u64;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a waveform span in frames, already clamped non-negative")]
        let frames = ((to_ms - from_ms) * f64::from(RATE) / 1000.0).ceil().max(1.0) as usize;
        stream.seek(first).map_err(|e| AppError::new(ErrorKind::Malformed, "That file could not be decoded.").with_detail(e.to_string()))?;
        let mut pcm = vec![0.0_f32; frames * 2];
        let read = stream.fill(&mut pcm)
            .map_err(|e| AppError::new(ErrorKind::Malformed, "That file could not be decoded.").with_detail(e.to_string()))?;
        let mut out = Vec::with_capacity(columns * 8);
        for column in 0..columns {
            let start = column * frames / columns;
            let end = ((column + 1) * frames / columns).max(start + 1).min(read);
            let mut left = (f32::INFINITY, f32::NEG_INFINITY);
            let mut right = (f32::INFINITY, f32::NEG_INFINITY);
            for frame in start..end {
                let at = frame * 2;
                left.0 = left.0.min(pcm[at]);
                left.1 = left.1.max(pcm[at]);
                right.0 = right.0.min(pcm[at + 1]);
                right.1 = right.1.max(pcm[at + 1]);
            }
            if !left.0.is_finite() { left = (0.0, 0.0); }
            if !right.0.is_finite() { right = (0.0, 0.0); }
            for sample in [left.0, left.1, right.0, right.1] {
                #[allow(clippy::cast_possible_truncation, reason = "clamped to [-1.0, 1.0] * i16::MAX, so it always fits")]
                let pcm16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                out.extend_from_slice(&pcm16.to_le_bytes());
            }
        }
        Ok(out)
    }).await.map(tauri::ipc::Response::new)
}

/// The requested span of a waveform tag, clamped to what is there.
///
/// Entries rather than bytes, so a caller never has to know a tag's stride,
/// and so a window can never land mid-entry and shear the bands apart.
fn window_of(data: &[u8], stride: usize, from: Option<u32>, len: Option<u32>) -> Vec<u8> {
    let stride = stride.max(1);
    let entries = data.len() / stride;
    let first = from.map_or(0, |f| f as usize).min(entries);
    let count = len.map_or(entries - first, |l| (l as usize).min(entries - first));
    data.get(first * stride..(first + count) * stride).unwrap_or(&[]).to_vec()
}

// ---------------------------------------------------------------- editing

/// What an edit changed, and therefore how much has to be re-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Touched {
    /// Only the playlist tree. Re-reading it costs 24 ms against 233 ms for
    /// the whole library, and it is by far the most common kind of edit.
    Playlists,
    /// A track column changed, so the ranks and the search arena are stale.
    Tracks,
    /// Only the Tag List.
    TagList,
    /// Existing tracks: rating, colour, comment, or play count.
    Metadata(Vec<String>),
    /// History membership, optionally with play counts to refresh.
    Histories(Vec<String>),
}

impl Touched {
    /// The event that tells the window. A Tag List edit leaves every other
    /// view as it was, so it has its own rather than `library:changed`,
    /// which makes every open list fetch its rows again.
    pub(crate) fn event(&self) -> &'static str {
        if matches!(self, Self::TagList) { "tag-list:changed" } else { "library:changed" }
    }
}

/// Commits an edit and refreshes the affected index on the same connection.
pub(crate) async fn edit<R: tauri::Runtime, F>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    name: &'static str,
    touched: Touched,
    action: F,
) -> AppResult<u32>
where
    F: FnOnce(&mut rbl_db::write::Writer) -> Result<(), rbl_db::DbError> + Send + 'static,
{
    let state = Arc::clone(&state);
    let event = touched.event();
    let (generation, history) = blocking(name, move || {
        let _gate = state.edit_gate.lock();
        let generation = state.write_then(action, |db, ()| refresh_after_edit(&state, db, touched)).map_err(write_error)?;
        // Any non-recorded edit after an undo starts a new branch.
        let mut history = state.edit_history.lock();
        history.clear_redo();
        Ok((generation, history_dto(generation, &history)))
    }).await?;
    let _ = tauri::Emitter::emit(&app, event, generation);
    let _ = tauri::Emitter::emit(&app, "edit-history:changed", history);
    Ok(generation)
}

/// Commits an edit that must never be traversed by undo and invalidates all
/// older tokens that could refer to rows the edit permanently removes.
pub(crate) async fn permanent_edit<R: tauri::Runtime, F>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    name: &'static str,
    touched: Touched,
    action: F,
) -> AppResult<u32>
where
    F: FnOnce(&mut rbl_db::write::Writer) -> Result<(), rbl_db::DbError> + Send + 'static,
{
    let state = Arc::clone(&state);
    let event = touched.event();
    let (generation, history) = blocking(name, move || {
        let _gate = state.edit_gate.lock();
        let generation = state.write_then(action, |db, ()| refresh_after_edit(&state, db, touched))
            .map_err(write_error)?;
        let mut history = state.edit_history.lock();
        history.clear();
        Ok((generation, history_dto(generation, &history)))
    }).await?;
    let _ = tauri::Emitter::emit(&app, event, generation);
    let _ = tauri::Emitter::emit(&app, "edit-history:changed", history);
    Ok(generation)
}

fn history_dto(generation: u32, history: &EditHistory) -> EditHistoryDto {
    EditHistoryDto {
        generation,
        can_undo: !history.undo.is_empty(),
        can_redo: !history.redo.is_empty(),
        undo_label: history.undo.last().map(|entry| entry.label.to_owned()),
        redo_label: history.redo.last().map(|entry| entry.label.to_owned()),
    }
}

pub(crate) async fn recorded_edit<R: tauri::Runtime, F>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    name: &'static str,
    touched: Touched,
    label: &'static str,
    action: F,
) -> AppResult<EditHistoryDto>
where
    F: FnOnce(&mut rbl_db::write::Writer) -> Result<LibraryEdit, rbl_db::DbError> + Send + 'static,
{
    let state = Arc::clone(&state);
    let dto = blocking(name, move || {
        let _gate = state.edit_gate.lock();
        let (generation, reversible) = state.write_then(
            action,
            |db, reversible| refresh_after_edit(&state, db, touched).map(|generation| (generation, reversible)),
        ).map_err(write_error)?;
        let mut history = state.edit_history.lock();
        if !reversible.is_empty() {
            history.record(reversible, label);
        }
        Ok(history_dto(generation, &history))
    }).await?;
    let _ = tauri::Emitter::emit(&app, "library:changed", dto.generation);
    let _ = tauri::Emitter::emit(&app, "edit-history:changed", dto.clone());
    Ok(dto)
}

fn touched_by(edit: &LibraryEdit) -> Touched {
    match edit {
        LibraryEdit::DeletePlaylist(_) | LibraryEdit::RenamePlaylist(_) |
        LibraryEdit::MovePlaylist(_) | LibraryEdit::RemovePlaylistTracks(_) => Touched::Playlists,
        // Tokens keep their database row ids private; a full reload after an
        // undo is uncommon and guarantees every view and sort follows it.
        LibraryEdit::Track(_) | LibraryEdit::TrackTags(_) => Touched::Tracks,
    }
}

fn apply_history(writer: &mut rbl_db::write::Writer, edit: &LibraryEdit, undo: bool) -> Result<(), rbl_db::DbError> {
    match edit {
        LibraryEdit::DeletePlaylist(value) => if undo { writer.restore_playlist(value) } else { writer.redo_playlist_deletion(value) }.map(|_| ()),
        LibraryEdit::RenamePlaylist(value) => if undo { writer.undo_rename(value) } else { writer.redo_rename(value) }.map(|_| ()),
        LibraryEdit::MovePlaylist(value) => if undo { writer.undo_move(value) } else { writer.redo_move(value) }.map(|_| ()),
        LibraryEdit::RemovePlaylistTracks(value) => if undo { writer.undo_track_removal(value) } else { writer.redo_track_removal(value) }.map(|_| ()),
        LibraryEdit::Track(values) => {
            let ordered: Box<dyn Iterator<Item = _>> = if undo {
                Box::new(values.iter().rev())
            } else {
                Box::new(values.iter())
            };
            for value in ordered {
                if undo { writer.undo_track_edit(value)?; } else { writer.redo_track_edit(value)?; }
            }
            Ok(())
        }
        LibraryEdit::TrackTags(value) => if undo {
            writer.undo_tag_edit(value)
        } else {
            writer.redo_tag_edit(value)
        }.map(|_| ()),
    }
}

/// Shared by desktop and CDJ edits; the writer holds the edit gate until
/// both persistence and the new index are visible.
pub(crate) fn refresh_after_edit(state: &AppState, db: &rbl_db::Library, touched: Touched) -> Result<u32, rbl_db::DbError> {
    match touched {
        Touched::Metadata(ids) => state.refresh_metadata(db, &ids, false),
        Touched::Histories(ids) if !ids.is_empty() => state.refresh_metadata(db, &ids, true),
        Touched::Tracks => {
            let started = std::time::Instant::now();
            let (library, _) = rbl_index::load(db)?;
            let load_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            state.set_library(library, rbl_db::is_rekordbox_running(), db.schema().db_version, load_ms, db.location().clone());
            Ok(state.summary().3)
        }
        touched => {
            let library = state.library().map_err(|e| rbl_db::DbError::Open(e.to_string()))?;
            match touched {
                Touched::TagList => {
                    library.set_tag_list(rbl_index::reload_tag_list(db, &library)?);
                    return Ok(state.invalidate_tag_list_views());
                }
                Touched::Playlists => library.set_playlists(rbl_index::reload_playlists(db, &library)?),
                Touched::Histories(_) => library.set_histories(rbl_index::reload_histories(db, &library)?),
                _ => unreachable!("track changes handled above"),
            }
            Ok(state.invalidate_views())
        }
    }
}

/// Maps a database refusal onto the error kind the frontend distinguishes.
pub(crate) fn write_error(error: rbl_db::DbError) -> AppError {
    match error {
        rbl_db::DbError::WriteRefused(reason) => AppError::new(ErrorKind::ReadOnly, reason),
        other => AppError::new(ErrorKind::Internal, other.to_string()),
    }
}

/// Re-reads the library on request: what the analysis queue asks for once
/// it has drained, so a run of a hundred tracks costs one reload, not a
/// hundred.
#[tauri::command]
pub async fn reload_library<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<u32> {
    reload(app, Arc::clone(&state)).await
}

/// Re-reads the library and returns the new generation.
pub(crate) async fn reload<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: Arc<AppState>) -> AppResult<u32> {
    let generation = blocking("reload", move || {
        let _gate = state.edit_gate.lock();
        let db = state.open_read_only().map_err(write_error)?;
        let db_version = db.schema().db_version;
        let location = db.location().clone();
        let started = std::time::Instant::now();
        let (library, _) = rbl_index::load(&db)
            .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))?;
        let load_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let read_only = rbl_db::is_rekordbox_running();
        state.set_library(library, read_only, db_version, load_ms, location);
        Ok(state.summary().3)
    })
    .await?;
    // Cached pages are keyed on the generation, so the frontend drops them.
    let _ = tauri::Emitter::emit(&app, "library:changed", generation);
    Ok(generation)
}

/// LINK as it stands: on or off, on which interface, and who is listening.
///
/// When it is off, the status carries why it cannot come on right now (usually
/// rekordbox holding the ports), so the shell can warn before the button is
/// ever pressed.
#[tauri::command]
pub async fn link_status(state: State<'_, Arc<AppState>>) -> AppResult<LinkStatusDto> {
    Ok(state.link_status().unwrap_or_else(|| LinkStatusDto::off(crate::link::refusal())))
}

/// The players and mixers heard on the network, whether or not LINK is on.
/// The shell shows the LINK button once this is non-empty.
#[tauri::command]
pub async fn link_peers(state: State<'_, Arc<AppState>>) -> AppResult<Vec<crate::link::PeerDto>> {
    Ok(crate::link::peers(&state))
}

/// Turns LINK on: announces as `rekordbox` on `interface` (the first one
/// when none is named) and serves the library to every player that asks.
///
/// Refused while rekordbox runs — it holds the ports. Starting binds seven
/// sockets and walks every track's path, so it runs off the async thread.
#[tauri::command]
pub async fn start_link_export<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    interface: Option<String>,
    device_settings: Option<rbl_prolink::DeviceSettings>,
    alphabetical_keys: Option<bool>,
) -> AppResult<LinkStatusDto> {
    if let Some(status) = state.link_status() {
        tracing::debug!("LINK asked to start while running; the running session stands");
        return Ok(status);
    }
    if let Some(problem) = crate::link::refusal() {
        tracing::warn!(%problem, "LINK refused");
        return Ok(LinkStatusDto::off(Some(problem)));
    }
    tracing::info!(interface = interface.as_deref().unwrap_or("auto"), "LINK starting");
    let owner = Arc::clone(&state);
    let emitter = app.clone();
    let library_emitter = app.clone();
    let device_settings = device_settings.unwrap_or_default();
    let started = blocking("start_link_export", move || {
        let key_order = if alphabetical_keys.unwrap_or(false) {
            rbl_link::KeyOrder::Alphabetical
        } else {
            rbl_link::KeyOrder::Musical
        };
        Ok(crate::link::Session::start(
            &owner,
            interface.as_deref(),
            device_settings,
            key_order,
            move |status| {
                let _ = tauri::Emitter::emit(&emitter, "link:status", status);
            },
            Arc::new(move |event, generation| {
                let _ = tauri::Emitter::emit(&library_emitter, event, generation);
            }),
        ))
    })
    .await?;
    match started {
        Ok(session) => {
            let status = session.status(state.library().ok().as_deref());
            // A session started twice at once: the second is dropped
            // outside the lock, which unbinds it.
            drop(state.set_link(Some(session)));
            let _ = tauri::Emitter::emit(&app, "link:status", status.clone());
            Ok(status)
        }
        Err(problem) => {
            tracing::warn!(%problem, "LINK could not start");
            Ok(LinkStatusDto::off(Some(problem)))
        }
    }
}

/// Turns LINK off: the players lose the source.
#[tauri::command]
pub async fn stop_link_export<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<LinkStatusDto> {
    // Dropped outside the lock, and off the async thread: stopping joins
    // the servers' threads.
    let session = state.set_link(None);
    if session.is_some() {
        tracing::info!("LINK stopping");
    } else {
        tracing::debug!("LINK asked to stop while off");
    }
    blocking("stop_link_export", move || {
        drop(session);
        Ok(())
    })
    .await?;
    let status = LinkStatusDto::off(None);
    let _ = tauri::Emitter::emit(&app, "link:status", status.clone());
    Ok(status)
}

/// Tells a CDJ on the link to load a specific track from our library.
#[tauri::command]
pub async fn link_load_track(
    state: State<'_, Arc<AppState>>,
    player_number: u8,
    track_id: String,
) -> AppResult<()> {
    let id: u32 = track_id.parse().map_err(|_| AppError::internal(format!("bad track id: {track_id}")))?;
    tracing::info!(player_number, track_id = id, "asking a player to load a track");
    state.link_load_track(player_number, id).map_err(|reason| {
        tracing::warn!(player_number, track_id = id, %reason, "the player could not be asked");
        AppError::internal(reason)
    })
}

/// Becomes the network's tempo master, or resigns, and returns LINK's fresh
/// status.
#[tauri::command]
pub async fn link_set_master(state: State<'_, Arc<AppState>>, on: bool) -> AppResult<LinkStatusDto> {
    state.link_set_master(on);
    Ok(state.link_status().unwrap_or_else(|| LinkStatusDto::off(crate::link::refusal())))
}

/// Nudges the master tempo by `delta_bpm` (rekordbox's −/+ is ±1), and
/// returns LINK's fresh status.
#[tauri::command]
pub async fn link_nudge_master(state: State<'_, Arc<AppState>>, delta_bpm: f64) -> AppResult<LinkStatusDto> {
    state.link_nudge_master(delta_bpm);
    Ok(state.link_status().unwrap_or_else(|| LinkStatusDto::off(crate::link::refusal())))
}

/// Takes the current master player's tempo as the master tempo (rekordbox's
/// ⟳), and returns LINK's fresh status.
#[tauri::command]
pub async fn link_take_master_tempo(state: State<'_, Arc<AppState>>) -> AppResult<LinkStatusDto> {
    state.link_take_master_tempo();
    Ok(state.link_status().unwrap_or_else(|| LinkStatusDto::off(crate::link::refusal())))
}

/// Writes a playlist to a stick: the tree's Export Playlist / Export Folder.
///
/// Copies the audio, re-emits the analysis, and writes `export.pdb` and
/// `exportLibrary.db`. Never re-analyses: an export moves what the library
/// already knows.
///
/// The playlist is added beside what the stick already holds, as rekordbox's
/// Export Playlist adds one ("Export the selected playlist to the device",
/// rekordbox 7.2.18 manual p. 256); only the Sync Manager's SYNC makes a
/// stick hold exactly a selection (#304).
#[tauri::command]
pub async fn export_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    playlist: String,
    destination: String,
    // What a stick with no settings of its own is given; see the DJ System
    // pane. A stick that has settings keeps them.
    defaults: Option<crate::device_settings::StickDefaultsDto>,
    delete_unlisted_music: Option<bool>,
    compatibility_format: Option<rbl_export::CompatibilityFormat>,
) -> AppResult<ExportReportDto> {
    let library = state.library()?;
    let share = state.share_root();
    let state = Arc::clone(&state);
    let progress_app = app.clone();
    let report = blocking("export_playlist", move || {
        let destination = std::path::Path::new(&destination);
        let selection = playlist_export_selection(&state, &library, &share, destination, &playlist, delete_unlisted_music.unwrap_or(false))?;
        write_export_with_progress(&progress_app, destination, &selection, defaults.as_ref(), compatibility_format)
    })
    .await?;

    let _ = tauri::Emitter::emit(&app, "export:done", &report);
    Ok(report)
}

/// The library playlists a stick holds from this library, in the order its
/// manifest lists them, and whether its record asks for automatic sync:
/// re-exported with what they hold now when something is added.
///
/// Only a manifest written from this library counts: another library's ids
/// name other playlists. A manifest from before rbxport recorded the library
/// counts when rekordbox's sync record names this library, as the export
/// itself accepts it. Folders, playlists since deleted from the library, and
/// the stick's own playlists (made on a player or by rekordbox) are left
/// out: an export with `keep_unlisted` keeps them as the stick has them.
fn held_on_stick(state: &AppState, library: &rbl_index::Library, stick: &std::path::Path) -> (Vec<String>, bool) {
    let Some(manifest) = rbl_export::Manifest::load(stick) else { return (Vec::new(), false) };
    let db_id = state.read_db(|db| rbl_db::export_info::db_id(db.connection())).unwrap_or(0);
    let record = rbl_export::sync_record::read(stick);
    let ours = manifest.db_id == db_id
        || (manifest.db_id == 0 && db_id != 0 && record.as_ref().is_some_and(|r| r.db_id == db_id));
    if !ours {
        return (Vec::new(), false);
    }
    let playlists = library.playlists();
    let held = manifest
        .playlists
        .iter()
        .filter(|p| !p.folder && !p.device_only && p.library_id != 0 && playlists.index_of(p.library_id).is_some())
        .map(|p| p.library_id.to_string())
        .collect();
    (held, record.is_some_and(|r| r.automatic))
}

/// What Export Playlist / Export Folder writes to `stick`: what it already
/// holds, then `chosen` (a playlist or a folder) after it.
fn playlist_export_selection(
    state: &AppState,
    library: &rbl_index::Library,
    share: &std::path::Path,
    stick: &std::path::Path,
    chosen: &str,
    delete_unlisted_music: bool,
) -> AppResult<ExportSelection> {
    if !stick.is_dir() {
        return Err(AppError::new(ErrorKind::NotFound, "That device is no longer connected."));
    }
    let (mut playlists, automatic) = held_on_stick(state, library, stick);
    if !playlists.iter().any(|id| id == chosen) {
        playlists.push(chosen.to_owned());
    }
    let mut selection = ExportSelection::from_playlists(state, library, share, &playlists, automatic)?;
    selection.keep_unlisted = true;
    let kept = selection.for_stick(state, library, share, stick, delete_unlisted_music)?;
    Ok(match kept {
        std::borrow::Cow::Owned(kept) => kept,
        std::borrow::Cow::Borrowed(_) => selection,
    })
}

/// Writes the same playlists to every destination, and says how each fared.
///
/// The Sync Manager's SYNC. The selection is built once — each track read
/// once however many sticks it goes to — and written to every selected stick
/// concurrently, so two sticks synced together hold the same thing. One stick failing (pulled,
/// full, refusing a write) must not stop the rest: the outcome is per stick,
/// and only building the selection can fail the whole run. Every export owns
/// its progress event, so the window can show each stick moving independently.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "the Sync Manager's own settings, one per IPC field the frontend already sends")]
pub async fn sync_devices<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    playlists: Vec<String>,
    destinations: Vec<String>,
    // What a stick with no settings of its own is given; see the DJ System
    // pane. A stick that has settings keeps them.
    defaults: Option<crate::device_settings::StickDefaultsDto>,
    // The Sync Manager's "Automatic synchronization": recorded on each
    // stick, and read back when it is next plugged in.
    automatic: Option<bool>,
    eject_after_sync: Option<bool>,
    delete_unlisted_music: Option<bool>,
    compatibility_format: Option<rbl_export::CompatibilityFormat>,
) -> AppResult<Vec<SyncDeviceReportDto>> {
    let library = state.library()?;
    let share = state.share_root();
    let state = Arc::clone(&state);
    blocking("sync_devices", move || {
        let selection =
            ExportSelection::from_playlists(&state, &library, &share, &playlists, automatic.unwrap_or(false))?;
        // The selection contains owned tracks and analysis. Clone it for each
        // worker rather than rereading the library, then preserve the user's
        // device order when collecting reports.
        std::thread::scope(|scope| {
            let workers: Vec<_> = destinations.into_iter().map(|destination| {
                let app = app.clone();
                let state = Arc::clone(&state);
                let library = Arc::clone(&library);
                let selection = selection.clone();
                let share = share.clone();
                let defaults = defaults.clone();
                scope.spawn(move || sync_one_device(
                    &app, &state, &library, &share, &selection, destination,
                    defaults.as_ref(), delete_unlisted_music.unwrap_or(false),
                    eject_after_sync.unwrap_or(false), compatibility_format,
                ))
            }).collect();
            Ok(workers.into_iter().map(|worker| worker.join().unwrap_or_else(|_| SyncDeviceReportDto {
                path: "Unknown device".to_owned(), report: None,
                error: Some("The sync worker stopped unexpectedly.".to_owned()),
                ejected: false, eject_error: None,
            })).collect())
        })
    })
    .await
}

/// Checks the exact playlist selection before any USB is touched.
#[tauri::command]
pub async fn validate_export_files(
    state: State<'_, Arc<AppState>>,
    playlists: Vec<String>,
) -> AppResult<Vec<MissingExportFileDto>> {
    let library = state.library()?;
    let share = state.share_root();
    let state = Arc::clone(&state);
    blocking("validate_export_files", move || {
        let selection =
            ExportSelection::from_playlists(&state, &library, &share, &playlists, false)?;
        Ok(selection.tracks.into_iter().filter_map(|track| {
            if track.source_path.is_file() {
                return None;
            }
            Some(MissingExportFileDto {
                title: if track.title.is_empty() {
                    "Untitled track".to_owned()
                } else {
                    track.title
                },
                path: track.source_path.to_string_lossy().into_owned(),
            })
        })
        .collect())
    })
    .await
}

#[allow(clippy::too_many_arguments, reason = "one independent USB sync worker")]
fn sync_one_device<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>, state: &AppState, library: &rbl_index::Library,
    share: &std::path::Path, selection: &ExportSelection, destination: String,
    defaults: Option<&crate::device_settings::StickDefaultsDto>, delete_unlisted_music: bool,
    eject_after_sync: bool, compatibility_format: Option<rbl_export::CompatibilityFormat>,
) -> SyncDeviceReportDto {
    let progress = |state: &'static str| {
        let _ = tauri::Emitter::emit(app, "sync:progress", SyncProgressDto { path: destination.clone(), state });
    };
    progress("writing");
    let stick = std::path::Path::new(&destination);
    let written = selection.for_stick(state, library, share, stick, delete_unlisted_music)
        .and_then(|selection| write_export_with_progress(app, stick, &selection, defaults, compatibility_format));
    match written {
        Ok(report) => {
            let mut result = SyncDeviceReportDto { path: destination.clone(), report: Some(report), error: None, ejected: false, eject_error: None };
            if eject_after_sync {
                if result.report.as_ref().is_some_and(|report| report.verified && report.skipped.is_empty() && report.failed.is_empty()) {
                    progress("ejecting");
                    set_export_stage(app, stick, "ejecting");
                    match rbl_devices::eject::eject(stick) {
                        Ok(()) => result.ejected = true,
                        Err(e) => result.eject_error = Some(e.to_string()),
                    }
                } else {
                    result.eject_error = result.report.as_ref().map(|report| {
                        let mut causes = Vec::new();
                        if !report.skipped.is_empty() {
                            causes.push(format!("{} missing or unreadable", report.skipped.len()));
                        }
                        if !report.failed.is_empty() {
                            causes.push(format!("{} could not be written", report.failed.len()));
                        }
                        if !report.verified {
                            causes.push("the written library did not read back correctly".to_owned());
                        }
                        format!(
                            "The sync was incomplete or could not be verified ({}). Review it before ejecting.",
                            causes.join(", ")
                        )
                    });
                }
            }
            progress("done");
            if eject_after_sync { set_export_stage(app, stick, "done"); }
            result
        },
        Err(e) => {
            progress("failed");
            tracing::warn!(destination, error = %e, detail = e.detail.as_deref().unwrap_or(""), "sync to one device failed");
            // The Sync Manager shows this line on its own, so an internal
            // error's detail — what actually failed — goes with its summary.
            let message = e.full_message();
            set_export_failure(app, stick, message.clone());
            SyncDeviceReportDto { path: destination, report: None, error: Some(message), ejected: false, eject_error: None }
        }
    }
}

/// What syncing `playlists` would use and give back on each stick, from the
/// record each one holds. Writes nothing to a stick or the library.
#[tauri::command]
pub async fn estimate_sync(
    state: State<'_, Arc<AppState>>,
    playlists: Vec<String>,
    destinations: Vec<String>,
    delete_unlisted_music: Option<bool>,
    compatibility_format: Option<rbl_export::CompatibilityFormat>,
) -> AppResult<Vec<crate::dto::SyncEstimateDto>> {
    let library = state.library()?;
    let share = state.share_root();
    let state = Arc::clone(&state);
    blocking("estimate_sync", move || {
        let selection = ExportSelection::from_playlists(&state, &library, &share, &playlists, false)?;
        Ok(destinations
            .into_iter()
            .map(|path| {
                let found = rbl_export::estimate::estimate(
                    std::path::Path::new(&path),
                    selection.sync.db_id,
                    &selection.tracks,
                    delete_unlisted_music.unwrap_or(false),
                    compatibility_format,
                );
                tracing::debug!(
                    %path, selected = selection.tracks.len(), copy_bytes = found.copy_bytes, reuse_bytes = found.reuse_bytes,
                    free_bytes = found.free_bytes, new = found.tracks_new, changed = found.tracks_changed,
                    kept = found.tracks_kept, removed = found.tracks_removed, missing = found.tracks_missing,
                    "sync estimate"
                );
                crate::dto::SyncEstimateDto {
                    path,
                    copy_bytes: found.copy_bytes,
                    reuse_bytes: found.reuse_bytes,
                    free_bytes: found.free_bytes,
                    tracks_new: found.tracks_new,
                    tracks_changed: found.tracks_changed,
                    tracks_kept: found.tracks_kept,
                    tracks_removed: found.tracks_removed,
                    tracks_missing: found.tracks_missing,
                    approximate: found.approximate,
                }
            })
            .collect())
    })
    .await
}

/// Export Track: puts tracks on a stick on their own, in no playlist,
/// beside what the stick's record says it holds — its playlists, synced
/// again from the library, and the tracks put there this way before.
#[tauri::command]
pub async fn export_tracks_to_device<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    destination: String,
    defaults: Option<crate::device_settings::StickDefaultsDto>,
    compatibility_format: Option<rbl_export::CompatibilityFormat>,
) -> AppResult<ExportReportDto> {
    let library = state.library()?;
    let share = state.share_root();
    let state = Arc::clone(&state);
    let progress_app = app.clone();
    let report = blocking("export_tracks_to_device", move || {
        let stick = std::path::Path::new(&destination);
        let selection = track_export_selection(&state, &library, &share, stick, &tracks)?;
        write_export_with_progress(&progress_app, stick, &selection, defaults.as_ref(), compatibility_format)
    })
    .await?;
    let _ = tauri::Emitter::emit(&app, "export:done", &report);
    Ok(report)
}

/// What Export Track writes to `stick`: what it already holds, with
/// `tracks` added in no playlist.
fn track_export_selection(
    state: &AppState,
    library: &rbl_index::Library,
    share: &std::path::Path,
    stick: &std::path::Path,
    tracks: &[String],
) -> AppResult<ExportSelection> {
    if !stick.is_dir() {
        return Err(AppError::new(ErrorKind::NotFound, "That device is no longer connected."));
    }
    let (playlists, automatic) = held_on_stick(state, library, stick);
    let mut loose = rbl_export::Manifest::load(stick).map(|m| m.loose).unwrap_or_default();
    for id in tracks.iter().filter_map(|t| t.parse::<u64>().ok()) {
        if !loose.contains(&id) {
            loose.push(id);
        }
    }
    let mut selection = ExportSelection::from_playlists_and_tracks(state, library, share, &playlists, &loose, automatic)?;
    selection.keep_unlisted = true;
    Ok(selection)
}

/// What a stick was last synced with, and what it holds.
///
/// The Sync Manager ticks a stick's last selection back on when the stick
/// is ticked, so a weekly sync is two clicks rather than a hunt through the
/// tree. That comes from our manifest, or failing that from the sync record
/// rekordbox leaves (`playlists3.sync`), which names the playlists by their
/// library ids — so a stick rekordbox synced from this library offers its
/// selection too, and its "Automatic synchronization" tick. What it holds
/// comes from `export.pdb` itself, whoever wrote it, because that is what
/// the player will show.
#[tauri::command]
pub async fn device_sync_state(state: State<'_, Arc<AppState>>, path: String) -> AppResult<DeviceSyncStateDto> {
    let library = state.library().ok();
    let state = Arc::clone(&state);
    blocking("device_sync_state", move || {
        let mount = std::path::Path::new(&path);
        if !mount.is_dir() {
            return Err(AppError::new(ErrorKind::NotFound, "That device is no longer connected."));
        }
        let record = rbl_export::sync_record::read(mount);
        // Only read when there is a record to match it against.
        let db_id = if record.is_some() {
            state.read_db(|db| rbl_db::export_info::db_id(db.connection())).unwrap_or(0)
        } else {
            0
        };
        // rekordbox's record names another library's playlists by ids this
        // one does not have; only a record from this library is a selection.
        let ours = record.as_ref().filter(|r| db_id != 0 && r.db_id == db_id);
        let mut selected: Vec<SyncPlaylistDto> = rbl_export::Manifest::load(mount)
            .filter(|m| m.db_id == db_id && db_id != 0)
            .map(|manifest| {
                manifest
                    .playlists
                    .into_iter()
                    .filter(|p| !p.folder)
                    .map(|playlist| SyncPlaylistDto {
                        library_id: playlist.library_id.to_string(),
                        name: playlist.name,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if selected.is_empty() {
            if let (Some(record), Some(library)) = (ours, library.as_ref()) {
                let playlists = library.playlists();
                selected = record
                    .ticked
                    .iter()
                    .filter_map(|&id| {
                        let index = playlists.index_of(id)?;
                        Some(SyncPlaylistDto { library_id: id.to_string(), name: playlists.name(index).to_owned() })
                    })
                    .collect();
            }
        }
        Ok(DeviceSyncStateDto {
            selected,
            on_device: playlists_on_device(mount),
            libraries: crate::usb_import::library_trees(mount)?,
            automatic: ours.is_some_and(|r| r.automatic),
        })
    })
    .await
}

/// The playlist names in a stick's `export.pdb`, in tree order, folders
/// left out. Empty when there is no export or it does not parse: a stick
/// that cannot be read holds nothing the window can name.
fn playlists_on_device(mount: &std::path::Path) -> Vec<String> {
    let pdb = rbl_devices::settings::export_root(mount).join("rekordbox/export.pdb");
    let Ok(bytes) = std::fs::read(&pdb) else { return Vec::new() };
    let Ok(parsed) = rbl_pdb::Pdb::parse(&bytes) else { return Vec::new() };
    let Some(table) = parsed.table(rbl_pdb::PageType::PlaylistTree) else { return Vec::new() };
    let mut nodes = parsed.playlist_nodes(table);
    nodes.sort_by_key(|node| (node.parent_id, node.sort_order));
    nodes.into_iter().filter(|node| !node.is_folder).map(|node| node.name).collect()
}

/// A My Tag row as the export takes it.
pub(crate) fn source_my_tag(tag: &rbl_db::export_info::MyTagRow) -> rbl_export::SourceMyTag {
    rbl_export::SourceMyTag {
        id: tag.id.parse().unwrap_or(0),
        seq: u32::try_from(tag.seq.max(0)).unwrap_or(u32::MAX),
        name: tag.name.clone(),
        attribute: u8::try_from(tag.attribute.clamp(0, 255)).unwrap_or(0),
        parent: tag.parent.parse().unwrap_or(0),
    }
}

/// What an export is asked to write: the tracks, the playlists that name
/// them by index, and the library's My Tags. Built once and written to as
/// many sticks as asked.
#[derive(Clone)]
pub(crate) struct ExportSelection {
    pub tracks: Vec<rbl_export::SourceTrack>,
    pub playlists: Vec<rbl_export::SourcePlaylist>,
    /// Every My Tag of the library, listed on the stick whole as rekordbox
    /// lists them.
    pub my_tags: Vec<rbl_export::SourceMyTag>,
    /// What the stick's sync record names: the library and its tree.
    pub sync: rbl_export::SyncSource,
    /// Add to what the stick holds (Export Playlist, Export Track) rather
    /// than make it hold exactly this (a SYNC): see
    /// [`rbl_export::ExportOptions::keep_unlisted`].
    pub keep_unlisted: bool,
}

/// What exporting the folder at `folder` writes, in tree order: the
/// playlists (plain and intelligent) under it at any depth, and the folder
/// itself with every folder under it, so empty ones keep their place.
fn folder_contents(playlists: &rbl_index::Playlists, folder: usize) -> (Vec<usize>, Vec<usize>) {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); playlists.len()];
    for index in 0..playlists.len() {
        if let Some(parent) = playlists.parent.get(index).copied().filter(|&p| p != rbl_index::NO_ID) {
            if let Some(bucket) = children.get_mut(parent as usize) {
                bucket.push(index);
            }
        }
    }
    let (mut leaves, mut folders) = (Vec::new(), Vec::new());
    // Iterative, with a visited set, as the tree builder is: a corrupt
    // parent cycle must not loop forever.
    let mut visited = vec![false; playlists.len()];
    let mut stack = vec![folder];
    while let Some(index) = stack.pop() {
        match visited.get_mut(index) {
            Some(seen) if !*seen => *seen = true,
            _ => continue,
        }
        if playlists.is_folder(index) {
            folders.push(index);
            if let Some(under) = children.get(index) {
                stack.extend(under.iter().rev());
            }
        } else {
            leaves.push(index);
        }
    }
    (leaves, folders)
}

impl ExportSelection {
    /// The union of these playlists, each track read once however many of
    /// them hold it. Ids are the tree's numeric playlist ids. An intelligent
    /// playlist is exported as what its rule admits now, which is what
    /// rekordbox writes to a stick for one too. A folder stands for every
    /// playlist under it, and goes on the stick as a folder with them inside.
    pub(crate) fn from_playlists(
        state: &AppState,
        library: &rbl_index::Library,
        share: &std::path::Path,
        playlist_ids: &[String],
        // Written into the stick's sync record: whether it is to be synced
        // again, on its own, when it is next plugged in.
        automatic: bool,
    ) -> AppResult<Self> {
        Self::from_playlists_and_tracks(state, library, share, playlist_ids, &[], automatic)
    }

    /// [`from_playlists`](Self::from_playlists) with tracks that go on the
    /// stick in no playlist: what Export Track put there, and what a
    /// stick's record says was put there before.
    pub(crate) fn from_playlists_and_tracks(
        state: &AppState,
        library: &rbl_index::Library,
        share: &std::path::Path,
        playlist_ids: &[String],
        loose: &[u64],
        automatic: bool,
    ) -> AppResult<Self> {
        let _editing = state.edit_gate.lock();
        let _analysis = state.analysis_write.lock();
        // Named first and read after: `source_rows` takes the playlists
        // itself, so the guard is let go before it is asked.
        let mut named: Vec<(u64, String, rbl_index::TrackSource)> = Vec::with_capacity(playlist_ids.len());
        // Folders asked for by name, kept on the stick even when nothing
        // under them is a playlist.
        let mut chosen_folders: Vec<u64> = Vec::new();
        {
            let playlists = library.playlists();
            let mut seen = std::collections::HashSet::new();
            for id in playlist_ids {
                let Some(index) = id.parse::<u64>().ok().and_then(|numeric| playlists.index_of(numeric)) else {
                    return Err(AppError::new(ErrorKind::NotFound, "That playlist is not in the library."));
                };
                // A folder has no tracks of its own: exported as a playlist
                // it would land on the stick empty.
                let (leaves, folders) = if playlists.is_folder(index) {
                    folder_contents(&playlists, index)
                } else {
                    (vec![index], Vec::new())
                };
                chosen_folders.extend(folders.into_iter().filter_map(|folder| playlists.ids.get(folder).copied()));
                for index in leaves {
                    if !seen.insert(index) {
                        continue;
                    }
                    let source = if playlists.is_smart(index) {
                        rbl_index::TrackSource::SmartPlaylist(index)
                    } else {
                        rbl_index::TrackSource::Playlist(index)
                    };
                    named.push((playlists.ids.get(index).copied().unwrap_or(0), playlists.name(index).to_owned(), source));
                }
            }
        }
        let rows_of: Vec<Vec<u32>> = named.iter().map(|(_, _, source)| library.source_rows(source)).collect();
        // A loose track the library no longer has is left off without a
        // word: the stick's record outlives the track.
        let loose_rows: Vec<u32> = loose.iter().filter_map(|&id| library.row_of_id(id)).collect();

        // What the index does not hold: the My Tags, and the other places a
        // cloud-synced file may be. One read for the whole selection.
        let ids: Vec<String> = rows_of
            .iter()
            .flatten()
            .chain(loose_rows.iter())
            .map(|&row| library.ids.get(row as usize).copied().unwrap_or(0).to_string())
            .collect();
        let (my_tags, extras, db_id) = state
            .read_db(|db| {
                let conn = db.connection();
                Ok((
                    rbl_db::export_info::my_tags(conn)?,
                    rbl_db::export_info::track_extras(conn, &ids)?,
                    rbl_db::export_info::db_id(conn)?,
                ))
            })
            .map_err(write_error)?;
        // The whole tree goes along; the record keeps the ticked playlists
        // and the folders above them.
        let tree = {
            let playlists = library.playlists();
            (0..playlists.len())
                .map(|index| rbl_export::SyncNode {
                    id: playlists.ids.get(index).copied().unwrap_or(0),
                    parent: match playlists.parent.get(index).copied() {
                        Some(parent) if parent != rbl_index::NO_ID => {
                            playlists.ids.get(parent as usize).copied().unwrap_or(0)
                        }
                        _ => 0,
                    },
                    attribute: playlists.attribute.get(index).copied().unwrap_or(0),
                })
                .collect()
        };
        let sync = rbl_export::SyncSource { db_id, tree, automatic };
        let my_tags: Vec<rbl_export::SourceMyTag> = my_tags.iter().map(source_my_tag).collect();

        let mut tracks: Vec<rbl_export::SourceTrack> = Vec::new();
        let mut position: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        let mut source_playlists = Vec::with_capacity(named.len());
        for ((id, name, _), rows) in named.into_iter().zip(rows_of) {
            let mut track_indices = Vec::with_capacity(rows.len());
            for row in rows {
                let at = if let Some(at)=position.get(&row) { *at } else {
                    let content = library.ids.get(row as usize).copied().unwrap_or(0).to_string();
                    let extra = extras.get(&content).cloned().unwrap_or_default();
                    tracks.push(source_track(library, share, row, &extra)?);
                    let at=tracks.len()-1; position.insert(row,at); at
                };
                track_indices.push(at);
            }
            source_playlists.push(rbl_export::SourcePlaylist { device_id: 0, device_only: false, parent_id: 0, folder: false, id, name, track_indices });
        }
        for row in loose_rows {
            if let std::collections::hash_map::Entry::Vacant(entry) = position.entry(row) {
                let content = library.ids.get(row as usize).copied().unwrap_or(0).to_string();
                let extra = extras.get(&content).cloned().unwrap_or_default();
                tracks.push(source_track(library, share, row, &extra)?);
                entry.insert(tracks.len() - 1);
            }
        }
        // Include ancestors as actual folder rows in both USB databases.
        let tree_view = library.playlists();
        let mut ancestors = std::collections::BTreeSet::new();
        for p in &mut source_playlists {
            p.parent_id = sync.tree.iter().find(|n| n.id == p.id).map_or(0, |n| n.parent);
            let mut parent = p.parent_id;
            while parent != 0 && ancestors.insert(parent) {
                parent = sync.tree.iter().find(|n| n.id == parent).map_or(0, |n| n.parent);
            }
        }
        for folder in chosen_folders {
            let mut next = folder;
            while next != 0 && ancestors.insert(next) {
                next = sync.tree.iter().find(|n| n.id == next).map_or(0, |n| n.parent);
            }
        }
        let mut folders = Vec::new();
        for id in ancestors {
            let Some(index) = tree_view.index_of(id) else { return Err(AppError::internal("Missing playlist ancestor")); };
            folders.push(rbl_export::SourcePlaylist { device_id: 0, device_only: false, id, name: tree_view.name(index).to_owned(), parent_id: sync.tree.iter().find(|n| n.id == id).map_or(0, |n| n.parent), folder: true, track_indices: Vec::new() });
        }
        folders.append(&mut source_playlists);
        source_playlists = folders;
        Ok(Self { tracks, playlists: source_playlists, my_tags, sync, keep_unlisted: false })
    }

    /// This selection with the loose tracks a stick's record names added,
    /// so a sync keeps what Export Track put there unless cleanup is enabled.
    /// Cleanup uses the playlist selection alone; the exporter removes only
    /// stale manifest-owned files after publishing the new databases. The selection itself
    /// when the record names none it does not already hold.
    pub(crate) fn for_stick<'a>(
        &'a self,
        state: &AppState,
        library: &rbl_index::Library,
        share: &std::path::Path,
        destination: &std::path::Path,
        delete_unlisted_music: bool,
    ) -> AppResult<std::borrow::Cow<'a, Self>> {
        if delete_unlisted_music {
            return Ok(std::borrow::Cow::Borrowed(self));
        }
        rbl_export::recover(destination).map_err(|e| AppError::internal(e.to_string()))?;
        let recorded = rbl_export::Manifest::load(destination).filter(|m| m.db_id == self.sync.db_id).map(|m| m.loose).unwrap_or_default();
        let held: std::collections::HashSet<u64> = self.tracks.iter().map(|t| t.id).collect();
        let missing: Vec<u64> = recorded.into_iter().filter(|id| !held.contains(id)).collect();
        if missing.is_empty() {
            return Ok(std::borrow::Cow::Borrowed(self));
        }
        let playlist_ids: Vec<String> = self.playlists.iter().filter(|p| !p.folder).map(|p| p.id.to_string()).collect();
        let loose: Vec<u64> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.playlists.iter().any(|p| p.track_indices.contains(index)))
            .map(|(_, t)| t.id)
            .chain(missing)
            .collect();
        let mut kept = Self::from_playlists_and_tracks(state, library, share, &playlist_ids, &loose, self.sync.automatic)?;
        kept.keep_unlisted = self.keep_unlisted;
        Ok(std::borrow::Cow::Owned(kept))
    }
}

/// One library row as the export wants it, analysis read from the share
/// tree, and what the index does not hold from `extra`.
fn source_track(
    library: &rbl_index::Library,
    share: &std::path::Path,
    row: u32,
    extra: &rbl_db::export_info::TrackExtras,
) -> AppResult<rbl_export::SourceTrack> {
    let i = row as usize;
    // What analysis wrote, from the row as it is now: an analysis run
    // re-reads the index only once it is over, and a sync made during one
    // must not take the tracks it has analysed as unanalysed (#309).
    let analysed = extra.analysis.as_ref();
    let analysis_path = analysed.map_or_else(|| library.analysis_path.get(i), |a| a.analysis_path.as_str());
    let artwork_path = analysed.map_or_else(|| library.artwork_path.get(i), |a| a.artwork_path.as_str());
    // The library's own image, share-relative like the analysis.
    let artwork = Some(artwork_path)
        .filter(|p| !p.is_empty())
        .map(|p| share.join(p.trim_start_matches(['/', '\\'])));
    Ok(rbl_export::SourceTrack {
        cues: Some(extra.cues.clone()),
        metadata: extra.metadata.clone(),
        device: None,
        // The content id is how a second export to the same stick
        // recognises a track it has already written.
        id: library.ids.get(i).copied().unwrap_or(0),
        source_path: source_audio(library.folder_path.get(i), &extra.alternate_paths),
        artwork,
        my_tags: extra.my_tags.iter().filter_map(|t| t.parse().ok()).collect(),
        title: library.title.get(i).to_owned(),
        artist: library.artist_name(row).to_owned(),
        album: library.album_name(row).to_owned(),
        genre: library.genre_name(row).to_owned(),
        label: library.label_name(row).to_owned(),
        key: analysed.map_or_else(|| library.key_name(row).to_owned(), |a| a.key.clone()),
        comment: library.comment.get(i).to_owned(),
        date_added: library.date_added.get(i).to_owned(),
        release_date: library.release_date.get(i).to_owned(),
        bpm_x100: analysed.map_or_else(|| library.bpm_x100.get(i).copied().unwrap_or(0), |a| a.bpm_x100),
        duration_sec: u16::try_from(analysed.map_or_else(|| library.length_sec.get(i).copied().unwrap_or(0), |a| a.length_sec)).unwrap_or(u16::MAX),
        rating: library.rating.get(i).copied().unwrap_or(0),
        color_id: library.color.get(i).copied().unwrap_or(0),
        bitrate: library.bitrate.get(i).copied().unwrap_or(0),
        sample_rate: library.sample_rate.get(i).copied().unwrap_or(0),
        file_size: library.file_size.get(i).copied().unwrap_or(0),
        year: library.year.get(i).copied().unwrap_or(0),
        analysis: read_analysis(share, analysis_path)?,
    })
}

/// Writes a selection to one destination and reads it back.
///
/// Runs inside a `blocking` closure: it copies audio over USB.
static EXPORT_PROGRESS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, ExportProgressDto>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
static EXPORT_CANCEL: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Arc<std::sync::atomic::AtomicBool>>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn set_export_stage<R: tauri::Runtime>(app: &tauri::AppHandle<R>, destination: &std::path::Path, state: &'static str) {
    let path = destination.to_string_lossy().into_owned();
    let progress = if let Ok(mut jobs) = EXPORT_PROGRESS.lock() {
        if let Some(job) = jobs.get_mut(&path) {
            job.state = state;
            job.title.clear();
            Some(job.clone())
        } else {
            None
        }
    } else { None };
    if let Some(progress) = progress { let _ = tauri::Emitter::emit(app, "export:progress", progress); }
}

fn set_export_failure<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    destination: &std::path::Path,
    message: String,
) {
    let path = destination.to_string_lossy().into_owned();
    let progress = if let Ok(mut jobs) = EXPORT_PROGRESS.lock() {
        let job = jobs.entry(path.clone()).or_insert_with(|| ExportProgressDto {
            path,
            state: "failed",
            done: 0,
            total: 0,
            title: String::new(),
        });
        job.state = "failed";
        job.title = message;
        Some(job.clone())
    } else {
        None
    };
    if let Some(progress) = progress {
        let _ = tauri::Emitter::emit(app, "export:progress", progress);
    }
}

#[tauri::command]
pub fn cancel_export(path: &str) {
    if let Ok(jobs) = EXPORT_CANCEL.lock() {
        if let Some(cancel) = jobs.get(path) {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

#[tauri::command]
pub fn export_progress() -> Vec<ExportProgressDto> {
    EXPORT_PROGRESS.lock().map(|jobs| jobs.values().cloned().collect()).unwrap_or_default()
}

#[tauri::command]
pub async fn eject_device(path: String) -> AppResult<()> {
    blocking("eject_device", move || {
        // Keep new exports from starting until the OS has finished ejecting.
        let jobs = EXPORT_PROGRESS.lock().map_err(|e| AppError::internal(e.to_string()))?;
        if jobs.get(&path).is_some_and(|job| job.state == "writing") {
            return Err(AppError::internal("This device is being exported to. Wait for the export to finish."));
        }
        let result = rbl_devices::eject::eject(std::path::Path::new(&path))
            .map_err(|e| AppError::internal(e.to_string()));
        drop(jobs);
        result
    }).await
}

/// How `format_device` lays the stick out.
#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum FormatLayout {
    Fat32,
    HfsPlus,
}

/// Erases every partition on the stick at `path` and formats it, keeping its name.
#[tauri::command]
pub async fn format_device(path: String, layout: FormatLayout) -> AppResult<()> {
    blocking("format_device", move || {
        // Keep new exports from starting until the stick has been formatted.
        let jobs = EXPORT_PROGRESS.lock().map_err(|e| AppError::internal(e.to_string()))?;
        if jobs.get(&path).is_some_and(|job| job.state == "writing") {
            return Err(AppError::internal("This device is being exported to. Wait for the export to finish."));
        }
        let layout = match layout {
            FormatLayout::Fat32 => rbl_devices::format::Layout::Fat32,
            FormatLayout::HfsPlus => rbl_devices::format::Layout::HfsPlus,
        };
        let result = rbl_devices::format::format(std::path::Path::new(&path), layout)
            .map_err(|e| AppError::internal(e.to_string()));
        drop(jobs);
        result
    }).await
}

/// An export that failed because the USB went away mid-way says so: the
/// error the file system gave ("Device not configured", "Input/output
/// error", "No such file or directory") names a file, not the cause. What
/// was written stays recoverable, and the next sync finishes or rolls it
/// back.
fn disconnected_during_export(destination: &std::path::Path, error: AppError) -> AppError {
    // Cancelled is not a failure, and a stick that was gone before the sync
    // began is already reported as such.
    if matches!(error.kind, ErrorKind::Cancelled | ErrorKind::NotFound) || destination.is_dir() {
        return error;
    }
    let detail = error.detail.clone().unwrap_or_else(|| error.message.clone());
    AppError::new(
        ErrorKind::NotFound,
        "The USB was disconnected during the sync. Plug it back in and sync again; the sync picks up from there.",
    )
    .with_detail(detail)
}

fn write_export_with_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    destination: &std::path::Path,
    selection: &ExportSelection,
    defaults: Option<&crate::device_settings::StickDefaultsDto>,
    compatibility_format: Option<rbl_export::CompatibilityFormat>,
) -> AppResult<ExportReportDto> {
    let total = u32::try_from(selection.tracks.len()).unwrap_or(u32::MAX);
    let path = destination.to_string_lossy().into_owned();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut jobs = EXPORT_CANCEL.lock().map_err(|e| AppError::internal(e.to_string()))?;
        if jobs.contains_key(&path) {
            return Err(AppError::internal("An export to this device is already running."));
        }
        // A new batch replaces terminal progress from the previous one. When
        // another job is active this export belongs to that same batch, so a
        // stick that finishes early remains in the aggregate denominator.
        if jobs.is_empty() {
            if let Ok(mut progress) = EXPORT_PROGRESS.lock() {
                progress.clear();
            }
        }
        jobs.insert(path.clone(), Arc::clone(&cancel));
    }
    let emit = |state, done, title: String| {
        let progress = ExportProgressDto {
            path: destination.to_string_lossy().into_owned(), state, done, total, title,
        };
        if let Ok(mut jobs) = EXPORT_PROGRESS.lock() {
            jobs.insert(progress.path.clone(), progress.clone());
        }
        let _ = tauri::Emitter::emit(app, "export:progress", progress);
    };
    emit("preparing", 0, String::new());
    let done = std::cell::Cell::new(0);
    let result = write_export_with_phase(
        destination, selection, defaults, compatibility_format,
        &mut |p| {
            done.set(u32::try_from(p.done).unwrap_or(u32::MAX));
            emit(p.stage, done.get(), p.title.clone());
        },
        &mut |phase| emit(phase, if phase == "verifying" { total } else { done.get() }, String::new()),
        &|| cancel.load(std::sync::atomic::Ordering::Relaxed),
    )
    .map_err(|error| disconnected_during_export(destination, error));
    let cancelled = result.as_ref().err().is_some_and(|e| matches!(e.kind, ErrorKind::Cancelled));
    emit(if result.is_ok() { "done" } else if cancelled { "cancelled" } else { "failed" }, if result.is_ok() { total } else { done.get() },
        result.as_ref().err().map_or_else(String::new, |e| e.message.clone()));
    if let Ok(mut jobs) = EXPORT_CANCEL.lock() { jobs.remove(&path); }
    result
}

fn write_export_with_phase(
    destination: &std::path::Path,
    selection: &ExportSelection,
    defaults: Option<&crate::device_settings::StickDefaultsDto>,
    compatibility_format: Option<rbl_export::CompatibilityFormat>,
    progress: &mut dyn FnMut(&rbl_export::ExportProgress),
    phase: &mut dyn FnMut(&'static str),
    cancelled: &dyn Fn() -> bool,
) -> AppResult<ExportReportDto> {
    if !destination.is_dir() {
        return Err(AppError::new(
            ErrorKind::NotFound,
            "That device is no longer connected. It may have been unplugged or renamed.",
        ));
    }
    if rbl_db::is_rekordbox_app_running() {
        return Err(AppError::new(ErrorKind::ReadOnly, "Quit rekordbox before syncing this USB so only one application writes its libraries."));
    }
    let preferred_root = rbl_devices::list()
        .into_iter()
        .find(|device| device.mount_point == destination)
        .and_then(|device| rbl_export::ExportRoot::for_file_system(&device.file_system));
    let root_name = rbl_export::export_root_name_with(destination, preferred_root)
        .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))?;
    let export_root = destination.join(root_name);
    let settings_root = crate::usb_import::settings_stash(&rbl_backup::state_dir());
    let imported_settings: Vec<_> = ["MYSETTING.DAT", "MYSETTING2.DAT", "DJMMYSETTING.DAT"].into_iter()
        .filter(|name| !export_root.join(name).exists())
        .filter_map(|name| std::fs::read(settings_root.join(name)).ok().map(|bytes| (name, bytes))).collect();
    let library_defaults = defaults.map(crate::device_settings::library_defaults);
    let report = rbl_export::export_cancellable(
        destination,
        &selection.tracks,
        &selection.playlists,
        &selection.my_tags,
        &rbl_export::ExportOptions {
            defaults: library_defaults.as_ref(),
            sync: Some(&selection.sync),
            compatibility: compatibility_format,
            root: preferred_root,
            keep_unlisted: selection.keep_unlisted,
        },
        progress,
        cancelled,
    )
    .map_err(|e| AppError::new(if matches!(e, rbl_export::ExportError::Cancelled) { ErrorKind::Cancelled } else { ErrorKind::Internal }, e.to_string()))?;
    if let Some(defaults) = defaults {
        crate::device_settings::write_dev_defaults(destination, defaults)?;
    }

    for (name, bytes) in imported_settings {
        crate::durable::write(&export_root.join(name), &bytes).map_err(|e| AppError::internal(e.to_string()))?;
    }
    // Re-read what was written with the independent parser: an export that
    // cannot be read back is not an export.
    phase("verifying");
    let check = rbl_export::verify_databases(destination)
        .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))?;
    if !check.is_ok() {
        return Err(AppError::internal(format!("The USB did not verify after the sync: {}", rbl_export::verification_failure(&check))));
    }
    if check.tracks != report.tracks {
        return Err(AppError::internal(format!("The USB did not verify after the sync: it lists {} tracks where {} were written.", check.tracks, report.tracks)));
    }

    Ok(ExportReportDto {
        tracks: u32::try_from(report.tracks).unwrap_or(0),
        playlists: u32::try_from(report.playlists).unwrap_or(0),
        bytes_copied: report.bytes_copied,
        analysis_files: u32::try_from(report.analysis_files).unwrap_or(0),
        reused: u32::try_from(report.reused).unwrap_or(0),
        removed: u32::try_from(report.removed).unwrap_or(0),
        tracks_added: u32::try_from(report.tracks_added).unwrap_or(0),
        tracks_updated: u32::try_from(report.tracks_updated).unwrap_or(0),
        playlists_added: u32::try_from(report.playlists_added).unwrap_or(0),
        playlists_removed: u32::try_from(report.playlists_removed).unwrap_or(0),
        skipped: report.skipped,
        failed: report.failed,
        verified: check.is_ok() && check.tracks == report.tracks,
    })
}

/// Lists the volumes an export could be written to, and what is on each.
///
/// Enumeration is cheap; reading a stick to see what it holds is not, so that
/// happens once per device here rather than on any timer. There is no polling
/// behind this — the panel asks when it is opened.
#[tauri::command]
pub async fn list_devices() -> AppResult<Vec<DeviceDto>> {
    blocking("list_devices", || {
        Ok(rbl_devices::list()
            .into_iter()
            .map(|device| {
                let found = rbl_devices::inspect(&device.mount_point);
                DeviceDto {
                    name: device.name,
                    path: device.mount_point.to_string_lossy().into_owned(),
                    total_bytes: device.total_bytes,
                    free_bytes: device.free_bytes,
                    file_system: device.file_system,
                    removable: device.removable,
                    volume_id: device.volume_id,
                    export: found.map(|export| DeviceExportDto {
                        tracks: u32::try_from(export.tracks).unwrap_or(u32::MAX),
                        playlists: u32::try_from(export.playlists).unwrap_or(u32::MAX),
                        ours: export.ours,
                        written: export.written,
                    }),
                }
            })
            .collect())
    })
    .await
}

/// Where a track's audio is read from.
///
/// `FolderPath` when the file is there. A cloud-synced track's `FolderPath`
/// can name a copy that is not (the Dropbox one, on a machine where the
/// folder is not synced down), so the other paths the row names —
/// `rb_LocalFolderPath`, then `OrgFolderPath` — are tried in turn, and
/// the first that exists is the source. None existing leaves `FolderPath`,
/// so the export reports the track as skipped under the name the row gives.
fn source_audio(folder_path: &str, alternates: &[String]) -> std::path::PathBuf {
    let first = std::path::PathBuf::from(folder_path);
    if first.is_file() {
        return first;
    }
    alternates
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
        .unwrap_or(first)
}

/// Reads a track's analysis files, so the export re-emits rather than
/// re-analysing.
fn read_analysis(share: &std::path::Path, relative: &str) -> AppResult<Vec<(String, Vec<u8>)>> {
    if relative.is_empty() {
        return Ok(Vec::new());
    }
    let base = share.join(relative.trim_start_matches(['/', '\\']));
    let mut out = Vec::new();
    // The three files rekordbox 7 copies to a stick [OBS 7.2.11]; a track
    // analysed by an older version has no .2EX, and none is written then.
    for extension in ["DAT", "EXT", "2EX"] {
        let path = base.with_extension(extension);
        match std::fs::read(&path) {
            Ok(bytes) => {
                rbl_anlz::parse(&bytes).map_err(|e|AppError::internal(format!("Invalid analysis {}: {e}",path.display())))?;
                out.push((extension.to_owned(), bytes));
            }
            Err(e) if e.kind()==std::io::ErrorKind::NotFound && extension!="DAT" => {},
            // The library names an analysis folder that is not on disk: the
            // track is exported without its analysis, as one the library
            // never analysed is, instead of failing the whole export.
            Err(e) if e.kind()==std::io::ErrorKind::NotFound => {
                tracing::warn!(path = %path.display(), "analysis file is missing; exporting the track without it");
                return Ok(Vec::new());
            }
            Err(e) => return Err(AppError::internal(format!("Cannot read analysis {}: {e}",path.display()))),
        }
    }
    Ok(out)
}

/// A track's whole beat grid, as raw bytes.
///
/// Read from the `PQTZ` tag of the track's analysis file. Seven bytes a beat —
/// a little-endian `u32` of milliseconds, the beat's number in its bar, and a
/// little-endian `u16` of the tempo there x100 — so a four-minute track costs
/// about 3.5 KB and one fetch per track replaces a fetch per window. Windowing
/// it meant re-reading and re-parsing the whole analysis file every time the
/// playhead moved on, which is the expensive part whatever slice comes back.
///
/// The beat number rather than a downbeat flag: it is what the tag holds, and
/// bar-aligned sync needs the position in the bar rather than only whether the
/// bar started. The tempo is per beat rather than one for the track because a
/// grid can change tempo partway through, and a track that speeds up at bar
/// 135 has to read as its new tempo from there on.
#[tauri::command]
pub async fn track_beats(
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<tauri::ipc::Response> {
    let library = state.library()?;
    let share = state.share_root();
    blocking("track_beats", move || {
        let Some(row) = library.row_of(&track) else { return Ok(Vec::new()) };
        let relative = library.analysis_path.get(row as usize);
        if relative.is_empty() {
            return Ok(Vec::new());
        }
        let beats = read_beat_grid(&share, relative);
        let mut out: Vec<u8> = Vec::with_capacity(beats.len() * BEAT_BYTES);
        for (time_ms, number, tempo_x100) in beats {
            out.extend_from_slice(&time_ms.to_le_bytes());
            out.push(number);
            out.extend_from_slice(&tempo_x100.to_le_bytes());
        }
        Ok(out)
    })
    .await
    .map(tauri::ipc::Response::new)
}

/// A track's beat grid from its `.DAT`: milliseconds, the beat's number in
/// the bar (1 is the downbeat) and the tempo there x100, at most `MAX_BEATS`
/// of them. Empty for a track without one.
pub(crate) fn read_beat_grid(share: &std::path::Path, relative: &str) -> Vec<(u32, u8, u16)> {
    let path = share.join(relative.trim_start_matches(['/', '\\']));
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    let Ok(file) = rbl_anlz::parse(&bytes) else { return Vec::new() };
    file.sections
        .iter()
        .find_map(rbl_anlz::Section::as_beat_grid)
        .map(|beats| {
            beats
                .iter()
                .take(MAX_BEATS)
                .map(|beat| {
                    (beat.time_ms, u8::try_from(beat.beat_number).unwrap_or(0), beat.tempo_x100)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Bytes one beat takes in that encoding: `u32` milliseconds, its number, then
/// a `u16` of the tempo there x100.
const BEAT_BYTES: usize = 7;

/// Points a deck at a track and starts loading it.
///
/// Returns as soon as the engine has been told; the deck reports itself ready
/// with a `deck:loaded` event, because opening a file means reading from a
/// disk that may be asleep.
#[tauri::command]
pub async fn deck_load<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    player: State<'_, Arc<crate::player::Player>>,
    preview: State<'_, Arc<crate::preview::Preview>>,
    deck: String,
    track: String,
    load_id: u64,
) -> AppResult<()> {
    // rekordbox's `ListViewer::loadTrack` stops the browser's preview before
    // it loads, whether or not the load then succeeds: a track going onto a
    // deck is not heard over a preview (#242).
    preview.stop();
    let library = state.library()?;
    let Some(path) = library.audio_path_of(&track).map(std::path::PathBuf::from) else {
        return Err(AppError::new(ErrorKind::NotFound, "That track's file could not be found.")
            .with_detail(format!("track {track}")));
    };
    // [OBS rekordbox 7.2.19 static] `UiPlayer::handleMessageDragAndDrop`
    // @0x101abadc4 opens a track only when its file is there; otherwise the
    // status bar says `kPlayerOperateErrorLoadMissingFile` (@0x101abb0f4)
    // and the deck is left as it was.
    if crate::relocate::is_missing_path(&path) {
        return Err(AppError::new(ErrorKind::NotFound, crate::relocate::LOAD_MISSING_FILE)
            .with_detail(format!("track {track}: {}", path.display())));
    }
    let engine = player.engine(&app)?;
    let which = crate::player::deck_of(&deck);
    // The engine's own thread does the opening; this only hands it the path.
    engine.load_as(which, &path, load_id);
    player.loaded_tracks.lock().insert(which, track.clone());
    // And the grid, for the metronome. Read off the async thread: it is a
    // file, and the deck is loading on its own thread anyway.
    let share = state.share_root();
    let relative = library.row_of(&track).map(|row| library.analysis_path.get(row as usize).to_owned());
    let grid = blocking("deck_load_grid", move || {
        Ok(relative
            .filter(|rel| !rel.is_empty())
            .map(|rel| read_beat_grid(&share, &rel))
            .unwrap_or_default())
    })
    .await?;
    // A newer track may have been selected while its predecessor's analysis
    // file was being read. Never put the older grid on the newer audio.
    if player.loaded_tracks.lock().get(&which) == Some(&track) {
        engine.set_metronome_grid(
            which,
            &grid.iter().map(|&(ms, number, _)| (ms, number == 1)).collect::<Vec<_>>(),
        );
    }
    Ok(())
}

/// The key, in semitones from the track's own — a CDJ's key shift.
#[tauri::command]
pub async fn deck_key_shift<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    semitones: i8,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    engine.set_key_shift(crate::player::deck_of(&deck), semitones);
    Ok(())
}

/// Switches a deck's metronome on or off: a click on every beat of the
/// grid while it plays.
#[tauri::command]
pub async fn deck_metronome<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    on: bool,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    engine.set_metronome(crate::player::deck_of(&deck), on);
    Ok(())
}

/// Gives a deck's metronome a grid at once, without a save. The GRID panel
/// sends a nudged grid here before the file is written, so the click moves
/// on the press.
#[tauri::command]
pub async fn deck_metronome_grid(
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    beats: Vec<(u32, bool)>,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.set_metronome_grid(crate::player::deck_of(&deck), &beats);
    }
    Ok(())
}

/// Preferences › Audio › Metronome: which click, and how loud.
#[tauri::command]
pub async fn set_metronome(
    player: State<'_, Arc<crate::player::Player>>,
    sound: u8,
    volume: String,
) -> AppResult<()> {
    let sound = match sound {
        1 => rbl_deck::ClickSound::One,
        3 => rbl_deck::ClickSound::Three,
        _ => rbl_deck::ClickSound::Two,
    };
    let volume = match volume.as_str() {
        "small" => rbl_deck::ClickVolume::Small,
        "middle" => rbl_deck::ClickVolume::Middle,
        _ => rbl_deck::ClickVolume::Large,
    };
    player.set_metronome(sound, volume);
    Ok(())
}

/// Preferences › Audio › Sample Rate and Buffer size. Takes effect the next
/// time a deck plays, as a device change does: a stream has the rate it was
/// opened at.
#[tauri::command]
pub async fn set_audio_config<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    sample_rate: Option<u32>,
    buffer_frames: Option<u32>,
) -> AppResult<()> {
    if player.set_wish(rbl_deck::StreamWish { sample_rate, buffer_frames }) {
        let _ = tauri::Emitter::emit(&app, "deck:reset", ());
    }
    Ok(())
}

#[tauri::command]
pub async fn deck_unload(
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
) -> AppResult<()> {
    player.loaded_tracks.lock().remove(&crate::player::deck_of(&deck));
    if let Some(engine) = player.opened() {
        engine.unload(crate::player::deck_of(&deck));
    }
    Ok(())
}

#[tauri::command]
pub async fn deck_play<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    preview: State<'_, Arc<crate::preview::Preview>>,
    deck: String,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    // A deck that plays stops the browser's preview, as rekordbox's does
    // outside PERFORMANCE mode; the two are never heard over each other (#242).
    preview.stop();
    engine.play(crate::player::deck_of(&deck));
    // The tick only runs while something is playing, so play is what starts it.
    crate::player::start_ticker(&app);
    Ok(())
}

/// Starts a deck after `delay_ms` of silence, counted by the audio callback:
/// quantized play on a synced deck, held for the master's next beat.
#[tauri::command]
pub async fn deck_play_after<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    preview: State<'_, Arc<crate::preview::Preview>>,
    deck: String,
    delay_ms: f64,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    // Held for a beat or not, this is the deck playing: see `deck_play`.
    preview.stop();
    // Clamped to a positive number first: a delay is at most a beat, and a
    // negative or absurd one is zero rather than a wrapped count.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let frames = if delay_ms.is_finite() && delay_ms > 0.0 {
        (delay_ms.min(60_000.0) * f64::from(engine.sample_rate()) / 1000.0).round() as u64
    } else {
        0
    };
    engine.play_after(crate::player::deck_of(&deck), frames);
    crate::player::start_ticker(&app);
    Ok(())
}

#[tauri::command]
pub async fn deck_pause(
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.pause(crate::player::deck_of(&deck));
    }
    Ok(())
}

/// Moves a deck's playhead. Frame-exact, whatever the file's packet size.
#[tauri::command]
pub async fn deck_seek<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    // Fractional: a cue point is a place in the music, and a whole
    // millisecond is 44 frames at 44.1 kHz.
    position_ms: f64,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.seek_ms(crate::player::deck_of(&deck), position_ms);
        // So the interface sees where it landed even while paused, when no
        // tick is running.
        crate::player::start_ticker(&app);
    }
    Ok(())
}

/// Moves a deck's playhead by `by_ms` from where the engine has it.
#[tauri::command]
pub async fn deck_move(
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    by_ms: f64,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.move_ms(crate::player::deck_of(&deck), by_ms);
    }
    Ok(())
}

/// Sets a deck's loop between two points and turns it on; a head already
/// past the out point goes back to the in point.
#[tauri::command]
pub async fn deck_set_loop<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    in_ms: f64,
    out_ms: f64,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.set_loop_ms(crate::player::deck_of(&deck), in_ms, out_ms);
        crate::player::start_ticker(&app);
    }
    Ok(())
}

/// RELOOP (on) and EXIT (off): back into the loop from its in point, or out
/// of it with the range kept.
#[tauri::command]
pub async fn deck_loop_active<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    on: bool,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.set_looping(crate::player::deck_of(&deck), on);
        crate::player::start_ticker(&app);
    }
    Ok(())
}

/// Forgets a deck's loop.
#[tauri::command]
pub async fn deck_clear_loop<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.clear_loop(crate::player::deck_of(&deck));
        crate::player::start_ticker(&app);
    }
    Ok(())
}

/// The master output level, 0 to +2 dB.
///
/// It reaches the meters on the next tick rather than coming back from here:
/// the level is the audio callback's to apply, and the interface reads what it
/// actually did rather than what it was asked for.
#[tauri::command]
pub async fn set_master_level<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    preview: State<'_, Arc<crate::preview::Preview>>,
    level: f32,
) -> AppResult<()> {
    player.engine(&app)?;
    player.set_master_level(level);
    // The browser's preview plays through its own engine and follows the
    // same knob.
    preview.set_master_level(level);
    Ok(())
}

/// One deck's channel strip: the trim, the three bands, and the kill buttons.
///
/// Every one of these is a knob position rather than a gain in dB — what a
/// position means is the mixer's to decide, and it changes with the EQ /
/// ISOLATOR switch. The interface should not have to know the curve.
#[tauri::command]
pub async fn set_channel_band<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    band: String,
    position: f32,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    if let Some(channel) = engine.mixer().channels.get(channel_of(&deck)) {
        channel.set_band(band_of(&band), position);
    }
    Ok(())
}

#[tauri::command]
pub async fn set_channel_kill<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    band: String,
    killed: bool,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    if let Some(channel) = engine.mixer().channels.get(channel_of(&deck)) {
        channel.set_kill(band_of(&band), killed);
    }
    Ok(())
}

/// The deck's gain, 0 to 2 — up to +6 dB, as a mixer's trim gives.
#[tauri::command]
pub async fn set_channel_trim<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    trim: f32,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    if let Some(channel) = engine.mixer().channels.get(channel_of(&deck)) {
        channel.set_trim(trim);
    }
    Ok(())
}

/// The crossfader: 0 is deck A alone, 1 is deck B alone, 0.5 is both.
#[tauri::command]
pub async fn set_crossfade<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    position: f32,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    engine.mixer().set_crossfade(position);
    Ok(())
}

/// EQ or ISOLATOR, which is what the bottom of each band's travel means.
#[tauri::command]
pub async fn set_eq_curve<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    isolator: bool,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    engine.mixer().set_curve(if isolator { Curve::Isolator } else { Curve::Eq });
    Ok(())
}

/// Which strip a deck name means. Anything but "b" is deck A, as everywhere.
fn channel_of(deck: &str) -> usize {
    usize::from(matches!(deck, "b" | "B"))
}

/// Which band a name means, defaulting to the one a typo cannot silence.
fn band_of(name: &str) -> Band {
    match name {
        "low" => Band::Low,
        "mid" => Band::Mid,
        _ => Band::High,
    }
}

/// How fast a deck plays, as a multiple of the file's own speed.
///
/// A ratio rather than a BPM: what BPM that comes to depends on the track, and
/// the deck does not need to know the track's to play it faster.
#[tauri::command]
pub async fn deck_tempo<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    tempo: f32,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    engine.set_tempo(crate::player::deck_of(&deck), tempo);
    Ok(())
}

/// Master Tempo: whether the pitch is held while the speed changes.
#[tauri::command]
pub async fn deck_master_tempo<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    on: bool,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    engine.set_master_tempo(crate::player::deck_of(&deck), on);
    Ok(())
}

/// The outputs the audio could go to, and which one is in use.
///
/// Read every time rather than cached: an interface is plugged in while the
/// app is open more often than not, and a list that was right at launch is a
/// list that does not have the thing somebody just connected.
#[tauri::command]
pub async fn audio_devices(
    player: State<'_, Arc<crate::player::Player>>,
) -> AppResult<AudioDevicesDto> {
    let chosen = player.device();
    Ok(AudioDevicesDto {
        devices: rbl_deck::output_devices()
            .into_iter()
            .map(|device| AudioDeviceDto { id: device.id, name: device.name })
            .collect(),
        default: rbl_deck::default_output_device().map(|device| device.id),
        chosen,
    })
}

/// Chooses an output. `None` — an absent id — is the system default.
///
/// It takes effect on the next thing played: a running stream belongs to the
/// device it was opened on, so the engine is dropped and rebuilt rather than
/// moved.
#[tauri::command]
pub async fn set_audio_device<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    device: Option<String>,
) -> AppResult<()> {
    if player.set_device(device) {
        let _ = tauri::Emitter::emit(&app, "deck:reset", ());
    }
    Ok(())
}

/// The master limiter as it stands.
#[tauri::command]
pub async fn master_limiter(
    player: State<'_, Arc<crate::player::Player>>,
) -> AppResult<LimiterDto> {
    Ok(player.limiter())
}

/// Sets the master limiter and returns what was actually set.
///
/// Takes effect at once if the engine is up, and is remembered for its build
/// if not — or its rebuild, after a device change. What comes back is the
/// engine's clamping of what was sent, so the interface shows the truth.
#[tauri::command]
pub async fn set_master_limiter(
    player: State<'_, Arc<crate::player::Player>>,
    preview: State<'_, Arc<crate::preview::Preview>>,
    limiter: LimiterDto,
) -> AppResult<LimiterDto> {
    let set = player.set_limiter(limiter);
    preview.set_limiter(set);
    Ok(set)
}

/// Shows a track's file in the Finder.
///
/// The OS does the revealing; this only resolves the id to the path the
/// library holds for it, and says so plainly when that file is not there —
/// about one track in thirty of the reference library sits on a volume that
/// is not mounted.
#[tauri::command]
pub async fn reveal_track<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<()> {
    let library = state.library()?;
    let Some(path) = library.audio_path_of(&track).map(std::path::PathBuf::from) else {
        return Err(AppError::new(ErrorKind::NotFound, "That track's file could not be found.")
            .with_detail(format!("track {track}")));
    };
    if !path.exists() {
        return Err(AppError::new(ErrorKind::NotFound, "That track's file could not be found.")
            .with_detail(path.display().to_string()));
    }
    app.opener().reveal_item_in_dir(&path).map_err(|e| {
        AppError::new(ErrorKind::Internal, "The Finder would not open.").with_detail(e.to_string())
    })
}

/// What the app is costing right now, for the title bar's readout.
///
/// Sampled on demand: nothing keeps this up to date in the background, so a
/// window with the readout hidden pays nothing for it.
/// The version this build carries — the tag it was built from, or 0.1.0
/// for a development build — for the About pane. Nothing is asked over the
/// network; that is the Update Manager's job.
#[tauri::command]
pub async fn app_version<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppResult<String> {
    Ok(app.package_info().version.to_string())
}

/// Opens the active daily log in the application associated with `.log` files
/// on this computer (Console on a stock macOS installation, for example).
#[tauri::command]
pub async fn open_log<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppResult<()> {
    blocking("open_log", move || {
        let path = crate::logging::latest_log_file()
            .map_err(|e| {
                AppError::internal("The log folder could not be read.").with_detail(e.to_string())
            })?
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "No application log was found."))?;
        app.opener()
            .open_path(path.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|e| {
                AppError::internal("The application log could not be opened.")
                    .with_detail(e.to_string())
            })
    })
    .await
}

#[tauri::command]
pub async fn app_diagnostics(player: State<'_, std::sync::Arc<crate::player::Player>>) -> AppResult<crate::diagnostics::Diagnostics> {
    // The sampler is kept between calls: CPU is a difference between two
    // readings, and a fresh `System` every second would always report zero.
    let health = player.opened().map(|engine| engine.audio_health()).unwrap_or_default();
    blocking("app_diagnostics", move || {
        let mut sample = crate::diagnostics::sample_shared();
        sample.audio_load = health.load;
        sample.audio_xruns = health.xruns;
        Ok(sample)
    }).await
}

/// Starts a drag on a deck.
///
/// Audio follows the pointer from here until `deck_scrub_end`: the deck reads
/// a decoded window at whatever rate the drag asks for, forwards or backwards,
/// which is what a hand on a record does and what a seek per pointer move
/// cannot do.
#[tauri::command]
pub async fn deck_scrub_begin<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
) -> AppResult<()> {
    let engine = player.engine(&app)?;
    engine.scrub_begin(crate::player::deck_of(&deck));
    crate::player::start_ticker(&app);
    Ok(())
}

/// Where the pointer is now, mid-drag.
#[tauri::command]
pub async fn deck_scrub_to(
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
    // Fractional: the head's speed comes from how far this moved since the
    // last one, so rounding it to a millisecond quantises the speed.
    position_ms: f64,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.scrub_to_ms(crate::player::deck_of(&deck), position_ms);
    }
    Ok(())
}

/// Ends a drag. The playhead stays where the head came to rest.
#[tauri::command]
pub async fn deck_scrub_end<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    player: State<'_, Arc<crate::player::Player>>,
    deck: String,
) -> AppResult<()> {
    if let Some(engine) = player.opened() {
        engine.scrub_end(crate::player::deck_of(&deck));
        crate::player::start_ticker(&app);
    }
    Ok(())
}

/// Both decks now, for the interface to anchor itself when it starts up or
/// comes back from a reload.
#[tauri::command]
pub async fn deck_state(
    player: State<'_, Arc<crate::player::Player>>,
) -> AppResult<crate::player::TickDto> {
    Ok(player.opened().map_or_else(crate::player::TickDto::silent, |engine| {
        crate::player::tick_of(&engine.snapshot(), engine.master())
    }))
}

/// Previews a track from `position_ms` without loading it onto a deck: a
/// click on the waveform in the browser's Preview column. See `preview.rs`
/// for what rekordbox does and how that was established.
#[tauri::command]
pub async fn preview_play<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    player: State<'_, Arc<crate::player::Player>>,
    preview: State<'_, Arc<crate::preview::Preview>>,
    track: String,
    position_ms: f64,
) -> AppResult<()> {
    let library = state.library()?;
    let Some(path) = library.audio_path_of(&track).map(std::path::PathBuf::from) else {
        return Err(AppError::new(ErrorKind::NotFound, "That track's file could not be found.")
            .with_detail(format!("track {track}")));
    };
    let decks = Arc::clone(&player);
    let preview = Arc::clone(&preview);
    // Waits on the file opening, which may be a disk waking up: off the
    // async runtime.
    blocking("preview_play", move || preview.play(&app, &decks, &track, &path, position_ms)).await
}

/// Stops the preview where it is.
#[tauri::command]
pub async fn preview_stop(preview: State<'_, Arc<crate::preview::Preview>>) -> AppResult<()> {
    preview.stop();
    Ok(())
}

/// The preview's track, whether it is playing, and where.
#[tauri::command]
pub async fn preview_state(
    preview: State<'_, Arc<crate::preview::Preview>>,
) -> AppResult<crate::preview::PreviewStateDto> {
    Ok(preview.state())
}

/// A track's cue points.
///
/// A hot cue's colour is what rekordbox paints for its `ColorTableIndex`,
/// from the complete `rbl_anlz::DRAWN_CUE_COLOURS` table extracted from
/// rekordbox. An invalid index is reported without a colour. A memory cue
/// never carries one.
#[tauri::command]
pub async fn track_cues(
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<Vec<CueDto>> {
    let library = state.library()?;
    let cue_state = Arc::clone(&state);
    blocking("track_cues", move || {
        const MEMORY_CSS: [&str; 8] = [
            "#E778F1", "#E33122", "#EBA44A", "#F4E458",
            "#66DD42", "#56BDF3", "#204FEF", "#8B1EEF",
        ];
        let Some(row) = library.row_of(&track) else { return Ok(Vec::new()) };
        // Rekordbox may have added cues since the library snapshot was built.
        // Refresh this track before reading its comments and colours so the
        // panel and waveform see the same current set of cues.
        let (comments, memory_colours) = cue_state.read_db(|db| {
            rbl_index::reload_cues_of(db, &library, &track)?;
            Ok((
                rbl_db::details::cue_comments(db.connection(), &track)?,
                rbl_db::details::memory_cue_colours(db.connection(), &track)?,
            ))
        }).map_err(write_error)?;
        Ok(library
            .cues_of(row)
            .iter()
            .map(|cue| CueDto {
                comment: comments.get(&cue.id.to_string()).cloned().unwrap_or_default(),
                id: if cue.id == 0 { String::new() } else { cue.id.to_string() },
                position_ms: cue.position_ms,
                out_ms: cue.out_ms,
                letter: cue.hot_letter().map(String::from).unwrap_or_default(),
                memory: cue.is_memory(),
                colour: if cue.is_memory() {
                    memory_colours.get(&cue.id.to_string()).and_then(|value| MEMORY_CSS.get(usize::from(*value))).map(|value| (*value).to_owned())
                } else { cue_colour_css(cue.colour) },
            })
            .collect())
    })
    .await
}

/// A track's phrases: the `INTRO` / `UP` / `CHORUS` strip rekordbox draws
/// above the waveform.
///
/// From `PSSI` in the `.EXT` file, with each phrase's beat resolved against
/// the `PQTZ` grid in the `.DAT` so the strip can be drawn on a time axis
/// without the caller fetching the grid as well.
#[tauri::command]
pub async fn track_phrases(
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<Vec<PhraseDto>> {
    let library = state.library()?;
    let share = state.share_root();
    blocking("track_phrases", move || {
        let Some(row) = library.row_of(&track) else { return Ok(Vec::new()) };
        let relative = library.analysis_path.get(row as usize);
        if relative.is_empty() {
            return Ok(Vec::new());
        }
        let dat = rbl_anlz::resolve(&share, relative);

        let Ok(ext) = rbl_anlz::Anlz::read(&rbl_anlz::sibling(&dat, "EXT")) else {
            // Not analysed for phrases, or the file is gone: draw no strip
            // rather than fail the view.
            return Ok(Vec::new());
        };
        let Some(phrases) = ext.phrases() else { return Ok(Vec::new()) };

        // The grid is optional here. A phrase without a time is still worth
        // returning, since its beat number is what the tag actually holds.
        let grid = rbl_anlz::Anlz::read(&dat).ok().and_then(|d| d.beat_grid());

        Ok(phrases
            .into_iter()
            .take(MAX_PHRASES)
            .map(|phrase| PhraseDto {
                beat: u32::from(phrase.beat),
                label: phrase.label.to_owned(),
                kind: phrase.kind,
                // Beat numbers in `PSSI` are 1-based; the grid is a list.
                time_ms: grid.as_ref().and_then(|g| {
                    g.get(usize::from(phrase.beat).checked_sub(1)?).map(|b| b.time_ms)
                }),
            })
            .collect())
    })
    .await
}

/// Where rekordbox heard a voice, one intensity byte per 46.44 ms.
///
/// From `PVDI` in the `.2EX` file. Raw bytes rather than JSON: a long track
/// has tens of thousands of them, and `from` / `len` window the strip the same
/// way [`track_waveform`] windows a waveform, which is what keeps a response
/// inside the IPC cap.
#[tauri::command]
pub async fn track_vocals(
    state: State<'_, Arc<AppState>>,
    track: String,
    from: Option<u32>,
    len: Option<u32>,
) -> AppResult<tauri::ipc::Response> {
    let library = state.library()?;
    let share = state.share_root();
    blocking("track_vocals", move || {
        let Some(row) = library.row_of(&track) else { return Ok(Vec::new()) };
        let relative = library.analysis_path.get(row as usize);
        if relative.is_empty() {
            return Ok(Vec::new());
        }
        let dat = rbl_anlz::resolve(&share, relative);
        let Ok(two) = rbl_anlz::Anlz::read(&rbl_anlz::sibling(&dat, "2EX")) else {
            return Ok(Vec::new());
        };
        Ok(window_of(two.vocals().unwrap_or_default(), 1, from, len))
    })
    .await
    .map(tauri::ipc::Response::new)
}

/// A page of the Collection tracks Auto Analysis would analyse: never
/// analysed, with their file where the library says.
///
/// rekordbox offers these at launch when Auto Analysis is on ("Auto Analysis
/// is starting.", OK/Cancel) [OBS: rekordbox 7.2.14 on chris-win11]. There can
/// be thousands, so they come a page at a time from row `from`; `next` is the
/// row to ask from for the next page, or `None` when the scan reached the end.
/// The pages are rows rather than offsets into the result so each file is
/// checked once however many pages are asked for.
#[tauri::command]
pub async fn unanalysed_tracks(
    state: State<'_, Arc<AppState>>,
    from: u32,
    limit: u32,
) -> AppResult<UnanalysedTracksDto> {
    if limit == 0 || limit > MAX_ROWS {
        return Err(
            AppError::new(ErrorKind::Malformed, "That page size is not valid.")
                .with_detail(format!("limit {limit} is outside 1..={MAX_ROWS}")),
        );
    }
    let library = state.library()?;
    blocking("unanalysed_tracks", move || {
        let wanted = limit as usize;
        let mut tracks = Vec::with_capacity(wanted);
        let mut next = None;
        for row in library.unanalysed_rows(from) {
            if tracks.len() == wanted {
                next = Some(row);
                break;
            }
            let index = row as usize;
            if !std::path::Path::new(library.folder_path.get(index)).exists() {
                continue;
            }
            tracks.push(UnanalysedTrackDto {
                id: library.ids.get(index).copied().unwrap_or(0).to_string(),
                title: library.title.get(index).to_owned(),
            });
        }
        Ok(UnanalysedTracksDto { tracks, next })
    })
    .await
}

/// How deep a chosen folder is walked. A music library is a handful of levels
/// deep; a folder that turns out to be a whole drive is not walked to the
/// bottom. Matches the relocate walk's ceiling.
const IMPORT_MAX_DEPTH: usize = 16;
/// How many entries the walk looks at in all, so a folder pointed at the root
/// of a disk ends rather than running for minutes. Shared across every chosen
/// path in one import.
const IMPORT_MAX_ENTRIES: usize = 500_000;

/// Expands the chosen paths into the audio files to import.
///
/// A file the user picked is kept as chosen — even a non-audio one, so
/// `import_file` still reports it as skipped rather than dropping it silently.
/// A directory is walked the way rekordbox walks one (depth first, name order,
/// hidden entries left alone; see [`rbl_db::import::audio_files_in`]), keeping
/// only the audio files rekordbox plays; a cover-art `.jpg` sitting beside the
/// tracks is simply not collected, not reported as skipped.
///
/// Pure filesystem work, so it runs under `blocking` with the writes below and
/// never touches the async thread.
fn expand_import_paths(paths: &[String]) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut budget = rbl_db::import::WalkBudget::new(IMPORT_MAX_DEPTH, IMPORT_MAX_ENTRIES);
    for path in paths {
        let path = std::path::Path::new(path);
        // Anything that is not a directory is imported exactly as chosen.
        if !path.is_dir() {
            files.push(path.to_path_buf());
            continue;
        }
        if budget.exhausted() {
            break;
        }
        files.extend(rbl_db::import::audio_files_in(path, &mut budget));
    }
    files
}

/// The files an import of `paths` would take, found without importing any:
/// the walk `import_files` does, so a caller can import them in slices and
/// show how far along it is.
#[tauri::command]
pub async fn list_import_files(paths: Vec<String>) -> AppResult<Vec<String>> {
    blocking("list_import_files", move || {
        Ok(expand_import_paths(&paths)
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect())
    })
    .await
}

/// Adds files to the library.
///
/// A chosen path may be a single file or a folder: a folder is walked
/// recursively for the audio files rekordbox plays (see [`expand_import_paths`]),
/// so picking a directory imports everything under it. Reports what happened
/// per file rather than failing the whole batch: a folder of a hundred tracks
/// with two unreadable ones should import ninety-eight, not nothing.
#[tauri::command]
pub async fn import_files<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    paths: Vec<String>,
) -> AppResult<ImportReportDto> {
    import_files_then(app, state, paths, true).await
}

/// One slice of a larger import: the same as [`import_files`] but the library
/// is not re-read afterwards, because a reload re-reads all of it. The caller
/// sends `reload_library` once, after the last slice.
#[tauri::command]
pub async fn import_files_slice<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    paths: Vec<String>,
) -> AppResult<ImportReportDto> {
    import_files_then(app, state, paths, false).await
}

async fn import_files_then<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    paths: Vec<String>,
    reload_after: bool,
) -> AppResult<ImportReportDto> {
    let state_for_edit = Arc::clone(&state);
    let writing = Arc::clone(&state);
    let report = blocking("import_files", move || {
        let files = expand_import_paths(&paths);
        writing
            .write(|writer| {
                let mut imported = 0_u32;
                let mut skipped = Vec::new();
                let mut tracks = Vec::new();
                let mut existing = Vec::new();
                // One transaction for the whole batch; a file already in the
                // library is not a failure, because a drop onto a playlist
                // still wants that track in the playlist.
                for (file, outcome) in files.iter().zip(writer.import_files(&files)?) {
                    let title = file
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    match outcome {
                        rbl_db::write::ImportOutcome::Existing(id) => {
                            existing.push(crate::dto::ImportedTrackDto { id, title });
                        }
                        rbl_db::write::ImportOutcome::Added(id) => {
                            imported += 1;
                            tracks.push(crate::dto::ImportedTrackDto { id, title });
                        }
                        rbl_db::write::ImportOutcome::Refused(reason) => {
                            skipped.push(format!("{}: {reason}", file.display()));
                        }
                    }
                }
                Ok(ImportReportDto { imported, skipped, tracks, existing })
            })
            .map_err(write_error)
    })
    .await?;

    // Only reload if anything landed; a batch that imported nothing has not
    // changed the library.
    if report.imported > 0 && reload_after {
        reload(app, state_for_edit).await?;
    }
    Ok(report)
}

/// A folder from Finder or Explorer dropped onto the Playlists root or a
/// playlist folder: one playlist named after it, holding every audio file
/// under it, subfolders flattened, the way rekordbox does it (see
/// [`rbl_db::write::Writer::import_folder_as_playlist`]).
///
/// One folder per call, because a same-named sibling stops that folder until
/// the user says whether to replace it: the clash comes back in `conflict`
/// with nothing written, and the call is repeated with `replace` set to it.
///
/// `at` is the drop's insert index among `parent`'s children (`None`: the
/// end). rekordbox puts every folder of one drop at that same index, so the
/// caller passes each folder the `at` the previous one returned.
#[tauri::command]
pub async fn import_folder_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    path: String,
    parent: String,
    replace: Option<String>,
    at: Option<u32>,
) -> AppResult<FolderPlaylistDto> {
    let writing = Arc::clone(&state);
    let report = blocking("import_folder_playlist", move || {
        let dir = std::path::PathBuf::from(&path);
        let name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut report = FolderPlaylistDto {
            name: name.clone(),
            playlist: None,
            conflict: None,
            folder: dir.is_dir(),
            imported: 0,
            skipped: Vec::new(),
            tracks: Vec::new(),
            existing: 0,
            at,
        };
        if !report.folder || name.is_empty() {
            return Ok(report);
        }
        let mut budget = rbl_db::import::WalkBudget::new(IMPORT_MAX_DEPTH, IMPORT_MAX_ENTRIES);
        let files = rbl_db::import::audio_files_in(&dir, &mut budget);
        let outcome = writing
            .write(|writer| {
                writer.import_folder_as_playlist(
                    &name,
                    &parent,
                    &files,
                    replace.as_deref(),
                    at.and_then(|at| usize::try_from(at).ok()),
                )
            })
            .map_err(write_error)?;
        report.playlist = outcome.playlist;
        report.conflict = outcome.conflict;
        report.imported = u32::try_from(outcome.imported.len()).unwrap_or(u32::MAX);
        report.existing = u32::try_from(outcome.existing.len()).unwrap_or(u32::MAX);
        report.skipped = outcome.skipped;
        report.at = outcome.at.map(|at| u32::try_from(at).unwrap_or(u32::MAX)).or(at);
        report.tracks = outcome
            .imported
            .into_iter()
            .map(|(id, file)| crate::dto::ImportedTrackDto {
                id,
                title: file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
            .collect();
        if report.playlist.is_some() {
            // An edit outside the undo history starts a new branch.
            writing.edit_history.lock().clear_redo();
        }
        Ok(report)
    })
    .await?;

    // Both the tree and, usually, the collection changed.
    if report.playlist.is_some() {
        reload(app, Arc::clone(&state)).await?;
    }
    Ok(report)
}

/// Relocate: points a track at the file chosen for it.
///
/// [OBS rekordbox 7.2.19 static, `MissingFileTable::showFileChooser`
/// @0x1012a8408] A file the collection already holds is refused with "This
/// file is already in the collection." and nothing is written; the answer
/// is then `false`.
#[tauri::command]
pub async fn relocate_track<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
    path: String,
) -> AppResult<bool> {
    let library = state.library()?;
    let chosen = std::path::PathBuf::from(&path);
    let held = blocking("relocate_track_check", move || Ok(library.row_for_path(&chosen).is_some())).await?;
    if held {
        return Ok(false);
    }
    edit(app, state, "relocate_track", Touched::Tracks, move |w| {
        w.relocate(&track, std::path::Path::new(&path)).map(|_| ())
    })
    .await?;
    Ok(true)
}

#[tauri::command]
pub async fn create_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    name: String,
    parent: String,
    index: Option<usize>,
) -> AppResult<u32> {
    edit(app, state, "create_playlist", Touched::Playlists, move |w| {
        let id = w.create_playlist(&name, &parent)?;
        if index.is_some() {
            w.move_to(&id, &parent, index)?;
        }
        Ok(())
    })
    .await
}

/// An intelligent playlist's rule, for the editor. Refused when the rule
/// nests groups, which the editor cannot show without losing them.
#[tauri::command]
pub async fn smart_rule(state: State<'_, Arc<AppState>>, playlist: String) -> AppResult<SmartRuleDto> {
    let library = state.library()?;
    blocking("smart_rule", move || {
        let playlists = library.playlists();
        let Some(index) = playlist.parse::<u64>().ok().and_then(|id| playlists.index_of(id)) else {
            return Err(AppError::new(ErrorKind::NotFound, "That playlist is not in the library."));
        };
        let Some(rule) = playlists.smart_rule(index) else {
            // A new intelligent playlist, or one whose rule does not parse,
            // starts from an empty "all of the following".
            return Ok(SmartRuleDto { logic: "all".to_owned(), conditions: Vec::new() });
        };
        rule_to_dto(&rule)
    })
    .await
}

fn rule_to_dto(rule: &rbl_index::SmartRule) -> AppResult<SmartRuleDto> {
    use rbl_index::smart::{Item, Logic};
    let mut conditions = Vec::with_capacity(rule.root.items.len());
    for item in &rule.root.items {
        match item {
            Item::Condition(c) => conditions.push(SmartConditionDto {
                property: c.property.name().to_owned(),
                operator: c.operator.code().to_owned(),
                left: c.left.clone(),
                right: c.right.clone(),
                unit: c.unit.clone(),
            }),
            Item::Group(_) => {
                return Err(AppError::new(
                    ErrorKind::Malformed,
                    "This intelligent playlist nests groups of conditions, which this editor cannot show.",
                ))
            }
        }
    }
    Ok(SmartRuleDto {
        logic: match rule.root.logic {
            Logic::All => "all",
            Logic::Any => "any",
        }
        .to_owned(),
        conditions,
    })
}

fn rule_from_dto(dto: &SmartRuleDto) -> AppResult<rbl_index::SmartRule> {
    use rbl_index::smart::{Condition, Group, Item, Logic, Operator, Property};
    let mut items = Vec::with_capacity(dto.conditions.len());
    for c in &dto.conditions {
        let property = Property::from_name(&c.property);
        if property == Property::Unsupported {
            return Err(AppError::new(ErrorKind::Malformed, format!("{:?} is not a property a rule can use here.", c.property)));
        }
        let Some(operator) = Operator::from_code(&c.operator) else {
            return Err(AppError::new(ErrorKind::Malformed, format!("{:?} is not an operator.", c.operator)));
        };
        items.push(Item::Condition(Condition {
            property,
            operator,
            left: c.left.clone(),
            right: c.right.clone(),
            unit: c.unit.clone(),
        }));
    }
    Ok(rbl_index::SmartRule {
        root: Group { logic: if dto.logic == "any" { Logic::Any } else { Logic::All }, items },
    })
}

/// Create New Intelligent Playlist: a rule under `parent`, named as given.
#[tauri::command]
pub async fn create_smart_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    name: String,
    parent: String,
    rule: SmartRuleDto,
) -> AppResult<u32> {
    let rule = rule_from_dto(&rule)?;
    edit(app, state, "create_smart_playlist", Touched::Playlists, move |w| {
        w.create_smart_playlist(&name, &parent, |id| rule.to_xml(id.parse().unwrap_or(0))).map(|_| ())
    })
    .await
}

/// Replaces an intelligent playlist's rule.
#[tauri::command]
pub async fn set_smart_rule<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    playlist: String,
    rule: SmartRuleDto,
) -> AppResult<u32> {
    let rule = rule_from_dto(&rule)?;
    let xml = rule.to_xml(playlist.parse().unwrap_or(0));
    edit(app, state, "set_smart_rule", Touched::Playlists, move |w| w.set_smart_list(&playlist, &xml).map(|_| ())).await
}

#[tauri::command]
pub async fn create_folder<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    name: String,
    parent: String,
) -> AppResult<u32> {
    edit(app, state, "create_folder", Touched::Playlists, move |w| w.create_folder(&name, &parent).map(|_| ())).await
}

#[tauri::command]
pub async fn rename_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    id: String,
    name: String,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "rename_playlist", Touched::Playlists, "Rename Playlist", move |w| {
        w.rename_with_undo(&id, &name).map(|(_, edit)| LibraryEdit::RenamePlaylist(edit))
    }).await
}

#[tauri::command]
pub async fn move_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    id: String,
    parent: String,
    index: Option<usize>,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "move_playlist", Touched::Playlists, "Move Playlist", move |w| {
        w.move_with_undo(&id, &parent, index).map(|(_, edit)| LibraryEdit::MovePlaylist(edit))
    }).await
}

#[tauri::command]
pub async fn delete_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "delete_playlist", Touched::Playlists, "Delete Playlist", move |w| {
        w.delete_playlist_with_undo(&id).map(|(_, edit)| LibraryEdit::DeletePlaylist(edit))
    }).await
}

#[tauri::command]
pub async fn undo_edit<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<EditHistoryDto> {
    let state = Arc::clone(&state);
    let dto = blocking("undo_edit", move || {
        let _gate = state.edit_gate.lock();
        let entry = state.edit_history.lock().undo.last().cloned()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "There is no library edit to undo."))?;
        let touched = touched_by(&entry.edit);
        let generation = state.write_then(
            |w| apply_history(w, &entry.edit, true),
            |db, ()| refresh_after_edit(&state, db, touched),
        ).map_err(write_error)?;
        let mut history = state.edit_history.lock();
        history.undo.pop();
        history.redo.push(entry);
        Ok(history_dto(generation, &history))
    }).await?;
    let _ = tauri::Emitter::emit(&app, "library:changed", dto.generation);
    let _ = tauri::Emitter::emit(&app, "edit-history:changed", dto.clone());
    Ok(dto)
}

#[tauri::command]
pub async fn redo_edit<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<EditHistoryDto> {
    let state = Arc::clone(&state);
    let dto = blocking("redo_edit", move || {
        let _gate = state.edit_gate.lock();
        let entry = state.edit_history.lock().redo.last().cloned()
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "There is no library edit to redo."))?;
        let touched = touched_by(&entry.edit);
        let generation = state.write_then(
            |w| apply_history(w, &entry.edit, false),
            |db, ()| refresh_after_edit(&state, db, touched),
        ).map_err(write_error)?;
        let mut history = state.edit_history.lock();
        history.redo.pop();
        history.undo.push(entry);
        Ok(history_dto(generation, &history))
    }).await?;
    let _ = tauri::Emitter::emit(&app, "library:changed", dto.generation);
    let _ = tauri::Emitter::emit(&app, "edit-history:changed", dto.clone());
    Ok(dto)
}

#[tauri::command]
pub async fn add_tracks_to_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    playlist: String,
    tracks: Vec<String>,
    allow_duplicates: Option<bool>,
) -> AppResult<u32> {
    let allow = allow_duplicates.unwrap_or(false);
    edit(app, state, "add_tracks_to_playlist", Touched::Playlists, move |w| {
        w.add_tracks_allowing(&playlist, &tracks, allow).map(|_| ())
    })
    .await
}

/// Which of `tracks` the playlist already holds, for the question asked
/// before they are added again.
#[tauri::command]
pub async fn playlist_duplicates(
    state: State<'_, Arc<AppState>>,
    playlist: String,
    tracks: Vec<String>,
) -> AppResult<Vec<String>> {
    let library = state.library()?;
    blocking("playlist_duplicates", move || {
        let playlists = library.playlists();
        let Some(index) = playlist.parse::<u64>().ok().and_then(|id| playlists.index_of(id)) else {
            return Err(AppError::new(ErrorKind::NotFound, "That playlist is not in the library."));
        };
        let held: std::collections::HashSet<_> = playlists.members.get(index).into_iter().flatten().copied().collect();
        drop(playlists);
        Ok(tracks
            .into_iter()
            .filter(|track| library.row_of(track).is_some_and(|row| held.contains(&row)))
            .collect())
    })
    .await
}

/// Reload Tag: the file's tags read again over each track's row.
#[tauri::command]
pub async fn reload_tags<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<u32> {
    edit(app, state, "reload_tags", Touched::Tracks, move |w| {
        for track in &tracks {
            w.reload_tags(track)?;
        }
        Ok(())
    })
    .await
}

/// Puts tracks on the Tag List, rekordbox's temporary list, on the end.
#[tauri::command]
pub async fn add_to_tag_list<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<u32> {
    edit(app, state, "add_to_tag_list", Touched::TagList, move |w| w.tag_list_add(&tracks).map(|_| ())).await
}

#[tauri::command]
pub async fn remove_from_tag_list<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<u32> {
    edit(app, state, "remove_from_tag_list", Touched::TagList, move |w| w.tag_list_remove(&tracks).map(|_| ()))
        .await
}

#[tauri::command]
pub async fn clear_tag_list<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<u32> {
    edit(app, state, "clear_tag_list", Touched::TagList, move |w| w.tag_list_clear().map(|_| ())).await
}

#[tauri::command]
pub async fn remove_tracks_from_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    playlist: String,
    tracks: Vec<String>,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "remove_tracks_from_playlist", Touched::Playlists, "Remove Tracks from Playlist", move |w| {
        w.remove_tracks_with_undo(&playlist, &tracks).map(|(_, edit)| LibraryEdit::RemovePlaylistTracks(edit))
    }).await
}

/// Tracks that share a title and an artist, case and accents aside.
///
/// One pass over the folded title column and the artists' folded names —
/// both already in memory for search and sort — into a map of groups, so
/// 38,681 tracks cost a few milliseconds. Same file size or same audio is
/// not tried: a re-encode of the same track has neither, and the same
/// title under the same artist is what a person calls a duplicate.
#[tauri::command]
pub async fn find_duplicates(state: State<'_, Arc<AppState>>, limit: u32) -> AppResult<DuplicatesDto> {
    let library = state.library()?;
    let wanted = (limit as usize).min(MAX_ROWS as usize);
    blocking("find_duplicates", move || {
        let mut groups: std::collections::HashMap<(&str, &str), Vec<usize>> = std::collections::HashMap::new();
        for index in 0..library.len() {
            let title = library.title_folded.get(index);
            if title.trim().is_empty() {
                continue;
            }
            let artist = library.artists.folded(library.artist.get(index).copied().unwrap_or(rbl_index::NO_ID));
            groups.entry((title, artist)).or_default().push(index);
        }
        let mut found: Vec<Vec<usize>> = groups.into_values().filter(|rows| rows.len() > 1).collect();
        // By title, so the list reads the same from one look to the next.
        found.sort_by(|a, b| {
            let name = |rows: &Vec<usize>| rows.first().map_or("", |&i| library.title_folded.get(i)).to_owned();
            name(a).cmp(&name(b))
        });
        let extra = found.iter().map(|rows| u32::try_from(rows.len() - 1).unwrap_or(u32::MAX)).fold(0_u32, u32::saturating_add);
        let shown = found
            .iter()
            .take(wanted)
            .map(|rows| {
                let first = rows.first().copied().unwrap_or(0);
                DuplicateGroupDto {
                    title: library.title.get(first).to_owned(),
                    artist: library.artist_name(u32::try_from(first).unwrap_or(0)).to_owned(),
                    tracks: rows
                        .iter()
                        .map(|&i| {
                            let path = library.folder_path.get(i);
                            DuplicateTrackDto {
                                id: library.ids.get(i).copied().unwrap_or(0).to_string(),
                                path: path.to_owned(),
                                duration_sec: library.length_sec.get(i).copied().unwrap_or(0),
                                present: !path.is_empty() && std::path::Path::new(path).is_file(),
                            }
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(DuplicatesDto { groups: u32::try_from(found.len()).unwrap_or(u32::MAX), extra, shown })
    })
    .await
}

/// Imports a rekordbox XML collection: the files it names into the
/// library, the playlist tree, and the cues of each track that landed.
#[tauri::command]
pub async fn import_xml<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    path: String,
    replace: Option<bool>,
) -> AppResult<XmlImportReportDto> {
    import_collection(app, state, path, "import_xml", replace.unwrap_or(false), |text| {
        let document = rbl_db::xml::XmlLibrary::parse(text);
        if document.tracks.is_empty() && document.nodes.is_empty() {
            return Err(AppError::new(ErrorKind::Malformed, "That is not a rekordbox XML collection."));
        }
        Ok(document)
    })
    .await
}

/// File › Import iTunes Library…: Music.app's `Library.xml`, its tracks
/// and its playlist tree, through the same importer as a rekordbox XML
/// collection.
#[tauri::command]
pub async fn import_itunes<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    path: String,
    replace: Option<bool>,
) -> AppResult<XmlImportReportDto> {
    import_collection(app, state, path, "import_itunes", replace.unwrap_or(false), |text| {
        rbl_db::itunes::parse(text)
            .ok_or_else(|| AppError::new(ErrorKind::Malformed, "That is not an iTunes or Music library file."))
    })
    .await
}

/// The iTunes column's playlist tree: folders and playlists only, flattened
/// with a 1-based depth to sit beside the rekordbox column, and an
/// `itunes:<index>` id so a selective import can name the chosen ones.
fn itunes_tree_dto(library: &rbl_db::xml::XmlLibrary) -> Vec<TreeNodeDto> {
    library
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| TreeNodeDto {
            id: format!("itunes:{index}"),
            name: node.name.clone(),
            kind: if node.folder { "folder" } else { "playlist" },
            // Parsed 0-based below ROOT; the Sync Manager draws top-level rows
            // at depth 1, as it does the rekordbox column.
            depth: u32::try_from(node.depth + 1).unwrap_or(1),
            expanded: None,
            child_count: None,
        })
        .collect()
}

/// The iTunes / Music library at its usual place, for the Sync Manager's iTunes
/// column. `None` when no shared `Library.xml` is found — Music.app writes one
/// only when "Share Library XML with other applications" is on — so the column
/// can offer a file picker instead.
#[tauri::command]
pub async fn itunes_default_library() -> AppResult<Option<ItunesLibraryDto>> {
    blocking("itunes_default_library", move || {
        let Some(music) = dirs::audio_dir() else { return Ok(None) };
        for path in rbl_db::itunes::candidate_library_paths(&music) {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if let Some(library) = rbl_db::itunes::parse(&text) {
                return Ok(Some(ItunesLibraryDto {
                    path: path.to_string_lossy().into_owned(),
                    tree: itunes_tree_dto(&library),
                }));
            }
        }
        Ok(None)
    })
    .await
}

/// The iTunes / Music library at a file the DJ chose, for the iTunes column.
#[tauri::command]
pub async fn itunes_library_at(path: String) -> AppResult<ItunesLibraryDto> {
    blocking("itunes_library_at", move || {
        let text = std::fs::read_to_string(&path).map_err(|e| {
            AppError::new(ErrorKind::NotFound, "That file could not be read.").with_detail(e.to_string())
        })?;
        let library = rbl_db::itunes::parse(&text)
            .ok_or_else(|| AppError::new(ErrorKind::Malformed, "That is not an iTunes or Music library file."))?;
        Ok(ItunesLibraryDto { path, tree: itunes_tree_dto(&library) })
    })
    .await
}

/// Imports the ticked iTunes playlists — `itunes:<index>` ids from the tree
/// [`itunes_library_at`] or [`itunes_default_library`] returned — into the
/// library: their tracks, the folders above them, and each track's rating and
/// comment, through the same importer a whole file goes through.
#[tauri::command]
pub async fn import_itunes_selected<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    path: String,
    ids: Vec<String>,
    replace: Option<bool>,
) -> AppResult<XmlImportReportDto> {
    let keep: std::collections::BTreeSet<usize> =
        ids.iter().filter_map(|id| id.strip_prefix("itunes:").and_then(|n| n.parse().ok())).collect();
    if keep.is_empty() {
        return Err(AppError::new(ErrorKind::Malformed, "Select at least one iTunes playlist to import."));
    }
    import_collection(app, state, path, "import_itunes_selected", replace.unwrap_or(false), move |text| {
        let full = rbl_db::itunes::parse(text)
            .ok_or_else(|| AppError::new(ErrorKind::Malformed, "That is not an iTunes or Music library file."))?;
        Ok(rbl_db::xml::subset(&full, &keep))
    })
    .await
}

/// Reads a collection file with `parse` and imports what it holds.
///
/// A folder or playlist the file holds that already stands in the library
/// under the same parent with the same name is replaced, as rekordbox does
/// (issue #152) — but only with `replace`. Without it, and when there is one,
/// nothing is written and the report's `same_named` lists them, so the caller
/// can ask rekordbox's question and import again with `replace` on OK.
async fn import_collection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    path: String,
    name: &'static str,
    replace: bool,
    parse: impl FnOnce(&str) -> AppResult<rbl_db::xml::XmlLibrary> + Send + 'static,
) -> AppResult<XmlImportReportDto> {
    let state_for_reload = Arc::clone(&state);
    let writing = Arc::clone(&state);
    let progress_app = app.clone();
    let (report, changed_playlists) = blocking(name, move || {
        let text = std::fs::read_to_string(&path).map_err(|e| {
            AppError::new(ErrorKind::NotFound, "That file could not be read.").with_detail(e.to_string())
        })?;
        let document = parse(&text)?;
        let mut on_progress = |done: usize, total: usize| {
            let _ = tauri::Emitter::emit(
                &progress_app,
                "import:progress",
                ExportProgressDto {
                    path: path.clone(),
                    state: "writing",
                    done: u32::try_from(done).unwrap_or(u32::MAX),
                    total: u32::try_from(total).unwrap_or(u32::MAX),
                    title: String::new(),
                },
            );
        };
        // Looked for under the same edit gate as the import, so nothing can
        // make a same-named list between the question and the write.
        let outcome = writing
            .write(|writer| {
                if !replace {
                    let same_named = rbl_db::xml::same_named_lists(writer.library(), &document)?;
                    if !same_named.is_empty() {
                        return Ok(Err(same_named));
                    }
                }
                on_progress(0, document.tracks.len());
                rbl_db::xml::import(writer, &document, &mut on_progress).map(Ok)
            })
            .map_err(write_error)?;
        let report = match outcome {
            Ok(report) => report,
            Err(same_named) => {
                return Ok((XmlImportReportDto {
                    imported: 0,
                    existing: 0,
                    skipped: Vec::new(),
                    playlists: 0,
                    cues: 0,
                    tracks: Vec::new(),
                    same_named,
                }, false));
            }
        };
        // A replaced playlist may have changed without a list or track being
        // made: that, too, needs a reload.
        let changed_playlists = report.playlist_tracks > 0 || report.playlists_replaced > 0;
        Ok((XmlImportReportDto {
            imported: u32::try_from(report.imported).unwrap_or(u32::MAX),
            existing: u32::try_from(report.existing).unwrap_or(u32::MAX),
            skipped: report.skipped,
            playlists: u32::try_from(report.playlists).unwrap_or(u32::MAX),
            cues: u32::try_from(report.cues).unwrap_or(u32::MAX),
            tracks: report.tracks.into_iter().map(|(id, title)| crate::dto::ImportedTrackDto { id, title }).collect(),
            same_named: Vec::new(),
        }, changed_playlists))
    })
    .await?;
    if report.imported > 0 || report.playlists > 0 || changed_playlists {
        reload(app, state_for_reload).await?;
    }
    Ok(report)
}

/// Export Loop As WAV: the loop's stretch of the track, `in_ms` to
/// `out_ms`, as a 16-bit WAV at `path`. Resolves to the frames written.
#[tauri::command]
pub async fn export_loop_wav(
    state: State<'_, Arc<AppState>>,
    track: String,
    in_ms: f64,
    out_ms: f64,
    path: String,
) -> AppResult<u64> {
    let library = state.library()?;
    blocking("export_loop_wav", move || {
        let Some(source) = library.audio_path_of(&track).map(std::path::PathBuf::from) else {
            return Err(AppError::new(ErrorKind::NotFound, "That track has no file to read."));
        };
        rbl_audio::write_range_wav(&source, in_ms / 1000.0, out_ms / 1000.0, std::path::Path::new(&path))
            .map_err(|e| AppError::new(ErrorKind::Malformed, format!("The loop could not be written: {e}")))
    })
    .await
}

/// Export a playlist to a file: `m3u8`, which any player reads, or the
/// tab-separated `txt` rekordbox writes. An intelligent playlist is what
/// its rule admits now. Resolves to how many tracks were written.
#[tauri::command]
pub async fn export_playlist_file(
    state: State<'_, Arc<AppState>>,
    playlist: String,
    path: String,
    format: String,
) -> AppResult<u32> {
    let library = state.library()?;
    blocking("export_playlist_file", move || {
        let playlists = library.playlists();
        let Some(index) = playlist.parse::<u64>().ok().and_then(|numeric| playlists.index_of(numeric)) else {
            return Err(AppError::new(ErrorKind::NotFound, "That playlist is not in the library."));
        };
        let source = if playlists.is_smart(index) {
            rbl_index::TrackSource::SmartPlaylist(index)
        } else {
            rbl_index::TrackSource::Playlist(index)
        };
        let rows = library.source_rows_unlocked(&playlists, &source);
        drop(playlists);
        let text = match format.as_str() {
            "txt" => playlist_txt(&library, &rows),
            _ => playlist_m3u8(&library, &rows),
        };
        std::fs::write(&path, text).map_err(|e| {
            AppError::new(ErrorKind::Internal, "The playlist file could not be written.").with_detail(e.to_string())
        })?;
        Ok(u32::try_from(rows.len()).unwrap_or(u32::MAX))
    })
    .await
}

/// An extended M3U: a line of length and title, then the file, per track.
fn playlist_m3u8(library: &rbl_index::Library, rows: &[u32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("#EXTM3U\n");
    for &row in rows {
        let i = row as usize;
        let artist = library.artist_name(row);
        let title = library.title.get(i);
        let name = if artist.is_empty() { title.to_owned() } else { format!("{artist} - {title}") };
        let _ = writeln!(out, "#EXTINF:{},{name}\n{}", library.length_sec.get(i).copied().unwrap_or(0), library.folder_path.get(i));
    }
    out
}

/// rekordbox's tab-separated listing: a header, then one line per track in
/// the playlist's order, times as `m:ss`.
fn playlist_txt(library: &rbl_index::Library, rows: &[u32]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("#\tTrack Title\tArtist\tAlbum\tGenre\tBPM\tRating\tTime\tKey\tDate Added\n");
    let clean = |text: &str| text.replace(['\t', '\n', '\r'], " ");
    for (n, &row) in rows.iter().enumerate() {
        let i = row as usize;
        let secs = library.length_sec.get(i).copied().unwrap_or(0);
        let bpm = f64::from(library.bpm_x100.get(i).copied().unwrap_or(0)) / 100.0;
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{bpm:.2}\t{}\t{}:{:02}\t{}\t{}",
            n + 1,
            clean(library.title.get(i)),
            clean(library.artist_name(row)),
            clean(library.album_name(row)),
            clean(library.genre_name(row)),
            library.rating.get(i).copied().unwrap_or(0),
            secs / 60,
            secs % 60,
            clean(library.key_name(row)),
            clean(library.date_added.get(i)),
        );
    }
    out
}

/// Writes the collection as rekordbox's XML to `path`; resolves to how many
/// tracks it holds.
#[tauri::command]
pub async fn export_xml(state: State<'_, Arc<AppState>>, path: String) -> AppResult<u32> {
    let library = state.library()?;
    blocking("export_xml", move || {
        let text = rbl_index::export_xml(&library);
        std::fs::write(&path, text).map_err(|e| {
            AppError::new(ErrorKind::Internal, "The XML could not be written.").with_detail(e.to_string())
        })?;
        Ok(u32::try_from(library.len()).unwrap_or(u32::MAX))
    })
    .await
}

#[tauri::command]
pub async fn backup_sizes(state: State<'_, Arc<AppState>>, refresh: Option<bool>) -> AppResult<crate::backup_sizes::BackupSizes> {
    let state = Arc::clone(&state);
    blocking("backup_sizes", move || crate::backup_sizes::cached(&state, refresh.unwrap_or(false))).await
}

/// Open the configured folder, creating it if no backup has been taken yet.
#[tauri::command]
pub async fn open_backup_directory<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    let path = state.backup_destination();
    blocking("open_backup_directory", move || {
        crate::durable::create_dir_all(&path).map_err(|e| {
            AppError::internal("The backup folder could not be created.").with_detail(e.to_string())
        })?;
        app.opener().open_path(path.to_string_lossy().into_owned(), None::<&str>).map_err(|e| {
            AppError::internal("The backup folder could not be opened.").with_detail(e.to_string())
        })
    }).await
}

/// The configured destination, whether or not any backups exist yet.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's State extractor is injected by value")]
pub fn backup_directory(state: State<'_, Arc<AppState>>) -> String {
    state.backup_destination().to_string_lossy().into_owned()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's State extractor is injected by value")]
pub fn backup_progress(state: State<'_, Arc<AppState>>) -> crate::backups::BackupProgress {
    state.backup_progress.lock().clone()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's State extractor is injected by value")]
pub fn cancel_backup(state: State<'_, Arc<AppState>>) {
    crate::backups::cancel(&state);
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's State extractor is injected by value")]
pub fn start_backup(state: State<'_, Arc<AppState>>) -> AppResult<()> {
    crate::backups::start(Arc::clone(&state))
}

/// Explicit backups, newest first.
#[tauri::command]
pub async fn list_backups(state: State<'_, Arc<AppState>>) -> AppResult<Vec<BackupDto>> {
    let state = Arc::clone(&state);
    blocking("list_backups", move || crate::backups::list(&state)).await
}

#[tauri::command]
pub async fn back_up_library(state: State<'_, Arc<AppState>>) -> AppResult<String> {
    let state = Arc::clone(&state);
    blocking("back_up_library", move || crate::backups::create(&state)).await
}

#[tauri::command]
pub async fn delete_backup(state: State<'_, Arc<AppState>>, path: String) -> AppResult<()> {
    let state = Arc::clone(&state);
    blocking("delete_backup", move || crate::backups::delete(&state, std::path::Path::new(&path))).await
}

#[tauri::command]
pub async fn set_backup_directory(state: State<'_, Arc<AppState>>, directory: String) -> AppResult<String> {
    let state = Arc::clone(&state);
    blocking("set_backup_directory", move || state.set_backup_destination(std::path::Path::new(&directory))).await
}

/// A play: the track goes on today's history session and its play count
/// goes up. What the player asks for after a minute of a track.
#[tauri::command]
pub async fn record_play<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<u32> {
    edit(app, state, "record_play", Touched::Histories(vec![track.clone()]), move |w| w.record_play(&track).map(|_| ())).await
}

/// Remove from History: the tracks' plays leave the session.
#[tauri::command]
pub async fn remove_from_history<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    history: String,
    tracks: Vec<String>,
) -> AppResult<u32> {
    edit(app, state, "remove_from_history", Touched::Histories(Vec::new()), move |w| {
        w.remove_from_history(&history, &tracks).map(|_| ())
    })
    .await
}

/// Opens a web address in the person's browser: the About pane's links.
/// Only `https://`, so nothing in the webview can hand the OS a file or a
/// scheme of its own.
#[tauri::command]
pub async fn open_url<R: tauri::Runtime>(app: tauri::AppHandle<R>, url: String) -> AppResult<()> {
    if !url.starts_with("https://") {
        return Err(AppError::new(ErrorKind::Malformed, "Only an https address can be opened."));
    }
    app.opener().open_url(&url, None::<&str>).map_err(|e| {
        AppError::new(ErrorKind::Internal, "That address could not be opened.").with_detail(e.to_string())
    })
}

/// Reset DJ Play Count: the tracks' counts go back to zero.
#[tauri::command]
pub async fn reset_play_count<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "reset_play_count", Touched::Metadata(tracks.clone()), "Track Edit", move |w| {
        let mut edits = Vec::with_capacity(tracks.len());
        for track in &tracks {
            let (_, edit) = w.set_field_with_undo(track, rbl_db::write::TrackField::PlayCount, "0")?;
            if !edit.is_empty() { edits.push(edit); }
        }
        Ok(LibraryEdit::Track(edits))
    })
    .await
}

/// Remove from Collection: the tracks leave the library and every playlist
/// they were in. The files stay where they are, as rekordbox leaves them.
#[tauri::command]
pub async fn remove_from_collection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<u32> {
    permanent_edit(app, state, "remove_from_collection", Touched::Tracks, move |w| {
        w.delete_tracks(&tracks).map(|_| ())
    })
    .await
}

#[tauri::command]
pub async fn reorder_playlist<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    playlist: String,
    tracks: Vec<String>,
) -> AppResult<u32> {
    edit(app, state, "reorder_playlist", Touched::Playlists, move |w| w.reorder(&playlist, &tracks).map(|_| ())).await
}

/// Runs one track edit on each of `tracks` and keeps them as one step of
/// history, so an edit made with several tracks selected undoes in one go.
///
/// rekordbox's information panel writes a multiple selection the same way:
/// `TrackInfoConcreteMediator::updateDB` takes the whole selected array and
/// sets the one value on each (7.2.11, static analysis).
///
/// Each track is its own transaction, so a failure partway through would
/// leave the tracks before it changed with nothing in the history to take
/// them back. The steps already written are undone, last first, before the
/// error is returned: the edit lands on every track or on none. Ids that
/// are no longer in the library are passed over first; the list keeps a
/// selection's ids after the tracks behind them are removed.
pub(crate) fn each_track<F>(
    w: &mut rbl_db::write::Writer,
    tracks: &[String],
    mut edit: F,
) -> Result<LibraryEdit, rbl_db::DbError>
where
    F: FnMut(&mut rbl_db::write::Writer, &str) -> Result<(rbl_db::write::Changed, rbl_db::write::TrackEdit), rbl_db::DbError>,
{
    let mut edits = Vec::with_capacity(tracks.len());
    for track in tracks {
        let step = match w.has_track(track) {
            Ok(false) => continue,
            Ok(true) => edit(w, track),
            Err(error) => Err(error),
        };
        match step {
            Ok((_, edit)) => {
                if !edit.is_empty() {
                    edits.push(edit);
                }
            }
            Err(error) => return Err(roll_back(w, &edits, error)),
        }
    }
    Ok(LibraryEdit::Track(edits))
}

/// Undoes `applied`, last first, after `error` stopped [`each_track`].
/// Returns the error to report: `error` itself, or one that also says how
/// many tracks could not be put back.
fn roll_back(
    w: &mut rbl_db::write::Writer,
    applied: &[rbl_db::write::TrackEdit],
    error: rbl_db::DbError,
) -> rbl_db::DbError {
    let mut stranded = 0usize;
    for step in applied.iter().rev() {
        if let Err(undo) = w.undo_track_edit(step) {
            tracing::error!(error = %undo, "could not undo a track edit after a failed multi-track edit");
            stranded += 1;
        }
    }
    if stranded == 0 {
        return error;
    }
    // `Io` displays the message as it is, with no prefix of its own.
    rbl_db::DbError::Io(std::io::Error::other(format!(
        "{error}; {stranded} of {} already-edited tracks could not be changed back",
        applied.len()
    )))
}

/// Rates every track in `tracks`: one track from the list, or the whole
/// selection from the information panel.
#[tauri::command]
pub async fn set_track_rating<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    stars: u8,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "set_track_rating", Touched::Metadata(tracks.clone()), "Track Edit", move |w| {
        each_track(w, &tracks, |w, track| w.set_rating_with_undo(track, stars))
    }).await
}

#[tauri::command]
pub async fn set_track_comment<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    comment: String,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "set_track_comment", Touched::Metadata(tracks.clone()), "Track Edit", move |w| {
        each_track(w, &tracks, |w, track| w.set_comment_with_undo(track, &comment))
    }).await
}

#[tauri::command]
pub async fn set_track_color<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    color: Option<String>,
) -> AppResult<EditHistoryDto> {
    recorded_edit(app, state, "set_track_color", Touched::Metadata(tracks.clone()), "Track Edit", move |w| {
        each_track(w, &tracks, |w, track| w.set_color_with_undo(track, color.as_deref()))
    }).await
}

/// The BPMs and keys the track filter bar can offer for a list.
///
/// Counted over the source and query alone — never over the filter's own
/// result, or a picked value would hide the others. One pass over the rows
/// in Rust; the frontend never scans a row array for this.
#[tauri::command]
pub async fn filter_values(
    state: State<'_, Arc<AppState>>,
    spec: ViewSpecDto,
) -> AppResult<FilterValuesDto> {
    let library = state.library()?;
    let parsed = spec_from_wire(&library, &spec);
    blocking("filter_values", move || {
        let values = library.filter_values_scoped(&parsed, spec.search_field);
        Ok(FilterValuesDto {
            bpms: values
                .bpms
                .into_iter()
                .map(|c| CountedDto { value: c.value, count: c.count })
                .collect(),
            keys: values
                .keys
                .into_iter()
                .map(|c| CountedDto { value: c.value, count: c.count })
                .collect(),
            tags: values
                .tags
                .into_iter()
                .map(|c| TagCategoryDto { name: c.name, tags: c.tags })
                .collect(),
        })
    })
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{expand_import_paths, read_analysis};

    /// A track whose analysis folder is gone from disk is exported without
    /// analysis; it does not stop the export of everything else.
    #[test]
    fn a_missing_analysis_file_does_not_fail_the_export() {
        let share = tempfile::tempdir().unwrap();
        assert_eq!(read_analysis(share.path(), "/PIONEER/USBANLZ/ca6/gone/ANLZ0000.DAT").unwrap(), []);
    }

    /// A fixture library loaded into a fresh [`AppState`], its tracks
    /// pointed at files that exist, and an empty folder standing in for a
    /// stick. `prepare` edits the library first.
    fn export_rig(
        dir: &std::path::Path,
        prepare: impl FnOnce(&mut rbl_db::write::Writer),
    ) -> (crate::state::AppState, rbl_db::LibraryLocation, std::path::PathBuf) {
        let shape = rbl_db::fixture::Shape::default();
        let location = rbl_db::fixture::build(dir, shape).unwrap();
        let audio = dir.join("audio");
        std::fs::create_dir_all(&audio).unwrap();
        for index in 0..shape.tracks {
            let file = audio.join(format!("{index}.mp3"));
            std::fs::write(&file, [u8::try_from(index).unwrap(); 64]).unwrap();
            rbl_db::fixture::point_at_audio(&location, index, file.to_str().unwrap(), 20).unwrap();
        }
        let mut writer = rbl_db::write::Writer::open(location.clone(), dir.join("backups")).unwrap();
        writer.disable_automatic_backups();
        prepare(&mut writer);
        drop(writer);
        let state = crate::state::AppState::with_backups(dir.join("backups"));
        reload(&state, &location);
        let stick = dir.join("usb");
        std::fs::create_dir_all(&stick).unwrap();
        (state, location, stick)
    }

    /// Reads the library into `state` again, as an edit does.
    fn reload(state: &crate::state::AppState, location: &rbl_db::LibraryLocation) {
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        drop(db);
        state.set_library(library, false, None, 0, location.clone());
    }

    /// The stick's manifest playlists as (library id, folder) in its order.
    fn on_stick(stick: &std::path::Path) -> Vec<(String, bool)> {
        rbl_export::Manifest::load(stick)
            .unwrap()
            .playlists
            .iter()
            .filter(|p| !p.device_only)
            .map(|p| (p.library_id.to_string(), p.folder))
            .collect()
    }

    fn export_playlist_to(state: &crate::state::AppState, location: &rbl_db::LibraryLocation, stick: &std::path::Path, playlist: &str) {
        let library = state.library().unwrap();
        let selection = super::playlist_export_selection(state, &library, &location.share_root, stick, playlist, false).unwrap();
        super::write_export_with_phase(stick, &selection, None, None, &mut |_| {}, &mut |_| {}, &|| false).unwrap();
    }

    fn export_track_to(state: &crate::state::AppState, location: &rbl_db::LibraryLocation, stick: &std::path::Path, track: &str) {
        let library = state.library().unwrap();
        let selection = super::track_export_selection(state, &library, &location.share_root, stick, &[track.to_owned()]).unwrap();
        super::write_export_with_phase(stick, &selection, None, None, &mut |_| {}, &mut |_| {}, &|| false).unwrap();
    }

    /// Export Playlist from the tree adds the playlist to what the stick
    /// already holds, as rekordbox's does, rather than replacing the stick's
    /// playlists with the one chosen (#304). Exporting one it holds already
    /// keeps its place.
    #[test]
    fn export_playlist_adds_to_what_the_stick_holds() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, stick) = export_rig(dir.path(), |_| {});
        let (a, b) = (rbl_db::fixture::playlist_id(0), rbl_db::fixture::playlist_id(1));
        let leaf = |id: &String| (id.clone(), false);

        export_playlist_to(&state, &location, &stick, &a);
        assert_eq!(on_stick(&stick), [leaf(&a)]);
        export_playlist_to(&state, &location, &stick, &b);
        assert_eq!(on_stick(&stick), [leaf(&a), leaf(&b)], "the first playlist stays on the stick");
        export_playlist_to(&state, &location, &stick, &a);
        assert_eq!(on_stick(&stick), [leaf(&a), leaf(&b)], "a playlist exported again keeps its place");
    }

    /// A track analysed after the index was read goes on the stick analysed:
    /// its BPM, key, length and analysis files, as the row now has them. An
    /// analysis run writes each track's row and files and re-reads the index
    /// only when the run is over; a sync made before then exported those
    /// tracks with no waveform, grid, BPM or key (#309).
    #[test]
    fn a_track_analysed_since_the_index_was_read_is_exported_analysed() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, stick) = export_rig(dir.path(), |_| {});
        let track = rbl_db::fixture::track_id(0);
        let relative = "/PIONEER/USBANLZ/P309/00000309/ANLZ0000.DAT";
        let dat = location.share_root.join(relative.trim_start_matches('/'));
        std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
        std::fs::write(&dat, rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish()).unwrap();
        // What `analyse_track` writes, without the reload the analysis
        // queue makes once it has drained.
        state
            .write(|w| {
                w.ensure_detected_key("F#m")?;
                w.set_analysis(&track, &rbl_db::write::AnalysisWrite { bpm_x100: 12_650, key: Some("F#m"), analysis_path: relative, length_sec: Some(241) })
            })
            .unwrap();
        let library = state.library().unwrap();
        let row = library.row_of(&track).unwrap() as usize;
        assert!(library.analysis_path.get(row).is_empty(), "the index still has the track unanalysed");

        export_playlist_to(&state, &location, &stick, &rbl_db::fixture::playlist_id(0));

        let snapshot = rbl_export::snapshot::Snapshot::read(&stick).unwrap();
        let one = snapshot.one.as_ref().unwrap();
        let id = snapshot.identity.iter().find(|(_, (_, content))| content.to_string() == track).map(|(id, _)| *id).unwrap();
        let exported = one.tracks.iter().find(|t| t.id == id).unwrap();
        assert_eq!((exported.bpm, exported.key.as_str()), (12_650, "F#m"));
        assert!(!exported.analysis.is_empty(), "the stick names the track's analysis");
        assert!(stick.join(exported.analysis.trim_start_matches('/')).is_file(), "and the file is there");
        let legacy = snapshot.legacy.as_ref().unwrap().tracks.iter().find(|t| t.id == id).unwrap();
        assert_eq!((legacy.bpm, legacy.analysis.as_str()), (12_650, exported.analysis.as_str()), "export.pdb says the same");
    }

    /// A playlist deleted from the library stays on the stick through Export
    /// Playlist and Export Track: the stick's copy may be the only one left.
    #[test]
    fn export_playlist_and_track_keep_a_playlist_the_library_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, stick) = export_rig(dir.path(), |_| {});
        let (a, b, c) = (rbl_db::fixture::playlist_id(0), rbl_db::fixture::playlist_id(1), rbl_db::fixture::playlist_id(2));
        let leaf = |id: &String| (id.clone(), false);
        export_playlist_to(&state, &location, &stick, &a);
        export_playlist_to(&state, &location, &stick, &b);
        let device_ids = |stick: &std::path::Path| -> Vec<u32> {
            rbl_export::Manifest::load(stick).unwrap().playlists.iter().map(|p| p.export_id).collect()
        };
        let ids_before = device_ids(&stick);

        let mut writer = rbl_db::write::Writer::open(location.clone(), dir.path().join("backups")).unwrap();
        writer.disable_automatic_backups();
        writer.delete_playlist(&a).unwrap();
        drop(writer);
        reload(&state, &location);

        export_playlist_to(&state, &location, &stick, &c);
        assert_eq!(on_stick(&stick), [leaf(&a), leaf(&b), leaf(&c)], "Export Playlist keeps the deleted playlist");
        assert_eq!(device_ids(&stick)[..2], ids_before[..], "in its place, under its device id");
        let snapshot = rbl_export::snapshot::Snapshot::read(&stick).unwrap();
        let held = snapshot.one.as_ref().unwrap().playlists.iter().find(|p| p.id == ids_before[0]).unwrap().tracks.len();
        assert_eq!(held, rbl_db::fixture::Shape::default().tracks_per_playlist, "with the tracks it held");

        export_track_to(&state, &location, &stick, &rbl_db::fixture::track_id(39));
        assert_eq!(on_stick(&stick), [leaf(&a), leaf(&b), leaf(&c)], "Export Track keeps it too");
        assert!(rbl_export::verify(&stick).unwrap().is_ok());
    }

    /// An empty folder Export Folder put on the stick stays there when a
    /// playlist is exported after it (#304).
    #[test]
    fn export_playlist_keeps_an_empty_folder_export_folder_put_there() {
        let dir = tempfile::tempdir().unwrap();
        let mut ids = (String::new(), String::new(), String::new());
        let (state, location, stick) = export_rig(dir.path(), |w| {
            let folder = w.create_folder("F", rbl_db::write::ROOT).unwrap();
            let empty = w.create_folder("E", &folder).unwrap();
            let inside = w.create_playlist("P", &folder).unwrap();
            w.add_tracks(&inside, &[rbl_db::fixture::track_id(30)]).unwrap();
            ids = (folder, empty, inside);
        });
        let (folder, empty, inside) = ids;
        let a = rbl_db::fixture::playlist_id(0);

        export_playlist_to(&state, &location, &stick, &folder);
        let mut after_folder = on_stick(&stick);
        after_folder.sort();
        let mut expected = vec![(folder.clone(), true), (empty.clone(), true), (inside.clone(), false)];
        expected.sort();
        assert_eq!(after_folder, expected);

        export_playlist_to(&state, &location, &stick, &a);
        let held = on_stick(&stick);
        assert!(held.contains(&(empty, true)), "the empty folder stays: {held:?}");
        assert!(held.contains(&(inside, false)) && held.contains(&(folder, true)) && held.contains(&(a, false)), "{held:?}");
        assert!(rbl_export::verify(&stick).unwrap().is_ok());
    }

    /// A stick an older rbxport synced records no library id. It counts as
    /// this library's when rekordbox's sync record names this library, as
    /// the export itself accepts it, so its playlists are refreshed and kept.
    #[test]
    fn held_on_stick_accepts_an_older_record_this_library_synced() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, stick) = export_rig(dir.path(), |_| {});
        let (a, b) = (rbl_db::fixture::playlist_id(0), rbl_db::fixture::playlist_id(1));
        export_playlist_to(&state, &location, &stick, &a);
        assert!(rbl_export::sync_record::read(&stick).is_some_and(|r| r.db_id != 0), "the export left rekordbox's record");
        let mut manifest = rbl_export::Manifest::load(&stick).unwrap();
        manifest.db_id = 0;
        manifest.save(&stick).unwrap();

        let library = state.library().unwrap();
        assert_eq!(super::held_on_stick(&state, &library, &stick).0, std::slice::from_ref(&a));
        export_playlist_to(&state, &location, &stick, &b);
        assert_eq!(on_stick(&stick), [(a, false), (b, false)]);
    }

    /// An edit made with three tracks selected writes all three, and one
    /// undo takes all three back: it is one step of history, as it is one
    /// action in rekordbox's information panel.
    #[test]
    fn an_edit_over_several_tracks_is_written_to_each_and_undone_as_one() {
        use crate::state::{AppState, LibraryEdit};
        use rbl_db::write::TrackField;

        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location);
        crate::backups::create(&state).unwrap();
        let tracks: Vec<String> = (1..=3).map(rbl_db::fixture::track_id).collect();
        let untouched = rbl_db::fixture::track_id(4);
        let genre = |state: &AppState, id: &str| {
            state.read_db(|db| db.track_details(id)).unwrap().unwrap().genre
        };

        let edit = state
            .write_then(
                |w| super::each_track(w, &tracks, |w, track| w.set_field_with_undo(track, TrackField::Genre, "Techno")),
                |_, edit| Ok(edit),
            )
            .unwrap();
        let steps = match &edit {
            LibraryEdit::Track(steps) => steps.len(),
            _ => 0,
        };
        assert_eq!(steps, 3, "one track edit with a step per track");
        for track in &tracks {
            assert_eq!(genre(&state, track), "Techno");
        }
        assert_eq!(genre(&state, &untouched), "");

        state.write_then(|w| super::apply_history(w, &edit, true), |_, ()| Ok(())).unwrap();
        for track in &tracks {
            assert_eq!(genre(&state, track), "", "undone on {track}");
        }
    }

    /// A step that fails partway through a multiple selection takes back the
    /// tracks already written, so the edit lands on all of them or on none:
    /// each track is its own transaction, and a failed edit records nothing
    /// an Undo could reach.
    #[test]
    fn a_failure_partway_through_several_tracks_puts_the_earlier_ones_back() {
        use crate::state::AppState;
        use rbl_db::write::TrackField;

        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location);
        crate::backups::create(&state).unwrap();
        let tracks: Vec<String> = (1..=4).map(rbl_db::fixture::track_id).collect();
        let failing = tracks[2].clone();
        let genre = |state: &AppState, id: &str| {
            state.read_db(|db| db.track_details(id)).unwrap().unwrap().genre
        };

        let result = state.write_then(
            |w| {
                super::each_track(w, &tracks, |w, track| {
                    if track == failing {
                        return Err(rbl_db::DbError::WriteRefused("disk full".to_owned()));
                    }
                    w.set_field_with_undo(track, TrackField::Genre, "Techno")
                })
            },
            |_, edit| Ok(edit),
        );

        let error = result.err().expect("the failing step is reported");
        assert!(error.to_string().contains("disk full"), "{error}");
        for track in &tracks {
            assert_eq!(genre(&state, track), "", "{track} is as it was");
        }
    }

    /// Sorted file names, so a filesystem-dependent walk order does not make
    /// the assertions flaky.
    fn names(paths: &[std::path::PathBuf]) -> Vec<String> {
        let mut out: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn a_directory_is_walked_recursively_for_audio_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // perf-ok: a test's fixture, not a command.
        std::fs::create_dir_all(root.join("subdir/deep")).unwrap();
        std::fs::write(root.join("top.mp3"), b"x").unwrap();
        std::fs::write(root.join("cover.jpg"), b"x").unwrap();
        std::fs::write(root.join("subdir/mid.flac"), b"x").unwrap();
        std::fs::write(root.join("subdir/deep/low.m4a"), b"x").unwrap();
        std::fs::write(root.join("subdir/notes.txt"), b"x").unwrap();

        let files = expand_import_paths(&[root.to_string_lossy().into_owned()]);
        // Every audio file at every depth is collected; the .jpg and .txt are
        // silently left out rather than reported as skipped.
        assert_eq!(names(&files), ["low.m4a", "mid.flac", "top.mp3"]);
    }

    #[test]
    fn a_chosen_file_is_kept_even_when_it_is_not_audio() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("song.mp3");
        let sheet = dir.path().join("liner.txt");
        std::fs::write(&song, b"x").unwrap();
        std::fs::write(&sheet, b"x").unwrap();

        // A file the user picked by hand reaches import_file as chosen, so a
        // non-audio pick is reported as skipped there rather than dropped here.
        let files = expand_import_paths(&[
            song.to_string_lossy().into_owned(),
            sheet.to_string_lossy().into_owned(),
        ]);
        assert_eq!(names(&files), ["liner.txt", "song.mp3"]);
    }

    #[test]
    fn hidden_directories_are_passed_over() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".Trashes")).unwrap();
        std::fs::write(dir.path().join(".Trashes/ghost.mp3"), b"x").unwrap();
        std::fs::write(dir.path().join("real.mp3"), b"x").unwrap();

        let files = expand_import_paths(&[dir.path().to_string_lossy().into_owned()]);
        assert_eq!(names(&files), ["real.mp3"]);
    }
}
