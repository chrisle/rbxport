//! Views: an ordered, filtered list of rows.
//!
//! A view is just a `Vec<Row>`. Sorting compares precomputed rank integers
//! rather than strings, and searching scans one packed folded haystack, so both
//! stay well inside the budgets on a 38k-track library.

use crate::{filter::TrackFilter, strings::fold, Library, Row};

/// A column the browser can order rows by.
///
/// rekordbox 7.2.11's `browse::BrowseHeaderManager::isSortableColumn` and
/// `browse::ListViewSorter::setCompFunc` decide which of its columns sort and
/// by what [OBS: static analysis of the macOS arm64 binary]. Each variant
/// below notes the comparator it follows. Ties break on row order, and an
/// empty text sorts first, as `ListViewSorter::compareJuceString` puts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortColumn {
    TrackNo,
    Title,
    Artist,
    Album,
    Genre,
    Label,
    Comment,
    Key,
    Bpm,
    Duration,
    Rating,
    PlayCount,
    DateAdded,
    ReleaseDate,
    /// The key column round the Camelot wheel, for the alphanumeric display:
    /// rekordbox sorts the column by what it shows.
    KeyCamelot,
    /// `compareFileSize`: the stored byte count.
    Size,
    /// `compareReleaseYear`: the year as a number, 0 (none) first.
    Year,
    /// `compareSampleRate`: Hz as a number.
    SampleRate,
    /// `compareBitrate`: kbps as a number.
    Bitrate,
    /// `compareColor`: the `ColorID` number, so the colours sort in
    /// rekordbox's palette order (pink first) rather than by name, and no
    /// colour (0) sorts first.
    Color,
    /// `compareFileName`: the file's name.
    FileName,
    /// `compareFilePath`: the whole path, so a folder's tracks sort together.
    Location,
    /// `compareComposer`.
    Composer,
    /// `compareAlbumArtist`.
    AlbumArtist,
    /// `compareRemixer`.
    Remixer,
    /// `compareOrgArtist`.
    OriginalArtist,
    /// `compareMixName`: `djmdContent.Subtitle`.
    MixName,
    /// `compareDiscNo`.
    DiscNo,
    /// `compareTrackNo`: the tag's track number. Not the `#` column, which is
    /// [`SortColumn::TrackNo`] and means the view's own order.
    TrackNumber,
    /// `compareFileType`: rekordbox's file type code (1 MP3, 4 M4A, 5 FLAC,
    /// 11 WAV, 12 AIFF), not the name it prints.
    FileType,
    /// `compareBitDepth`.
    BitDepth,
    /// `compareLyricist`.
    Lyricist,
    /// `compareDateCreated`: the stored `YYYY-MM-DD` text.
    DateCreated,
    /// `comparePublic`: rekordbox returns `b` when `a` is off and `b - 1`
    /// when it is on, so ascending puts the ticked tracks first.
    PublishTrackInfo,
    /// `comparePublicComment`: `djmdContent.DeliveryComment`.
    Message,
}

impl SortColumn {
    /// Every column, in rank-slot order. Public so the wire mapping's tests
    /// can check that each one is reachable.
    pub const ALL: [SortColumn; 35] = [
        SortColumn::TrackNo, SortColumn::Title, SortColumn::Artist, SortColumn::Album,
        SortColumn::Genre, SortColumn::Label, SortColumn::Comment, SortColumn::Key, SortColumn::Bpm,
        SortColumn::Duration, SortColumn::Rating, SortColumn::PlayCount, SortColumn::DateAdded, SortColumn::ReleaseDate,
        SortColumn::KeyCamelot, SortColumn::Size, SortColumn::Year, SortColumn::SampleRate,
        SortColumn::Bitrate, SortColumn::Color, SortColumn::FileName, SortColumn::Location,
        SortColumn::Composer, SortColumn::AlbumArtist, SortColumn::Remixer, SortColumn::OriginalArtist,
        SortColumn::MixName, SortColumn::DiscNo, SortColumn::TrackNumber, SortColumn::FileType,
        SortColumn::BitDepth, SortColumn::Lyricist, SortColumn::DateCreated,
        SortColumn::PublishTrackInfo, SortColumn::Message,
    ];

    pub(crate) fn rank_slot(self) -> usize {
        match self {
            SortColumn::TrackNo => 0,
            SortColumn::Title => 1,
            SortColumn::Artist => 2,
            SortColumn::Album => 3,
            SortColumn::Genre => 4,
            SortColumn::Label => 5,
            SortColumn::Comment => 6,
            SortColumn::Key => 7,
            SortColumn::Bpm => 8,
            SortColumn::Duration => 9,
            SortColumn::Rating => 10,
            SortColumn::PlayCount => 11,
            SortColumn::DateAdded => 12,
            SortColumn::ReleaseDate => 13,
            SortColumn::KeyCamelot => 14,
            SortColumn::Size => 15,
            SortColumn::Year => 16,
            SortColumn::SampleRate => 17,
            SortColumn::Bitrate => 18,
            SortColumn::Color => 19,
            SortColumn::FileName => 20,
            SortColumn::Location => 21,
            SortColumn::Composer => 22,
            SortColumn::AlbumArtist => 23,
            SortColumn::Remixer => 24,
            SortColumn::OriginalArtist => 25,
            SortColumn::MixName => 26,
            SortColumn::DiscNo => 27,
            SortColumn::TrackNumber => 28,
            SortColumn::FileType => 29,
            SortColumn::BitDepth => 30,
            SortColumn::Lyricist => 31,
            SortColumn::DateCreated => 32,
            SortColumn::PublishTrackInfo => 33,
            SortColumn::Message => 34,
        }
    }

    /// The `search_extra` slot a column reads its text from, for the five
    /// uncommon text fields that live there.
    pub(crate) fn extra_slot(self) -> Option<usize> {
        match self {
            SortColumn::Composer => Some(0),
            SortColumn::AlbumArtist => Some(1),
            SortColumn::Remixer => Some(2),
            SortColumn::OriginalArtist => Some(3),
            SortColumn::MixName => Some(4),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackSource {
    Collection,
    /// Index into `Library::playlists`, not a rekordbox id.
    Playlist(usize),
    /// Tracks in every descendant playlist, with repeated tracks listed once.
    PlaylistFolder(usize),
    /// Index into `Library::histories`. A session, or a folder of them —
    /// a folder has no members of its own, so it opens empty.
    History(usize),
    /// Index into `Library::playlists` of an intelligent playlist: the rows
    /// are whatever its rule admits at the moment it is opened.
    SmartPlaylist(usize),
    /// rekordbox's Related Tracks: the tracks that go with one track under a
    /// criterion. A `track` past the end of the library — no track loaded —
    /// opens empty.
    Related { track: Row, criterion: RelatedCriterion },
    /// rekordbox's Tag List, in its own order.
    TagList,
}

/// The Related Tracks section's criteria, the three rekordbox's Export
/// mode lists [DOC: the rekordbox manual's Related Tracks; the panel itself
/// has no capture here].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelatedCriterion {
    /// `BPM + KEY`: within 6 % of the track's BPM [ASSUME] and in its key or
    /// a key beside it on the wheel (Relative Key 1).
    BpmAndKey,
    /// `Same genre in 30 days`: the track's genre, added in the last thirty
    /// days [ASSUME: what the thirty days count].
    SameGenreRecent,
    /// `Same artist`.
    SameArtist,
    /// rekordbox's Track Suggestion: what was played after this track in
    /// the histories, the most often first, then the most recently, and
    /// with no history of the track, what goes with it by BPM and key.
    /// rekordbox 7.2.11's own panel (captured 2026-09-18) is titled "Era",
    /// takes its track from the list, the master or player A, and scopes
    /// to the collection; what it ranks by is not shown and not
    /// documented, so this is a stand-in, not a copy.
    Suggestion,
}

/// Search categories in the order shown by the browser's search menu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchField {
    #[default]
    All, Title, Artist, Album, Genre, Year, Bpm, Composer, AlbumArtist,
    Remixer, Label, Comment, OriginalArtist, MixName,
}

impl SearchField {
    fn index(self) -> Option<usize> {
        match self {
            Self::All => None, Self::Title => Some(0), Self::Artist => Some(1), Self::Album => Some(2),
            Self::Genre => Some(3), Self::Year => Some(4), Self::Bpm => Some(5), Self::Composer => Some(6),
            Self::AlbumArtist => Some(7), Self::Remixer => Some(8), Self::Label => Some(9),
            Self::Comment => Some(10), Self::OriginalArtist => Some(11), Self::MixName => Some(12),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ViewSpec {
    pub source: TrackSource,
    pub sort: SortColumn,
    pub descending: bool,
    pub query: String,
    /// The track filter bar's picks. Default is no constraint.
    pub filter: TrackFilter,
}

#[derive(Debug)]
pub struct View {
    pub rows: Vec<Row>,
    /// The stored `TrackNo` for each row of an ordinary playlist, kept beside
    /// the sorted view rows. Searching or sorting a playlist must not turn
    /// this into the row's current visible position: DJs use it to see where
    /// a track sits in the set.
    playlist_track_nos: Option<Vec<u32>>,
}

impl View {
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// A window of rows, clamped to what exists.
    pub fn window(&self, offset: usize, len: usize) -> &[Row] {
        let start = offset.min(self.rows.len());
        let end = start.saturating_add(len).min(self.rows.len());
        self.rows.get(start..end).unwrap_or(&[])
    }

    /// The number shown in the browser's `#` column at this view position.
    /// Only ordinary playlists have a durable track order; every other view
    /// numbers its visible rows.
    #[must_use]
    pub fn track_no_at(&self, position: usize) -> u32 {
        self.playlist_track_nos
            .as_ref()
            .and_then(|numbers| numbers.get(position).copied())
            .unwrap_or_else(|| u32::try_from(position.saturating_add(1)).unwrap_or(u32::MAX))
    }
}

impl Library {
    /// Builds a view. This is the only place ordering is decided.
    pub fn open_view(&self, spec: &ViewSpec) -> View {
        self.open_view_scoped(spec, SearchField::All)
    }

    pub fn open_view_scoped(&self, spec: &ViewSpec, field: SearchField) -> View {
        let playlist_numbers = matches!(&spec.source, TrackSource::Playlist(_));
        // Keep the playlist's stored position coupled to its row while the
        // view is filtered and sorted. A map by row would lose duplicate
        // playlist entries, which rekordbox permits.
        let mut rows: Vec<(Row, u32)> = self.source_rows(&spec.source).into_iter().enumerate()
            .map(|(position, row)| (row, u32::try_from(position.saturating_add(1)).unwrap_or(u32::MAX)))
            .collect();

        let query = fold(spec.query.trim());
        if !query.is_empty() {
            rows.retain(|&(row, _)| self.row_matches_in(row, &query, field));
        }

        // The filter bar, in the same pass as the search: a handful of integer
        // compares per row against masks built once, so a ticked column costs
        // about what a one-letter query does.
        if !spec.filter.is_empty() {
            let compiled = spec.filter.compile();
            rows.retain(|&(row, _)| compiled.matches(self, row));
        }

        // `TrackNo` is not a column to sort by — it *is* the view's own order:
        // the collection's row order, or a playlist's membership order. Ranking
        // by it would reorder a playlist into collection order, which is
        // exactly what turning sorting off must not do.
        if spec.sort == SortColumn::TrackNo {
            if spec.descending {
                rows.reverse();
            }
        } else if let Some(rank) = self.ranks.get(spec.sort.rank_slot()) {
            if spec.descending {
                rows.sort_unstable_by_key(|&(row, _)| std::cmp::Reverse(rank.get(row as usize).copied().unwrap_or(0)));
            } else {
                rows.sort_unstable_by_key(|&(row, _)| rank.get(row as usize).copied().unwrap_or(0));
            }
        }
        let (rows, numbers): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
        View { rows, playlist_track_nos: playlist_numbers.then_some(numbers) }
    }

    /// Narrows an existing view. Typing another character only has to filter the
    /// previous match set, not the whole library.
    pub fn refine(&self, previous: &View, query: &str) -> View {
        let folded = fold(query.trim());
        if folded.is_empty() {
            // perf-ok: clearing the query copies at most 40k u32 (~160 KB, tens
            // of microseconds) and only on the keystroke that empties the box.
            return View { rows: previous.rows.clone(), playlist_track_nos: previous.playlist_track_nos.clone() };
        }
        let matched: Vec<_> = previous.rows.iter().copied().enumerate()
            .filter(|&(_, row)| self.row_matches(row, &folded))
            .collect();
        let playlist_track_nos = previous.playlist_track_nos.as_ref().map(|numbers| {
            matched.iter().filter_map(|&(position, _)| numbers.get(position).copied()).collect()
        });
        View { rows: matched.into_iter().map(|(_, row)| row).collect(), playlist_track_nos }
    }

    pub(crate) fn row_matches(&self, row: Row, folded_query: &str) -> bool {
        self.row_matches_in(row, folded_query, SearchField::All)
    }

    pub(crate) fn row_matches_in(&self, row: Row, folded_query: &str, field: SearchField) -> bool {
        let all = self.search.get(row as usize);
        let hay = field.index().map_or(all, |index| all.split('\t').nth(index).unwrap_or(""));
        // Every token must appear, so "artbat 128" narrows as a user expects.
        folded_query.split_whitespace().all(|token| {
            memchr::memmem::find(hay.as_bytes(), token.as_bytes()).is_some()
        })
    }

    /// Orders rows by a column's precomputed ranks. Public for the link
    /// export, whose menus sort scopes the views do not have (an artist's
    /// tracks, a key's) with the same ranks the browser uses.
    pub fn sort_rows(&self, rows: &mut [Row], column: SortColumn, descending: bool) {
        let Some(rank) = self.ranks.get(column.rank_slot()) else { return };
        if descending {
            rows.sort_unstable_by_key(|&r| std::cmp::Reverse(rank.get(r as usize).copied().unwrap_or(0)));
        } else {
            rows.sort_unstable_by_key(|&r| rank.get(r as usize).copied().unwrap_or(0));
        }
    }

    /// Ids of rows between two view positions, inclusive. Used for shift-click
    /// across rows the frontend has never fetched.
    pub fn ids_in_range(&self, view: &View, from: usize, to: usize) -> Vec<u64> {
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        let hi = hi.min(view.rows.len().saturating_sub(1));
        view.rows
            .get(lo..=hi)
            .unwrap_or(&[])
            .iter()
            .filter_map(|&r| self.ids.get(r as usize).copied())
            .collect()
    }

    /// Builds the per-column rank arrays. Called once at load.
    pub(crate) fn build_ranks(&mut self) {
        self.rebuild_ranks(&SortColumn::ALL);
    }

    pub(crate) fn rebuild_ranks(&mut self, columns: &[SortColumn]) {
        self.ranks.resize_with(SortColumn::ALL.len(), Vec::new);
        for &column in columns {
            self.ranks[column.rank_slot()] = self.column_rank(column);
        }
    }

    /// At most four CPU workers, including the caller. Small libraries avoid
    /// thread startup entirely. Search and independent sort columns overlap.
    pub(crate) fn build_indexes(&mut self) {
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(1).clamp(1, 4));
        if self.count < 4096 || workers < 2 {
            self.build_ranks();
            self.build_search();
            return;
        }
        let library = &*self;
        let (ranks, search) = std::thread::scope(|scope| {
            let chunk = SortColumn::ALL.len().div_ceil(workers - 1);
            let jobs: Vec<_> = SortColumn::ALL.chunks(chunk).map(|columns| {
                (columns, std::thread::Builder::new().name("startup-index".into()).spawn_scoped(scope,
                    move || columns.iter().map(|&c| (c.rank_slot(), library.column_rank(c))).collect::<Vec<_>>()))
            }).collect();
            let search = library.search_column();
            let mut ranks = vec![Vec::new(); SortColumn::ALL.len()];
            for (columns, job) in jobs {
                let result = job.ok().and_then(|job| job.join().ok())
                    .unwrap_or_else(|| columns.iter().map(|&c| (c.rank_slot(), library.column_rank(c))).collect());
                for (slot, rank) in result { ranks[slot] = rank; }
            }
            (ranks, search)
        });
        self.ranks = ranks;
        self.search = search;
    }

    fn column_rank(&self, column: SortColumn) -> Vec<u32> {
        let n = self.count;
        let mut order: Vec<Row> = (0..u32::try_from(n).unwrap_or(u32::MAX)).collect();
        // Ties break on row order so a sort is reproducible: every sort here
        // is stable and starts from row order.
        match column {
            SortColumn::TrackNo => {}
            SortColumn::Title => order.sort_by(|&a, &b| self.title_folded.get(a as usize).cmp(self.title_folded.get(b as usize))),
            SortColumn::Artist => order.sort_by(|&a, &b| Self::folded_lookup(&self.artists, &self.artist, a).cmp(Self::folded_lookup(&self.artists, &self.artist, b))),
            SortColumn::Album => order.sort_by(|&a, &b| Self::folded_lookup(&self.albums, &self.album, a).cmp(Self::folded_lookup(&self.albums, &self.album, b))),
            SortColumn::Genre => order.sort_by(|&a, &b| Self::folded_lookup(&self.genres, &self.genre, a).cmp(Self::folded_lookup(&self.genres, &self.genre, b))),
            SortColumn::Label => order.sort_by(|&a, &b| Self::folded_lookup(&self.labels, &self.label, a).cmp(Self::folded_lookup(&self.labels, &self.label, b))),
            // By the key's own rule, not the fold: the fold drops `#`,
            // which put F and F# on top of each other.
            SortColumn::Key => order.sort_by(|&a, &b| crate::key::cmp_names(self.key_name(a), self.key_name(b))),
            SortColumn::KeyCamelot => order.sort_by(|&a, &b| {
                crate::key::camelot_rank(self.key_name(a))
                    .cmp(&crate::key::camelot_rank(self.key_name(b)))
                    .then_with(|| crate::key::cmp_names(self.key_name(a), self.key_name(b)))
            }),
            // Text that is a name or prose: the same fold as the artist and
            // comment columns.
            SortColumn::Comment | SortColumn::Composer | SortColumn::AlbumArtist | SortColumn::Remixer
            | SortColumn::OriginalArtist | SortColumn::MixName | SortColumn::Lyricist | SortColumn::Message => {
                order.sort_by_cached_key(|&r| fold(self.sort_text(r, column)));
            }
            // A path keeps its separators and punctuation, which the general
            // fold drops: without them `A/z.mp3` would sort after `AB/a.mp3`
            // and a folder's tracks would no longer sit together.
            SortColumn::FileName | SortColumn::Location => {
                order.sort_by_cached_key(|&r| crate::strings::fold_smart(self.sort_text(r, column)));
            }
            // Dates are stored as `YYYY-MM-DD` text, which orders as it reads.
            SortColumn::DateAdded | SortColumn::ReleaseDate | SortColumn::DateCreated => {
                order.sort_by(|&a, &b| self.sort_text(a, column).cmp(self.sort_text(b, column)));
            }
            SortColumn::Bpm | SortColumn::Duration | SortColumn::Rating | SortColumn::PlayCount | SortColumn::Size
            | SortColumn::Year | SortColumn::SampleRate | SortColumn::Bitrate | SortColumn::Color
            | SortColumn::DiscNo | SortColumn::TrackNumber | SortColumn::FileType | SortColumn::BitDepth
            | SortColumn::PublishTrackInfo => {
                order.sort_by_key(|&r| self.sort_number(r, column));
            }
        }
        let mut rank = vec![0_u32; n];
        for (position, &row) in order.iter().enumerate() {
            if let Some(slot) = rank.get_mut(row as usize) {
                *slot = u32::try_from(position).unwrap_or(u32::MAX);
            }
        }
        rank
    }

    /// A row's text under a text column, as stored. Empty for a numeric
    /// column, or a lookup column whose text lives in an interner.
    pub(crate) fn sort_text(&self, row: Row, column: SortColumn) -> &str {
        let row = row as usize;
        if let Some(slot) = column.extra_slot() {
            return self.search_extra.get(slot).map_or("", |values| values.get(row));
        }
        match column {
            SortColumn::Comment => self.comment.get(row),
            SortColumn::FileName => self.file_name.get(row),
            // The resolved path. A cloud-library track's resolves to its
            // local copy, where the Location cell prints the `/contents_`
            // path rekordbox stores; every other track's is the same text.
            SortColumn::Location => self.folder_path.get(row),
            SortColumn::Lyricist => self.lyricist.get(row),
            SortColumn::Message => self.message.get(row),
            SortColumn::DateAdded => self.date_added.get(row),
            SortColumn::ReleaseDate => self.release_date.get(row),
            SortColumn::DateCreated => self.date_created.get(row),
            _ => "",
        }
    }

    /// A row's value under a numeric column, widened so one key type serves
    /// them all. 0 for a text column.
    pub(crate) fn sort_number(&self, row: Row, column: SortColumn) -> u64 {
        fn at<T: Copy + Into<u64>>(values: &[T], row: usize) -> u64 {
            values.get(row).map_or(0, |&value| value.into())
        }
        let row = row as usize;
        match column {
            SortColumn::Bpm => at(&self.bpm_x100, row),
            SortColumn::Duration => at(&self.length_sec, row),
            SortColumn::Rating => at(&self.rating, row),
            SortColumn::PlayCount => at(&self.play_count, row),
            SortColumn::Size => at(&self.file_size, row),
            SortColumn::Year => at(&self.year, row),
            SortColumn::SampleRate => at(&self.sample_rate, row),
            SortColumn::Bitrate => at(&self.bitrate, row),
            SortColumn::Color => at(&self.color, row),
            SortColumn::DiscNo => at(&self.disc_no, row),
            SortColumn::TrackNumber => at(&self.track_number, row),
            SortColumn::FileType => at(&self.file_type, row),
            SortColumn::BitDepth => at(&self.bit_depth, row),
            // Ticked first, as `comparePublic` orders it.
            SortColumn::PublishTrackInfo => u64::from(at(&self.publish, row) == 0),
            _ => 0,
        }
    }

    /// Free function: it reads only its arguments, not `self`.
    fn folded_lookup<'a>(interner: &'a crate::strings::Interner, ids: &[u32], row: Row) -> &'a str {
        interner.folded(ids.get(row as usize).copied().unwrap_or(crate::NO_ID))
    }

    /// Builds the folded search haystack. Called once at load.
    pub(crate) fn build_search(&mut self) {
        self.search = self.search_column();
    }

    fn search_column(&self) -> crate::strings::StrColumn {
        let mut search = crate::strings::StrColumn::with_capacity(self.count, self.count * 64);
        for row in 0..self.count {
            search.push(&self.search_text(row));
        }
        search
    }

    pub(crate) fn search_text(&self, row: usize) -> String {
        let year = self.year.get(row).filter(|&&n| n != 0).map_or_else(String::new, u16::to_string);
        let bpm = self.bpm_x100.get(row).filter(|&&n| n != 0).map_or_else(String::new, |n| format!("{}.{:02}", n / 100, n % 100));
        let values = [
            self.title.get(row), self.artists.name(self.artist.get(row).copied().unwrap_or(crate::NO_ID)),
            self.albums.name(self.album.get(row).copied().unwrap_or(crate::NO_ID)),
            self.genres.name(self.genre.get(row).copied().unwrap_or(crate::NO_ID)), &year, &bpm,
            self.search_extra[0].get(row), self.search_extra[1].get(row), self.search_extra[2].get(row),
            self.labels.name(self.label.get(row).copied().unwrap_or(crate::NO_ID)), self.comment.get(row),
            self.search_extra[3].get(row), self.search_extra[4].get(row),
        ];
        // Tabs delimit fields; embedded tabs are whitespace inside a value.
        values.iter().map(|value| fold(&value.replace('\t', " "))).collect::<Vec<_>>().join("\t")
    }
}

#[cfg(test)]
mod startup_tests {
    use crate::testing::{library_from, TestTrack};

    #[test]
    fn parallel_indexes_match_serial_indexes_including_ties() {
        let tracks: Vec<_> = (0..5000).map(|i| TestTrack {
            id: i, title: if i % 2 == 0 { "Écho" } else { "echo" },
            artist: if i % 3 == 0 { "A" } else { "B" },
            key: if i % 5 == 0 { "F#" } else { "F" },
            bpm_x100: 12000 + u32::try_from(i % 11).unwrap_or(0),
            ..TestTrack::default()
        }).collect();
        let mut lib = library_from(&tracks);
        let ranks = lib.ranks.clone();
        let search = lib.search.clone();
        lib.build_indexes();
        assert_eq!(lib.ranks, ranks);
        for row in 0..tracks.len() { assert_eq!(lib.search.get(row), search.get(row)); }
    }
}
