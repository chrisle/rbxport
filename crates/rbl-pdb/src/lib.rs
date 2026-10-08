//! Reader for the `DeviceSQL` database rekordbox writes to USB and SD media
//! (`PIONEER/rekordbox/export.pdb`).
//!
//! The file is fixed-size pages. Page 0 is a header listing the tables, each of
//! which is a linked list of pages. Within a page, rows are scattered through a
//! heap and located by an index built *backwards* from the end of the page, with
//! a presence bitmap — deleted rows leave their offsets behind, so a reader that
//! ignores the bitmap will parse garbage.
//!
//! Layouts follow the community Kaitai spec (`rekordbox_pdb.ksy`), and are
//! validated against a real rekordbox-authored export.

pub mod build;
pub mod reference;
pub mod rows;

use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum PdbError {
    #[error("file is too small to be a DeviceSQL database")]
    TooSmall,
    #[error("implausible page size {0}")]
    BadPageSize(u32),
    #[error("page {0} is past the end of the file")]
    PageOutOfRange(u32),
}

pub type Result<T> = std::result::Result<T, PdbError>;

/// Table kinds we understand. Everything else is carried as its raw number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PageType {
    Tracks,
    Genres,
    Artists,
    Albums,
    Labels,
    Keys,
    Colors,
    PlaylistTree,
    PlaylistEntries,
    Artwork,
    Columns,
    HistoryPlaylists,
    HistoryEntries,
    /// Type 19: the device name, track count, date and Device Library
    /// background colour. See [`rows::PdbProperty`].
    Property,
    Other(u32),
}

impl PageType {
    fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Tracks,
            1 => Self::Genres,
            2 => Self::Artists,
            3 => Self::Albums,
            4 => Self::Labels,
            5 => Self::Keys,
            6 => Self::Colors,
            7 => Self::PlaylistTree,
            8 => Self::PlaylistEntries,
            13 => Self::Artwork,
            16 => Self::Columns,
            17 => Self::HistoryPlaylists,
            18 => Self::HistoryEntries,
            19 => Self::Property,
            other => Self::Other(other),
        }
    }

    pub fn name(self) -> String {
        match self {
            Self::Tracks => "tracks".into(),
            Self::Genres => "genres".into(),
            Self::Artists => "artists".into(),
            Self::Albums => "albums".into(),
            Self::Labels => "labels".into(),
            Self::Keys => "keys".into(),
            Self::Colors => "colors".into(),
            Self::PlaylistTree => "playlist_tree".into(),
            Self::PlaylistEntries => "playlist_entries".into(),
            Self::Artwork => "artwork".into(),
            Self::Columns => "columns".into(),
            Self::HistoryPlaylists => "history_playlists".into(),
            Self::HistoryEntries => "history_entries".into(),
            Self::Property => "property".into(),
            Self::Other(v) => format!("unknown_{v}"),
        }
    }
}

/// A table's page chain.
#[derive(Debug, Clone, Copy)]
pub struct TableRef {
    pub page_type: PageType,
    pub first_page: u32,
    pub last_page: u32,
}

/// A row, still located in the file: the caller decodes the fields it needs.
#[derive(Debug, Clone, Copy)]
pub struct RowRef {
    /// Offset of the row's first byte from the start of the file.
    pub offset: usize,
    /// Start of the page holding the row, for resolving string pointers.
    pub page_offset: usize,
}

pub struct Pdb<'a> {
    bytes: &'a [u8],
    pub page_size: u32,
    pub tables: Vec<TableRef>,
}

const PAGE_HEADER_LEN: usize = 0x28;

fn u1(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}
fn u2(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([u1(b, at), u1(b, at + 1)])
}
pub(crate) fn u4(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([u1(b, at), u1(b, at + 1), u1(b, at + 2), u1(b, at + 3)])
}

impl<'a> Pdb<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < 28 {
            return Err(PdbError::TooSmall);
        }
        let page_size = u4(bytes, 4);
        // Pages are a power of two in a sane range; anything else means we are
        // not looking at a DeviceSQL file.
        if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
            return Err(PdbError::BadPageSize(page_size));
        }
        let num_tables = u4(bytes, 8) as usize;

        let mut tables = Vec::with_capacity(num_tables.min(64));
        for i in 0..num_tables.min(64) {
            let at = 28 + i * 16;
            if at + 16 > bytes.len() {
                break;
            }
            tables.push(TableRef {
                page_type: PageType::from_u32(u4(bytes, at)),
                first_page: u4(bytes, at + 8),
                last_page: u4(bytes, at + 12),
            });
        }

        Ok(Self { bytes, page_size, tables })
    }

    fn page_offset(&self, index: u32) -> Option<usize> {
        let offset = (index as usize).checked_mul(self.page_size as usize)?;
        (offset + PAGE_HEADER_LEN <= self.bytes.len()).then_some(offset)
    }

    /// Walks a table's page chain, yielding every present row.
    ///
    /// Rows whose presence bit is clear are skipped: their offsets remain in the
    /// index after a deletion and the bytes they point at are not valid data.
    pub fn rows(&self, table: &TableRef) -> Vec<RowRef> {
        let mut out = Vec::new();
        let mut page_index = table.first_page;
        let mut guard = 0_u32;

        loop {
            guard += 1;
            // A corrupt next-page pointer must not spin forever.
            if guard > 100_000 {
                break;
            }
            let Some(page) = self.page_offset(page_index) else { break };
            let p = self.bytes.get(page..).unwrap_or(&[]);

            let page_flags = u1(p, 0x1b);
            let is_data_page = page_flags & 0x40 == 0;
            let num_rows_small = u32::from(u1(p, 0x18));
            let num_rows_large = u32::from(u2(p, 0x22));
            let num_rows = if num_rows_large > num_rows_small && num_rows_large != 0x1fff {
                num_rows_large
            } else {
                num_rows_small
            };

            if is_data_page && num_rows > 0 {
                let groups = (num_rows - 1) / 16 + 1;
                for group in 0..groups {
                    // Groups build backwards from the end of the page.
                    let base = self.page_size as usize - (group as usize * 0x24);
                    let flags_at = page + base.wrapping_sub(4);
                    let present = u2(self.bytes, flags_at);
                    let in_group =
                        if group < groups - 1 { 16 } else { (num_rows - 1) % 16 + 1 };
                    for row in 0..in_group {
                        if present & (1 << row) == 0 {
                            continue; // deleted: index entry survives, data does not
                        }
                        let ofs_at = page + base.wrapping_sub(6 + 2 * row as usize);
                        let row_offset = u2(self.bytes, ofs_at) as usize;
                        let absolute = page + PAGE_HEADER_LEN + row_offset;
                        if absolute < self.bytes.len() {
                            out.push(RowRef { offset: absolute, page_offset: page });
                        }
                    }
                }
            }

            let next = u4(p, 0x0c);
            if page_index == table.last_page || next == page_index || next == 0 {
                break;
            }
            page_index = next;
        }
        out
    }

    /// Reads a `DeviceSQL` string at an absolute offset.
    ///
    /// Three encodings share one field: a short ASCII form with a mangled
    /// length byte, and long ASCII / UTF-16LE forms flagged by `0x40` / `0x90`.
    pub fn string_at(&self, offset: usize) -> String {
        let b = self.bytes;
        match u1(b, offset) {
            0x40 => {
                // Flag, u16 length counting the four header bytes, a pad
                // byte, then the text; see `build::long_ascii`.
                let len = u2(b, offset + 1) as usize;
                let start = offset + 4;
                b.get(start..start + len.saturating_sub(4))
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .unwrap_or_default()
            }
            0x90 => {
                let len = u2(b, offset + 1) as usize;
                // Length counts the header and two trailing NULs.
                let start = offset + 4;
                let take = len.saturating_sub(4);
                let raw = b.get(start..start + take).unwrap_or(&[]);
                let units: Vec<u16> = raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&u| u != 0)
                    .collect();
                String::from_utf16_lossy(&units)
            }
            mangled if mangled % 2 == 1 => {
                // "incremented, doubled, and incremented again"
                let len = ((mangled as usize).saturating_sub(1) / 2).saturating_sub(1);
                b.get(offset + 1..offset + 1 + len)
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    /// A string located by a two-byte offset stored in the row.
    ///
    /// The offset is relative to the start of the **row**, not the page — the
    /// spec's `pos: _parent.row_base + ofs`. Reading it relative to the page
    /// yields plausible-looking fragments of neighbouring strings, which is
    /// exactly how this was first got wrong.
    pub fn string_ref(&self, row: RowRef, ptr_at_row_offset: usize) -> String {
        let ptr = u2(self.bytes, row.offset + ptr_at_row_offset) as usize;
        if ptr == 0 {
            return String::new();
        }
        self.string_at(row.offset + ptr)
    }

    pub fn u1_at(&self, row: RowRef, at: usize) -> u8 {
        u1(self.bytes, row.offset + at)
    }
    pub fn u2_at(&self, row: RowRef, at: usize) -> u16 {
        u2(self.bytes, row.offset + at)
    }
    pub fn u4_at(&self, row: RowRef, at: usize) -> u32 {
        u4(self.bytes, row.offset + at)
    }

    /// Row counts per table, which is the cheapest useful summary of a file.
    pub fn census(&self) -> BTreeMap<String, usize> {
        self.tables
            .iter()
            .map(|t| (t.page_type.name(), self.rows(t).len()))
            .collect()
    }
}

/// A row from a name table (`artists`, `genres`, `labels`, `keys`, `colors`,
/// `albums`). Their layouts differ, so each has its own decoder.
#[derive(Debug, Clone)]
pub struct NamedRow {
    pub id: u32,
    pub name: String,
}

/// The subset of a track row the application uses.
#[derive(Debug, Clone, Default)]
pub struct TrackRow {
    pub hot_cue_auto_load: bool,
    pub sample_depth: u16,
    pub disc_number: u16,
    pub id: u32,
    pub artist_id: u32,
    pub album_id: u32,
    pub genre_id: u32,
    pub key_id: u32,
    pub label_id: u32,
    pub artwork_id: u32,
    pub color_id: u8,
    pub rating: u8,
    /// BPM x100.
    pub tempo_x100: u32,
    pub duration_sec: u16,
    pub year: u16,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub file_size: u32,
    pub track_number: u32,
    pub play_count: u16,
    pub title: String,
    /// Path on the media, relative to its root.
    pub file_path: String,
    pub filename: String,
    pub analyze_path: String,
    pub date_added: String,
    pub release_date: String,
    pub comment: String,
}

#[derive(Debug, Clone)]
pub struct PlaylistNode {
    pub id: u32,
    pub parent_id: u32,
    pub sort_order: u32,
    pub is_folder: bool,
    pub name: String,
}

#[derive(Debug, Clone, Copy)]
pub struct PlaylistEntry {
    pub entry_index: u32,
    pub track_id: u32,
    pub playlist_id: u32,
}

impl Pdb<'_> {
    /// `genres` and `labels`: u4 id then an inline string.
    fn simple_named(&self, table: &TableRef, id_at: usize, name_at: usize) -> Vec<NamedRow> {
        self.rows(table)
            .into_iter()
            .map(|row| NamedRow {
                id: self.u4_at(row, id_at),
                name: self.string_at(row.offset + name_at),
            })
            .collect()
    }

    /// Actual played-history tables are types 11/12; the reference tables
    /// at 17/18 are browse settings, despite their historical enum names.
    pub fn played_histories(&self) -> Vec<NamedRow> {
        self.table(PageType::Other(11)).map(|t| self.simple_named(t, 0, 4)).unwrap_or_default()
    }
    pub fn played_history_entries(&self) -> Vec<PlaylistEntry> {
        self.table(PageType::Other(12)).map(|t| self.rows(t).into_iter().map(|r| PlaylistEntry {
            track_id: self.u4_at(r,0), playlist_id: self.u4_at(r,4), entry_index: self.u4_at(r,8),
        }).collect()).unwrap_or_default()
    }

    /// Decodes a name table, dispatching on its layout.
    pub fn named_rows(&self, table: &TableRef) -> Vec<NamedRow> {
        match table.page_type {
            // Artwork rows name a path where a genre names a genre.
            PageType::Genres | PageType::Labels | PageType::Artwork => self.simple_named(table, 0, 4),
            // Keys carry the id twice.
            PageType::Keys => self.simple_named(table, 0, 8),
            // Colors: five pad bytes, u2 id, one pad byte, then the name.
            PageType::Colors => self
                .rows(table)
                .into_iter()
                .map(|row| NamedRow {
                    id: u32::from(self.u2_at(row, 5)),
                    name: self.string_at(row.offset + 8),
                })
                .collect(),
            // Artists and albums point at their name by offset, and artists use
            // a two-byte offset when the subtype says the name is far away.
            PageType::Artists => self
                .rows(table)
                .into_iter()
                .map(|row| {
                    let subtype = self.u2_at(row, 0);
                    let ofs = if subtype == 0x64 {
                        self.u2_at(row, 0x0a) as usize
                    } else {
                        usize::from(self.u1_at(row, 0x09))
                    };
                    NamedRow {
                        id: self.u4_at(row, 4),
                        name: if ofs == 0 { String::new() } else { self.string_at(row.offset + ofs) },
                    }
                })
                .collect(),
            PageType::Albums => self
                .rows(table)
                .into_iter()
                .map(|row| {
                    let ofs = usize::from(self.u1_at(row, 0x15));
                    NamedRow {
                        id: self.u4_at(row, rows::ALBUM_ID_AT),
                        name: if ofs == 0 { String::new() } else { self.string_at(row.offset + ofs) },
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Decodes track rows.
    ///
    /// Fixed fields occupy 0x00..0x5e; the 21 string offsets follow at 0x5e.
    pub fn track_rows(&self, table: &TableRef) -> Vec<TrackRow> {
        // Slot numbers from the spec's `ofs_strings` index.
        const DATE_ADDED: usize = 10;
        const RELEASE_DATE: usize = 11;
        const ANALYZE_PATH: usize = 14;
        const COMMENT: usize = 16;
        const TITLE: usize = 17;
        const FILENAME: usize = 19;
        const FILE_PATH: usize = 20;

        self.rows(table)
            .into_iter()
            .map(|row| {
                let text = |slot: usize| self.string_ref(row, 0x5e + slot * 2);
                TrackRow {
                    hot_cue_auto_load: text(7) == "ON",
                    sample_depth: self.u2_at(row, 0x52),
                    disc_number: self.u2_at(row, 0x4c),
                    sample_rate: self.u4_at(row, 0x08),
                    file_size: self.u4_at(row, 0x10),
                    artwork_id: self.u4_at(row, 0x1c),
                    key_id: self.u4_at(row, 0x20),
                    label_id: self.u4_at(row, 0x28),
                    bitrate: self.u4_at(row, 0x30),
                    track_number: self.u4_at(row, 0x34),
                    tempo_x100: self.u4_at(row, 0x38),
                    genre_id: self.u4_at(row, 0x3c),
                    album_id: self.u4_at(row, 0x40),
                    artist_id: self.u4_at(row, 0x44),
                    id: self.u4_at(row, 0x48),
                    play_count: self.u2_at(row, 0x4e),
                    year: self.u2_at(row, 0x50),
                    duration_sec: self.u2_at(row, 0x54),
                    color_id: self.u1_at(row, 0x58),
                    rating: self.u1_at(row, 0x59),
                    date_added: text(DATE_ADDED),
                    release_date: text(RELEASE_DATE),
                    analyze_path: text(ANALYZE_PATH),
                    comment: text(COMMENT),
                    title: text(TITLE),
                    filename: text(FILENAME),
                    file_path: text(FILE_PATH),
                }
            })
            .collect()
    }

    /// Decodes the playlist folder/list tree.
    pub fn playlist_nodes(&self, table: &TableRef) -> Vec<PlaylistNode> {
        self.rows(table)
            .into_iter()
            // Five words before the name; see `rows::playlist_row` for the
            // row rekordbox writes.
            .map(|row| PlaylistNode {
                parent_id: self.u4_at(row, 0),
                sort_order: self.u4_at(row, 8),
                id: self.u4_at(row, 12),
                is_folder: self.u4_at(row, 16) != 0,
                name: self.string_at(row.offset + rows::PLAYLIST_NAME_AT),
            })
            .collect()
    }

    /// Decodes playlist membership.
    pub fn playlist_entries(&self, table: &TableRef) -> Vec<PlaylistEntry> {
        self.rows(table)
            .into_iter()
            .map(|row| PlaylistEntry {
                entry_index: self.u4_at(row, 0),
                track_id: self.u4_at(row, 4),
                playlist_id: self.u4_at(row, 8),
            })
            .collect()
    }

    /// Finds a table by kind.
    pub fn table(&self, kind: PageType) -> Option<&TableRef> {
        self.tables.iter().find(|t| t.page_type == kind)
    }

    /// The live `property` row (type 19). `None` when the table or its row
    /// is missing, or the row does not have the known layout. When more
    /// than one row is live, the last one is used: rekordbox adds a row for
    /// each change.
    #[must_use]
    pub fn property(&self) -> Option<rows::PdbProperty> {
        let row = *self.rows(self.table(PageType::Property)?).last()?;
        if self.u2_at(row, 0) != 0x0280 {
            return None;
        }
        let date_at = row.offset + rows::PROPERTY_DATE_AT;
        let gap_at = date_at + self.string_len_at(date_at)?;
        if self.bytes.get(gap_at..gap_at + 2)? != rows::PROPERTY_GAP {
            return None;
        }
        let version_at = gap_at + 2;
        let name_at = version_at + self.string_len_at(version_at)?;
        self.string_len_at(name_at)?;
        Some(rows::PdbProperty {
            device_name: self.string_at(name_at),
            db_version: self.string_at(version_at),
            contents: self.u4_at(row, 4),
            created_date: self.string_at(date_at),
            background_color: self.u1_at(row, rows::PROPERTY_COLOR_AT),
        })
    }

    /// The encoded length of the `DeviceSQL` string at `offset`, header
    /// included. `None` when the header is not a string header or the
    /// string runs past the end of the file.
    fn string_len_at(&self, offset: usize) -> Option<usize> {
        let len = match *self.bytes.get(offset)? {
            0x40 | 0x90 => usize::from(u2(self.bytes, offset + 1)),
            mangled if mangled % 2 == 1 => usize::from(mangled.saturating_sub(1) / 2),
            _ => return None,
        };
        (len > 0 && offset + len <= self.bytes.len()).then_some(len)
    }
}
