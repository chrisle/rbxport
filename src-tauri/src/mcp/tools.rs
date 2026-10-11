//! The tools the MCP server offers.
//!
//! Reads run on a blocking worker against the in-memory index
//! (`query.rs`). Writes call the window's own commands, after the same
//! Library Protection check an AppleScript edit makes, so what an assistant
//! does is what a click does: serialized with every other edit, refused while
//! rekordbox is running, recorded for Undo where the window records it, and
//! announced with `library:changed`.

use std::sync::{Arc, Mutex as StdMutex};

use rbl_index::{Library, RelatedCriterion, SearchField};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, Runtime};

use super::query::{self, Search, Source, DEFAULT_LIMIT};
use crate::commands::{blocking, Touched};
use crate::error::{AppError, ErrorKind};
use crate::scripting::Bridge;
use crate::state::AppState;

/// What a tool answers: JSON text, or a message saying what went wrong.
type Answer = Result<String, String>;

const INSTRUCTIONS: &str = "rbxport manages a rekordbox DJ library. Track, playlist and history ids are decimal strings. \
Find tracks with search_tracks (query, key, BPM, rating, play count and date filters; sort and page with offset). \
Keys accept musical names (Am, F#) or Camelot codes (8A); set compatible_keys for harmonic mixing. \
Edits change the user's real library: they are refused while Library Protection is on or rekordbox is running, \
and undo_last_edit reverts the last undoable one. Confirm destructive actions with the user first.";

pub struct Tools<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
    tool_router: ToolRouter<Self>,
}

// Not derived: a derive would ask `R: Clone`, which no runtime is.
impl<R: Runtime> Clone for Tools<R> {
    fn clone(&self) -> Self {
        Self { app: self.app.clone(), tool_router: self.tool_router.clone() }
    }
}

fn json<T: Serialize>(value: &T) -> Answer {
    serde_json::to_string(value).map_err(|e| format!("The answer could not be written: {e}"))
}

/// A refusal, worded for the assistant. Not through `Err(…)` inline: the
/// locale build collects literals beside `Err(` as interface text, and
/// these are never shown in the window.
fn refuse<T>(text: &str) -> Result<T, String> {
    Err(text.to_owned())
}

fn message(error: AppError) -> String {
    error.message
}

fn malformed(text: String) -> AppError {
    AppError::new(ErrorKind::Malformed, text)
}

fn parse_id(id: &str, what: &str) -> Result<u64, String> {
    id.trim().parse().map_err(|_| format!("{id:?} is not a {what} id. Ids are decimal strings."))
}

fn limit(asked: Option<usize>) -> usize {
    asked.unwrap_or(DEFAULT_LIMIT).clamp(1, query::MAX_LIMIT)
}

// ------------------------------------------------------------- parameters

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Words to search for, matched as rekordbox's search box matches them: every word must appear in the field(s), case and accents ignored.
    pub query: Option<String>,
    /// The field `query` searches: all (default), title, artist, album, genre, year, bpm, composer, albumArtist, remixer, label, comment, originalArtist, mixName.
    pub search_field: Option<String>,
    /// Search only this playlist, folder (every playlist under it) or intelligent playlist.
    pub playlist_id: Option<String>,
    /// Search only this history session.
    pub history_id: Option<String>,
    /// The artist contains this text.
    pub artist: Option<String>,
    /// The genre contains this text.
    pub genre: Option<String>,
    /// The album contains this text.
    pub album: Option<String>,
    /// The label contains this text.
    pub label: Option<String>,
    /// The audio file's name contains this text, for example ".flac".
    pub file_name: Option<String>,
    /// The track is in one of these keys: musical names (Am, F#, Dbm) or Camelot codes (8A, 3B).
    pub keys: Option<Vec<String>>,
    /// Also admit the keys harmonically compatible with `keys`: the relative major/minor and one step either way on the Camelot wheel.
    pub compatible_keys: Option<bool>,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    /// At least this many stars, 0 to 5.
    pub min_rating: Option<u8>,
    /// The track has one of these colours: Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple.
    pub colors: Option<Vec<String>>,
    pub min_play_count: Option<u32>,
    /// 0 finds tracks never played.
    pub max_play_count: Option<u32>,
    pub year_min: Option<u16>,
    pub year_max: Option<u16>,
    /// Added on or after this date, YYYY-MM-DD.
    pub added_after: Option<String>,
    /// Added on or before this date, YYYY-MM-DD.
    pub added_before: Option<String>,
    /// true for analysed tracks only, false for unanalysed ones.
    pub analysed: Option<bool>,
    /// Sort column: title, artist, album, genre, label, key, keyCamelot, bpm, duration, rating, djPlayCount, dateAdded, releaseDate, year, bitrate, color, fileName. Default: the source's own order.
    pub sort: Option<String>,
    pub descending: Option<bool>,
    /// Skip this many matches, to page through a long result.
    pub offset: Option<usize>,
    /// At most this many tracks, default 50, at most 200.
    pub limit: Option<usize>,
    /// Include each track's audio file path.
    pub include_paths: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrackIdsParams {
    /// Track ids.
    pub track_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTracksParams {
    /// The playlist or history session id.
    pub id: String,
    pub offset: Option<usize>,
    /// At most this many tracks, default 50, at most 200.
    pub limit: Option<usize>,
    /// Include each track's audio file path.
    pub include_paths: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListSessionsParams {
    /// The session's name (rekordbox names sessions by date, YYYY-MM-DD) contains this text, for example "2024-05".
    pub name_contains: Option<String>,
    /// Only sessions with at least this many tracks.
    pub min_tracks: Option<usize>,
    /// At most this many sessions, newest first; default 50, at most 200.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct HistoryStatsParams {
    /// How many of the most-played tracks to list, default 20.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeParams {
    /// What to group by: genre, artist, album, label, key (Camelot), year, rating, color, bpm (5-BPM bands), added (year-month).
    pub group_by: String,
    /// Group only this playlist's tracks.
    pub playlist_id: Option<String>,
    /// Group only this history session's tracks.
    pub history_id: Option<String>,
    /// At most this many groups, largest first; default 50, at most 200.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RelatedParams {
    pub track_id: String,
    /// bpmKey (default: within 6% BPM and a compatible key), artist (same artist), genreRecent (same genre, added in the last 30 days), suggestion (what followed it in the play histories).
    pub criterion: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LimitParams {
    /// At most this many results, default 50, at most 200.
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MissingParams {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    /// Check every file on disk again rather than reuse the last check. Slower on a large library.
    pub rescan: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreatePlaylistParams {
    pub name: String,
    /// The folder to create it in. Default: the top of the playlist tree.
    pub parent_id: Option<String>,
    /// Create a folder rather than a playlist.
    pub folder: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenamePlaylistParams {
    pub playlist_id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MovePlaylistParams {
    pub playlist_id: String,
    /// The folder to move it into. Default: the top of the playlist tree.
    pub parent_id: Option<String>,
    /// Its position among the folder's contents, 0 first. Default: last.
    pub index: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlaylistParams {
    pub playlist_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlaylistTracksParams {
    pub playlist_id: String,
    pub track_ids: Vec<String>,
    /// Add tracks the playlist already holds again. Default: skip them.
    pub allow_duplicates: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemovePlaylistTracksParams {
    pub playlist_id: String,
    pub track_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReorderParams {
    pub playlist_id: String,
    /// The playlist's tracks in their new order. Tracks left out keep their relative order after these.
    pub track_ids: Vec<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct UpdateTracksParams {
    /// The tracks to change. title and bpm can be set on one track at a time only.
    pub track_ids: Vec<String>,
    /// 0 to 5 stars.
    pub rating: Option<u8>,
    /// Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple, or "none".
    pub color: Option<String>,
    pub comment: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub label: Option<String>,
    /// A musical key name such as Am or F#.
    pub key: Option<String>,
    pub year: Option<u16>,
    pub composer: Option<String>,
    pub remixer: Option<String>,
    pub original_artist: Option<String>,
    pub lyricist: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub play_count: Option<u32>,
    /// Retimes the beat grid to this tempo.
    pub bpm: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImportParams {
    /// Absolute paths of audio files or folders; folders are searched recursively for mp3, m4a, flac, wav, aiff and the other formats rekordbox plays.
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveMissingParams {
    /// The missing tracks to remove. Leave out to remove every track whose file is missing.
    pub track_ids: Option<Vec<String>>,
}

// ------------------------------------------------------------------ track details

#[derive(Debug, Serialize)]
struct CueInfo {
    /// A to P for a hot cue; absent for a memory cue.
    #[serde(skip_serializing_if = "Option::is_none")]
    hot_cue: Option<char>,
    position_sec: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    loop_end_sec: Option<f64>,
}

#[derive(Debug, Serialize)]
struct TrackDetails {
    #[serde(flatten)]
    track: query::TrackSummary,
    /// The file is not where the library says.
    missing: bool,
    bitrate: u32,
    sample_rate: u32,
    file_size: u64,
    cues: Vec<CueInfo>,
    /// What the database adds: composer, remixer, mix name, My Tags…
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    more: serde_json::Map<String, serde_json::Value>,
}

fn seconds(ms: u32) -> f64 {
    f64::from(ms) / 1000.0
}

fn details_of(library: &Library, row: rbl_index::Row) -> TrackDetails {
    let index = row as usize;
    let track = query::summary(library, row, true);
    TrackDetails {
        missing: crate::relocate::is_missing(library.folder_path.get(index)),
        bitrate: library.bitrate.get(index).copied().unwrap_or(0),
        sample_rate: library.sample_rate.get(index).copied().unwrap_or(0),
        file_size: library.file_size.get(index).copied().unwrap_or(0),
        cues: library
            .cues_of(row)
            .iter()
            .map(|cue| CueInfo {
                hot_cue: cue.hot_letter(),
                position_sec: seconds(cue.position_ms),
                loop_end_sec: (cue.out_ms > 0).then(|| seconds(cue.out_ms)),
            })
            .collect(),
        more: serde_json::Map::new(),
        track,
    }
}

/// The database's extra fields for a track, left out when the read fails:
/// the index has everything else, and a details call is no reason to fail.
fn database_details(state: &AppState, id: &str) -> serde_json::Map<String, serde_json::Value> {
    let read = state.read_db(|db| {
        let Some(d) = rbl_db::details::browser_details(db.connection(), id)? else { return Ok(None) };
        let tags = rbl_db::details::my_tag_names(db.connection(), id);
        Ok(Some((d, tags)))
    });
    let mut out = serde_json::Map::new();
    let Ok(Some((d, tags))) = read else { return out };
    let mut text = |name: &str, value: String| {
        if !value.is_empty() {
            out.insert(name.to_owned(), json!(value));
        }
    };
    text("album_artist", d.album_artist);
    text("original_artist", d.original_artist);
    text("composer", d.composer);
    text("remixer", d.remixer);
    text("lyricist", d.lyricist);
    text("mix_name", d.mix_name);
    text("message", d.message);
    text("release_date", d.release_date);
    if d.track_number > 0 {
        out.insert("track_number".into(), json!(d.track_number));
    }
    if d.disc_number > 0 {
        out.insert("disc_number".into(), json!(d.disc_number));
    }
    if d.bit_depth > 0 {
        out.insert("bit_depth".into(), json!(d.bit_depth));
    }
    if !tags.is_empty() {
        out.insert("my_tags".into(), json!(tags));
    }
    out
}

fn criterion(name: Option<&str>) -> Result<RelatedCriterion, String> {
    Ok(match name.unwrap_or("bpmKey") {
        "bpmKey" => RelatedCriterion::BpmAndKey,
        "artist" => RelatedCriterion::SameArtist,
        "genreRecent" => RelatedCriterion::SameGenreRecent,
        "suggestion" => RelatedCriterion::Suggestion,
        other => return Err(format!("{other} is not a criterion. Use bpmKey, artist, genreRecent or suggestion.")),
    })
}

fn search_field(name: Option<&str>) -> Result<SearchField, String> {
    match name {
        None | Some("") => Ok(SearchField::All),
        Some(name) => serde_json::from_value(json!(name)).map_err(|_| format!("{name} is not a search field.")),
    }
}

fn source(playlist: Option<&str>, history: Option<&str>) -> Result<Source, String> {
    match (playlist, history) {
        (Some(_), Some(_)) => refuse("Give a playlist_id or a history_id, not both."),
        (Some(id), None) => Ok(Source::Playlist(parse_id(id, "playlist")?)),
        (None, Some(id)) => Ok(Source::History(parse_id(id, "history")?)),
        (None, None) => Ok(Source::Collection),
    }
}

fn to_search(p: SearchParams) -> Result<Search, String> {
    let colors = p
        .colors
        .unwrap_or_default()
        .iter()
        .map(|name| query::color_id(name).ok_or_else(|| format!("{name} is not a track colour.")))
        .collect::<Result<Vec<u8>, String>>()?;
    Ok(Search {
        source: source(p.playlist_id.as_deref(), p.history_id.as_deref())?,
        query: p.query.unwrap_or_default(),
        field: search_field(p.search_field.as_deref())?,
        artist: p.artist,
        genre: p.genre,
        album: p.album,
        label: p.label,
        file_name: p.file_name,
        keys: p.keys.unwrap_or_default(),
        compatible_keys: p.compatible_keys.unwrap_or(false),
        bpm_min: p.bpm_min,
        bpm_max: p.bpm_max,
        min_rating: p.min_rating,
        colors,
        min_play_count: p.min_play_count,
        max_play_count: p.max_play_count,
        year_min: p.year_min,
        year_max: p.year_max,
        added_after: p.added_after,
        added_before: p.added_before,
        analysed: p.analysed,
        sort: p.sort.unwrap_or_default(),
        descending: p.descending.unwrap_or(false),
        offset: p.offset.unwrap_or(0),
        limit: limit(p.limit),
        with_paths: p.include_paths.unwrap_or(false),
    })
}

/// The edits an update asks for, as `(field, value)` pairs for
/// `set_track_field`, after the three that have commands of their own.
fn field_edits(p: &UpdateTracksParams) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut text = |field: &'static str, value: Option<&String>| {
        if let Some(value) = value {
            out.push((field, value.clone()));
        }
    };
    text("title", p.title.as_ref());
    text("artist", p.artist.as_ref());
    text("album", p.album.as_ref());
    text("genre", p.genre.as_ref());
    text("label", p.label.as_ref());
    text("key", p.key.as_ref());
    text("composer", p.composer.as_ref());
    text("remixer", p.remixer.as_ref());
    text("originalArtist", p.original_artist.as_ref());
    text("lyricist", p.lyricist.as_ref());
    if let Some(year) = p.year {
        out.push(("year", year.to_string()));
    }
    if let Some(n) = p.track_number {
        out.push(("trackNumber", n.to_string()));
    }
    if let Some(n) = p.disc_number {
        out.push(("discNumber", n.to_string()));
    }
    if let Some(n) = p.play_count {
        out.push(("playCount", n.to_string()));
    }
    if let Some(bpm) = p.bpm {
        out.push(("bpm", format!("{bpm:.2}")));
    }
    out
}

// ------------------------------------------------------------------ the tools

impl<R: Runtime> Tools<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app, tool_router: Self::tool_router() }
    }

    fn state(&self) -> Arc<AppState> {
        Arc::clone(self.app.state::<Arc<AppState>>().inner())
    }

    /// Runs `read` over the library on a blocking worker.
    async fn read<T, F>(&self, name: &'static str, read: F) -> Answer
    where
        T: Serialize + Send + 'static,
        F: FnOnce(&Library) -> Result<T, String> + Send + 'static,
    {
        let library = self.state().library().map_err(message)?;
        let value = blocking(name, move || read(&library).map_err(malformed)).await.map_err(message)?;
        json(&value)
    }

    /// Refuses an edit while Library Protection is on, as the window and
    /// AppleScript refuse one.
    fn writable(&self) -> Result<(), String> {
        let bridge = self.app.state::<Arc<Bridge>>();
        bridge.refuse_if_protected().map_err(|e| e.message)
    }

}

fn done(what: &str) -> Answer {
    json(&json!({ "ok": true, "result": what }))
}

#[tool_router]
impl<R: Runtime> Tools<R> {
    /// Whether the library is loaded and editable, and its size.
    #[tool(annotations(read_only_hint = true))]
    async fn library_status(&self) -> Answer {
        let state = self.state();
        let protected = self.writable().err();
        let library = state.library().map_err(message)?;
        let (read_only, db_version, _, _) = state.summary();
        let rekordbox_running = rbl_db::is_rekordbox_running();
        json(&json!({
            "tracks": library.len(),
            "playlists": library.playlists().len(),
            "history_sessions": library.histories().len(),
            "db_version": db_version,
            "rekordbox_running": rekordbox_running,
            "editable": protected.is_none() && !read_only && !rekordbox_running,
            "why_not_editable": protected.or_else(|| (read_only || rekordbox_running).then(|| "rekordbox is running. Quit it to edit the library.".to_owned())),
        }))
    }

    /// Search and filter tracks in the collection, a playlist or a history session. Returns the total number of matches and one page of tracks.
    #[tool(annotations(read_only_hint = true))]
    async fn search_tracks(&self, Parameters(params): Parameters<SearchParams>) -> Answer {
        let search = to_search(params)?;
        self.read("mcp_search_tracks", move |library| query::search(library, &search)).await
    }

    /// Everything about some tracks: tags, file path and format, cue points, My Tags. At most 50 ids.
    #[tool(annotations(read_only_hint = true))]
    async fn get_track_details(&self, Parameters(TrackIdsParams { track_ids }): Parameters<TrackIdsParams>) -> Answer {
        if track_ids.len() > 50 {
            return refuse("Ask for at most 50 tracks at a time.");
        }
        let state = self.state();
        self.read("mcp_get_track_details", move |library| {
            let mut found = Vec::with_capacity(track_ids.len());
            let mut unknown = Vec::new();
            for id in track_ids {
                match library.row_of(&id) {
                    Some(row) => {
                        let mut details = details_of(library, row);
                        details.more = database_details(&state, &id);
                        found.push(details);
                    }
                    None => unknown.push(id),
                }
            }
            Ok(json!({ "tracks": found, "unknown_ids": unknown }))
        })
        .await
    }

    /// Which of these track ids exist in the library.
    #[tool(annotations(read_only_hint = true))]
    async fn validate_track_ids(&self, Parameters(TrackIdsParams { track_ids }): Parameters<TrackIdsParams>) -> Answer {
        self.read("mcp_validate_track_ids", move |library| Ok(query::validate(library, &track_ids))).await
    }

    /// Every playlist, folder and intelligent playlist, in tree order, with ids, paths and track counts.
    #[tool(annotations(read_only_hint = true))]
    async fn list_playlists(&self) -> Answer {
        self.read("mcp_list_playlists", |library| Ok(query::playlists(library))).await
    }

    /// The tracks of a playlist in its order. A folder lists every track under it; an intelligent playlist lists what its rule admits now.
    #[tool(annotations(read_only_hint = true))]
    async fn get_playlist_tracks(&self, Parameters(p): Parameters<ListTracksParams>) -> Answer {
        let search = Search {
            source: Source::Playlist(parse_id(&p.id, "playlist")?),
            offset: p.offset.unwrap_or(0),
            limit: limit(p.limit),
            with_paths: p.include_paths.unwrap_or(false),
            ..Search::default()
        };
        self.read("mcp_get_playlist_tracks", move |library| query::search(library, &search)).await
    }

    /// The DJ history sessions, newest first, with their track counts.
    #[tool(annotations(read_only_hint = true))]
    async fn list_history_sessions(&self, Parameters(p): Parameters<ListSessionsParams>) -> Answer {
        self.read("mcp_list_history_sessions", move |library| {
            let needle = p.name_contains.unwrap_or_default();
            let mut sessions: Vec<_> = query::histories(library)
                .into_iter()
                .filter(|s| s.kind == "session")
                .filter(|s| s.name.contains(&needle) && s.count.unwrap_or(0) >= p.min_tracks.unwrap_or(0))
                .collect();
            sessions.sort_by(|a, b| b.name.cmp(&a.name));
            let total = sessions.len();
            sessions.truncate(limit(p.limit));
            Ok(json!({ "total": total, "sessions": sessions }))
        })
        .await
    }

    /// The tracks of one history session, in the order they were played.
    #[tool(annotations(read_only_hint = true))]
    async fn get_session_tracks(&self, Parameters(p): Parameters<ListTracksParams>) -> Answer {
        let search = Search {
            source: Source::History(parse_id(&p.id, "history")?),
            offset: p.offset.unwrap_or(0),
            limit: limit(p.limit),
            with_paths: p.include_paths.unwrap_or(false),
            ..Search::default()
        };
        self.read("mcp_get_session_tracks", move |library| query::search(library, &search)).await
    }

    /// What the play history adds up to: sessions, plays, the most-played tracks, genres and artists.
    #[tool(annotations(read_only_hint = true))]
    async fn get_history_stats(&self, Parameters(p): Parameters<HistoryStatsParams>) -> Answer {
        let wanted = p.limit.unwrap_or(20);
        self.read("mcp_get_history_stats", move |library| Ok(query::history_stats(library, wanted))).await
    }

    /// Library totals: tracks, playtime, average BPM, analysed and rated counts, and the top genres, keys and artists.
    #[tool(annotations(read_only_hint = true))]
    async fn get_library_stats(&self) -> Answer {
        self.read("mcp_get_library_stats", |library| Ok(query::stats(library))).await
    }

    /// Group the library, a playlist or a history session by a property and count each group, with its playtime and average BPM.
    #[tool(annotations(read_only_hint = true))]
    async fn analyze_library(&self, Parameters(p): Parameters<AnalyzeParams>) -> Answer {
        let by = query::GroupBy::parse(&p.group_by).ok_or_else(|| format!("{} cannot be grouped by.", p.group_by))?;
        let source = source(p.playlist_id.as_deref(), p.history_id.as_deref())?;
        let wanted = limit(p.limit);
        self.read("mcp_analyze_library", move |library| {
            let rows = query::source_rows(library, &source)?;
            let (groups, shown) = query::group(library, &rows, by, wanted);
            Ok(json!({ "tracks": rows.len(), "groups": groups, "shown": shown }))
        })
        .await
    }

    /// rekordbox's Related Tracks: tracks that mix well with one track, by BPM and key, artist, recent genre, or past play histories.
    #[tool(annotations(read_only_hint = true))]
    async fn find_related_tracks(&self, Parameters(p): Parameters<RelatedParams>) -> Answer {
        let by = criterion(p.criterion.as_deref())?;
        let wanted = limit(p.limit);
        self.read("mcp_find_related_tracks", move |library| query::related(library, &p.track_id, by, wanted)).await
    }

    /// Tracks that share a title and artist, with their files, so copies can be found and removed.
    #[tool(annotations(read_only_hint = true))]
    async fn find_duplicates(&self, Parameters(p): Parameters<LimitParams>) -> Answer {
        let wanted = u32::try_from(limit(p.limit)).unwrap_or(u32::MAX);
        let found = crate::commands::find_duplicates(self.app.state(), wanted).await.map_err(message)?;
        json(&found)
    }

    /// Tracks whose audio file is no longer where the library says.
    #[tool(annotations(read_only_hint = true))]
    async fn find_missing_tracks(&self, Parameters(p): Parameters<MissingParams>) -> Answer {
        let offset = u32::try_from(p.offset.unwrap_or(0)).unwrap_or(u32::MAX);
        let wanted = u32::try_from(limit(p.limit)).unwrap_or(u32::MAX);
        let found = crate::relocate::missing_tracks(self.app.state(), offset, wanted, p.rescan.unwrap_or(false), None).await.map_err(message)?;
        json(&found)
    }

    // -------------------------------------------------------------- edits

    /// Create a playlist or folder. Returns its id.
    #[tool(annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false))]
    async fn create_playlist(&self, Parameters(p): Parameters<CreatePlaylistParams>) -> Answer {
        self.writable()?;
        let name = p.name.trim().to_owned();
        if name.is_empty() {
            return refuse("A playlist needs a name.");
        }
        let parent = match p.parent_id {
            Some(id) => parse_id(&id, "folder")?.to_string(),
            None => rbl_db::write::ROOT.to_owned(),
        };
        let folder = p.folder.unwrap_or(false);
        let written = Arc::new(StdMutex::new(None::<String>));
        let into = Arc::clone(&written);
        crate::commands::edit(self.app.clone(), self.app.state(), "mcp_create_playlist", Touched::Playlists, move |w| {
            let id = if folder { w.create_folder(&name, &parent)? } else { w.create_playlist(&name, &parent)? };
            if let Ok(mut slot) = into.lock() {
                *slot = Some(id);
            }
            Ok(())
        })
        .await
        .map_err(message)?;
        let id = written.lock().ok().and_then(|slot| slot.clone()).unwrap_or_default();
        json(&json!({ "ok": true, "id": id }))
    }

    #[tool(description = "Rename a playlist or folder.", annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true))]
    async fn rename_playlist(&self, Parameters(p): Parameters<RenamePlaylistParams>) -> Answer {
        self.writable()?;
        let id = parse_id(&p.playlist_id, "playlist")?.to_string();
        crate::commands::rename_playlist(self.app.clone(), self.app.state(), id, p.name).await.map_err(message)?;
        done("renamed")
    }

    #[tool(description = "Move a playlist or folder into another folder, or to the top of the tree.", annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true))]
    async fn move_playlist(&self, Parameters(p): Parameters<MovePlaylistParams>) -> Answer {
        self.writable()?;
        let id = parse_id(&p.playlist_id, "playlist")?.to_string();
        let parent = match p.parent_id {
            Some(parent) => parse_id(&parent, "folder")?.to_string(),
            None => rbl_db::write::ROOT.to_owned(),
        };
        crate::commands::move_playlist(self.app.clone(), self.app.state(), id, parent, p.index).await.map_err(message)?;
        done("moved")
    }

    /// Delete a playlist, or a folder with everything in it. The tracks stay in the collection. Undoable with `undo_last_edit`.
    #[tool(annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false))]
    async fn delete_playlist(&self, Parameters(p): Parameters<PlaylistParams>) -> Answer {
        self.writable()?;
        let id = parse_id(&p.playlist_id, "playlist")?.to_string();
        crate::commands::delete_playlist(self.app.clone(), self.app.state(), id).await.map_err(message)?;
        done("deleted")
    }

    /// Add tracks to the end of a playlist. Tracks it already holds are skipped unless `allow_duplicates` is set.
    #[tool(annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false))]
    async fn add_tracks_to_playlist(&self, Parameters(p): Parameters<PlaylistTracksParams>) -> Answer {
        self.writable()?;
        let id = parse_id(&p.playlist_id, "playlist")?.to_string();
        let count = p.track_ids.len();
        crate::commands::add_tracks_to_playlist(self.app.clone(), self.app.state(), id, p.track_ids, p.allow_duplicates)
            .await
            .map_err(message)?;
        done(&format!("{count} track(s) sent to the playlist"))
    }

    /// Take tracks off a playlist. They stay in the collection. Undoable with `undo_last_edit`.
    #[tool(annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true))]
    async fn remove_tracks_from_playlist(&self, Parameters(p): Parameters<RemovePlaylistTracksParams>) -> Answer {
        self.writable()?;
        let id = parse_id(&p.playlist_id, "playlist")?.to_string();
        crate::commands::remove_tracks_from_playlist(self.app.clone(), self.app.state(), id, p.track_ids).await.map_err(message)?;
        done("removed")
    }

    /// Put a playlist's tracks in a new order.
    #[tool(annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true))]
    async fn reorder_playlist(&self, Parameters(p): Parameters<ReorderParams>) -> Answer {
        self.writable()?;
        let id = parse_id(&p.playlist_id, "playlist")?.to_string();
        crate::commands::reorder_playlist(self.app.clone(), self.app.state(), id, p.track_ids).await.map_err(message)?;
        done("reordered")
    }

    /// Change tracks' tags, rating, colour or comment. Only the fields given change. Undoable with `undo_last_edit`, except bpm.
    #[tool(annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true))]
    async fn update_tracks(&self, Parameters(p): Parameters<UpdateTracksParams>) -> Answer {
        self.writable()?;
        if p.track_ids.is_empty() {
            return refuse("No track was given.");
        }
        if p.rating.is_some_and(|stars| stars > 5) {
            return refuse("A rating is 0 to 5 stars.");
        }
        let color = match p.color.as_deref() {
            None => None,
            Some(name) if name.eq_ignore_ascii_case("none") => Some("0".to_owned()),
            Some(name) => Some(query::color_id(name).ok_or_else(|| format!("{name} is not a track colour."))?.to_string()),
        };
        let fields = field_edits(&p);
        if p.rating.is_none() && color.is_none() && p.comment.is_none() && fields.is_empty() {
            return refuse("Nothing to change was given.");
        }
        let mut changed = Vec::new();
        if let Some(stars) = p.rating {
            crate::commands::set_track_rating(self.app.clone(), self.app.state(), p.track_ids.clone(), stars).await.map_err(message)?;
            changed.push("rating");
        }
        if let Some(color) = color {
            crate::commands::set_track_color(self.app.clone(), self.app.state(), p.track_ids.clone(), Some(color)).await.map_err(message)?;
            changed.push("color");
        }
        if let Some(comment) = p.comment.clone() {
            crate::commands::set_track_comment(self.app.clone(), self.app.state(), p.track_ids.clone(), comment).await.map_err(message)?;
            changed.push("comment");
        }
        for (field, value) in fields {
            crate::details::set_track_field(self.app.clone(), self.app.state(), p.track_ids.clone(), field.to_owned(), value, None)
                .await
                .map_err(|e| format!("{field}: {}", e.message))?;
            changed.push(field);
        }
        done(&format!("{} on {} track(s)", changed.join(", "), p.track_ids.len()))
    }

    /// Add audio files to the collection. The new tracks are not analysed yet: open rbxport to analyse them for BPM, key and waveform.
    #[tool(annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true))]
    async fn import_tracks(&self, Parameters(p): Parameters<ImportParams>) -> Answer {
        self.writable()?;
        if p.paths.is_empty() {
            return refuse("No path was given.");
        }
        let report = crate::commands::import_files(self.app.clone(), self.app.state(), p.paths).await.map_err(message)?;
        json(&report)
    }

    /// Remove tracks from the collection and every playlist. The audio files stay on disk. Cannot be undone.
    #[tool(annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true))]
    async fn remove_tracks_from_collection(&self, Parameters(TrackIdsParams { track_ids }): Parameters<TrackIdsParams>) -> Answer {
        self.writable()?;
        let count = track_ids.len();
        crate::commands::remove_from_collection(self.app.clone(), self.app.state(), track_ids).await.map_err(message)?;
        done(&format!("{count} track(s) removed from the collection"))
    }

    /// Remove tracks whose audio file is missing from the collection and every playlist. Cannot be undone.
    #[tool(annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true))]
    async fn remove_missing_tracks(&self, Parameters(p): Parameters<RemoveMissingParams>) -> Answer {
        self.writable()?;
        let removed = crate::relocate::remove_missing_tracks(self.app.clone(), self.app.state(), p.track_ids).await.map_err(message)?;
        done(&format!("{removed} missing track(s) removed"))
    }

    /// Undo the last undoable library edit, from this assistant or the window: a rename, move, delete, track removal from a playlist, or tag change.
    #[tool(annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false))]
    async fn undo_last_edit(&self) -> Answer {
        self.writable()?;
        let history = crate::commands::undo_edit(self.app.clone(), self.app.state()).await.map_err(message)?;
        json(&json!({ "ok": true, "next_undo": history.undo_label, "can_redo": history.can_redo }))
    }
}

// The macro writes the handler's methods as `async` whether or not they wait.
#[allow(clippy::unused_async_trait_impl)]
#[tool_handler(router = self.tool_router)]
impl<R: Runtime> ServerHandler for Tools<R> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("rbxport", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_listed_with_a_description_and_read_tools_say_so() {
        let tools = Tools::<tauri::test::MockRuntime>::tool_router().list_all();
        assert!(tools.len() >= 25, "{} tools", tools.len());
        for tool in &tools {
            assert!(tool.description.as_deref().is_some_and(|d| !d.is_empty()), "{} has no description", tool.name);
            let annotations = tool.annotations.as_ref().unwrap_or_else(|| panic!("{} has no annotations", tool.name));
            assert!(annotations.read_only_hint.is_some(), "{} does not say whether it writes", tool.name);
        }
        let search = tools.iter().find(|t| t.name == "search_tracks").unwrap();
        assert_eq!(search.annotations.as_ref().unwrap().read_only_hint, Some(true));
        let delete = tools.iter().find(|t| t.name == "delete_playlist").unwrap();
        assert_eq!(delete.annotations.as_ref().unwrap().destructive_hint, Some(true));
    }

    #[test]
    fn parameters_translate_or_are_refused_with_a_reason() {
        assert_eq!(search_field(Some("albumArtist")).unwrap(), SearchField::AlbumArtist);
        assert!(search_field(Some("everything")).is_err());
        assert_eq!(criterion(None).unwrap(), RelatedCriterion::BpmAndKey);
        assert!(criterion(Some("vibes")).is_err());
        assert_eq!(source(Some("12"), None).unwrap(), Source::Playlist(12));
        assert!(source(Some("12"), Some("13")).is_err());
        assert!(source(Some("twelve"), None).is_err());
        let search = to_search(SearchParams { colors: Some(vec!["green".into()]), limit: Some(10_000), ..SearchParams::default() }).unwrap();
        assert_eq!((search.colors, search.limit), (vec![5], query::MAX_LIMIT));
        assert!(to_search(SearchParams { colors: Some(vec!["Beige".into()]), ..SearchParams::default() }).is_err());
    }

    #[test]
    fn an_update_sends_only_the_fields_given_by_their_backend_names() {
        let p = UpdateTracksParams { track_ids: vec!["1".into()], genre: Some("Techno".into()), year: Some(1999), original_artist: Some("X".into()), bpm: Some(128.0), ..UpdateTracksParams::default() };
        assert_eq!(
            field_edits(&p),
            [("genre", "Techno".to_owned()), ("originalArtist", "X".to_owned()), ("year", "1999".to_owned()), ("bpm", "128.00".to_owned())]
        );
        for (field, _) in field_edits(&p) {
            assert!(rbl_db::write::TrackField::parse(field).is_some(), "{field}");
        }
    }
}
