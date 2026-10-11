//! What the MCP tools read, answered from the in-memory index.
//!
//! Everything here takes a `&Library` and nothing else, so it runs on a
//! blocking worker and is tested against `rbl_index::testing` libraries
//! without a window or a database. Searching and sorting go through the
//! index's own views — the same code the track list runs — and the extra
//! conditions a tool takes (a BPM range, a play count, a year…) are tested
//! row by row over the view's result, which is a pass over integers already
//! in memory.

use std::collections::HashMap;

use rbl_index::key::camelot_rank;
use rbl_index::strings::fold;
use rbl_index::{Library, Playlists, RelatedCriterion, Row, SearchField, TrackFilter, TrackSource, ViewSpec, COLOR_NAMES, NO_ID};
use serde::Serialize;

use crate::state::sort_from_wire;

/// The most rows one call returns. A row is a couple of hundred bytes of
/// JSON, so a full page stays far below what a model's context should take
/// in at once; the caller pages with `offset` for more.
pub const MAX_LIMIT: usize = 200;
/// Rows returned when the caller does not say.
pub const DEFAULT_LIMIT: usize = 50;

/// One track, as a tool answers with it. Empty text and zero numbers are
/// left out: most tracks have no label, comment or year, and every byte of a
/// result is read by the model.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TrackSummary {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub artist: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub album: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub genre: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub label: String,
    /// The key as the library stores it (`Am`, `F#`, or `8A`).
    #[serde(skip_serializing_if = "str::is_empty")]
    pub key: String,
    /// The same key on the Camelot wheel, when it is one of the 24.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camelot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    pub duration_sec: u32,
    pub rating: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<&'static str>,
    pub play_count: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub date_added: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub comment: String,
    pub analysed: bool,
    /// The audio file, only when the caller asks for paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The Camelot code of a key name: `Am` is `8A`, `C` is `8B`.
pub fn camelot(name: &str) -> Option<String> {
    let rank = camelot_rank(name);
    (rank != u32::MAX).then(|| format!("{}{}", rank / 2 + 1, if rank % 2 == 0 { 'A' } else { 'B' }))
}

/// A track's colour name, `None` for no colour.
pub fn color_name(id: u8) -> Option<&'static str> {
    usize::from(id).checked_sub(1).and_then(|at| COLOR_NAMES.get(at).copied())
}

/// The colour id of a name, case aside: `Pink` is 1, `Purple` 8.
pub fn color_id(name: &str) -> Option<u8> {
    COLOR_NAMES
        .iter()
        .position(|c| c.eq_ignore_ascii_case(name.trim()))
        .and_then(|at| u8::try_from(at + 1).ok())
}

pub fn summary(library: &Library, row: Row, with_path: bool) -> TrackSummary {
    let index = row as usize;
    let key = library.key_name(row);
    let bpm_x100 = library.bpm_x100.get(index).copied().unwrap_or(0);
    let year = library.year.get(index).copied().unwrap_or(0);
    TrackSummary {
        id: library.ids.get(index).copied().unwrap_or(0).to_string(),
        title: library.title.get(index).to_owned(),
        artist: library.artist_name(row).to_owned(),
        album: library.album_name(row).to_owned(),
        genre: library.genre_name(row).to_owned(),
        label: library.label_name(row).to_owned(),
        key: key.to_owned(),
        camelot: camelot(key),
        bpm: (bpm_x100 > 0).then(|| f64::from(bpm_x100) / 100.0),
        duration_sec: library.length_sec.get(index).copied().unwrap_or(0),
        rating: library.rating.get(index).copied().unwrap_or(0),
        color: color_name(library.color.get(index).copied().unwrap_or(0)),
        play_count: library.play_count.get(index).copied().unwrap_or(0),
        year: (year > 0).then_some(year),
        date_added: library.date_added.get(index).to_owned(),
        comment: library.comment.get(index).to_owned(),
        analysed: library.analysed.get(index).copied().unwrap_or(0) != 0,
        path: with_path.then(|| library.folder_path.get(index).to_owned()),
    }
}

/// Where a search looks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Collection,
    /// A playlist, folder or intelligent playlist, by rekordbox id.
    Playlist(u64),
    /// A history session, by rekordbox id.
    History(u64),
}

/// What a search asks for. Every condition is optional and they all apply.
#[derive(Debug, Clone, Default)]
pub struct Search {
    pub source: Source,
    /// rekordbox's own search box: words matched across the fields, or the
    /// one field `field` names.
    pub query: String,
    pub field: SearchField,
    pub artist: Option<String>,
    pub genre: Option<String>,
    pub album: Option<String>,
    pub label: Option<String>,
    pub file_name: Option<String>,
    /// Keys by any spelling (`Am`, `A minor` is not one, `8A`).
    pub keys: Vec<String>,
    /// Widen `keys` to the keys beside them on the wheel: the same number
    /// either letter, and one step either way on the same letter.
    pub compatible_keys: bool,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    pub min_rating: Option<u8>,
    pub colors: Vec<u8>,
    pub min_play_count: Option<u32>,
    pub max_play_count: Option<u32>,
    pub year_min: Option<u16>,
    pub year_max: Option<u16>,
    /// `YYYY-MM-DD`, inclusive.
    pub added_after: Option<String>,
    pub added_before: Option<String>,
    pub analysed: Option<bool>,
    /// A wire sort name (`title`, `bpm`, `djPlayCount`…); the source's own
    /// order when empty.
    pub sort: String,
    pub descending: bool,
    pub offset: usize,
    pub limit: usize,
    pub with_paths: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchResult {
    /// How many tracks match, over every page.
    pub total: usize,
    pub offset: usize,
    pub tracks: Vec<TrackSummary>,
}

/// The index's source for a rekordbox id, or why there is none.
pub fn track_source(library: &Library, source: &Source) -> Result<TrackSource, String> {
    match *source {
        Source::Collection => Ok(TrackSource::Collection),
        Source::Playlist(id) => {
            let playlists = library.playlists();
            let index = playlists.index_of(id).ok_or_else(|| format!("No playlist has the id {id}."))?;
            Ok(if playlists.is_folder(index) {
                TrackSource::PlaylistFolder(index)
            } else if playlists.is_smart(index) {
                TrackSource::SmartPlaylist(index)
            } else {
                TrackSource::Playlist(index)
            })
        }
        Source::History(id) => library
            .histories()
            .index_of(id)
            .map(TrackSource::History)
            .ok_or_else(|| format!("No history session has the id {id}.")),
    }
}

/// Wheel positions a key list admits, widened to its neighbours on request.
fn wheel(keys: &[String], compatible: bool) -> Result<Vec<u32>, String> {
    let mut ranks = Vec::with_capacity(keys.len() * 4);
    for name in keys {
        let rank = camelot_rank(name);
        if rank == u32::MAX {
            return Err(format!("{name} is not a key. Use a name such as Am or F# or a Camelot code such as 8A."));
        }
        ranks.push(rank);
        if compatible {
            // The relative major or minor, and one step round the wheel.
            ranks.push(rank ^ 1);
            ranks.push((rank + 2) % 24);
            ranks.push((rank + 22) % 24);
        }
    }
    ranks.sort_unstable();
    ranks.dedup();
    Ok(ranks)
}

/// Contains, case and accents aside, as the search box matches.
fn contains(haystack: &str, folded_needle: &str) -> bool {
    folded_needle.is_empty() || fold(haystack).contains(folded_needle)
}

pub fn search(library: &Library, search: &Search) -> Result<SearchResult, String> {
    let source = track_source(library, &search.source)?;
    let spec = ViewSpec {
        source,
        sort: sort_from_wire(&search.sort),
        descending: search.descending,
        query: search.query.clone(),
        filter: TrackFilter { colors: (!search.colors.is_empty()).then(|| search.colors.clone()), ..TrackFilter::default() },
    };
    let view = library.open_view_scoped(&spec, search.field);
    let keys = if search.keys.is_empty() { None } else { Some(wheel(&search.keys, search.compatible_keys)?) };
    let artist = search.artist.as_deref().map(fold);
    let genre = search.genre.as_deref().map(fold);
    let album = search.album.as_deref().map(fold);
    let label = search.label.as_deref().map(fold);
    let file_name = search.file_name.as_deref().map(fold);
    let bpm_min = search.bpm_min.map(|bpm| bpm * 100.0);
    let bpm_max = search.bpm_max.map(|bpm| bpm * 100.0);

    let matches = |row: Row| -> bool {
        let i = row as usize;
        if let Some(keys) = &keys {
            if keys.binary_search(&camelot_rank(library.key_name(row))).is_err() {
                return false;
            }
        }
        let bpm = f64::from(library.bpm_x100.get(i).copied().unwrap_or(0));
        if bpm_min.is_some_and(|min| bpm < min) || bpm_max.is_some_and(|max| bpm > max) {
            return false;
        }
        if search.min_rating.is_some_and(|min| library.rating.get(i).copied().unwrap_or(0) < min) {
            return false;
        }
        let plays = u32::from(library.play_count.get(i).copied().unwrap_or(0));
        if search.min_play_count.is_some_and(|min| plays < min) || search.max_play_count.is_some_and(|max| plays > max) {
            return false;
        }
        let year = library.year.get(i).copied().unwrap_or(0);
        if search.year_min.is_some_and(|min| year < min) || search.year_max.is_some_and(|max| year == 0 || year > max) {
            return false;
        }
        // `YYYY-MM-DD` compares as text; a longer stored stamp still
        // starts with the date.
        let added = library.date_added.get(i);
        if search.added_after.as_deref().is_some_and(|after| added.get(..after.len()).unwrap_or(added) < after)
            || search.added_before.as_deref().is_some_and(|before| added.get(..before.len()).unwrap_or(added) > before)
        {
            return false;
        }
        if search.analysed.is_some_and(|wanted| (library.analysed.get(i).copied().unwrap_or(0) != 0) != wanted) {
            return false;
        }
        artist.as_deref().is_none_or(|a| contains(library.artist_name(row), a))
            && genre.as_deref().is_none_or(|g| contains(library.genre_name(row), g))
            && album.as_deref().is_none_or(|a| contains(library.album_name(row), a))
            && label.as_deref().is_none_or(|l| contains(library.label_name(row), l))
            && file_name.as_deref().is_none_or(|f| contains(library.file_name.get(i), f))
    };

    let limit = search.limit.clamp(1, MAX_LIMIT);
    let mut total = 0;
    let mut tracks = Vec::with_capacity(limit);
    for &row in &view.rows {
        if !matches(row) {
            continue;
        }
        if total >= search.offset && tracks.len() < limit {
            tracks.push(summary(library, row, search.with_paths));
        }
        total += 1;
    }
    Ok(SearchResult { total, offset: search.offset, tracks })
}

/// rekordbox's Related Tracks for one track.
pub fn related(library: &Library, track: &str, criterion: RelatedCriterion, limit: usize) -> Result<SearchResult, String> {
    let row = library.row_of(track).ok_or_else(|| format!("No track has the id {track}."))?;
    let rows = library.related_rows(row, criterion);
    let tracks = rows.iter().take(limit.clamp(1, MAX_LIMIT)).map(|&r| summary(library, r, false)).collect();
    Ok(SearchResult { total: rows.len(), offset: 0, tracks })
}

/// Which of `ids` name a track in the library.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Validation {
    pub valid: Vec<String>,
    pub invalid: Vec<String>,
}

pub fn validate(library: &Library, ids: &[String]) -> Validation {
    let (valid, invalid) = ids.iter().cloned().partition(|id| library.row_of(id).is_some());
    Validation { valid, invalid }
}

// ------------------------------------------------------------------ trees

/// A playlist, folder or history session.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ListNode {
    pub id: String,
    pub name: String,
    /// `playlist`, `folder`, `smart` or `session`.
    pub kind: &'static str,
    /// Its folders' names and its own, joined with `/`.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// The tracks in it; for a folder, the playlists and folders directly
    /// under it. Left out for an intelligent playlist, whose tracks are
    /// worked out when it is opened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// The tree in the order the window shows it: each folder followed by what
/// is in it.
fn tree_order(lists: &Playlists) -> Vec<usize> {
    let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
    for index in 0..lists.len() {
        children.entry(lists.parent.get(index).copied().unwrap_or(NO_ID)).or_default().push(index);
    }
    for list in children.values_mut() {
        list.sort_by_key(|&i| lists.seq.get(i).copied().unwrap_or(u32::MAX));
    }
    let mut out = Vec::with_capacity(lists.len());
    let mut stack: Vec<usize> = children.get(&NO_ID).map(|top| top.iter().rev().copied().collect()).unwrap_or_default();
    while let Some(index) = stack.pop() {
        out.push(index);
        if let Some(below) = u32::try_from(index).ok().and_then(|i| children.get(&i)) {
            stack.extend(below.iter().rev());
        }
    }
    out
}

fn path_of(lists: &Playlists, index: usize) -> String {
    let mut names = vec![lists.name(index)];
    let mut at = lists.parent.get(index).copied().unwrap_or(NO_ID);
    // Bounded by the tree's size, so a parent loop in a damaged library
    // ends rather than spins.
    for _ in 0..lists.len() {
        if at == NO_ID {
            break;
        }
        names.push(lists.name(at as usize));
        at = lists.parent.get(at as usize).copied().unwrap_or(NO_ID);
    }
    names.reverse();
    names.join("/")
}

fn nodes(lists: &Playlists, history: bool) -> Vec<ListNode> {
    let mut direct: HashMap<u32, usize> = HashMap::new();
    for index in 0..lists.len() {
        *direct.entry(lists.parent.get(index).copied().unwrap_or(NO_ID)).or_default() += 1;
    }
    tree_order(lists)
        .into_iter()
        .map(|index| {
            let parent = lists.parent.get(index).copied().unwrap_or(NO_ID);
            let folder = lists.is_folder(index);
            let smart = lists.is_smart(index);
            ListNode {
                id: lists.ids.get(index).copied().unwrap_or(0).to_string(),
                name: lists.name(index).to_owned(),
                kind: match (folder, smart, history) {
                    (true, _, _) => "folder",
                    (_, true, _) => "smart",
                    (_, _, true) => "session",
                    _ => "playlist",
                },
                path: path_of(lists, index),
                parent_id: (parent != NO_ID).then(|| lists.ids.get(parent as usize).copied().unwrap_or(0).to_string()),
                count: if folder {
                    Some(u32::try_from(index).ok().and_then(|i| direct.get(&i).copied()).unwrap_or(0))
                } else if smart {
                    None
                } else {
                    Some(lists.members.get(index).map_or(0, Vec::len))
                },
            }
        })
        .collect()
}

pub fn playlists(library: &Library) -> Vec<ListNode> {
    nodes(&library.playlists(), false)
}

pub fn histories(library: &Library) -> Vec<ListNode> {
    nodes(&library.histories(), true)
}

// ------------------------------------------------------------------ stats

/// A value and how much of the library carries it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Group {
    pub value: String,
    pub tracks: usize,
    pub total_duration_sec: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_bpm: Option<f64>,
}

/// What `group_by` can group on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Genre,
    Artist,
    Album,
    Label,
    Key,
    Year,
    Rating,
    Color,
    /// Whole BPMs, in fives: `120-124`.
    Bpm,
    /// The year and month a track was added: `2024-05`.
    Added,
}

impl GroupBy {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "genre" => Self::Genre,
            "artist" => Self::Artist,
            "album" => Self::Album,
            "label" => Self::Label,
            "key" => Self::Key,
            "year" => Self::Year,
            "rating" => Self::Rating,
            "color" => Self::Color,
            "bpm" => Self::Bpm,
            "added" | "dateAdded" => Self::Added,
            _ => return None,
        })
    }
}

fn group_value(library: &Library, row: Row, by: GroupBy) -> String {
    let i = row as usize;
    match by {
        GroupBy::Genre => library.genre_name(row).to_owned(),
        GroupBy::Artist => library.artist_name(row).to_owned(),
        GroupBy::Album => library.album_name(row).to_owned(),
        GroupBy::Label => library.label_name(row).to_owned(),
        GroupBy::Key => camelot(library.key_name(row)).unwrap_or_else(|| library.key_name(row).to_owned()),
        GroupBy::Year => match library.year.get(i).copied().unwrap_or(0) {
            0 => String::new(),
            year => year.to_string(),
        },
        GroupBy::Rating => format!("{} stars", library.rating.get(i).copied().unwrap_or(0)),
        GroupBy::Color => color_name(library.color.get(i).copied().unwrap_or(0)).unwrap_or("").to_owned(),
        GroupBy::Bpm => match library.bpm_x100.get(i).copied().unwrap_or(0) / 100 {
            0 => String::new(),
            bpm => {
                let low = bpm - bpm % 5;
                format!("{low}-{}", low + 4)
            }
        },
        GroupBy::Added => library.date_added.get(i).get(..7).unwrap_or("").to_owned(),
    }
}

fn mean(sum: u64, count: u64) -> Option<f64> {
    // Two decimal places: x100 BPMs summed, then divided.
    let sum = u32::try_from(sum / count.max(1)).ok()?;
    (count > 0 && sum > 0).then(|| f64::from(sum) / 100.0)
}

/// Groups `rows` by a property, the largest groups first. Values no track
/// carries are not listed; tracks with no value are, as `""`.
pub fn group(library: &Library, rows: &[Row], by: GroupBy, limit: usize) -> (usize, Vec<Group>) {
    struct Acc {
        tracks: usize,
        seconds: u64,
        bpm_sum: u64,
        bpm_count: u64,
    }
    let mut groups: HashMap<String, Acc> = HashMap::new();
    for &row in rows {
        let i = row as usize;
        let acc = groups.entry(group_value(library, row, by)).or_insert(Acc { tracks: 0, seconds: 0, bpm_sum: 0, bpm_count: 0 });
        acc.tracks += 1;
        acc.seconds += u64::from(library.length_sec.get(i).copied().unwrap_or(0));
        let bpm = library.bpm_x100.get(i).copied().unwrap_or(0);
        if bpm > 0 {
            acc.bpm_sum += u64::from(bpm);
            acc.bpm_count += 1;
        }
    }
    let count = groups.len();
    let mut out: Vec<Group> = groups
        .into_iter()
        .map(|(value, acc)| Group { value, tracks: acc.tracks, total_duration_sec: acc.seconds, average_bpm: mean(acc.bpm_sum, acc.bpm_count) })
        .collect();
    // Ties by value, so the answer reads the same from one call to the next.
    out.sort_by(|a, b| b.tracks.cmp(&a.tracks).then_with(|| a.value.cmp(&b.value)));
    out.truncate(limit.clamp(1, MAX_LIMIT));
    (count, out)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LibraryStats {
    pub tracks: usize,
    pub playlists: usize,
    pub history_sessions: usize,
    pub total_duration_sec: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_bpm: Option<f64>,
    pub analysed: usize,
    pub rated: usize,
    pub never_played: usize,
    pub artists: usize,
    pub genres: usize,
    pub top_genres: Vec<Group>,
    pub top_keys: Vec<Group>,
    pub top_artists: Vec<Group>,
}

pub fn stats(library: &Library) -> LibraryStats {
    let rows: Vec<Row> = (0..u32::try_from(library.len()).unwrap_or(u32::MAX)).collect();
    let (genres, top_genres) = group(library, &rows, GroupBy::Genre, 10);
    let (artists, top_artists) = group(library, &rows, GroupBy::Artist, 10);
    let (_, top_keys) = group(library, &rows, GroupBy::Key, 10);
    let (bpm_sum, bpm_count) = library
        .bpm_x100
        .iter()
        .filter(|&&bpm| bpm > 0)
        .fold((0_u64, 0_u64), |(sum, n), &bpm| (sum + u64::from(bpm), n + 1));
    let playlists = library.playlists();
    LibraryStats {
        tracks: library.len(),
        playlists: (0..playlists.len()).filter(|&i| !playlists.is_folder(i)).count(),
        history_sessions: {
            let histories = library.histories();
            (0..histories.len()).filter(|&i| !histories.is_folder(i)).count()
        },
        total_duration_sec: library.length_sec.iter().map(|&s| u64::from(s)).sum(),
        average_bpm: mean(bpm_sum, bpm_count),
        analysed: library.analysed.iter().filter(|&&a| a != 0).count(),
        rated: library.rating.iter().filter(|&&r| r > 0).count(),
        never_played: library.play_count.iter().filter(|&&p| p == 0).count(),
        artists,
        genres,
        top_genres,
        top_keys,
        top_artists,
    }
}

/// The rows of a source, for grouping a playlist or session rather than the
/// whole library.
pub fn source_rows(library: &Library, source: &Source) -> Result<Vec<Row>, String> {
    Ok(library.source_rows(&track_source(library, source)?))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlayedTrack {
    #[serde(flatten)]
    pub track: TrackSummary,
    /// The sessions it is in.
    pub sessions: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HistoryStats {
    pub sessions: usize,
    /// Every play over every session.
    pub plays: usize,
    pub distinct_tracks: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_session: Option<String>,
    pub most_played: Vec<PlayedTrack>,
    pub top_genres: Vec<Group>,
    pub top_artists: Vec<Group>,
}

/// What the history sessions add up to. Session names are rekordbox's
/// dates (`2024-05-01`, with ` (2)` for a second that day), so the first and
/// last are the least and greatest name.
pub fn history_stats(library: &Library, limit: usize) -> HistoryStats {
    let histories = library.histories();
    let sessions: Vec<usize> = (0..histories.len()).filter(|&i| !histories.is_folder(i)).collect();
    let mut appearances: HashMap<Row, usize> = HashMap::new();
    let mut plays = Vec::new();
    for &session in &sessions {
        let members = histories.members.get(session).map_or(&[][..], Vec::as_slice);
        plays.extend_from_slice(members);
        let mut seen = members.to_vec();
        seen.sort_unstable();
        seen.dedup();
        for row in seen {
            *appearances.entry(row).or_default() += 1;
        }
    }
    let names = sessions.iter().map(|&i| histories.name(i));
    let first_session = names.clone().min().map(str::to_owned);
    let last_session = names.max().map(str::to_owned);
    drop(histories);
    let mut ranked: Vec<(Row, usize)> = appearances.iter().map(|(&row, &n)| (row, n)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let limit = limit.clamp(1, MAX_LIMIT);
    let (_, top_genres) = group(library, &plays, GroupBy::Genre, 10);
    let (_, top_artists) = group(library, &plays, GroupBy::Artist, 10);
    HistoryStats {
        sessions: sessions.len(),
        plays: plays.len(),
        distinct_tracks: appearances.len(),
        first_session,
        last_session,
        most_played: ranked
            .into_iter()
            .take(limit)
            .map(|(row, sessions)| PlayedTrack { track: summary(library, row, false), sessions })
            .collect(),
        top_genres,
        top_artists,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rbl_index::testing::{add_folder, add_history, add_playlist, library_from, TestTrack};

    fn library() -> Library {
        let mut lib = library_from(&[
            TestTrack { id: 1, title: "Alpha", artist: "Octave One", genre: "Techno", key: "Am", bpm_x100: 12800, rating: 5, play_count: 3, year: 2000, date_added: "2024-01-10", length_sec: 300, path: "/m/alpha.mp3", ..TestTrack::default() },
            TestTrack { id: 2, title: "Beta", artist: "Octave One", genre: "Techno", key: "C", bpm_x100: 12600, rating: 3, play_count: 0, year: 2010, date_added: "2024-03-01", length_sec: 240, color: 5, path: "/m/beta.flac", ..TestTrack::default() },
            TestTrack { id: 3, title: "Gamma", artist: "Kerri Chandler", genre: "Deep House", key: "Em", bpm_x100: 12200, rating: 0, play_count: 7, date_added: "2024-05-20", length_sec: 420, path: "/m/gamma.wav", ..TestTrack::default() },
            TestTrack { id: 4, title: "Delta", artist: "Kerri Chandler", genre: "House", key: "3B", bpm_x100: 0, rating: 4, play_count: 1, date_added: "2023-12-31", length_sec: 200, path: "/m/delta.mp3", ..TestTrack::default() },
        ]);
        let folder = add_folder(&mut lib, "Sets");
        let set = add_playlist(&mut lib, "Friday", &[2, 0]);
        {
            // Put Friday inside Sets, as a tree read from a library would.
            let mut playlists = lib.playlists().clone();
            playlists.parent[set] = u32::try_from(folder).unwrap();
            lib.set_playlists(playlists);
        }
        add_history(&mut lib, "2024-05-01", &[0, 2]);
        add_history(&mut lib, "2024-06-01", &[2, 3, 2]);
        lib
    }

    fn ids(result: &SearchResult) -> Vec<&str> {
        result.tracks.iter().map(|t| t.id.as_str()).collect()
    }

    fn search_all(lib: &Library, search: Search) -> SearchResult {
        super::search(lib, &Search { limit: DEFAULT_LIMIT, ..search }).unwrap()
    }

    #[test]
    fn keys_match_by_wheel_position_whatever_the_spelling() {
        let lib = library();
        assert_eq!(camelot("Am").as_deref(), Some("8A"));
        assert_eq!(camelot("C").as_deref(), Some("8B"));
        assert_eq!(camelot("3B").as_deref(), Some("3B"));
        assert_eq!(camelot("nonsense"), None);
        let exact = search_all(&lib, Search { keys: vec!["8A".into()], ..Search::default() });
        assert_eq!(ids(&exact), ["1"]);
        // 8A's neighbours: 8B (C), 9A (Em), 7A.
        let compatible = search_all(&lib, Search { keys: vec!["Am".into()], compatible_keys: true, ..Search::default() });
        assert_eq!(ids(&compatible), ["1", "2", "3"]);
        assert!(super::search(&lib, &Search { keys: vec!["H".into()], limit: 10, ..Search::default() }).is_err());
    }

    #[test]
    fn range_and_text_conditions_all_apply() {
        let lib = library();
        let bpm = search_all(&lib, Search { bpm_min: Some(124.0), bpm_max: Some(128.0), ..Search::default() });
        assert_eq!(ids(&bpm), ["1", "2"]);
        let rated = search_all(&lib, Search { min_rating: Some(4), ..Search::default() });
        assert_eq!(ids(&rated), ["1", "4"]);
        let unplayed = search_all(&lib, Search { max_play_count: Some(0), ..Search::default() });
        assert_eq!(ids(&unplayed), ["2"]);
        let artist = search_all(&lib, Search { artist: Some("kerri".into()), genre: Some("house".into()), ..Search::default() });
        assert_eq!(ids(&artist), ["3", "4"]);
        let added = search_all(&lib, Search { added_after: Some("2024-02-01".into()), added_before: Some("2024-04-30".into()), ..Search::default() });
        assert_eq!(ids(&added), ["2"]);
        let years = search_all(&lib, Search { year_max: Some(2005), ..Search::default() });
        assert_eq!(ids(&years), ["1"]);
        let file = search_all(&lib, Search { file_name: Some(".flac".into()), ..Search::default() });
        assert_eq!(ids(&file), ["2"]);
        let green = search_all(&lib, Search { colors: vec![color_id("green").unwrap()], ..Search::default() });
        assert_eq!(ids(&green), ["2"]);
    }

    #[test]
    fn a_page_counts_every_match_and_returns_its_window() {
        let lib = library();
        let page = super::search(&lib, &Search { sort: "djPlayCount".into(), descending: true, offset: 1, limit: 2, ..Search::default() }).unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(ids(&page), ["1", "4"]);
        assert!(page.tracks.iter().all(|t| t.path.is_none()));
        let with_paths = super::search(&lib, &Search { limit: 1, with_paths: true, ..Search::default() }).unwrap();
        assert_eq!(with_paths.tracks[0].path.as_deref(), Some("/m/alpha.mp3"));
    }

    #[test]
    fn a_playlist_or_session_is_searched_in_its_own_order() {
        let lib = library();
        let friday = lib.playlists().ids[1];
        let result = search_all(&lib, Search { source: Source::Playlist(friday), ..Search::default() });
        assert_eq!(ids(&result), ["3", "1"]);
        let session = lib.histories().ids[1];
        let result = search_all(&lib, Search { source: Source::History(session), ..Search::default() });
        assert_eq!(ids(&result), ["3", "4", "3"]);
        assert!(super::search(&lib, &Search { source: Source::Playlist(999_999), limit: 1, ..Search::default() }).is_err());
    }

    #[test]
    fn the_tree_lists_folders_before_their_contents_with_paths() {
        let lib = library();
        let nodes = playlists(&lib);
        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[0].kind, nodes[0].path.as_str(), nodes[0].count), ("folder", "Sets", Some(1)));
        assert_eq!((nodes[1].kind, nodes[1].path.as_str(), nodes[1].count), ("playlist", "Sets/Friday", Some(2)));
        assert_eq!(nodes[1].parent_id.as_deref(), Some(nodes[0].id.as_str()));
        let sessions = histories(&lib);
        assert_eq!(sessions.iter().map(|s| (s.kind, s.count)).collect::<Vec<_>>(), [("session", Some(2)), ("session", Some(3))]);
    }

    #[test]
    fn stats_and_groups_add_up() {
        let lib = library();
        let s = stats(&lib);
        assert_eq!((s.tracks, s.playlists, s.history_sessions), (4, 1, 2));
        assert_eq!(s.total_duration_sec, 1160);
        assert_eq!(s.average_bpm, Some(125.33));
        // The test library counts a track with a BPM as analysed.
        assert_eq!((s.analysed, s.rated, s.never_played, s.artists, s.genres), (3, 3, 1, 2, 3));
        assert_eq!(s.top_genres[0].value, "Techno");
        assert_eq!(s.top_genres[0].tracks, 2);
        let rows: Vec<Row> = (0..4).collect();
        let (count, bpm) = group(&lib, &rows, GroupBy::Bpm, 10);
        assert_eq!(count, 3);
        assert_eq!(bpm.iter().map(|g| (g.value.as_str(), g.tracks)).collect::<Vec<_>>(), [("125-129", 2), ("", 1), ("120-124", 1)]);
    }

    #[test]
    fn history_stats_count_sessions_not_repeats() {
        let lib = library();
        let h = history_stats(&lib, 10);
        assert_eq!((h.sessions, h.plays, h.distinct_tracks), (2, 5, 3));
        assert_eq!(h.first_session.as_deref(), Some("2024-05-01"));
        assert_eq!(h.last_session.as_deref(), Some("2024-06-01"));
        assert_eq!(h.most_played[0].track.id, "3");
        assert_eq!(h.most_played[0].sessions, 2);
    }

    #[test]
    fn validation_splits_known_from_unknown_ids() {
        let lib = library();
        let v = validate(&lib, &["1".into(), "42".into(), "x".into()]);
        assert_eq!(v.valid, ["1"]);
        assert_eq!(v.invalid, ["42", "x"]);
    }

    #[test]
    fn related_tracks_come_from_the_index() {
        let lib = library();
        // 8A at 128: 8B at 126 and 9A at 122 are within 6 % and beside it
        // on the wheel; 3B with no BPM is neither.
        let r = related(&lib, "1", RelatedCriterion::BpmAndKey, 10).unwrap();
        assert_eq!(ids(&r), ["2", "3"]);
        assert!(related(&lib, "404", RelatedCriterion::SameArtist, 10).is_err());
    }
}
