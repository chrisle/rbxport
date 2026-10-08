//! The information panel's commands: one track's full record, the lists its
//! dropdowns offer, and the fields it may write.
//!
//! The record is a point read from `djmdContent` by id, not a widening of the
//! columnar index — see `rbl_db::details`. The deck's INFO tab reads the same
//! record, which is why the DTO carries everything a panel could show rather
//! than only what the row DTO lacks.

use std::sync::Arc;

use serde::Serialize;
use tauri::{Manager, State};

use crate::commands::{blocking, edit, recorded_edit, write_error, Touched};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::state::{AppState, LibraryEdit};

/// One track, in full. About 1 KB of JSON.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackDetailsDto {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub original_artist: String,
    pub composer: String,
    pub remixer: String,
    pub lyricist: String,
    pub genre: String,
    pub label: String,
    pub key: String,
    pub comment: String,
    pub mix_name: String,
    pub message: String,
    /// `"0"` or empty for none, `"1"` to `"8"` for rekordbox's eight colours.
    pub color: String,
    pub rating: u8,
    pub bpm_x100: u32,
    pub duration_sec: u32,
    pub year: u32,
    pub track_number: u32,
    pub disc_number: u32,
    pub play_count: u32,
    /// rekordbox's own code: 1 MP3, 4 M4A, 5 FLAC, 11 WAV, 12 AIFF.
    pub file_type: u32,
    pub file_size: u64,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub date_created: String,
    pub release_date: String,
    pub path: String,
    pub hot_cue_auto_load: bool,
    pub publish: bool,
    /// Whether `rbl://localhost/artwork/<id>` will serve anything for this track.
    pub has_artwork: bool,
    /// The ids of the My Tags on the track.
    pub my_tags: Vec<String>,
}

/// One My Tag, for the Info tab's toggles.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyTagDto {
    pub id: String,
    pub name: String,
}

/// A My Tag category and the tags under it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MyTagCategoryDto {
    pub name: String,
    pub tags: Vec<MyTagDto>,
}

/// What the Info tab's dropdowns offer.
///
/// Keys and genres come from the index's interners — what the library holds
/// — rather than a fixed list, because rekordbox's own dropdowns offer what
/// the library holds. 25 keys and 448 genres in the reference library, well
/// inside the response cap.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackLookupsDto {
    pub keys: Vec<String>,
    pub genres: Vec<String>,
    /// The library's My Tags, by category, with the ids the toggles set.
    pub my_tag_categories: Vec<MyTagCategoryDto>,
}

/// Names past this many are dropped, so a library with an absurd genre list
/// cannot push one response over the cap.
const MAX_NAMES: usize = 2000;

#[tauri::command]
pub async fn track_details(
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<TrackDetailsDto> {
    let state = Arc::clone(&state);
    blocking("track_details", move || {
        let library = state.library()?;
        let has_artwork = library.artwork_path_of(&track).is_some_and(|p| !p.is_empty());
        let details = state
            .read_db(|db| db.track_details(&track))
            .map_err(write_error)?;
        let Some(d) = details else {
            return Err(AppError::new(ErrorKind::NotFound, "That track is no longer in the library.")
                .with_detail(format!("track {track}")));
        };
        Ok(TrackDetailsDto {
            id: d.id,
            title: d.title,
            artist: d.artist,
            album: d.album,
            album_artist: d.album_artist,
            original_artist: d.original_artist,
            composer: d.composer,
            remixer: d.remixer,
            lyricist: d.lyricist,
            genre: d.genre,
            label: d.label,
            key: d.key,
            comment: d.comment,
            mix_name: d.mix_name,
            message: d.message,
            color: d.color,
            rating: d.rating,
            bpm_x100: d.bpm_x100,
            duration_sec: d.duration_sec,
            year: d.year,
            track_number: d.track_number,
            disc_number: d.disc_number,
            play_count: d.play_count,
            file_type: d.file_type,
            file_size: d.file_size,
            bitrate: d.bitrate,
            sample_rate: d.sample_rate,
            bit_depth: d.bit_depth,
            date_created: d.date_created,
            release_date: d.release_date,
            path: d.path,
            hot_cue_auto_load: d.hot_cue_auto_load,
            publish: d.publish,
            has_artwork,
            my_tags: d.my_tags,
        })
    })
    .await
}

#[tauri::command]
pub async fn track_lookups(state: State<'_, Arc<AppState>>) -> AppResult<TrackLookupsDto> {
    let library = state.library()?;
    let state = Arc::clone(&state);
    blocking("track_lookups", move || {
        // Categories first in their order, then each one's tags in theirs.
        let rows = state
            .read_db(|db| rbl_db::export_info::my_tags(db.connection()))
            .map_err(write_error)?;
        let mut my_tag_categories: Vec<(String, MyTagCategoryDto)> = rows
            .iter()
            .filter(|t| t.attribute == 1)
            .map(|t| (t.id.clone(), MyTagCategoryDto { name: t.name.clone(), tags: Vec::new() }))
            .collect();
        for tag in rows.iter().filter(|t| t.attribute == 0) {
            if let Some((_, category)) = my_tag_categories.iter_mut().find(|(id, _)| *id == tag.parent) {
                category.tags.push(MyTagDto { id: tag.id.clone(), name: tag.name.clone() });
            }
        }
        let my_tag_categories = my_tag_categories.into_iter().map(|(_, c)| c).collect();
        let names = |interner: &rbl_index::strings::Interner| -> Vec<String> {
            let mut out: Vec<String> = (0..interner.len())
                .filter_map(|i| u32::try_from(i).ok())
                .map(|i| interner.name(i))
                .filter(|n| !n.is_empty())
                .take(MAX_NAMES)
                .map(str::to_owned)
                .collect();
            out.sort_unstable_by_key(|n| n.to_lowercase());
            out
        };
        Ok(TrackLookupsDto { keys: names(&library.keys), genres: names(&library.genres), my_tag_categories })
    })
    .await
}

/// Sets the My Tags on a track to exactly the ids given.
#[tauri::command]
pub async fn set_my_tags<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
    tags: Vec<String>,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "set_my_tags", Touched::Tracks, "Track Edit", move |w| {
        w.set_my_tags_with_undo(&track, &tags).map(|(_, edit)| LibraryEdit::TrackTags(edit))
    }).await
}

/// Add Artwork: the image at `image` is filed in the share tree and the
/// track points at it.
#[tauri::command]
pub async fn add_artwork<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
    image: String,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "add_artwork", Touched::Tracks, "Track Edit", move |w| {
        w.set_artwork_with_undo(&track, Some(std::path::Path::new(&image)))
            .map(|(_, edit)| LibraryEdit::Track(vec![edit]))
    }).await
}

/// Add Artwork on a playlist or folder: the tree menu's own.
#[tauri::command]
pub async fn add_playlist_artwork<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    playlist: String,
    image: String,
) -> AppResult<u32> {
    edit(app, state, "add_playlist_artwork", Touched::Playlists, move |w| {
        w.set_playlist_artwork(&playlist, Some(std::path::Path::new(&image))).map(|_| ())
    })
    .await
}

/// Delete Artwork: the track points at no image; the file stays.
#[tauri::command]
pub async fn clear_artwork<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "clear_artwork", Touched::Tracks, "Track Edit", move |w| {
        w.set_artwork_with_undo(&track, None).map(|(_, edit)| LibraryEdit::Track(vec![edit]))
    }).await
}

/// Writes one of the Info tab's editable fields.
///
/// `field` is the wire name — `title`, `artist`, `year`, … — and the set of
/// names the writer accepts is the whole list of what is safe to write; a
/// name it does not know is refused here rather than mapped to a guess.
#[tauri::command]
pub async fn set_track_field<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
    field: String,
    value: String,
) -> AppResult<crate::dto::EditHistoryDto> {
    let Some(which) = rbl_db::write::TrackField::parse(&field) else {
        return Err(AppError::new(ErrorKind::ReadOnly, format!("{field} cannot be edited here.")));
    };
    if field == "bpm" {
        let state = Arc::clone(&state);
        let writing = Arc::clone(&state);
        let reported = track.clone();
        if let Some(editor) = app.try_state::<Arc<crate::grid::GridEditor>>() {
            if editor.is_locked(&track) { return Err(AppError::new(ErrorKind::ReadOnly, "The beat grid is locked. Unlock it to edit.")); }
        }
        blocking("set_track_bpm", move || crate::grid::set_tempo(&writing, &track, &value)).await?;
        if let Some(editor) = app.try_state::<Arc<crate::grid::GridEditor>>() { editor.forget_history(&reported); }
        let _ = tauri::Emitter::emit(&app, "grid:changed", reported);
        let generation = crate::commands::reload(app.clone(), state.clone()).await?;
        let dto = {
            let mut history = state.edit_history.lock();
            history.clear_redo();
            crate::dto::EditHistoryDto {
                generation,
                can_undo: !history.undo.is_empty(),
                can_redo: !history.redo.is_empty(),
                undo_label: history.undo.last().map(|entry| entry.label.to_owned()),
                redo_label: history.redo.last().map(|entry| entry.label.to_owned()),
            }
        };
        let _ = tauri::Emitter::emit(&app, "edit-history:changed", dto.clone());
        return Ok(dto);
    }
    recorded_edit(app, state, "set_track_field", Touched::Tracks, "Track Edit", move |w| {
        w.set_field_with_undo(&track, which, &value).map(|(_, edit)| LibraryEdit::Track(vec![edit]))
    })
    .await
}
