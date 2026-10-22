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

use crate::commands::{blocking, each_track, edit, recorded_edit, write_error, Touched};
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
    /// `djmdMyTag.ID`: 1 to 4 in a library rekordbox made.
    pub id: String,
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
        Ok(details_dto(d, has_artwork))
    })
    .await
}

fn details_dto(d: rbl_db::details::TrackDetails, has_artwork: bool) -> TrackDetailsDto {
    TrackDetailsDto {
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
    }
}

/// The playlists that hold a track, for the information panel's
/// "Playlists" row. Smart playlists are tested one track at a time, so a
/// track that their rule admits is listed too. Ordered by path, then name.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedPlaylistDto {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[tauri::command]
pub async fn track_playlists(
    state: State<'_, Arc<AppState>>,
    track: String,
) -> AppResult<Vec<RelatedPlaylistDto>> {
    let state = Arc::clone(&state);
    blocking("track_playlists", move || {
        let library = state.library()?;
        let row = library
            .row_of(&track)
            .ok_or_else(|| AppError::new(ErrorKind::NotFound, "That track is no longer in the library.").with_detail(format!("track {track}")))?;
        let indices = library.playlists_containing(row);
        let playlists = library.playlists();
        let mut out: Vec<RelatedPlaylistDto> = indices
            // An index the id column no longer reaches is no playlist: a
            // stale row lists nothing rather than a playlist "0".
            .into_iter()
            .filter_map(|index| {
                let id = playlists.ids.get(index).copied()?;
                let name = playlists.name(index).to_owned();
                let mut parts = Vec::new();
                let mut parent = playlists.parent.get(index).copied().unwrap_or(rbl_index::NO_ID);
                while parent != rbl_index::NO_ID {
                    let parent_index = parent as usize;
                    if parent_index >= playlists.len() {
                        break;
                    }
                    parts.push(playlists.name(parent_index).to_owned());
                    parent = playlists.parent.get(parent_index).copied().unwrap_or(rbl_index::NO_ID);
                }
                parts.reverse();
                Some(RelatedPlaylistDto { id: id.to_string(), name, path: parts.join(" / ") })
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path).then(a.name.cmp(&b.name)));
        Ok(out)
    })
    .await
}

/// Several tracks as the information panel shows them: the first one's
/// record, how many there are, and which fields differ between them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionDetailsDto {
    pub first: TrackDetailsDto,
    pub count: u32,
    /// `TrackDetailsDto` field names, plus `artwork` when the tracks do not
    /// all show the same image.
    pub mixed: Vec<String>,
}

/// The information panel's record for a multiple selection.
///
/// rekordbox fills its panel from the first selected track and blanks each
/// field the selected tracks do not share (`TrackInfoConcreteMediator::
/// getTrackProp`, `tracksHaveSameInfo`), and shows artwork only when every
/// track has the same image (`tracksHaveSameArtwork`); 7.2.11, static
/// analysis. `tracks` comes in the order the list shows them, so "first" is
/// the topmost row: the track table reports its selection in list order, as
/// rekordbox builds its selected array by walking the list's `SparseSet` of
/// selected rows from the lowest index up (`BrowseBasicView::
/// changedSelectedRows`, 7.2.11, static analysis), not in click order.
#[tauri::command]
pub async fn selection_details(
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<SelectionDetailsDto> {
    let state = Arc::clone(&state);
    blocking("selection_details", move || {
        let library = state.library()?;
        let selection = state
            .read_db(|db| db.selection_details(&tracks))
            .map_err(write_error)?;
        let Some(selection) = selection else {
            return Err(AppError::new(ErrorKind::NotFound, "That track is no longer in the library.")
                .with_detail(format!("{} tracks", tracks.len())));
        };
        let artwork_of = |id: &str| library.artwork_path_of(id).filter(|p| !p.is_empty());
        let first_artwork = artwork_of(&selection.first.id);
        let mut mixed: Vec<String> = selection.mixed.iter().map(|&name| name.to_owned()).collect();
        if tracks.iter().any(|id| artwork_of(id) != first_artwork) {
            mixed.push("artwork".to_owned());
        }
        Ok(SelectionDetailsDto {
            first: details_dto(selection.first, first_artwork.is_some()),
            count: u32::try_from(selection.count).unwrap_or(u32::MAX),
            mixed,
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
            .map(|t| (t.id.clone(), MyTagCategoryDto { id: t.id.clone(), name: t.name.clone(), tags: Vec::new() }))
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

/// Sets the My Tags on a track to exactly the ids given. With `comments`
/// (Add "My Tag" to the "Comments" on, #605) the track's comment is
/// rewritten with them, in the same history entry.
#[tauri::command]
pub async fn set_my_tags<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track: String,
    tags: Vec<String>,
    comments: Option<bool>,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "set_my_tags", Touched::Tracks, "Track Edit", move |w| {
        let (_, edit) = w.set_my_tags_with_undo(&track, &tags)?;
        let tags = vec![edit];
        let comments = if comments.unwrap_or(false) {
            refresh_comments(w, std::slice::from_ref(&track), &tags)?
        } else {
            Vec::new()
        };
        Ok(LibraryEdit::TrackTags { tags, comments })
    }).await
}

/// Add "My Tag" to the "Comments" (#605): rewrites the comment of each of
/// `tracks` with its My Tags, as rekordbox's `BrowseListViewer::
/// updateCommentWithMyTag` does for every selected track after a My Tag
/// edit, whether or not that track's own tags changed. A track no longer in
/// the collection is passed over. On a failure the comments already
/// written and `tags` are put back, and the error returned.
fn refresh_comments(
    w: &mut rbl_db::write::Writer,
    tracks: &[String],
    tags: &[rbl_db::write::TrackTagEdit],
) -> Result<Vec<rbl_db::write::TrackEdit>, rbl_db::DbError> {
    let mut comments: Vec<rbl_db::write::TrackEdit> = Vec::with_capacity(tracks.len());
    for track in tracks {
        let step = match w.has_track(track) {
            Ok(false) => continue,
            Ok(true) => w.refresh_my_tag_comment_with_undo(track),
            Err(error) => Err(error),
        };
        match step {
            Ok((_, edit)) => {
                if !edit.is_empty() {
                    comments.push(edit);
                }
            }
            Err(error) => {
                for applied in comments.iter().rev() {
                    if let Err(undo) = w.undo_track_edit(applied) {
                        tracing::error!(%undo, "a My Tag comment could not be put back after a failure");
                    }
                }
                for applied in tags.iter().rev().filter(|edit| !edit.is_empty()) {
                    if let Err(undo) = w.undo_tag_edit(applied) {
                        tracing::error!(%undo, "a My Tag edit could not be put back after a failure");
                    }
                }
                return Err(error);
            }
        }
    }
    Ok(comments)
}

/// The My Tags every one of `tracks` has: the ticks of the My Tag panel.
#[tauri::command]
pub async fn selection_my_tags(
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<Vec<String>> {
    let state = Arc::clone(&state);
    blocking("selection_my_tags", move || {
        state
            .read_db(|db| Ok(rbl_db::details::shared_my_tags(db.connection(), &tracks)))
            .map_err(write_error)
    })
    .await
}

/// A tick in the My Tag panel: `tag` goes on each of `tracks`, or comes
/// off, as one step of history. Every track or none, as [`each_track`].
/// With `comments` (Add "My Tag" to the "Comments" on, #605) each track's
/// comment is rewritten too, in the same step.
#[tauri::command]
pub async fn put_my_tag<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    tag: String,
    on: bool,
    comments: Option<bool>,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "put_my_tag", Touched::Tracks, "Track Edit", move |w| {
        let mut edits: Vec<rbl_db::write::TrackTagEdit> = Vec::with_capacity(tracks.len());
        for track in &tracks {
            let step = match w.has_track(track) {
                Ok(false) => continue,
                Ok(true) => w.put_my_tag_with_undo(track, &tag, on),
                Err(error) => Err(error),
            };
            match step {
                Ok((_, edit)) => {
                    if !edit.is_empty() {
                        edits.push(edit);
                    }
                }
                Err(error) => {
                    for applied in edits.iter().rev() {
                        if let Err(undo) = w.undo_tag_edit(applied) {
                            tracing::error!(%undo, "a My Tag edit could not be put back after a failure");
                        }
                    }
                    return Err(error);
                }
            }
        }
        let comments = if comments.unwrap_or(false) { refresh_comments(w, &tracks, &edits)? } else { Vec::new() };
        Ok(LibraryEdit::TrackTags { tags: edits, comments })
    }).await
}

/// Preferences > Advanced > Browse > My Tag > Add "My Tag" to the
/// "Comments" turned on (`on`) or off (#605): every track with a My Tag has
/// the tags written after its comment, or taken off, as one step of
/// history. The window asks first, as rekordbox does ("This operation can
/// take few minutes."), and changes the setting only once this succeeds.
#[tauri::command]
pub async fn write_my_tag_comments<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    on: bool,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "write_my_tag_comments", Touched::Tracks, "Track Edit", move |w| {
        let (_, comments) = w.write_my_tag_comments_with_undo(on)?;
        Ok(LibraryEdit::TrackTags { tags: Vec::new(), comments })
    }).await
}

/// The tag Adding My Tag made.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMyTagDto {
    pub id: String,
    pub name: String,
}

/// Adding My Tag: a new tag at the end of `category`, named "My Comment"
/// or the first "My Comment (n)" the category does not have yet.
#[tauri::command]
pub async fn create_my_tag<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    category: String,
) -> AppResult<NewMyTagDto> {
    let made = Arc::new(parking_lot::Mutex::new(None::<rbl_db::write::NewMyTag>));
    let slot = Arc::clone(&made);
    // A new tag is on no track yet, but the index's tag list (which the
    // Track Filter's tag columns read) is loaded with the tracks, so they
    // are read again; the user may keep "My Comment" without renaming it.
    edit(app, state, "create_my_tag", Touched::Tracks, move |w| {
        *slot.lock() = Some(w.create_my_tag(&category)?);
        Ok(())
    })
    .await?;
    let Some(made) = made.lock().take() else {
        return Err(AppError::new(ErrorKind::Malformed, "The new My Tag was not made."));
    };
    Ok(NewMyTagDto { id: made.id, name: made.name })
}

/// Renames a My Tag or a category. The My Tag column shows names, so the
/// track list is read again.
///
/// `comments` are the selected tracks while Add "My Tag" to the "Comments"
/// is on (#605): their comments are rewritten with the new name, as
/// rekordbox's `didChangeMyTagName` refreshes only the selected tracks.
/// Not recorded, as the rename is not.
#[tauri::command]
pub async fn rename_my_tag<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    id: String,
    name: String,
    comments: Option<Vec<String>>,
) -> AppResult<u32> {
    edit(app, state, "rename_my_tag", Touched::Tracks, move |w| {
        w.rename_my_tag(&id, &name)?;
        rewrite_comments(w, comments.as_deref().unwrap_or_default())
    }).await
}

/// Deletes a My Tag and takes it off every track. Not undoable, and the
/// history before it goes: an older tick could name the tag it removes.
/// `comments` as for [`rename_my_tag`]: the selected tracks' comments lose
/// the tag (rekordbox's `didDeleteMyTag`); other tracks keep theirs.
#[tauri::command]
pub async fn delete_my_tag<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    id: String,
    comments: Option<Vec<String>>,
) -> AppResult<u32> {
    crate::commands::permanent_edit(app, state, "delete_my_tag", Touched::Tracks, move |w| {
        w.delete_my_tag(&id)?;
        rewrite_comments(w, comments.as_deref().unwrap_or_default())
    })
    .await
}

/// The comment rewrite after a rename or a delete, which no history entry
/// holds: each track still in the collection, written as it comes.
fn rewrite_comments(w: &mut rbl_db::write::Writer, tracks: &[String]) -> Result<(), rbl_db::DbError> {
    for track in tracks {
        if w.has_track(track)? {
            w.refresh_my_tag_comment_with_undo(track)?;
        }
    }
    Ok(())
}

/// Add Artwork: the image at `image` is filed in the share tree and each of
/// `tracks` points at it.
///
/// Track by track, as rekordbox's `TrackInfoConcreteMediator::addArtwork`
/// calls `DatabaseIF::modifyTrackArtwork` once per selected track (7.2.11,
/// static analysis).
#[tauri::command]
pub async fn add_artwork<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    image: String,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "add_artwork", Touched::Tracks, "Track Edit", move |w| {
        each_track(w, &tracks, |w, track| {
            w.set_artwork_with_undo(track, Some(std::path::Path::new(&image)))
        })
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
    recorded_edit(app, state, "add_playlist_artwork", Touched::Playlists, "Add Artwork", move |w| {
        w.set_playlist_artwork_with_undo(&playlist, Some(std::path::Path::new(&image)))
            .map(|(_, edit)| LibraryEdit::PlaylistEdit(edit))
    })
    .await
    .map(|history| history.generation)
}

/// Delete Artwork: each of `tracks` points at no image; the files stay.
#[tauri::command]
pub async fn clear_artwork<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
) -> AppResult<crate::dto::EditHistoryDto> {
    recorded_edit(app, state, "clear_artwork", Touched::Tracks, "Track Edit", move |w| {
        each_track(w, &tracks, |w, track| w.set_artwork_with_undo(track, None))
    }).await
}

/// Writes one of the Info tab's editable fields on each of `tracks`.
///
/// `field` is the wire name — `title`, `artist`, `year`, … — and the set of
/// names the writer accepts is the whole list of what is safe to write; a
/// name it does not know is refused here rather than mapped to a guess.
///
/// Several tracks are the information panel's multiple selection, which
/// rekordbox writes field by field to every selected track. Its Track Title
/// box is the one it greys out for more than one track
/// (`TrackInfoConcreteMediator::isTrackEditabled`, item 0, 7.2.11 static
/// analysis), so a title is refused for more than one here too. A BPM is
/// refused for more than one as well, which is this app's rule rather than
/// rekordbox's: writing it retimes the track's beat grid, which the grid
/// editor does one track at a time. The panel never sends one; its BPM box
/// is locked.
#[tauri::command]
pub async fn set_track_field<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Vec<String>,
    field: String,
    value: String,
    write_key_tag: Option<bool>,
) -> AppResult<crate::dto::EditHistoryDto> {
    let Some(which) = rbl_db::write::TrackField::parse(&field) else {
        return Err(AppError::new(ErrorKind::ReadOnly, format!("{field} cannot be edited here.")));
    };
    if tracks.len() > 1 && matches!(field.as_str(), "title" | "bpm") {
        return Err(AppError::new(ErrorKind::ReadOnly, format!("{field} cannot be edited here.")));
    }
    if field == "bpm" {
        let Some(track) = tracks.into_iter().next() else {
            return Err(AppError::new(ErrorKind::Malformed, "No track was given."));
        };
        let state = Arc::clone(&state);
        let writing = Arc::clone(&state);
        let reported = track.clone();
        let Some(editor) = app.try_state::<Arc<crate::grid::GridEditor>>().map(|editor| Arc::clone(&editor)) else {
            return Err(AppError::internal("the grid editor is not running"));
        };
        if editor.is_locked(&track) { return Err(AppError::new(ErrorKind::ReadOnly, "The beat grid is locked. Unlock it to edit.")); }
        // Recorded in the track's grid history as the grid editor's Set
        // Tempo, so the grid panel's Undo puts back the grid and the BPM
        // together and the track's earlier grid edits stay undoable (#505).
        blocking("set_track_bpm", move || crate::grid::set_tempo(&writing, &editor, &track, &value)).await?;
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
    // "Write the value to the ID3 tag": rekordbox's `modifyTrack` writes an
    // edited key into each track's file after saving it, with the name as
    // edited [OBS chris-win11: `Am` typed under Alphanumeric wrote `Am`].
    // Only the tracks the edit changed: `updateDB` hands `modifyTrackData`
    // (and so `modifyTrack` -> `modifyTagData`) only the tracks whose value
    // differs [static 7.2.19 `TrackInfoConcreteMediator::updateDB`
    // @0x100371814; #279], so a file whose key already matched, or a cleared
    // key on a track that had none, is not rewritten.
    //
    // The files are written under the edit gate, right after the library,
    // and the history entry keeps each one written with the key its tag
    // held, so Undo and Redo put the file back with the library (#507).
    let key_tag = which == rbl_db::write::TrackField::Key && write_key_tag == Some(true);
    let library = if key_tag { Some(state.library()?) } else { None };
    let key = value.trim().to_owned();
    recorded_edit(app, state, "set_track_field", Touched::Tracks, crate::commands::TRACK_EDIT, move |w| {
        let edit = each_track(w, &tracks, |w, track| w.set_field_with_undo(track, which, &value))?;
        let (Some(library), LibraryEdit::Track(steps)) = (library, &edit) else { return Ok(edit) };
        let files: Vec<_> = steps
            .iter()
            .filter_map(|step| {
                let track = step.content();
                let row = library.row_of(track)?;
                crate::key_tag::write_recorded(track, library.folder_path.get(row as usize), &key)
            })
            .collect();
        if files.is_empty() {
            return Ok(edit);
        }
        let LibraryEdit::Track(tracks) = edit else { return Ok(edit) };
        Ok(LibraryEdit::KeyEdit { tracks, files })
    })
    .await
}
