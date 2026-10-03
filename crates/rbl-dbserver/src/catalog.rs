//! What a database server needs from the library to answer a player.
//!
//! The server owns the protocol — request kinds, item layouts, paging — and
//! asks the catalog for rows. The catalog is implemented over the index in
//! `rbl-link`; the tests here use a small in-memory one.

use crate::item::TrackRow;

/// How a track list is ordered: the ids of the sort menu (`1400`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sort {
    Default,
    Alphabet,
    Artist,
    Album,
    Bpm,
    Rating,
    Key,
    Label,
    Genre,
    DateAdded,
    DjPlayCount,
}

/// The value shown beside a track title in player browse lists.
///
/// Rekordbox calls this the sub-column. The selected field determines both the
/// secondary text and the composite item type; sorting is configured
/// separately.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackColumn {
    Title,
    Album,
    Genre,
    Artist,
    Rating,
    Duration,
    Bpm,
    Label,
    Key,
    Bitrate,
    Color,
    #[default]
    Comment,
    OriginalArtist,
    Remixer,
    DjPlayCount,
    DateAdded,
}

impl TrackColumn {
    /// The render-time selector used by rekordbox track-list requests.
    pub fn from_id(id: u32) -> Self {
        match id {
            2 => Self::Artist,
            3 => Self::Album,
            4 => Self::Bpm,
            5 => Self::Rating,
            6 => Self::Genre,
            7 => Self::Comment,
            8 => Self::Duration,
            9 => Self::Remixer,
            10 => Self::Label,
            11 => Self::OriginalArtist,
            12 => Self::Key,
            13 => Self::Bitrate,
            15 => Self::Color,
            16 => Self::DjPlayCount,
            17 => Self::DateAdded,
            _ => Self::Title,
        }
    }
}

/// The artist-reference field used by an advanced browse category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtistRole {
    Original,
    Remixer,
}

impl Sort {
    pub const DEFAULTS: [Self; 7] = [
        Self::Default,
        Self::Alphabet,
        Self::Artist,
        Self::Album,
        Self::Bpm,
        Self::Rating,
        Self::Key,
    ];

    /// The secondary column an RX3 displays while this sort is active.
    pub fn track_column(self) -> Option<TrackColumn> {
        match self {
            Self::Default | Self::Alphabet => None,
            Self::Artist => Some(TrackColumn::Artist),
            Self::Album => Some(TrackColumn::Album),
            Self::Bpm => Some(TrackColumn::Bpm),
            Self::Rating => Some(TrackColumn::Rating),
            Self::Key => Some(TrackColumn::Key),
            Self::Label => Some(TrackColumn::Label),
            Self::Genre => Some(TrackColumn::Genre),
            Self::DateAdded => Some(TrackColumn::DateAdded),
            Self::DjPlayCount => Some(TrackColumn::DjPlayCount),
        }
    }

    pub fn from_id(id: u32) -> Self {
        match id {
            1 => Self::Alphabet,
            2 => Self::Artist,
            3 => Self::Album,
            4 => Self::Bpm,
            5 => Self::Rating,
            6 => Self::Genre,
            0xa => Self::Label,
            0xc => Self::Key,
            0x10 => Self::DjPlayCount,
            0x11 => Self::DateAdded,
            _ => Self::Default,
        }
    }
}

/// Which tracks a list holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TrackScope {
    All,
    FileName,
    Matching(u32),
    Bpm {
        bpm_x100: u32,
        tolerance_pct: u32,
    },
    Rating(u32),
    Bitrate(u32),
    Color(u32),
    DurationMinute(u32),
    ReleaseYear {
        decade: u32,
        year: Option<u32>,
    },
    TagList,
    /// A genre's tracks, optionally narrowed to an artist and album.
    Genre {
        genre: u32,
        artist: Option<u32>,
        album: Option<u32>,
    },
    /// A label's tracks, optionally narrowed to an artist and album.
    Label {
        label: u32,
        artist: Option<u32>,
        album: Option<u32>,
    },
    ArtistRole {
        role: ArtistRole,
        artist: u32,
        album: Option<u32>,
    },
    /// An artist's tracks, on one album or (`None`) all of them.
    Artist {
        artist: u32,
        album: Option<u32>,
    },
    Album(u32),
    /// Tracks in a key, widened by the related-key distance (0–2).
    Key {
        key: u32,
        distance: u32,
    },
    Playlist(u32),
    History(u32),
    /// Tracks added in a year, a month of it, or a day of that month.
    DateAdded {
        year: u32,
        month: Option<u32>,
        day: Option<u32>,
    },
    /// A player's text search: the string as typed (upper case).
    Search(String),
}

/// A menu whose rows come from the library.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Query {
    BpmBuckets,
    Ratings,
    Bitrates,
    Colors,
    DurationMinutes,
    ReleaseDecades,
    ReleaseYears(u32),
    Genres(Sort),
    /// The artists with tracks in a genre.
    GenreArtists(u32),
    /// The albums in a genre, optionally narrowed to an artist.
    GenreArtistAlbums {
        genre: u32,
        artist: Option<u32>,
    },
    Labels(Sort),
    /// The artists with tracks on a label.
    LabelArtists(u32),
    /// The albums on a label, optionally narrowed to an artist.
    LabelArtistAlbums {
        label: u32,
        artist: Option<u32>,
    },
    ArtistRoleArtists(ArtistRole),
    ArtistRoleAlbums {
        role: ArtistRole,
        artist: u32,
    },
    Artists(Sort),
    Albums(Sort),
    /// An artist's albums; the server puts `⟨ALL⟩` before them.
    ArtistAlbums(u32),
    /// A playlist folder's children; 0 is the root.
    Folder(u32),
    /// The HISTORY menu. rekordbox 7.2.11 lists only the history of the
    /// running link session there — none until a player has added a
    /// track — not the folders and sessions of its own history tree.
    Histories,
    Years,
    Months(u32),
    Days {
        year: u32,
        month: u32,
    },
    Tracks {
        scope: TrackScope,
        sort: Sort,
    },
}

/// One row of a library menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// An artist, album, genre, key or session: id and name.
    Named { id: u32, name: String },
    /// A playlist folder or list, with its position under its parent.
    List {
        id: u32,
        name: String,
        folder: bool,
        position: u32,
    },
    /// A year, month or day.
    Date(u32),
    /// A track by id, and its position where the list has one (a playlist's
    /// order, an album's track number), else 0. The row itself is fetched
    /// when it is rendered, so a 40,000-track list is a vector of ids.
    Track { id: u32, position: u32 },
}

/// Everything the metadata and track-info replies show about one track.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackDetails {
    pub row: TrackRow,
    /// `djmdContent.Commnt`.
    pub comment: String,
    /// Opaque `djmdContent.KeyID`, used by metadata replies.
    pub key_id: u32,
    /// `djmdKey.ScaleName`, independent of the browse-list sub-column.
    pub key_name: String,
    pub artist_id: u32,
    pub artist: String,
    pub album_id: u32,
    pub album: String,
    pub duration_s: u32,
    pub rating: u32,
    pub colour: u32,
    pub genre_id: u32,
    pub genre: String,
    /// `YYYY-MM-DD`.
    pub date_added: String,
    pub year: u32,
    pub bit_rate_kbps: u32,
    pub label_id: u32,
    pub label: String,
    pub original_artist: String,
    pub remixer: String,
    /// The absolute host path a player opens over NFS.
    pub path: String,
    pub file_size: u32,
    /// `djmdContent.FileType`; 0 when unknown.
    pub file_type: u32,
}

/// One Hot Cue Bank shown by the RX3.  A bank may be a folder containing
/// banks; leaf banks carry up to three cue points.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotCueBank {
    pub id: u32,
    pub name: String,
    pub folder: bool,
}

/// The settled, time-domain representation of one RX3 Hot Cue Bank slot.
/// The session owns conversion to the legacy 36-byte player record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotCueBankCue {
    /// One-based slot number; the RX3 supports slots 1 through 3.
    pub slot: u8,
    pub content: u32,
    pub in_ms: u32,
    pub out_ms: Option<u32>,
    pub color: u32,
    pub color_table_index: u32,
    pub active_loop: bool,
    pub beat_loop_size: u32,
    pub cue_microsec: u32,
}

/// One ordinary USB cue in the legacy `4702` reply.  The RX3 asks for this
/// list after applying a Hot Cue Bank edit, so the bank write is followed by
/// the edited track's own cues rather than by another bank-cue reply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UsbCue {
    /// A-C are 1-3; zero denotes a memory cue.
    pub slot: u8,
    pub in_ms: u32,
    pub out_ms: Option<u32>,
    pub color_table_index: u32,
}

/// The per-track blobs a player asks for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Analysis {
    /// A tag copied whole from the `.EXT` or `.2EX` analysis file, named by
    /// its fourcc (`b"PWV4"`) and the file's extension (`b"EXT"`).
    Tag {
        fourcc: [u8; 4],
        extension: [u8; 3],
    },
    BeatGrid,
    CueList,
    ExtendedCueList,
    WaveformPreview,
    WaveformDetail,
}

/// The library, as a player browses it.
pub trait Catalog: Send + Sync {
    /// Sort menu entries, in display order.
    fn sorts(&self) -> Vec<Sort> {
        Sort::DEFAULTS.to_vec()
    }

    /// Key menu display order; protocol identifiers remain unchanged.
    fn key_ids(&self) -> Vec<u32> {
        (1..=24).collect()
    }
    /// The rows of a menu, whole and in order.
    fn list(&self, query: &Query) -> Vec<Row>;

    /// A track row by id, optionally using a render-time column override.
    fn track_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow>;

    /// A track row whose primary text is its file name.
    fn file_name_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
        self.track_row(id, column)
    }

    /// The whole record, for metadata and track info.
    fn track(&self, id: u32) -> Option<TrackDetails>;

    /// Artwork bytes (JPEG) for the artwork id a title item carries, the
    /// way a player without the size argument asks (`[ctx, artwork]`).
    fn artwork(&self, id: u32) -> Option<Vec<u8>>;

    /// Artwork bytes (JPEG) for a menu item's own id: a track's, or an
    /// album's. A CDJ-3000 asking with the size argument (`[ctx, id, 1]`)
    /// names the row it drew, not the artwork field in it: every one of the
    /// 244 requests in a rekordbox 7.2.11 browse capture carried a track
    /// row's id (180) or an album row's (64), and rekordbox answered 135 of
    /// the track ones and 3 of the album ones with an image.
    fn item_artwork(&self, id: u32) -> Option<Vec<u8>>;

    /// A track's analysis blob, in the layout the reply carries.
    fn analysis(&self, track: u32, what: &Analysis) -> Option<Vec<u8>>;

    /// Hot Cue Banks below `parent`; `None` is the root.
    fn hot_cue_banks(&self, _parent: Option<u32>) -> Vec<HotCueBank> {
        Vec::new()
    }

    /// The up-to-three cue points in one Hot Cue Bank.
    fn hot_cue_bank_cues(&self, _bank: u32) -> Vec<HotCueBankCue> {
        Vec::new()
    }

    /// Tracks assigned to a Hot Cue Bank, in the bank's slot order.  The RX3
    /// uses this for `0x2001` mode 0; it is not a second bank hierarchy.
    fn hot_cue_bank_tracks(&self, _bank: u32) -> Vec<TrackRow> {
        Vec::new()
    }

    /// Legacy USB cues for a track, used by the follow-up to an RX3 Hot Cue
    /// Bank edit.  Only hot cues A-C fit this legacy representation.
    fn usb_cues(&self, _track: u32) -> Vec<UsbCue> {
        Vec::new()
    }

    /// Signed millisecond correction, separate from the original beat times.
    fn grid_offset(&self, _track: u32) -> i16 {
        0
    }

    /// Apply a player edit. Read-only catalogs refuse it explicitly.
    fn edit(&self, _edit: &Edit) -> bool {
        false
    }

    fn tagged(&self, _track: u32) -> bool {
        false
    }

    fn filter_rows(&self, rows: &mut Vec<Row>, filter: &crate::filter::TrackFilter) {
        if !filter.enabled {
            return;
        }
        rows.retain(|row| match row {
            Row::Track { id, .. } => self
                .track(*id)
                .is_some_and(|t| filter.matches(t.row.bpm_x100, t.row.key, t.rating, t.colour)),
            _ => true,
        });
    }

    /// Whether a player has loaded this track since the session began: the
    /// rows a player greys as played.
    fn played(&self, _track: u32) -> bool {
        false
    }
}

/// Library edits made from a player, acknowledged only after they succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Tag {
        track: u32,
        add: bool,
    },
    ClearTags,
    Rating {
        track: u32,
        stars: u8,
    },
    GridOffset {
        track: u32,
        offset_ms: i16,
    },
    /// A replacement received in RX3's legacy 36-byte Hot Cue Bank record.
    HotCueBankCue {
        bank: u32,
        cue: HotCueBankCue,
    },
    /// A player's play, for the history of this link session.
    HistoryAdd {
        track: u32,
    },
    /// Every play of a track off the link session's history.
    HistoryRemove {
        track: u32,
    },
    /// The player deleted a history: `u32::MAX` names the link session's own.
    HistoryDelete {
        history: u32,
    },
}
