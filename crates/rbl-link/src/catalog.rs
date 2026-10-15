//! The database server's questions, answered out of the index.
//!
//! `rbl-dbserver` owns the protocol and asks a [`Catalog`] for rows; this is
//! the catalog over `rbl-index`, so a player browses the same columns the
//! browser does, sorted by the same ranks. The few fields the index does not
//! hold — bit rate, file size, the original artist — come from a point read
//! of the database through the [`Source`].
//!
//! IDs on the wire preserve their `djmd*` database values so requests remain
//! stable across reloads. Tracks fit a `u32` in every library measured;
//! 0 means "none" in an album row. Artwork is asked for by the track's or
//! album's id, the one a track row's artwork field carries.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use rbl_anlz::Anlz;
use rbl_dbserver::catalog::{
    Analysis as Wanted, ArtistRole, ArtworkSize, Catalog, Edit, HotCueBank, HotCueBankCue, MyTag, Query, RootCategory, Row, Sort,
    TrackColumn,
    TrackDetails, TrackScope, UsbCue,
};
use rbl_dbserver::item::TrackRow;
use rbl_dbserver::keys;
use rbl_index::{Library, NO_ID, SortColumn, TrackSource, ViewSpec, key::camelot_rank, smart::Date};

use crate::blobs::{self, Analysis, ExtendedCue};

/// How keys are written on player screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyNotation {
    Classic,
    Alphanumeric,
}

/// How keys are ordered in track lists and the KEY browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOrder {
    Musical,
    Alphabetical,
}

/// Where the catalog reads from. The app implements this over its state so a
/// library reloaded behind a running link session is what the next request
/// sees, and so `rbl-link` needs nothing from the webview.
pub trait Source: Send + Sync {
    /// The library as it is now.
    fn library(&self) -> Option<Arc<Library>>;
    fn key_notation(&self) -> KeyNotation {
        KeyNotation::Alphanumeric
    }
    fn device_settings(&self) -> rbl_prolink::DeviceSettings {
        rbl_prolink::DeviceSettings {
            key_display: match self.key_notation() {
                KeyNotation::Classic => rbl_prolink::KeyDisplay::Classic,
                KeyNotation::Alphanumeric => rbl_prolink::KeyDisplay::Alphanumeric,
            },
            ..rbl_prolink::DeviceSettings::default()
        }
    }
    fn key_order(&self) -> KeyOrder {
        KeyOrder::Musical
    }
    /// The date relative Smart Playlist conditions count back from.
    fn today(&self) -> Date {
        Date::today()
    }
    fn sorts(&self) -> Vec<Sort> {
        Sort::DEFAULTS.to_vec()
    }
    fn root_categories(&self) -> Vec<RootCategory> {
        RootCategory::defaults()
    }
    /// The field shown beside track titles in browse lists.
    fn track_column(&self) -> TrackColumn {
        TrackColumn::Comment
    }
    /// Where analysis files and artwork live.
    fn share_root(&self) -> PathBuf;
    /// The fields the index does not hold, by `djmdContent.ID`.
    fn details(&self, id: &str) -> Option<rbl_db::details::TrackDetails>;
    /// Content IDs paired with `seed` in rekordbox's Matching table.
    fn matching_ids(&self, _seed: u32) -> Vec<u32> {
        Vec::new()
    }
    fn artist_role_names(&self, _role: ArtistRole) -> Vec<(u32, String)> { Vec::new() }
    fn artist_role_track_ids(&self, _role: ArtistRole, _artist: u32) -> Vec<u32> { Vec::new() }
    fn hot_cue_banks(&self, _parent: Option<u32>) -> Vec<rbl_db::details::HotCueBank> { Vec::new() }
    fn hot_cue_bank_cues(&self, _bank: u32) -> Vec<rbl_db::details::HotCueBankCue> { Vec::new() }
    fn hot_cue_bank_track_ids(&self, _bank: u32) -> Vec<u32> { Vec::new() }
    /// The live `djmdMyTag` rows, categories and tags.
    fn my_tags(&self) -> Vec<rbl_db::export_info::MyTagRow> { Vec::new() }
    fn edit(&self, _edit: &Edit) -> bool {
        false
    }
    /// Makes the history of a new link session, returning its id; `None`
    /// when the library cannot be written.
    fn new_link_history(&self) -> Option<u32> {
        None
    }
    /// Puts a play on the end of a history session.
    fn add_to_history(&self, _history: u32, _track: u32) -> bool {
        false
    }
    /// Takes every play of a track off a history session.
    fn remove_from_history(&self, _history: u32, _track: u32) -> bool {
        false
    }
}

/// Artwork larger than this is not sent: the protocol carries it whole in
/// one reply, and a player draws a thumbnail.
const MAX_ARTWORK: u64 = 4 * 1024 * 1024;

/// rekordbox keeps each artwork three ways beside `djmdContent.ImagePath`:
/// the file it names (`artwork.jpg`, up to a few hundred KB), a medium
/// `artwork_m.jpg` and a small `artwork_s.jpg`. It serves a player that asks
/// with the size argument the medium one (measured: rekordbox 7.2.11's 127
/// artwork replies to a CDJ-3000 ran 2.5–32 KB, the `_m` files' range); sent
/// the full file, a CDJ-3000 left much of its list without art.
const MEDIUM_ARTWORK_SUFFIX: &str = "_m";
/// The small file, served to a request without the size argument (see
/// [`ArtworkSize`]).
const SMALL_ARTWORK_SUFFIX: &str = "_s";

/// A player asks for a dozen blobs when it loads a track and two for every
/// row it draws; this many parsed analysis files stay in memory so each is
/// read once per track, not once per blob.
const ANALYSIS_CACHE: usize = 32;
/// Complete menus are small (mostly track ids) and shared by every player.
const QUERY_CACHE: usize = 24;
/// Artwork is disk-bound and can be comparatively large, so bound it by bytes.
const ARTWORK_CACHE_BYTES: usize = 32 * 1024 * 1024;

struct QueryCacheEntry {
    library: Weak<Library>,
    today: Date,
    query: Query,
    rows: Arc<[Row]>,
}

struct ArtworkCacheEntry {
    library: Weak<Library>,
    row: rbl_index::Row,
    size: ArtworkSize,
    bytes: Arc<[u8]>,
}

#[derive(Default)]
struct BrowseCache {
    queries: Vec<QueryCacheEntry>,
    artwork: Vec<ArtworkCacheEntry>,
    artwork_bytes: usize,
}

/// The three analysis files of one track, parsed, or `None` where absent.
struct Parsed {
    row: rbl_index::Row,
    dat: Option<Anlz>,
    ext: Option<Anlz>,
    two_ex: Option<Anlz>,
}

/// The tracks players have added to this link session's history with the
/// `0x3001` command. The catalog greys their rows the way rekordbox greys
/// what is in its link history. Cloned handles share one set.
#[derive(Clone, Default)]
pub struct Played(Arc<Mutex<HashSet<u32>>>);

impl Played {
    pub fn mark(&self, track: u32) {
        self.0.lock().insert(track);
    }

    pub fn contains(&self, track: u32) -> bool {
        self.0.lock().contains(&track)
    }
}

/// The history this link session writes, as rekordbox 7.2.11 keeps it
/// (`PSvDBMain`): made on the first play a player adds, then added to until
/// it is deleted from the library or from a player.
#[derive(Default)]
struct LinkHistory {
    session: Option<u32>,
    /// The track added last, which is not added again straight after
    /// itself; 0 for none.
    last: u32,
}

impl LinkHistory {
    /// The session, while the library still has it.
    fn session_in(&self, library: &Library) -> Option<u32> {
        self.session
            .filter(|&id| library.histories().index_of(u64::from(id)).is_some())
    }
}

/// The library as a player browses it.
pub struct IndexCatalog {
    source: Arc<dyn Source>,
    played: Played,
    /// Most recently used last.
    analysis: Mutex<Vec<Arc<Parsed>>>,
    /// Menus and their visible rows are shared between every player session.
    /// Entries carry a weak library identity, so a reload makes them vanish.
    browse: Mutex<BrowseCache>,
    /// Held across the write, so two players' first plays make one session.
    link_history: Mutex<LinkHistory>,
}

impl IndexCatalog {
    pub fn new(source: Arc<dyn Source>, played: Played) -> Self {
        Self {
            source,
            played,
            analysis: Mutex::new(Vec::with_capacity(ANALYSIS_CACHE)),
            browse: Mutex::new(BrowseCache::default()),
            link_history: Mutex::new(LinkHistory::default()),
        }
    }

    /// A player's history edits, as `PSvDBMain::OnHistoryCmd` makes them.
    fn history_edit(&self, edit: &Edit) -> bool {
        let Some(library) = self.source.library() else {
            return false;
        };
        let mut history = self.link_history.lock();
        match *edit {
            Edit::HistoryAdd { track } => {
                let session = match history.session_in(&library) {
                    // The same track straight after itself is not added again.
                    Some(_) if history.last == track => return true,
                    Some(session) => session,
                    None => {
                        let Some(session) = self.source.new_link_history() else {
                            return false;
                        };
                        *history = LinkHistory {
                            session: Some(session),
                            last: 0,
                        };
                        session
                    }
                };
                let added = self.source.add_to_history(session, track);
                if added {
                    history.last = track;
                    self.played.mark(track);
                }
                added
            }
            Edit::HistoryRemove { track } => {
                let Some(session) = history.session_in(&library) else {
                    return false;
                };
                if !self.source.remove_from_history(session, track) {
                    return false;
                }
                // What is now last, which a repeat of it may not follow.
                history.last = self
                    .source
                    .library()
                    .and_then(|library| {
                        let histories = library.histories();
                        let &row = histories
                            .members
                            .get(histories.index_of(u64::from(session))?)?
                            .last()?;
                        Self::track_id(&library, row)
                    })
                    .unwrap_or(0);
                true
            }
            // rekordbox deletes only its own link session's history, named
            // or as `-1`, and afterwards starts a new one. `[ASSUME]` that the
            // session stays in the library: rekordbox 7.2.11's master.db
            // controller refuses `deleteLinkHistory`, and the per-session
            // path it takes on a player's delete only clears played marks.
            Edit::HistoryDelete { history: named } => {
                if named == u32::MAX || history.session == Some(named) {
                    *history = LinkHistory::default();
                }
                true
            }
            _ => false,
        }
    }

    /// A track's My Tags as the wire carries them.
    fn wire_my_tags(library: &Library, row: rbl_index::Row) -> Vec<u32> {
        library
            .my_tag_keys(row as usize)
            .iter()
            .map(|&key| u32::from_ne_bytes(key.to_ne_bytes()))
            .collect()
    }

    fn row_of(library: &Library, id: u32) -> Option<rbl_index::Row> {
        library.row_of_id(u64::from(id))
    }

    /// The row a menu item's id names: the track with that id, or the first
    /// track with artwork on the album with that id. The two id spaces
    /// overlap for small numbers, and a
    /// track wins: it is what a player asks about far more often.
    fn item_row(library: &Library, id: u32) -> Option<rbl_index::Row> {
        if let Some(row) = Self::row_of(library, id) {
            return Some(row);
        }
        let album = Self::dense_id(&library.album_ids, id);
        if album == NO_ID {
            return None;
        }
        library
            .album
            .iter()
            .enumerate()
            .find(|&(row, &of)| of == album && !library.artwork_path.get(row).is_empty())
            .and_then(|(row, _)| rbl_index::Row::try_from(row).ok())
    }

    /// The artwork file of a row, read whole, in rekordbox's order: for
    /// [`ArtworkSize::Medium`] the `_m` file it keeps beside the one the
    /// library names, else the `_s` one, else the named one; for
    /// [`ArtworkSize::Small`] the `_s` file, else the named one.
    fn artwork_at(
        &self,
        library: &Arc<Library>,
        row: rbl_index::Row,
        size: ArtworkSize,
    ) -> Option<Vec<u8>> {
        {
            let mut cache = self.browse.lock();
            if let Some(at) = cache.artwork.iter().position(|entry| {
                entry.row == row
                    && entry.size == size
                    && entry
                        .library
                        .upgrade()
                        .is_some_and(|cached| Arc::ptr_eq(&cached, library))
            }) {
                let hit = cache.artwork.remove(at);
                let bytes = hit.bytes.to_vec();
                cache.artwork.push(hit);
                return Some(bytes);
            }
            cache.artwork.retain(|entry| {
                entry
                    .library
                    .upgrade()
                    .is_some_and(|cached| Arc::ptr_eq(&cached, library))
            });
            cache.artwork_bytes = cache.artwork.iter().map(|entry| entry.bytes.len()).sum();
        }
        let relative = library.artwork_path.get(row as usize);
        if relative.is_empty() {
            return None;
        }
        let share = self.source.share_root();
        let path = artwork_candidates(relative, size)
            .iter()
            .filter_map(|candidate| resolve_under(&share, candidate))
            .find(|candidate| candidate.is_file())?;
        let meta = std::fs::metadata(&path).ok()?;
        if meta.len() > MAX_ARTWORK {
            return None;
        }
        let bytes: Arc<[u8]> = std::fs::read(&path).ok()?.into();
        let mut cache = self.browse.lock();
        while cache.artwork_bytes + bytes.len() > ARTWORK_CACHE_BYTES && !cache.artwork.is_empty() {
            let evicted = cache.artwork.remove(0);
            cache.artwork_bytes -= evicted.bytes.len();
        }
        if bytes.len() <= ARTWORK_CACHE_BYTES {
            cache.artwork_bytes += bytes.len();
            cache.artwork.push(ArtworkCacheEntry {
                library: Arc::downgrade(library),
                row,
                size,
                bytes: Arc::clone(&bytes),
            });
        }
        Some(bytes.to_vec())
    }

    fn cached_list(&self, library: &Arc<Library>, query: &Query) -> Vec<Row> {
        let today = self.source.today();
        // Link history changes independently of the indexed library, so its
        // menu intentionally remains live rather than cacheable.
        if matches!(
            query,
            Query::Histories
                | Query::Tracks {
                    scope: TrackScope::History(_) | TrackScope::TagList,
                    ..
                }
        ) {
            return self.list_uncached(library, query, &today);
        }
        {
            let mut cache = self.browse.lock();
            if let Some(at) = cache.queries.iter().position(|entry| {
                entry.query == *query
                    && entry.today == today
                    && entry
                        .library
                        .upgrade()
                        .is_some_and(|cached| Arc::ptr_eq(&cached, library))
            }) {
                let hit = cache.queries.remove(at);
                let rows = hit.rows.to_vec();
                cache.queries.push(hit);
                return rows;
            }
            cache.queries.retain(|entry| {
                entry.today == today
                    && entry
                        .library
                        .upgrade()
                        .is_some_and(|cached| Arc::ptr_eq(&cached, library))
            });
        }
        let rows = self.list_uncached(library, query, &today);
        let mut cache = self.browse.lock();
        if cache.queries.len() >= QUERY_CACHE {
            cache.queries.remove(0);
        }
        cache.queries.push(QueryCacheEntry {
            library: Arc::downgrade(library),
            today,
            query: query.clone(),
            rows: Arc::from(rows.clone()),
        });
        rows
    }

    fn list_uncached(&self, library: &Library, query: &Query, today: &Date) -> Vec<Row> {
        match query {
            Query::BpmBuckets => library.bpm_buckets().into_iter().map(Row::Date).collect(),
            Query::Ratings => library.ratings().into_iter().map(Row::Date).collect(),
            Query::Bitrates => library.bitrates().into_iter().map(Row::Date).collect(),
            Query::Colors => library
                .color_ids()
                .into_iter()
                .map(|id| Row::Named {
                    id,
                    name: color_name(id).to_owned(),
                })
                .collect(),
            Query::DurationMinutes => library
                .duration_minute_buckets()
                .into_iter()
                .map(Row::Date)
                .collect(),
            Query::ReleaseDecades => library
                .release_decades()
                .into_iter()
                .map(Row::Date)
                .collect(),
            Query::ReleaseYears(decade) => library
                .release_years(*decade)
                .into_iter()
                .map(Row::Date)
                .collect(),
            Query::Genres(_) => Self::named(&library.genre, &library.genres, &library.genre_ids),
            Query::GenreArtists(genre) => Self::genre_artists(library, *genre),
            Query::GenreArtistAlbums { genre, artist } => Self::genre_albums(library, *genre, *artist),
            Query::Labels(_) => Self::named(&library.label, &library.labels, &library.label_ids),
            Query::LabelArtists(label) => Self::label_artists(library, *label),
            Query::LabelArtistAlbums { label, artist } => Self::label_albums(library, *label, *artist),
            Query::ArtistRoleArtists(role) => self
                .source
                .artist_role_names(*role)
                .into_iter()
                .filter(|(_, name)| !name.is_empty())
                .map(|(id, name)| Row::Named { id, name })
                .collect(),
            Query::ArtistRoleAlbums { role, artist } => self.role_albums(library, *role, *artist),
            Query::Artists(_) => Self::named(&library.artist, &library.artists, &library.artist_ids),
            Query::Albums(_) => Self::named(&library.album, &library.albums, &library.album_ids),
            Query::ArtistAlbums(artist) => Self::artist_albums(library, *artist),
            Query::Folder(parent) => Self::folder(&library.playlists(), *parent),
            Query::Histories => self.histories(library),
            Query::Years => Self::date_parts(library, "", 0..4, true),
            Query::Months(year) => Self::date_parts(library, &date_prefix(*year, None, None), 5..7, false),
            Query::Days { year, month } => Self::date_parts(library, &date_prefix(*year, Some(*month), None), 8..10, false),
            Query::Tracks { scope, sort } => {
                self.tracks(library, scope, *sort, self.source.key_order(), today)
            }
        }
    }

    fn track_id(library: &Library, row: rbl_index::Row) -> Option<u32> {
        library
            .ids
            .get(row as usize)
            .and_then(|&id| u32::try_from(id).ok())
    }

    /// The Camelot key id a player uses, 1–24, or 0 for a key the wheel does
    /// not know.
    fn key_id(library: &Library, row: rbl_index::Row) -> u32 {
        player_key_id(library.key_name(row))
    }

    fn displayed_key_name(&self, library: &Library, row: rbl_index::Row) -> String {
        let name = library.key_name(row);
        if self.source.key_notation() == KeyNotation::Classic {
            return name.to_owned();
        }

        let key = player_key_id(name);
        if key == 0 {
            String::new()
        } else {
            camelot_name(key)
        }
    }

    fn dense_id(wire_ids: &[u32], wire_id: u32) -> u32 {
        wire_ids
            .iter()
            .position(|&id| id == wire_id)
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(NO_ID)
    }

    fn wire_id(wire_ids: &[u32], dense_id: u32) -> u32 {
        if dense_id == NO_ID {
            return 0;
        }

        wire_ids.get(dense_id as usize).copied().unwrap_or(0)
    }

    fn sort_column(sort: Sort, scope: &TrackScope, key_order: KeyOrder) -> Option<SortColumn> {
        Some(match sort {
            // A playlist or history keeps its own order; every other list
            // is alphabetical, which is what rekordbox sent for TRACK.
            Sort::Default => match scope {
                TrackScope::FileName
                | TrackScope::Playlist(_)
                | TrackScope::History(_)
                | TrackScope::TagList => return None,
                _ if album_constrained(scope) => return None,
                _ => SortColumn::Title,
            },
            Sort::Alphabet => SortColumn::Title,
            Sort::Artist => SortColumn::Artist,
            Sort::Album => SortColumn::Album,
            Sort::Bpm => SortColumn::Bpm,
            Sort::Rating => SortColumn::Rating,
            Sort::Genre => SortColumn::Genre,
            Sort::Label => SortColumn::Label,
            Sort::DateAdded => SortColumn::DateAdded,
            Sort::DjPlayCount => SortColumn::PlayCount,
            Sort::Key => {
                if key_order == KeyOrder::Alphabetical {
                    SortColumn::Key
                } else {
                    SortColumn::KeyCamelot
                }
            }
        })
    }

    /// The rows of a scope, in the order the library holds them, each with
    /// the position a list gives it (0 where the list has none).
    #[allow(clippy::too_many_lines, reason = "one match arm per protocol track scope")]
    fn scope_rows(
        &self,
        library: &Library,
        scope: &TrackScope,
        today: &Date,
    ) -> Vec<(rbl_index::Row, u32)> {
        let all = || (0..u32::try_from(library.len()).unwrap_or(u32::MAX)).map(|row| (row, 0));
        match scope {
            TrackScope::All => all().collect(),
            TrackScope::FileName => library
                .filename_rows()
                .into_iter()
                .map(|row| (row, 0))
                .collect(),
            TrackScope::Matching(seed) => self
                .source
                .matching_ids(*seed)
                .into_iter()
                .filter_map(|id| library.row_of_id(u64::from(id)).map(|row| (row, 0)))
                .collect(),
            TrackScope::Bpm {
                bpm_x100,
                tolerance_pct,
            } => all()
                .filter(|&(row, _)| library.bpm_matches(row, *bpm_x100, *tolerance_pct))
                .collect(),
            TrackScope::Rating(rating) => all()
                .filter(|&(row, _)| library.rating_matches(row, *rating))
                .collect(),
            TrackScope::Bitrate(bitrate) => all()
                .filter(|&(row, _)| library.bitrate_matches(row, *bitrate))
                .collect(),
            TrackScope::Color(color) => all()
                .filter(|&(row, _)| library.color_matches(row, *color))
                .collect(),
            TrackScope::DurationMinute(minute) => all()
                .filter(|&(row, _)| library.duration_minute_matches(row, *minute))
                .collect(),
            TrackScope::ReleaseYear { decade, year } => all()
                .filter(|&(row, _)| match year {
                    Some(year) => library.release_year_matches(row, *year),
                    None => library
                        .year
                        .get(row as usize)
                        .copied()
                        .map(u32::from)
                        .is_some_and(|year| year != 0 && year <= 2999 && year / 10 * 10 == *decade),
                })
                .collect(),
            TrackScope::TagList => library
                .tag_list()
                .into_iter()
                .enumerate()
                .map(|(i, row)| (row, u32::try_from(i + 1).unwrap_or(u32::MAX)))
                .collect(),
            TrackScope::Genre {
                genre,
                artist,
                album,
            } => {
                let genre = Self::dense_id(&library.genre_ids, *genre);
                let artist = artist.map(|id| Self::dense_id(&library.artist_ids, id));
                let album = album.map(|id| Self::dense_id(&library.album_ids, id));
                all()
                    .filter(|&(row, _)| {
                        let at = row as usize;
                        library.genre.get(at) == Some(&genre)
                            && artist.is_none_or(|id| library.artist.get(at) == Some(&id))
                            && album.is_none_or(|id| library.album.get(at) == Some(&id))
                    })
                    .collect()
            }
            TrackScope::Label {
                label,
                artist,
                album,
            } => {
                let label = Self::dense_id(&library.label_ids, *label);
                let artist = artist.map(|id| Self::dense_id(&library.artist_ids, id));
                let album = album.map(|id| Self::dense_id(&library.album_ids, id));
                all()
                    .filter(|&(row, _)| {
                        let at = row as usize;
                        library.label.get(at) == Some(&label)
                            && artist.is_none_or(|id| library.artist.get(at) == Some(&id))
                            && album.is_none_or(|id| library.album.get(at) == Some(&id))
                    })
                    .collect()
            }
            TrackScope::ArtistRole { role, artist, album } => {
                let album = album.map(|id| Self::dense_id(&library.album_ids, id));
                self.source.artist_role_track_ids(*role, *artist).into_iter()
                    .filter_map(|id| library.row_of_id(u64::from(id)))
                    .filter(|&row| album.is_none_or(|id| library.album.get(row as usize) == Some(&id)))
                    .map(|row| (row, 0))
                    .collect()
            }
            TrackScope::Artist { artist, album } => {
                let artist = Self::dense_id(&library.artist_ids, *artist);
                let album = album.map(|id| Self::dense_id(&library.album_ids, id));
                all()
                    .filter(|&(row, _)| {
                        library.artist.get(row as usize) == Some(&artist)
                            && album.is_none_or(|a| library.album.get(row as usize) == Some(&a))
                    })
                    .collect()
            }
            TrackScope::Album(album) => {
                let album = Self::dense_id(&library.album_ids, *album);
                all()
                    .filter(|&(row, _)| library.album.get(row as usize) == Some(&album))
                    .collect()
            }
            TrackScope::Key { key, distance } => {
                let wanted: HashSet<u32> = keys::related(*key, *distance).into_iter().collect();
                all()
                    .filter(|&(row, _)| wanted.contains(&Self::key_id(library, row)))
                    .collect()
            }
            TrackScope::Playlist(id) => {
                let playlists = library.playlists();
                let Some(index) = playlists.index_of(u64::from(*id)) else {
                    return Vec::new();
                };
                let rows = if playlists.is_smart(index) {
                    library.source_rows_unlocked_on(
                        &playlists,
                        &TrackSource::SmartPlaylist(index),
                        today,
                    )
                } else {
                    playlists.members.get(index).cloned().unwrap_or_default()
                };
                rows.into_iter()
                    .enumerate()
                    .map(|(i, row)| (row, u32::try_from(i + 1).unwrap_or(u32::MAX)))
                    .collect()
            }
            TrackScope::History(id) => {
                let histories = library.histories();
                let Some(index) = histories.index_of(u64::from(*id)) else {
                    return Vec::new();
                };
                histories
                    .members
                    .get(index)
                    .map(|rows| {
                        rows.iter()
                            .enumerate()
                            .map(|(i, &row)| (row, u32::try_from(i + 1).unwrap_or(u32::MAX)))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            TrackScope::DateAdded { year, month, day } => {
                let prefix = date_prefix(*year, *month, *day);
                all()
                    .filter(|&(row, _)| library.date_added.get(row as usize).starts_with(&prefix))
                    .collect()
            }
            TrackScope::Search(text) => {
                let view = library.open_view(&ViewSpec {
                    source: TrackSource::Collection,
                    sort: SortColumn::Title,
                    descending: false,
                    query: text.clone(),
                    filter: rbl_index::TrackFilter::default(),
                });
                view.rows.iter().map(|&row| (row, 0)).collect()
            }
        }
    }

    fn tracks(
        &self,
        library: &Library,
        scope: &TrackScope,
        sort: Sort,
        key_order: KeyOrder,
        today: &Date,
    ) -> Vec<Row> {
        let mut rows = self.scope_rows(library, scope, today);
        if track_number_visible(scope) {
            for (row, position) in &mut rows {
                *position = Self::track_id(library, *row)
                    .and_then(|id| self.source.details(&id.to_string()))
                    .map_or(0, |details| details.track_number);
            }
        }

        if album_constrained(scope) && sort == Sort::Default {
            rows.sort_by_key(|&(row, position)| (position == 0, position, row));
        }
        if let Some(column) = Self::sort_column(sort, scope, key_order) {
            let mut order: Vec<rbl_index::Row> = rows.iter().map(|&(row, _)| row).collect();
            library.sort_rows(&mut order, column, false);
            // Positions travel with their rows; the sort reorders the pairs.
            let mut position_of = std::collections::HashMap::with_capacity(rows.len());
            for (row, position) in rows {
                position_of.insert(row, position);
            }
            rows = order
                .into_iter()
                .map(|row| (row, position_of.get(&row).copied().unwrap_or(0)))
                .collect();
        }
        rows.into_iter()
            .filter_map(|(row, position)| {
                Some(Row::Track {
                    id: Self::track_id(library, row)?,
                    position,
                })
            })
            .collect()
    }

    /// The names of a lookup column that at least one track uses, sorted
    /// as the browser sorts them, as `(id, name)` rows.
    fn named(
        column: &[u32],
        interner: &rbl_index::strings::Interner,
        wire_ids: &[u32],
    ) -> Vec<Row> {
        let mut used = vec![false; interner.len()];
        for &id in column {
            if let Some(slot) = used.get_mut(id as usize) {
                *slot = true;
            }
        }
        let mut ids: Vec<u32> = (0..u32::try_from(interner.len()).unwrap_or(u32::MAX))
            .filter(|&id| {
                used.get(id as usize).copied().unwrap_or(false) && !interner.name(id).is_empty()
            })
            .collect();
        ids.sort_by(|&a, &b| {
            interner
                .folded(a)
                .cmp(interner.folded(b))
                .then_with(|| a.cmp(&b))
        });
        ids.into_iter()
            .map(|id| Row::Named {
                id: Self::wire_id(wire_ids, id),
                name: interner.name(id).to_owned(),
            })
            .collect()
    }

    fn artist_albums(library: &Library, artist: u32) -> Vec<Row> {
        let artist = Self::dense_id(&library.artist_ids, artist);
        let albums: Vec<u32> = library
            .artist
            .iter()
            .zip(&library.album)
            .filter(|&(&a, _)| a == artist)
            .map(|(_, &album)| album)
            .collect();
        Self::album_rows(library, albums)
    }

    fn album_rows(library: &Library, mut albums: Vec<u32>) -> Vec<Row> {
        albums.sort_unstable();
        albums.dedup();
        albums.sort_by(|&a, &b| {
            let name = |id| {
                if id == NO_ID {
                    "unknown"
                } else {
                    library.albums.folded(id)
                }
            };
            name(a).cmp(name(b)).then_with(|| a.cmp(&b))
        });
        albums
            .into_iter()
            .filter(|&album| album == NO_ID || !library.albums.name(album).is_empty())
            .map(|album| Row::Named {
                id: Self::wire_id(&library.album_ids, album),
                name: if album == NO_ID {
                    "Unknown".to_owned()
                } else {
                    library.albums.name(album).to_owned()
                },
            })
            .collect()
    }

    fn genre_artists(library: &Library, genre: u32) -> Vec<Row> {
        let genre = Self::dense_id(&library.genre_ids, genre);
        let artists: Vec<u32> = library
            .genre
            .iter()
            .zip(&library.artist)
            .filter(|&(&track_genre, &artist)| track_genre == genre && artist != NO_ID)
            .map(|(_, &artist)| artist)
            .collect();
        Self::named(&artists, &library.artists, &library.artist_ids)
    }

    fn genre_albums(library: &Library, genre: u32, artist: Option<u32>) -> Vec<Row> {
        let genre = Self::dense_id(&library.genre_ids, genre);
        let include_unknown = artist.is_some();
        let artist = artist.map(|id| Self::dense_id(&library.artist_ids, id));
        let mut albums: Vec<u32> = library
            .genre
            .iter()
            .zip(&library.artist)
            .zip(&library.album)
            .filter(|&((&track_genre, &track_artist), _)| {
                track_genre == genre && artist.is_none_or(|id| track_artist == id)
            })
            .map(|(_, &album)| album)
            .collect();
        if !include_unknown {
            albums.retain(|&album| album != NO_ID);
        }
        Self::album_rows(library, albums)
    }

    fn label_artists(library: &Library, label: u32) -> Vec<Row> {
        let label = Self::dense_id(&library.label_ids, label);
        let artists: Vec<u32> = library
            .label
            .iter()
            .zip(&library.artist)
            .filter(|&(&track_label, &artist)| track_label == label && artist != NO_ID)
            .map(|(_, &artist)| artist)
            .collect();
        Self::named(&artists, &library.artists, &library.artist_ids)
    }

    fn label_albums(library: &Library, label: u32, artist: Option<u32>) -> Vec<Row> {
        let label = Self::dense_id(&library.label_ids, label);
        let include_unknown = artist.is_some();
        let artist = artist.map(|id| Self::dense_id(&library.artist_ids, id));
        let mut albums: Vec<u32> = library
            .label
            .iter()
            .zip(&library.artist)
            .zip(&library.album)
            .filter(|&((&track_label, &track_artist), _)| {
                track_label == label && artist.is_none_or(|id| track_artist == id)
            })
            .map(|(_, &album)| album)
            .collect();
        if !include_unknown {
            albums.retain(|&album| album != NO_ID);
        }
        Self::album_rows(library, albums)
    }

    fn role_albums(&self, library: &Library, role: ArtistRole, artist: u32) -> Vec<Row> {
        let albums = self
            .source
            .artist_role_track_ids(role, artist)
            .into_iter()
            .filter_map(|id| library.row_of_id(u64::from(id)))
            .filter_map(|row| library.album.get(row as usize).copied())
            .collect();
        Self::album_rows(library, albums)
    }

    /// A folder's children, folders first and then lists, in `Seq` order.
    fn folder(lists: &rbl_index::Playlists, parent: u32) -> Vec<Row> {
        let parent_index = if parent == 0 {
            NO_ID
        } else {
            match lists.index_of(u64::from(parent)) {
                Some(index) => u32::try_from(index).unwrap_or(NO_ID),
                None => return Vec::new(),
            }
        };
        let mut children: Vec<usize> = (0..lists.len())
            .filter(|&i| lists.parent.get(i).copied() == Some(parent_index))
            .collect();
        children.sort_by_key(|&i| {
            (
                !lists.is_folder(i),
                lists.seq.get(i).copied().unwrap_or(0),
            )
        });
        children
            .into_iter()
            .filter_map(|i| {
                Some(Row::List {
                    id: u32::try_from(*lists.ids.get(i)?).ok()?,
                    name: lists.name(i).to_owned(),
                    folder: lists.is_folder(i),
                    position: lists.seq.get(i).copied().unwrap_or(0),
                })
            })
            .collect()
    }

    /// The HISTORY menu: the link session's own history and nothing else,
    /// as rekordbox 7.2.11 answers it (`PSvAppSyncDBIF::getHistory_Root`
    /// selects the one session by id; before a player has added a track
    /// the id is -1 and the menu is empty).
    fn histories(&self, library: &Library) -> Vec<Row> {
        let history = self.link_history.lock();
        let Some(session) = history.session_in(library) else {
            return Vec::new();
        };
        let lists = library.histories();
        let Some(index) = lists.index_of(u64::from(session)) else {
            return Vec::new();
        };
        vec![Row::Named {
            id: session,
            name: lists.name(index).to_owned(),
        }]
    }

    /// The distinct values of one part of the date-added column under a
    /// prefix: years (newest first), or months and days (ascending).
    fn date_parts(
        library: &Library,
        prefix: &str,
        at: std::ops::Range<usize>,
        newest_first: bool,
    ) -> Vec<Row> {
        let mut values: Vec<u32> = (0..library.len())
            .filter_map(|row| {
                let date = library.date_added.get(row);
                if !date.starts_with(prefix) {
                    return None;
                }
                date.get(at.clone())?.parse().ok()
            })
            .collect();
        values.sort_unstable();
        values.dedup();
        if newest_first {
            values.reverse();
        }
        values
            .into_iter()
            .filter(|&v| v != 0)
            .map(Row::Date)
            .collect()
    }

    /// The parsed analysis files of a track, from the cache or the disk.
    fn parsed(&self, library: &Library, row: rbl_index::Row) -> Option<Arc<Parsed>> {
        {
            let mut cache = self.analysis.lock();
            if let Some(at) = cache.iter().position(|p| p.row == row) {
                let hit = cache.remove(at);
                cache.push(Arc::clone(&hit));
                return Some(hit);
            }
        }
        let relative = library.analysis_path.get(row as usize);
        if relative.is_empty() {
            return None;
        }
        let dat_path = rbl_anlz::resolve(&self.source.share_root(), relative);
        let read = |path: &std::path::Path| {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| rbl_anlz::parse(&bytes).ok())
        };
        let parsed = Arc::new(Parsed {
            row,
            dat: read(&dat_path),
            ext: read(&rbl_anlz::sibling(&dat_path, "EXT")),
            two_ex: read(&rbl_anlz::sibling(&dat_path, "2EX")),
        });
        let mut cache = self.analysis.lock();
        if cache.len() >= ANALYSIS_CACHE {
            cache.remove(0);
        }
        cache.push(Arc::clone(&parsed));
        Some(parsed)
    }

    /// Forgets parsed analysis files, for after a library reload or an
    /// analysis run: the next request reads the files again.
    pub fn forget_analysis(&self) {
        self.analysis.lock().clear();
    }
}

/// A `djmdMyTag.ID` as a player sees it: the 32 bits of the id the index
/// reads the track's tags as ([`rbl_index::smart::my_tag_key`]), so a
/// category's tags and a track's tags compare as rekordbox compares them.
/// rekordbox sends these ids as unsigned numbers (tag 108516952 in the #316
/// captures).
fn wire_my_tag_id(text: &str) -> u32 {
    u32::from_ne_bytes(rbl_index::smart::my_tag_key(text).to_ne_bytes())
}

fn album_constrained(scope: &TrackScope) -> bool {
    matches!(
        scope,
        TrackScope::Album(_)
            | TrackScope::Artist { album: Some(_), .. }
            | TrackScope::Genre { album: Some(_), .. }
            | TrackScope::Label { album: Some(_), .. }
    )
}

fn track_number_visible(scope: &TrackScope) -> bool {
    album_constrained(scope) || matches!(scope, TrackScope::Matching(_))
}

fn color_name(id: u32) -> &'static str {
    match id {
        1 => "Pink",
        2 => "Red",
        3 => "Orange",
        4 => "Yellow",
        5 => "Green",
        6 => "Aqua",
        7 => "Blue",
        8 => "Purple",
        _ => "",
    }
}

fn secondary_column(
    library: &Library,
    row: rbl_index::Row,
    column: TrackColumn,
    preserve_cached_text: bool,
    key_name: &str,
    details: Option<&rbl_db::details::TrackDetails>,
) -> (String, u32) {
    let at = row as usize;
    let bpm = library.bpm_x100.get(at).copied().unwrap_or(0);
    let named = |name: &str, value: u32| (name.to_owned(), value);

    match column {
        TrackColumn::Title => (String::new(), 0),
        TrackColumn::Album => details.map_or_else(
            || named(library.album_name(row), 0),
            |d| named(&d.album, d.album_id),
        ),
        TrackColumn::Genre => details.map_or_else(
            || named(library.genre_name(row), 0),
            |d| named(&d.genre, d.genre_id),
        ),
        TrackColumn::Artist => details.map_or_else(
            || named(library.artist_name(row), 0),
            |d| named(&d.artist, d.artist_id),
        ),
        TrackColumn::Rating => {
            let rating = u32::from(library.rating.get(at).copied().unwrap_or(0));
            let text = if preserve_cached_text {
                "★".repeat(rating as usize)
            } else {
                String::new()
            };
            (text, rating)
        }
        TrackColumn::Duration => {
            let seconds = library.length_sec.get(at).copied().unwrap_or(0);
            let text = if preserve_cached_text {
                format!("{}:{:02}", seconds / 60, seconds % 60)
            } else {
                String::new()
            };
            (text, seconds)
        }
        TrackColumn::Bpm => (
            if preserve_cached_text { format_bpm(bpm) } else { String::new() },
            bpm,
        ),
        TrackColumn::Label => details.map_or_else(
            || named(library.label_name(row), 0),
            |d| named(&d.label, d.label_id),
        ),
        TrackColumn::Key => {
            let value = details.map_or(0, |d| d.key_id);
            let text = if bpm == 0 || !preserve_cached_text {
                key_name.to_owned()
            } else {
                format!("{} - {}", key_name, format_bpm(bpm))
            };
            (text, value)
        }
        TrackColumn::Bitrate => {
            let bitrate = library.bitrate.get(at).copied().unwrap_or(0);
            let text = if preserve_cached_text {
                format_nonzero(bitrate, " kbps")
            } else {
                String::new()
            };
            (text, bitrate)
        }
        TrackColumn::Color => {
            let color = u32::from(library.color.get(at).copied().unwrap_or(0));
            (color_name(color).to_owned(), color)
        }
        TrackColumn::Comment => (library.comment.get(at).to_owned(), 0),
        TrackColumn::OriginalArtist => details.map_or_else(
            || (String::new(), 0),
            |d| named(&d.original_artist, d.original_artist_id),
        ),
        TrackColumn::Remixer => {
            details.map_or_else(|| (String::new(), 0), |d| named(&d.remixer, d.remixer_id))
        }
        TrackColumn::DjPlayCount => {
            let count = u32::from(library.play_count.get(at).copied().unwrap_or(0));
            let text = if preserve_cached_text { count.to_string() } else { String::new() };
            (text, count)
        }
        TrackColumn::DateAdded => (library.date_added.get(at).to_owned(), 0),
    }
}

fn format_bpm(bpm_x100: u32) -> String {
    if bpm_x100 == 0 {
        String::new()
    } else {
        let tenths = bpm_x100.saturating_add(5) / 10;
        format!("{}.{:01} bpm", tenths / 10, tenths % 10)
    }
}

fn format_nonzero(value: u32, suffix: &str) -> String {
    if value == 0 {
        String::new()
    } else {
        format!("{value}{suffix}")
    }
}

fn camelot_name(id: u32) -> String {
    let side = if id % 2 == 1 { 'A' } else { 'B' };
    format!("{}{side}", id.div_ceil(2))
}

/// The key id rekordbox puts on a track row. Besides exact key names it
/// accepts a case-sensitive musical-key prefix after folding fullwidth ASCII.
fn player_key_id(name: &str) -> u32 {
    let rank = camelot_rank(name);
    if rank != u32::MAX {
        return rank + 1;
    }

    let normalized: String = name
        .chars()
        .map(|character| match character {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(u32::from(character) - 0xfee0).unwrap_or(character),
            _ => character,
        })
        .collect();
    (1..=24)
        .find(|&id| normalized.starts_with(keys::name(id)))
        .unwrap_or(0)
}

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD` as a prefix of `StockDate`.
fn date_prefix(year: u32, month: Option<u32>, day: Option<u32>) -> String {
    match (month, day) {
        (Some(m), Some(d)) => format!("{year:04}-{m:02}-{d:02}"),
        (Some(m), None) => format!("{year:04}-{m:02}"),
        _ => format!("{year:04}"),
    }
}

impl Catalog for IndexCatalog {
    fn root_categories(&self) -> Vec<RootCategory> {
        self.source.root_categories()
    }

    fn sorts(&self) -> Vec<Sort> {
        self.source.sorts()
    }

    fn key_ids(&self) -> Vec<u32> {
        let mut ids: Vec<_> = (1..=24).collect();
        if self.source.key_order() == KeyOrder::Alphabetical {
            ids.sort_by(|a, b| rbl_index::key::cmp_names(keys::name(*a), keys::name(*b)));
        }
        ids
    }

    fn key_name(&self, id: u32) -> String {
        match self.source.key_notation() {
            KeyNotation::Classic => keys::name(id).to_owned(),
            KeyNotation::Alphanumeric if (1..=24).contains(&id) => camelot_name(id),
            KeyNotation::Alphanumeric => String::new(),
        }
    }

    fn list(&self, query: &Query) -> Vec<Row> {
        let Some(library) = self.source.library() else {
            return Vec::new();
        };
        self.cached_list(&library, query)
    }

    fn track_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
        let library = self.source.library()?;
        let row = Self::row_of(&library, id)?;
        let at = row as usize;
        let configured_column = self.source.track_column();
        let column = column.unwrap_or(configured_column);
        let details = self.source.details(&id.to_string());
        let key = Self::key_id(&library, row);
        let key_name = self.displayed_key_name(&library, row);
        let (secondary_text, column_value) = secondary_column(
            &library,
            row,
            column,
            column == configured_column,
            &key_name,
            details.as_ref(),
        );
        Some(TrackRow {
            id,
            title: library.title.get(at).to_owned(),
            secondary_text,
            column,
            column_value,
            key,
            key_id: details
                .filter(|_| !library.key_name(row).is_empty())
                .map_or(0, |details| details.key_id),
            key_name,
            bpm_x100: library.bpm_x100.get(at).copied().unwrap_or(0),
        })
    }

    fn file_name_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
        let library = self.source.library()?;
        let row = Self::row_of(&library, id)?;
        let mut item = self.track_row(id, column)?;
        library.file_name.get(row as usize).clone_into(&mut item.title);
        Some(item)
    }

    fn track(&self, id: u32) -> Option<TrackDetails> {
        let library = self.source.library()?;
        let row = Self::row_of(&library, id)?;
        let at = row as usize;
        let lookup_id = |column: &[u32], wire_ids: &[u32]| {
            column
                .get(at)
                .map_or(0, |&dense| Self::wire_id(wire_ids, dense))
        };
        let path = library.folder_path.get(at).to_owned();
        // The database's row for what the index leaves out; a read that
        // fails costs those fields, not the reply.
        let details = self.source.details(&id.to_string());
        let file_size = details
            .as_ref()
            .map(|d| d.file_size)
            .filter(|&size| size > 0)
            .or_else(|| std::fs::metadata(&path).ok().map(|m| m.len()))
            .unwrap_or(0);
        let track_row = self.track_row(id, None)?;
        let key_name = track_row.key_name.clone();
        Some(TrackDetails {
            row: track_row,
            comment: library.comment.get(at).to_owned(),
            key_id: details.as_ref().map_or(0, |d| d.key_id),
            key_name,
            artist_id: lookup_id(&library.artist, &library.artist_ids),
            artist: library.artist_name(row).to_owned(),
            album_id: lookup_id(&library.album, &library.album_ids),
            album: library.album_name(row).to_owned(),
            duration_s: library.length_sec.get(at).copied().unwrap_or(0),
            rating: u32::from(library.rating.get(at).copied().unwrap_or(0)),
            colour: u32::from(library.color.get(at).copied().unwrap_or(0)),
            genre_id: lookup_id(&library.genre, &library.genre_ids),
            genre: library.genre_name(row).to_owned(),
            date_added: library.date_added.get(at).to_owned(),
            year: details.as_ref().map_or(0, |d| d.year),
            bit_rate_kbps: details.as_ref().map_or(0, |d| d.bitrate),
            label_id: lookup_id(&library.label, &library.label_ids),
            label: library.label_name(row).to_owned(),
            original_artist: details
                .as_ref()
                .map(|d| d.original_artist.clone())
                .unwrap_or_default(),
            remixer: details
                .as_ref()
                .map(|d| d.remixer.clone())
                .unwrap_or_default(),
            path,
            file_size: u32::try_from(file_size).unwrap_or(u32::MAX),
            file_type: details.as_ref().map_or(0, |d| d.file_type),
        })
    }

    fn artwork(&self, id: u32, size: ArtworkSize) -> Option<Vec<u8>> {
        let library = self.source.library()?;
        let row = Self::item_row(&library, id)?;
        self.artwork_at(&library, row, size)
    }

    fn analysis(&self, track: u32, what: &Wanted) -> Option<Vec<u8>> {
        let library = self.source.library()?;
        let row = Self::row_of(&library, track)?;
        match what {
            // [ASSUME] Preserve the existing VBR compatibility placeholder.
            // Track-specific retrieval and unavailable/client behavior remain
            // blocked in issue 13; this does not describe the track's cues.
            Wanted::Vbr => Some(blobs::vbr_compatibility_blob()),
            Wanted::ExtendedCueList => {
                let cues: Vec<ExtendedCue> =
                    library.cues_of(row).iter().map(ExtendedCue::from).collect();
                Some(blobs::extended_cues_blob(&cues).0)
            }
            _ => {
                let parsed = self.parsed(&library, row)?;
                let analysis = Analysis {
                    dat: parsed.dat.as_ref(),
                    ext: parsed.ext.as_ref(),
                    two_ex: parsed.two_ex.as_ref(),
                };
                match what {
                    Wanted::BeatGrid => analysis.beat_grid(),
                    Wanted::WaveformPreview => analysis.waveform_preview(),
                    Wanted::WaveformDetail => analysis.waveform_detail(),
                    Wanted::Tag { fourcc, extension } => analysis.tag(fourcc, extension),
                    Wanted::Vbr | Wanted::ExtendedCueList => None,
                }
            }
        }
    }

    fn hot_cue_banks(&self, parent: Option<u32>) -> Vec<HotCueBank> {
        self.source.hot_cue_banks(parent).into_iter().map(|bank| HotCueBank {
            id: bank.id,
            name: bank.name,
            folder: bank.folder,
        }).collect()
    }

    fn hot_cue_bank_cues(&self, bank: u32) -> Vec<HotCueBankCue> {
        self.source.hot_cue_bank_cues(bank).into_iter().map(|cue| HotCueBankCue {
            slot: cue.slot,
            content: cue.content,
            in_ms: cue.in_ms,
            out_ms: cue.out_ms,
            color: cue.color,
            color_table_index: cue.color_table_index,
            active_loop: cue.active_loop,
            beat_loop_size: cue.beat_loop_size,
            cue_microsec: cue.cue_microsec,
        }).collect()
    }

    fn hot_cue_bank_tracks(&self, bank: u32) -> Vec<TrackRow> {
        self.source.hot_cue_bank_track_ids(bank).into_iter()
            .filter_map(|id| self.track_row(id, None))
            .collect()
    }

    fn usb_cues(&self, track: u32) -> Vec<UsbCue> {
        let Some(library) = self.source.library() else { return Vec::new() };
        let Some(row) = Self::row_of(&library, track) else { return Vec::new() };
        library.cues_of(row).into_iter().filter_map(|cue| {
            let slot = cue.hot_letter().map_or(0, |letter| letter as u8 - b'A' + 1);
            // The old USB cue record only has three hot-cue slots.  Later
            // hot cues are carried by the extended cue protocol instead.
            (slot <= 3).then_some(UsbCue {
                slot,
                in_ms: cue.position_ms,
                out_ms: (cue.out_ms > cue.position_ms).then_some(cue.out_ms),
                color_table_index: u32::from(cue.colour),
            })
        }).collect()
    }

    fn grid_offset(&self, track: u32) -> i16 {
        let offset = || {
            let library = self.source.library()?;
            let parsed = self.parsed(&library, Self::row_of(&library, track)?)?;
            parsed.dat.as_ref()?.grid_offset()
        };
        offset().unwrap_or(0)
    }

    fn edit(&self, edit: &Edit) -> bool {
        if matches!(
            edit,
            Edit::HistoryAdd { .. } | Edit::HistoryRemove { .. } | Edit::HistoryDelete { .. }
        ) {
            return self.history_edit(edit);
        }
        let success = self.source.edit(edit);
        if success && matches!(edit, Edit::GridOffset { .. }) {
            self.forget_analysis();
        }
        success
    }

    fn tagged(&self, track: u32) -> bool {
        self.source.library().is_some_and(|lib| {
            Self::row_of(&lib, track).is_some_and(|row| lib.tag_list().contains(&row))
        })
    }

    fn listed(&self, scope: &TrackScope) -> bool {
        if let TrackScope::Playlist(id) = scope {
            let Some(library) = self.source.library() else {
                return true;
            };
            let playlists = library.playlists();
            let Some(index) = playlists.index_of(u64::from(*id)) else {
                return true;
            };

            return !playlists.is_smart(index);
        }

        matches!(
            scope,
            TrackScope::Genre { .. }
                | TrackScope::Label { .. }
                | TrackScope::Artist { .. }
                | TrackScope::Album(_)
                | TrackScope::TagList
        )
    }

    fn filter_rows(&self, rows: &mut Vec<Row>, filter: &rbl_dbserver::filter::TrackFilter) {
        if !filter.enabled {
            return;
        }
        let Some(lib) = self.source.library() else {
            rows.clear();
            return;
        };
        let tags = filter.has_my_tag_conditions();
        rows.retain(|row| match row {
            Row::Track { id, .. } => Self::row_of(&lib, *id).is_some_and(|row| {
                let at = row as usize;
                filter.matches(
                    lib.bpm_x100[at],
                    Self::key_id(&lib, row),
                    u32::from(lib.rating[at]),
                    u32::from(lib.color[at]),
                ) && (!tags || filter.matches_my_tags(&Self::wire_my_tags(&lib, row)))
            }),
            _ => true,
        });
    }

    fn my_tags(&self) -> Vec<MyTag> {
        self.source
            .my_tags()
            .into_iter()
            .map(|row| MyTag {
                id: wire_my_tag_id(&row.id),
                // A category's parent is `root`, which reads as 0.
                parent: wire_my_tag_id(&row.parent),
                seq: u32::try_from(row.seq).unwrap_or(0),
                name: row.name,
            })
            .collect()
    }

    fn track_my_tags(&self, track: u32) -> Vec<u32> {
        self.source
            .library()
            .and_then(|lib| Self::row_of(&lib, track).map(|row| Self::wire_my_tags(&lib, row)))
            .unwrap_or_default()
    }

    fn my_tags_in_use(&self) -> HashSet<u32> {
        let Some(lib) = self.source.library() else {
            return HashSet::new();
        };
        (0..lib.bpm_x100.len())
            .flat_map(|row| lib.my_tag_keys(row).iter().map(|&key| u32::from_ne_bytes(key.to_ne_bytes())))
            .collect()
    }

    fn played(&self, track: u32) -> bool {
        self.played.contains(track)
    }
}

/// The files to try for artwork of `size`, first first: the sized files
/// beside the one `ImagePath` names, then that one.
fn artwork_candidates(relative: &str, size: ArtworkSize) -> Vec<String> {
    let mut candidates = Vec::with_capacity(3);
    if size == ArtworkSize::Medium {
        candidates.push(sized_artwork(relative, MEDIUM_ARTWORK_SUFFIX));
    }
    candidates.push(sized_artwork(relative, SMALL_ARTWORK_SUFFIX));
    candidates.push(relative.to_owned());
    candidates
}

/// The sized file beside the artwork `ImagePath` names: `suffix` before the
/// extension. A path with no extension is returned as it is.
fn sized_artwork(relative: &str, suffix: &str) -> String {
    let stem_end = relative
        .rfind('.')
        .filter(|&dot| !relative[dot..].contains(['/', '\\']));
    match stem_end {
        Some(dot) => format!(
            "{}{suffix}{}",
            &relative[..dot],
            &relative[dot..]
        ),
        None => relative.to_owned(),
    }
}

/// `share_root/relative`, refused if the relative path climbs out.
fn resolve_under(share: &std::path::Path, relative: &str) -> Option<PathBuf> {
    let relative = relative.trim_start_matches(['/', '\\']);
    if relative.split(['/', '\\']).any(|part| part == "..") {
        return None;
    }
    Some(share.join(relative))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rbl_index::Cue;
    use rbl_index::testing::{
        add_folder, add_history, add_playlist, add_smart_playlist_with_members, library_from,
        TestTrack,
    };

    struct Fixed(Arc<Library>);

    impl Source for Fixed {
        fn library(&self) -> Option<Arc<Library>> {
            Some(Arc::clone(&self.0))
        }
        fn share_root(&self) -> PathBuf {
            PathBuf::from("/nonexistent")
        }
        fn details(&self, _id: &str) -> Option<rbl_db::details::TrackDetails> {
            None
        }
    }

    struct Clocked {
        library: Arc<Library>,
        today: Mutex<Date>,
    }

    impl Source for Clocked {
        fn library(&self) -> Option<Arc<Library>> {
            Some(Arc::clone(&self.library))
        }
        fn today(&self) -> Date {
            *self.today.lock()
        }
        fn share_root(&self) -> PathBuf {
            PathBuf::from("/nonexistent")
        }
        fn details(&self, _id: &str) -> Option<rbl_db::details::TrackDetails> {
            None
        }
    }

    fn library() -> Library {
        let t = |id, title, artist, album, genre, label, key, bpm, date, play_count| TestTrack {
            id,
            title,
            artist,
            album,
            genre,
            label,
            key,
            bpm_x100: bpm,
            date_added: date,
            play_count,
            path: "/Volumes/SD/RB/x.mp3",
            ..TestTrack::default()
        };
        let mut lib = library_from(&[
            t(
                10,
                "Zebra",
                "Bob",
                "Second",
                "House",
                "Zulu",
                "Am",
                12_800,
                "2025-03-04",
                2,
            ),
            t(
                11,
                "Apple",
                "Alice",
                "First",
                "Techno",
                "Alpha",
                "Abm",
                12_000,
                "2026-01-09",
                9,
            ),
            t(
                12,
                "Mango",
                "Carol",
                "Second",
                "Ambient",
                "Mike",
                "B",
                13_000,
                "2026-01-20",
                5,
            ),
        ]);
        let folder = add_folder(&mut lib, "Crates");
        add_playlist(&mut lib, "Warm up", &[2, 0]);
        let _ = folder;
        add_history(&mut lib, "HISTORY 2026-09-01", &[1]);
        add_history(&mut lib, "HISTORY 2026-09-02", &[0]);
        lib
    }

    fn catalog() -> IndexCatalog {
        IndexCatalog::new(Arc::new(Fixed(Arc::new(library()))), Played::default())
    }

    fn ids(rows: &[Row]) -> Vec<u32> {
        rows.iter()
            .map(|r| match r {
                Row::Track { id, .. } | Row::Named { id, .. } | Row::List { id, .. } => *id,
                Row::Date(v) => *v,
            })
            .collect()
    }

    #[test]
    fn the_track_menu_is_alphabetical_by_default_and_sorts_on_request() {
        let c = catalog();
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::Default
            })),
            [11, 12, 10]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::Bpm
            })),
            [11, 10, 12]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::Artist
            })),
            [11, 10, 12]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::Genre
            })),
            [12, 10, 11]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::Label
            })),
            [11, 12, 10]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::DateAdded
            })),
            [10, 11, 12]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::DjPlayCount
            })),
            [10, 12, 11]
        );
    }

    #[test]
    fn key_notation_and_order_are_independent() {
        struct Configured {
            fixed: Fixed,
            notation: KeyNotation,
            order: KeyOrder,
        }
        impl Source for Configured {
            fn key_notation(&self) -> KeyNotation {
                self.notation
            }
            fn key_order(&self) -> KeyOrder {
                self.order
            }
            fn library(&self) -> Option<Arc<Library>> {
                self.fixed.library()
            }
            fn share_root(&self) -> PathBuf {
                self.fixed.share_root()
            }
            fn details(&self, id: &str) -> Option<rbl_db::details::TrackDetails> {
                self.fixed.details(id)
            }
        }

        for notation in [KeyNotation::Classic, KeyNotation::Alphanumeric] {
            for order in [KeyOrder::Musical, KeyOrder::Alphabetical] {
                let c = IndexCatalog::new(
                    Arc::new(Configured {
                        fixed: Fixed(Arc::new(library())),
                        notation,
                        order,
                    }),
                    Played::default(),
                );
                let expected_tracks = match order {
                    KeyOrder::Musical => [11, 12, 10],
                    KeyOrder::Alphabetical => [11, 10, 12],
                };
                assert_eq!(
                    ids(&c.list(&Query::Tracks {
                        scope: TrackScope::All,
                        sort: Sort::Key,
                    })),
                    expected_tracks,
                    "{notation:?} notation with {order:?} order",
                );

                let key_ids = c.key_ids();
                match order {
                    KeyOrder::Musical => assert_eq!(key_ids, (1..=24).collect::<Vec<_>>()),
                    KeyOrder::Alphabetical => {
                        assert_eq!(key_ids.first().copied().map(keys::name), Some("A"));
                        assert_eq!(key_ids.iter().position(|id| *id == 1), Some(2));
                    }
                }

                let expected_name = match notation {
                    KeyNotation::Classic => "Am",
                    KeyNotation::Alphanumeric => "8A",
                };
                assert_eq!(c.key_name(15), expected_name);
                let row = c.track_row(10, Some(TrackColumn::Key)).unwrap();
                assert_eq!(row.key_name, expected_name);
                assert_eq!(row.secondary_text, expected_name);
                assert_eq!(c.track(10).unwrap().key_name, expected_name);
            }
        }
    }

    #[test]
    fn custom_key_names_keep_their_notation_behavior() {
        struct Notation {
            library: Arc<Library>,
            notation: KeyNotation,
        }
        impl Source for Notation {
            fn library(&self) -> Option<Arc<Library>> {
                Some(Arc::clone(&self.library))
            }
            fn key_notation(&self) -> KeyNotation {
                self.notation
            }
            fn share_root(&self) -> PathBuf {
                PathBuf::from("/nonexistent")
            }
            fn details(&self, _: &str) -> Option<rbl_db::details::TrackDetails> {
                None
            }
        }
        let library = || {
            Arc::new(library_from(&[
                TestTrack {
                    id: 20,
                    key: "Alpha",
                    ..TestTrack::default()
                },
                TestTrack {
                    id: 21,
                    key: "Beta Alpha",
                    ..TestTrack::default()
                },
                TestTrack {
                    id: 22,
                    key: "alpha",
                    ..TestTrack::default()
                },
                TestTrack {
                    id: 23,
                    key: "Ａｌｐｈａ",
                    ..TestTrack::default()
                },
            ]))
        };

        let classic = IndexCatalog::new(
            Arc::new(Notation {
                library: library(),
                notation: KeyNotation::Classic,
            }),
            Played::default(),
        );
        assert_eq!(classic.track_row(20, None).unwrap().key_name, "Alpha");
        assert_eq!(classic.track_row(21, None).unwrap().key_name, "Beta Alpha");
        assert_eq!(classic.track_row(22, None).unwrap().key_name, "alpha");
        assert_eq!(classic.track_row(23, None).unwrap().key_name, "Ａｌｐｈａ");

        let alphanumeric = IndexCatalog::new(
            Arc::new(Notation {
                library: library(),
                notation: KeyNotation::Alphanumeric,
            }),
            Played::default(),
        );
        assert_eq!(
            alphanumeric.track_row(20, None).unwrap().key_name,
            "11B"
        );
        assert_eq!(
            alphanumeric.track_row(21, None).unwrap().key_name,
            "1B"
        );
        assert_eq!(
            alphanumeric.track_row(22, None).unwrap().key_name,
            ""
        );
        assert_eq!(
            alphanumeric.track_row(23, None).unwrap().key_name,
            "11B"
        );
        assert_eq!(alphanumeric.track(20).unwrap().key_name, "11B");
    }

    #[test]
    fn key_sort_uses_the_wheel_and_preserves_playlist_positions() {
        let c = catalog();
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::All,
                sort: Sort::Key
            })),
            [11, 12, 10]
        );
        let mut lib = library();
        let playlist = add_playlist(&mut lib, "Keys", &[0, 2, 1]);
        let playlist = u32::try_from(lib.playlists().ids[playlist]).unwrap();
        let c = IndexCatalog::new(Arc::new(Fixed(Arc::new(lib))), Played::default());
        assert_eq!(
            c.list(&Query::Tracks {
                scope: TrackScope::Playlist(playlist),
                sort: Sort::Key
            }),
            vec![
                Row::Track {
                    id: 11,
                    position: 3
                },
                Row::Track {
                    id: 12,
                    position: 2
                },
                Row::Track {
                    id: 10,
                    position: 1
                },
            ]
        );
    }

    #[test]
    fn artists_and_their_albums_preserve_database_ids() {
        // The test builder interns one row per track, as `djmdArtist` can:
        // a name under two ids is two menu rows, which is what rekordbox
        // sends too ("Aaliyah" and "Aaliyah ft. Dash!e" were separate ids).
        let mut lib = library();
        lib.artist_ids = vec![5001, 5002, 5003];
        lib.album_ids = vec![6001, 6002, 6003];
        let c = IndexCatalog::new(Arc::new(Fixed(Arc::new(lib))), Played::default());
        let artists = c.list(&Query::Artists(Sort::Default));
        assert_eq!(
            artists,
            vec![
                Row::Named {
                    id: 5002,
                    name: "Alice".into()
                },
                Row::Named {
                    id: 5001,
                    name: "Bob".into()
                },
                Row::Named {
                    id: 5003,
                    name: "Carol".into()
                },
            ]
        );
        assert_eq!(
            c.list(&Query::ArtistAlbums(5002)),
            vec![Row::Named {
                id: 6002,
                name: "First".into()
            }]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Artist {
                    artist: 5002,
                    album: Some(6002)
                },
                sort: Sort::Default
            })),
            [11]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Artist {
                    artist: 5002,
                    album: None
                },
                sort: Sort::Default
            })),
            [11]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Album(6001),
                sort: Sort::Default
            })),
            [10]
        );
    }

    #[test]
    fn labels_drill_through_artists_and_albums() {
        let c = catalog();
        assert_eq!(ids(&c.list(&Query::Labels(Sort::Default))), [2, 3, 1]);
        assert_eq!(
            c.list(&Query::LabelArtists(2)),
            vec![Row::Named {
                id: 2,
                name: "Alice".into(),
            }]
        );
        assert_eq!(
            c.list(&Query::LabelArtistAlbums {
                label: 2,
                artist: Some(2),
            }),
            vec![Row::Named {
                id: 2,
                name: "First".into(),
            }]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Label {
                    label: 2,
                    artist: Some(2),
                    album: Some(2),
                },
                sort: Sort::Default,
            })),
            [11]
        );
    }

    #[test]
    fn a_labels_all_path_keeps_tracks_without_an_album() {
        let mut lib = library();
        lib.label[2] = lib.label[1];
        lib.artist[2] = lib.artist[1];
        lib.album[2] = NO_ID;
        let c = IndexCatalog::new(Arc::new(Fixed(Arc::new(lib))), Played::default());

        assert_eq!(
            ids(&c.list(&Query::LabelArtistAlbums {
                label: 2,
                artist: None,
            })),
            [2]
        );
        assert_eq!(
            ids(&c.list(&Query::LabelArtistAlbums {
                label: 2,
                artist: Some(2),
            })),
            [2, 0]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Label {
                    label: 2,
                    artist: Some(2),
                    album: None,
                },
                sort: Sort::Default,
            })),
            [11, 12]
        );
    }

    #[test]
    fn an_albums_default_order_and_positions_use_track_numbers() {
        struct Numbered(Arc<Library>);
        impl Source for Numbered {
            fn library(&self) -> Option<Arc<Library>> {
                Some(Arc::clone(&self.0))
            }
            fn share_root(&self) -> PathBuf {
                PathBuf::from("/nonexistent")
            }
            fn details(&self, id: &str) -> Option<rbl_db::details::TrackDetails> {
                let track_number = match id {
                    "10" => 2,
                    "11" => 1,
                    _ => 0,
                };
                Some(rbl_db::details::TrackDetails {
                    track_number,
                    ..rbl_db::details::TrackDetails::default()
                })
            }
        }

        let mut lib = library();
        lib.album[1] = lib.album[0];
        let c = IndexCatalog::new(Arc::new(Numbered(Arc::new(lib))), Played::default());
        assert_eq!(
            c.list(&Query::Tracks {
                scope: TrackScope::Album(1),
                sort: Sort::Default,
            }),
            vec![
                Row::Track {
                    id: 11,
                    position: 1,
                },
                Row::Track {
                    id: 10,
                    position: 2,
                },
            ]
        );
    }

    #[test]
    fn matching_tracks_keep_track_numbers_while_sorting_by_title() {
        struct Matching(Arc<Library>);
        impl Source for Matching {
            fn library(&self) -> Option<Arc<Library>> {
                Some(Arc::clone(&self.0))
            }
            fn share_root(&self) -> PathBuf {
                PathBuf::from("/nonexistent")
            }
            fn details(&self, id: &str) -> Option<rbl_db::details::TrackDetails> {
                Some(rbl_db::details::TrackDetails {
                    track_number: match id {
                        "10" => 2,
                        "11" => 1,
                        _ => 0,
                    },
                    ..rbl_db::details::TrackDetails::default()
                })
            }
            fn matching_ids(&self, seed: u32) -> Vec<u32> {
                assert_eq!(seed, 99);
                vec![10, 11]
            }
        }

        let c = IndexCatalog::new(Arc::new(Matching(Arc::new(library()))), Played::default());
        assert_eq!(
            c.list(&Query::Tracks {
                scope: TrackScope::Matching(99),
                sort: Sort::Default,
            }),
            vec![
                Row::Track {
                    id: 11,
                    position: 1,
                },
                Row::Track {
                    id: 10,
                    position: 2,
                },
            ]
        );
    }

    #[test]
    fn keys_are_camelot_ids_and_related_keys_widen_the_list() {
        let c = catalog();
        // Abm is 1A = id 1; B is 1B = id 2; Am is 8A = id 15.
        assert_eq!(c.track_row(11, None).unwrap().key, 1);
        assert_eq!(c.track_row(12, None).unwrap().key, 2);
        assert_eq!(c.track_row(10, None).unwrap().key, 15);
        assert_eq!(player_key_id("Alpha"), 22);
        assert_eq!(player_key_id("Beta Alpha"), 2);
        assert_eq!(player_key_id("alpha"), 0);
        assert_eq!(player_key_id("Ａｌｐｈａ"), 22);
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Key {
                    key: 1,
                    distance: 0
                },
                sort: Sort::Default
            })),
            [11]
        );
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Key {
                    key: 1,
                    distance: 1
                },
                sort: Sort::Default
            })),
            [11, 12]
        );
    }

    #[test]
    fn playlists_keep_their_own_order_and_number_their_rows() {
        let c = catalog();
        let root = c.list(&Query::Folder(0));
        assert_eq!(root.len(), 2);
        let Row::List {
            id: warm_up,
            folder: false,
            ..
        } = root[1]
        else {
            panic!("{root:?}")
        };
        let rows = c.list(&Query::Tracks {
            scope: TrackScope::Playlist(warm_up),
            sort: Sort::Default,
        });
        assert_eq!(
            rows,
            vec![
                Row::Track {
                    id: 12,
                    position: 1
                },
                Row::Track {
                    id: 10,
                    position: 2
                }
            ]
        );
        // Sorted on request, positions still their own.
        let rows = c.list(&Query::Tracks {
            scope: TrackScope::Playlist(warm_up),
            sort: Sort::Alphabet,
        });
        assert_eq!(
            rows,
            vec![
                Row::Track {
                    id: 12,
                    position: 1
                },
                Row::Track {
                    id: 10,
                    position: 2
                }
            ]
        );
    }

    #[test]
    fn smart_playlists_evaluate_rules_instead_of_stored_membership() {
        let mut lib = library();
        let rule = concat!(
            "<NODE Id=\"1\" LogicalOperator=\"1\" AutomaticUpdate=\"1\">",
            "<CONDITION PropertyName=\"genre\" Operator=\"1\" ValueUnit=\"\" ",
            "ValueLeft=\"House\" ValueRight=\"\"/>",
            "</NODE>"
        );
        let index = add_smart_playlist_with_members(&mut lib, "House", &[1], rule);
        let playlist = u32::try_from(lib.playlists().ids[index]).unwrap();
        let c = IndexCatalog::new(Arc::new(Fixed(Arc::new(lib))), Played::default());

        assert_eq!(
            c.list(&Query::Tracks {
                scope: TrackScope::Playlist(playlist),
                sort: Sort::Default,
            }),
            vec![Row::Track {
                id: 10,
                position: 1,
            }]
        );
        assert!(!c.listed(&TrackScope::Playlist(playlist)));
    }

    #[test]
    fn smart_playlists_use_the_source_date_and_expire_cached_rows() {
        let mut lib = library();
        let rule = concat!(
            "<NODE Id=\"1\" LogicalOperator=\"1\" AutomaticUpdate=\"1\">",
            "<CONDITION PropertyName=\"stockDate\" Operator=\"6\" ValueUnit=\"day\" ",
            "ValueLeft=\"1\" ValueRight=\"\"/>",
            "</NODE>"
        );
        let index = add_smart_playlist_with_members(&mut lib, "Recent", &[], rule);
        let playlist = u32::try_from(lib.playlists().ids[index]).unwrap();
        let source = Arc::new(Clocked {
            library: Arc::new(lib),
            today: Mutex::new(Date {
                year: 2026,
                month: 1,
                day: 20,
            }),
        });
        let c = IndexCatalog::new(source.clone(), Played::default());
        let query = Query::Tracks {
            scope: TrackScope::Playlist(playlist),
            sort: Sort::Default,
        };

        assert_eq!(ids(&c.list(&query)), [12]);
        *source.today.lock() = Date {
            year: 2026,
            month: 1,
            day: 9,
        };
        assert_eq!(ids(&c.list(&query)), [11, 12]);
    }

    #[test]
    fn playlist_folders_sort_before_lists_regardless_of_sequence() {
        let mut lib = library_from(&[]);
        add_playlist(&mut lib, "First sequence", &[]);
        add_folder(&mut lib, "Later folder");
        let c = IndexCatalog::new(Arc::new(Fixed(Arc::new(lib))), Played::default());

        assert_eq!(
            c.list(&Query::Folder(0)),
            vec![
                Row::List {
                    id: 1001,
                    name: "Later folder".into(),
                    folder: true,
                    position: 1,
                },
                Row::List {
                    id: 1000,
                    name: "First sequence".into(),
                    folder: false,
                    position: 0,
                },
            ]
        );
    }

    /// A library that takes the history writes a player's plays make.
    struct Writable(Arc<Library>);

    impl Source for Writable {
        fn library(&self) -> Option<Arc<Library>> {
            Some(Arc::clone(&self.0))
        }
        fn share_root(&self) -> PathBuf {
            PathBuf::from("/nonexistent")
        }
        fn details(&self, _id: &str) -> Option<rbl_db::details::TrackDetails> {
            None
        }
        fn new_link_history(&self) -> Option<u32> {
            let mut lib = Library::default();
            lib.set_histories((*self.0.histories()).clone());
            let index = add_history(&mut lib, "LINK HISTORY 2026-09-26", &[]);
            let id = lib.histories().ids[index];
            self.0.set_histories((*lib.histories()).clone());
            u32::try_from(id).ok()
        }
        fn add_to_history(&self, history: u32, track: u32) -> bool {
            let mut lists = (*self.0.histories()).clone();
            let (Some(index), Some(row)) = (
                lists.index_of(u64::from(history)),
                self.0.row_of_id(u64::from(track)),
            ) else {
                return false;
            };
            lists.members[index].push(row);
            self.0.set_histories(lists);
            true
        }
        fn remove_from_history(&self, history: u32, track: u32) -> bool {
            let mut lists = (*self.0.histories()).clone();
            let (Some(index), Some(row)) = (
                lists.index_of(u64::from(history)),
                self.0.row_of_id(u64::from(track)),
            ) else {
                return false;
            };
            lists.members[index].retain(|&r| r != row);
            self.0.set_histories(lists);
            true
        }
    }

    fn history_menu(c: &IndexCatalog) -> Vec<String> {
        c.list(&Query::Histories)
            .into_iter()
            .map(|r| match r {
                Row::Named { name, .. } => name,
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn the_history_menu_holds_only_this_link_sessions_history() {
        let source = Arc::new(Writable(Arc::new(library())));
        let played = Played::default();
        let c = IndexCatalog::new(
            Arc::clone(&source) as Arc<dyn Source>,
            played.clone(),
        );
        // The library's own sessions are not offered; nothing is until a play.
        assert_eq!(history_menu(&c), [] as [String; 0]);
        assert!(!played.contains(11), "loading is not a history event");

        assert!(c.edit(&Edit::HistoryAdd { track: 11 }));
        assert!(played.contains(11));
        assert!(
            c.edit(&Edit::HistoryAdd { track: 11 }),
            "a repeat is taken, not written"
        );
        assert!(c.edit(&Edit::HistoryAdd { track: 10 }));
        assert!(c.edit(&Edit::HistoryAdd { track: 11 }));
        assert_eq!(history_menu(&c), ["LINK HISTORY 2026-09-26"]);
        let Row::Named { id: session, .. } = c.list(&Query::Histories)[0].clone() else {
            panic!()
        };
        let tracks = |c: &IndexCatalog| {
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::History(session),
                sort: Sort::Default,
            }))
        };
        assert_eq!(tracks(&c), [11, 10, 11]);
        assert_eq!(
            source.0.histories().len(),
            3,
            "one session for the link, beside the library's two"
        );

        assert!(c.edit(&Edit::HistoryRemove { track: 11 }));
        assert_eq!(tracks(&c), [10]);
        // 10 is last again, so it is not added straight after itself.
        assert!(c.edit(&Edit::HistoryAdd { track: 10 }));
        assert_eq!(tracks(&c), [10]);
        assert!(!c.edit(&Edit::HistoryRemove { track: 999 }));

        // Deleted from a player: the next play starts another session.
        assert!(c.edit(&Edit::HistoryDelete { history: u32::MAX }));
        assert_eq!(history_menu(&c), [] as [String; 0]);
        assert!(
            !c.edit(&Edit::HistoryRemove { track: 10 }),
            "no session to take it off"
        );
        assert!(c.edit(&Edit::HistoryAdd { track: 12 }));
        assert!(played.contains(12));
        let Row::Named { id: next, .. } = c.list(&Query::Histories)[0].clone() else {
            panic!()
        };
        assert_ne!(next, session);
        // A delete naming some other history leaves this one.
        assert!(c.edit(&Edit::HistoryDelete { history: session }));
        assert_eq!(c.list(&Query::Histories).len(), 1);
    }

    #[test]
    fn a_library_that_cannot_be_written_keeps_the_history_menu_empty() {
        let c = catalog();
        assert!(!c.edit(&Edit::HistoryAdd { track: 11 }));
        assert_eq!(history_menu(&c), [] as [String; 0]);
    }

    #[test]
    fn date_added_walks_years_months_and_days() {
        let c = catalog();
        assert_eq!(
            c.list(&Query::Years),
            vec![Row::Date(2026), Row::Date(2025)]
        );
        assert_eq!(c.list(&Query::Months(2026)), vec![Row::Date(1)]);
        assert_eq!(
            c.list(&Query::Days {
                year: 2026,
                month: 1
            }),
            vec![Row::Date(9), Row::Date(20)]
        );
        let january = TrackScope::DateAdded {
            year: 2026,
            month: Some(1),
            day: None,
        };
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: january,
                sort: Sort::Default
            })),
            [11, 12]
        );
        let the_ninth = TrackScope::DateAdded {
            year: 2026,
            month: Some(1),
            day: Some(9),
        };
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: the_ninth,
                sort: Sort::Default
            })),
            [11]
        );
    }

    #[test]
    fn search_matches_the_browser_search() {
        let c = catalog();
        assert_eq!(
            ids(&c.list(&Query::Tracks {
                scope: TrackScope::Search("second".into()),
                sort: Sort::Default
            })),
            [12, 10]
        );
        assert_eq!(
            c.list(&Query::Tracks {
                scope: TrackScope::Search("nothing".into()),
                sort: Sort::Default
            }),
            [] as [rbl_dbserver::catalog::Row; 0]
        );
    }

    #[test]
    fn track_details_come_from_the_index_with_the_absolute_path() {
        let c = catalog();
        let details = c.track(10).unwrap();
        assert_eq!(details.artist, "Bob");
        assert_eq!(details.album, "Second");
        assert_eq!(details.path, "/Volumes/SD/RB/x.mp3");
        assert_eq!(details.date_added, "2025-03-04");
        assert_eq!(details.row.bpm_x100, 12_800);
        assert!(c.track(99).is_none());
    }

    #[test]
    fn a_track_without_analysis_has_no_blobs() {
        let c = catalog();
        assert!(c.analysis(10, &Wanted::BeatGrid).is_none());
        // Retained compatibility policy, not a claim about the track's VBR.
        assert_eq!(c.analysis(10, &Wanted::Vbr).unwrap(), vec![0_u8; 1604]);
        assert_eq!(
            c.analysis(10, &Wanted::ExtendedCueList).unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn usb_cues_keep_only_the_legacy_hot_slots_and_memory_cues() {
        let lib = library();
        let row = lib.row_of_id(10).unwrap();
        lib.set_cues_of(row, vec![
            Cue { position_ms: 1_000, out_ms: 2_000, kind: 1, colour: 21, ..Cue::default() },
            Cue { position_ms: 3_000, out_ms: 0, kind: 0, colour: 0, ..Cue::default() },
            // D is represented only in the extended cue protocol.
            Cue { position_ms: 4_000, out_ms: 0, kind: 5, colour: 35, ..Cue::default() },
        ]);
        let c = IndexCatalog::new(Arc::new(Fixed(Arc::new(lib))), Played::default());
        assert_eq!(c.usb_cues(10), vec![
            UsbCue { slot: 1, in_ms: 1_000, out_ms: Some(2_000), color_table_index: 21 },
            UsbCue { slot: 0, in_ms: 3_000, out_ms: None, color_table_index: 0 },
        ]);
    }

    #[test]
    fn a_cdj_3000_names_the_track_or_album_whose_artwork_it_wants() {
        let mut lib = library();
        for relative in [
            "",
            "/PIONEER/Artwork/a/artwork.jpg",
            "/PIONEER/Artwork/b/artwork.jpg",
        ] {
            lib.artwork_path.push(relative);
        }
        // Tracks by id, then albums: the test interner gives each track its
        // own album entry, so album ids 1, 2, 3 hold tracks 10, 11, 12.
        assert_eq!(IndexCatalog::item_row(&lib, 12), Some(2));
        assert_eq!(IndexCatalog::item_row(&lib, 11), Some(1));
        assert_eq!(
            IndexCatalog::item_row(&lib, 3),
            Some(2),
            "album 3's track has artwork"
        );
        assert_eq!(IndexCatalog::item_row(&lib, 2), Some(1));
        assert_eq!(
            IndexCatalog::item_row(&lib, 1),
            None,
            "album 1's only track has none"
        );
        assert_eq!(IndexCatalog::item_row(&lib, 99), None);
    }

    /// #462: a client without the size argument (prolink-connect, so
    /// Prolink Tools; older players) names the track by the id a title
    /// row's artwork field carries, as one with it does, and gets the small
    /// file rekordbox would send; RBX used to read that id as a row plus two.
    #[test]
    fn artwork_is_found_by_track_id_with_or_without_the_size_argument() {
        struct Share(Arc<Library>, PathBuf);
        impl Source for Share {
            fn library(&self) -> Option<Arc<Library>> {
                Some(Arc::clone(&self.0))
            }
            fn share_root(&self) -> PathBuf {
                self.1.clone()
            }
            fn details(&self, _id: &str) -> Option<rbl_db::details::TrackDetails> {
                None
            }
        }
        let share = tempfile::tempdir().unwrap();
        let mut lib = library();
        for relative in ["", "/PIONEER/Artwork/a/artwork.jpg", "/PIONEER/Artwork/b/artwork.jpg"] {
            lib.artwork_path.push(relative);
        }
        for (relative, bytes) in [
            ("PIONEER/Artwork/a/artwork.jpg", b"a-full".as_slice()),
            ("PIONEER/Artwork/a/artwork_m.jpg", b"a-m"),
            ("PIONEER/Artwork/a/artwork_s.jpg", b"a-s"),
            ("PIONEER/Artwork/b/artwork.jpg", b"b-full"),
        ] {
            let path = share.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let c = IndexCatalog::new(
            Arc::new(Share(Arc::new(lib), share.path().to_owned())),
            Played::default(),
        );
        assert_eq!(c.artwork(11, ArtworkSize::Small).unwrap(), b"a-s");
        assert_eq!(c.artwork(11, ArtworkSize::Medium).unwrap(), b"a-m");
        // Asked twice, each size keeps its own cached file.
        assert_eq!(c.artwork(11, ArtworkSize::Small).unwrap(), b"a-s");
        assert_eq!(c.artwork(12, ArtworkSize::Small).unwrap(), b"b-full");
        assert_eq!(c.artwork(12, ArtworkSize::Medium).unwrap(), b"b-full");
        assert_eq!(c.artwork(10, ArtworkSize::Small), None, "track 10 has none");
    }

    #[test]
    fn artwork_is_the_sized_file_beside_the_one_the_library_names() {
        assert_eq!(
            artwork_candidates("/PIONEER/Artwork/5ba/0a225-f6b2/artwork.jpg", ArtworkSize::Medium),
            [
                "/PIONEER/Artwork/5ba/0a225-f6b2/artwork_m.jpg",
                "/PIONEER/Artwork/5ba/0a225-f6b2/artwork_s.jpg",
                "/PIONEER/Artwork/5ba/0a225-f6b2/artwork.jpg",
            ]
        );
        assert_eq!(
            artwork_candidates("/PIONEER/Artwork/5ba/0a225-f6b2/artwork.jpg", ArtworkSize::Small),
            [
                "/PIONEER/Artwork/5ba/0a225-f6b2/artwork_s.jpg",
                "/PIONEER/Artwork/5ba/0a225-f6b2/artwork.jpg",
            ]
        );
        assert_eq!(sized_artwork("art.v2.png", "_m"), "art.v2_m.png");
        assert_eq!(sized_artwork("/a.b/artwork", "_s"), "/a.b/artwork");
    }

    #[test]
    fn secondary_numeric_values_are_formatted_like_rekordbox() {
        assert_eq!(format_bpm(11_216), "112.2 bpm");
        assert_eq!(format_bpm(0), "");
        assert_eq!(camelot_name(5), "3A");
        assert_eq!(camelot_name(6), "3B");
        assert_eq!(format_nonzero(320, " kbps"), "320 kbps");
        assert_eq!(format_nonzero(0, " kbps"), "");
    }

    /// My Tags reach the player with the ids rekordbox sends: a category's
    /// `ID` and `root` parent read as numbers, a tag past 2^31 the same 32
    /// bits whether named by `djmdMyTag` or by a track's `djmdSongMyTag`.
    #[test]
    fn my_tags_reach_the_server_with_the_ids_a_track_carries() {
        struct Tagged(Arc<Library>);
        impl Source for Tagged {
            fn library(&self) -> Option<Arc<Library>> {
                Some(Arc::clone(&self.0))
            }
            fn share_root(&self) -> PathBuf {
                PathBuf::from("/nonexistent")
            }
            fn details(&self, _id: &str) -> Option<rbl_db::details::TrackDetails> {
                None
            }
            fn my_tags(&self) -> Vec<rbl_db::export_info::MyTagRow> {
                let row = |id: &str, seq, name: &str, attribute, parent: &str| rbl_db::export_info::MyTagRow {
                    id: id.into(),
                    seq,
                    name: name.into(),
                    attribute,
                    parent: parent.into(),
                };
                vec![
                    row("2", 2, "Traits", 1, "root"),
                    row("108516952", 1, "reese", 0, "2"),
                    row("3000000000", 2, "wide", 0, "2"),
                    row("242910998", 3, "wubby", 0, "2"),
                ]
            }
        }
        let mut lib = library();
        // Rows 0, 1, 2 are tracks 10, 11, 12.
        rbl_index::testing::set_track_my_tags(
            &mut lib,
            vec![
                (0, rbl_index::smart::my_tag_key("108516952")),
                (0, rbl_index::smart::my_tag_key("3000000000")),
                (1, rbl_index::smart::my_tag_key("3000000000")),
            ],
        );
        let c = IndexCatalog::new(Arc::new(Tagged(Arc::new(lib))), Played::default());
        let tags = c.my_tags();
        assert_eq!(
            tags.iter().map(|t| (t.id, t.parent, t.seq)).collect::<Vec<_>>(),
            [(2, 0, 2), (108_516_952, 2, 1), (3_000_000_000, 2, 2), (242_910_998, 2, 3)]
        );
        let mut on_ten = c.track_my_tags(10);
        on_ten.sort_unstable();
        assert_eq!(on_ten, [108_516_952, 3_000_000_000]);
        assert_eq!(c.track_my_tags(12), Vec::<u32>::new());
        assert_eq!(c.my_tags_in_use(), HashSet::from([108_516_952, 3_000_000_000]));

        let mut filter = rbl_dbserver::filter::TrackFilter::default();
        filter.enabled = true;
        filter.set_my_tag_condition(2, 1, rbl_dbserver::filter::MY_TAG_AND);
        assert!(filter.tick_my_tag(2, 3_000_000_000, true));
        let all = || c.list(&Query::Tracks { scope: TrackScope::All, sort: Sort::Default });
        let mut rows = all();
        c.filter_rows(&mut rows, &filter);
        assert_eq!(ids(&rows), [11, 10]);
        assert!(filter.tick_my_tag(2, 108_516_952, true));
        let mut rows = all();
        c.filter_rows(&mut rows, &filter);
        assert_eq!(ids(&rows), [10]);
        filter.set_my_tag_condition(2, 1, rbl_dbserver::filter::MY_TAG_OR);
        let mut rows = all();
        c.filter_rows(&mut rows, &filter);
        assert_eq!(ids(&rows), [11, 10]);
    }
}
