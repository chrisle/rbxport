//! Menu rows as a player draws them: the sixteen-argument `4101` message.
//!
//! Every row a database server sends — a category heading, an artist, a
//! playlist folder, a track, a metadata line — is the same sixteen
//! arguments, and only a few of them are used by each kind of row. The
//! layouts here are the ones rekordbox 7.2.11 sent a CDJ-3000 (measured,
//! `docs/pre-release/design-notes/link-export-capture.md`); the tests hold
//! captured rows and check these build them byte for byte.

use crate::catalog::TrackColumn;
use crate::{kind, Argument, Message};

/// Item types, as the seventh argument names them.
pub mod item_type {
    pub const FOLDER: u32 = 0x01;
    pub const ALBUM: u32 = 0x02;
    pub const GENRE: u32 = 0x06;
    pub const ARTIST: u32 = 0x07;
    pub const PLAYLIST: u32 = 0x08;
    pub const RATING: u32 = 0x0a;
    pub const DURATION: u32 = 0x0b;
    pub const TEMPO: u32 = 0x0d;
    pub const LABEL: u32 = 0x0e;
    pub const KEY: u32 = 0x0f;
    /// Bitrate in kbps. Live Link Export uses `10` for this row.
    pub const BIT_RATE: u32 = 0x10;
    /// Release year. Live Link Export uses `11` for this row.
    pub const YEAR: u32 = 0x11;
    pub const COLOUR: u32 = 0x13;
    pub const COLOR_PINK: u32 = 0x14;
    pub const COLOR_RED: u32 = 0x15;
    pub const COLOR_ORANGE: u32 = 0x16;
    pub const COLOR_YELLOW: u32 = 0x17;
    pub const COLOR_GREEN: u32 = 0x18;
    pub const COLOR_AQUA: u32 = 0x19;
    pub const COLOR_BLUE: u32 = 0x1a;
    pub const COLOR_PURPLE: u32 = 0x1b;
    pub const COMMENT: u32 = 0x23;
    pub const HISTORY: u32 = 0x24;
    pub const ORIGINAL_ARTIST: u32 = 0x28;
    pub const REMIXER: u32 = 0x29;
    pub const DATE_ADDED: u32 = 0x2e;
    /// `djmdContent.FileType` (1 mp3, 4 m4a, 5 flac, 11 wav, 12 aiff), a
    /// row of the delivery info only.
    pub const FILE_TYPE: u32 = 0x12;
    /// Two text rows of the delivery info rekordbox sends empty; what the
    /// player would deliver in them is not known.
    pub const DELIVERY_TEXT_36: u32 = 0x36;
    pub const DELIVERY_TEXT_37: u32 = 0x37;
    /// The delivery info's closing row: the track id, a `1` in the ninth
    /// slot, and a KUVO id in the second text that rekordbox leaves empty.
    pub const DELIVERY_ID: u32 = 0x4f;
    /// A title-only track row, the shape the delivery info names the
    /// track in (the full row is [`TRACK`]).
    pub const TITLE: u32 = 0x04;
    /// A comment-secondary track row.
    pub const TRACK: u32 = 0x2304;
    /// The `⟨ALL⟩` row that heads a sub-menu.
    pub const ALL: u32 = 0xa0;
    /// The `2f` row in track info; meaning unknown, value copied.
    pub const INFO_UNKNOWN: u32 = 0x2f;
    /// A leaf Hot Cue Bank in the RX3's `0x2001` hierarchy.  It opens its
    /// assigned track list with mode 0, unlike a generic title row.
    pub const HOT_CUE_BANK: u32 = 0x2b;
    /// The path row in track info.
    pub const PATH: u32 = 0x00;
    // Root-menu categories and sort options carry their own codes.
    pub const MENU_GENRE: u32 = 0x80;
    pub const MENU_ARTIST: u32 = 0x81;
    pub const MENU_ALBUM: u32 = 0x82;
    pub const MENU_TRACK: u32 = 0x83;
    pub const MENU_PLAYLIST: u32 = 0x84;
    pub const MENU_BPM: u32 = 0x85;
    pub const MENU_RATING: u32 = 0x86;
    pub const MENU_YEAR: u32 = 0x87;
    pub const MENU_REMIXER: u32 = 0x88;
    pub const MENU_LABEL: u32 = 0x89;
    pub const MENU_ORIGINAL_ARTIST: u32 = 0x8a;
    pub const MENU_KEY: u32 = 0x8b;
    pub const MENU_DATE_ADDED: u32 = 0x8c;
    pub const MENU_COLOR: u32 = 0x8e;
    pub const MENU_SEARCH: u32 = 0x91;
    pub const MENU_TIME: u32 = 0x92;
    pub const MENU_BITRATE: u32 = 0x93;
    pub const MENU_FILE_NAME: u32 = 0x94;
    pub const MENU_HISTORY: u32 = 0x95;
    pub const MENU_DJ_PLAY_COUNT: u32 = 0x97;
    pub const MENU_HOT_CUE_BANK: u32 = 0x98;
    pub const MENU_DEFAULT: u32 = 0xa1;
    pub const MENU_ALPHABET: u32 = 0xa2;
    pub const MENU_MATCHING: u32 = 0xaa;
}

/// Bits of a track row's flags, measured from rekordbox 7.2.11 serving a
/// CDJ-3000.
pub mod track_flags {
    /// The track has been loaded on a player this session; the player greys
    /// the row. Set on every row of a history, and on a metadata row.
    pub const PLAYED: u32 = 0x100;
    /// Set on the rows of an artist's, an album's or a playlist's tracks and
    /// on no other list; what it tells the player is not known.
    pub const LISTED: u32 = 0x0100_0000;
}

/// The sixteen arguments of one row.
///
/// `text`, `text2` and `text3` each go out preceded by their byte length in
/// UTF-16 including the NUL, which is what the player counts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Item {
    pub a: u32,
    pub id: u32,
    pub text: String,
    pub text2: String,
    pub item_type: u32,
    pub flags: u32,
    pub c: u32,
    pub d: u32,
    pub e: u32,
    pub key: u32,
    pub art: u32,
    pub text3: String,
    pub f: u32,
}

/// UTF-16 byte length of a string including its NUL.
fn utf16_len(text: &str) -> u32 {
    let units = text.encode_utf16().count() + 1;
    u32::try_from(units * 2).unwrap_or(u32::MAX)
}

impl Item {
    /// The row as a message under `transaction`.
    pub fn message(&self, transaction: u32) -> Message {
        Message::new(
            transaction,
            kind::MENU_ITEM,
            vec![
                Argument::Number(self.a),
                Argument::Number(self.id),
                Argument::Number(utf16_len(&self.text)),
                Argument::String(self.text.clone()),
                Argument::Number(utf16_len(&self.text2)),
                Argument::String(self.text2.clone()),
                Argument::Number(self.item_type),
                Argument::Number(self.flags),
                Argument::Number(self.c),
                Argument::Number(self.d),
                Argument::Number(self.e),
                Argument::Number(self.key),
                Argument::Number(self.art),
                Argument::Number(utf16_len(&self.text3)),
                Argument::String(self.text3.clone()),
                Argument::Number(self.f),
            ],
        )
    }

    /// A heading the player localises: the label wrapped in U+FFFA / U+FFFB.
    pub fn heading(id: u32, label: &str, item_type: u32) -> Self {
        Self {
            id,
            text: format!("\u{fffa}{label}\u{fffb}"),
            item_type,
            ..Self::default()
        }
    }

    /// The `⟨ALL⟩` row that heads an artist's albums, a year's months and a
    /// month's days.
    pub fn all() -> Self {
        Self::heading(0xffff_ffff, "ALL", item_type::ALL)
    }

    /// A named row that carries its id again in the ninth slot: an artist or
    /// an album.
    pub fn named_twice(id: u32, name: &str, item_type: u32) -> Self {
        Self {
            id,
            text: name.to_owned(),
            item_type,
            c: id,
            ..Self::default()
        }
    }

    /// A named row with nothing but its id: a key, a history session, a
    /// genre.
    pub fn named(id: u32, name: &str, item_type: u32) -> Self {
        Self {
            id,
            text: name.to_owned(),
            item_type,
            ..Self::default()
        }
    }

    /// A playlist folder or list, with its position under its parent.
    pub fn list(id: u32, name: &str, folder: bool, position: u32) -> Self {
        Self {
            id,
            text: name.to_owned(),
            item_type: if folder {
                item_type::FOLDER
            } else {
                item_type::PLAYLIST
            },
            d: position,
            ..Self::default()
        }
    }

    /// A year, month or day under DATE ADDED: a number with an empty name.
    pub fn date_part(value: u32) -> Self {
        Self {
            id: value,
            item_type: item_type::DATE_ADDED,
            ..Self::default()
        }
    }

    /// A track row.
    pub fn track(row: &TrackRow, flags: u32, position: u32) -> Self {
        let a = if matches!(
            row.column,
            TrackColumn::Comment | TrackColumn::DateAdded
        ) {
            row.id
        } else {
            row.column_value
        };
        Self {
            a,
            id: row.id,
            text: row.title.clone(),
            text2: row.secondary_text.clone(),
            item_type: track_item_type(row.column, row.column_value),
            flags,
            c: row.id,
            d: position,
            e: 0x100,
            key: row.key,
            art: row.key_id,
            text3: row.key_name.clone(),
            f: row.bpm_x100,
        }
    }

    /// A metadata line: `[a, id, text, type]`.
    pub fn line(a: u32, id: u32, text: &str, item_type: u32) -> Self {
        Self {
            a,
            id,
            text: text.to_owned(),
            item_type,
            ..Self::default()
        }
    }

    /// A numeric selector whose label is formatted by the player.
    pub fn number(id: u32, item_type: u32) -> Self {
        Self {
            id,
            item_type,
            ..Self::default()
        }
    }
}

/// What a track row shows: enough for the list, not the whole record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackRow {
    /// `djmdContent.ID`.
    pub id: u32,
    pub title: String,
    /// Formatted value shown in the right-hand column.
    pub secondary_text: String,
    pub column: TrackColumn,
    /// Raw database id or numeric value backing the selected column.
    pub column_value: u32,
    /// Camelot index 1–24 (see [`crate::keys`]), 0 when unknown.
    pub key: u32,
    /// Original `djmdContent.KeyID`, independent of the selected column.
    pub key_id: u32,
    pub key_name: String,
    pub bpm_x100: u32,
}

impl TrackRow {
    /// Formats the paired Key/BPM summary used for an RX3's active sort
    /// column. Explicit render overrides retain their raw extractor shape.
    pub fn format_sort_column(&mut self) {
        let bpm = format_bpm(self.bpm_x100);
        let key = if self.key == 0 { "" } else { &self.key_name };

        self.secondary_text = match self.column {
            TrackColumn::Bpm => join_secondary(&bpm, key),
            TrackColumn::Key => join_secondary(key, &bpm),
            _ => return,
        };
    }
}

fn format_bpm(bpm_x100: u32) -> String {
    if bpm_x100 == 0 {
        return String::new();
    }

    let tenths = bpm_x100.saturating_add(5) / 10;
    format!("{}.{:01} bpm", tenths / 10, tenths % 10)
}

fn join_secondary(primary: &str, secondary: &str) -> String {
    match (primary.is_empty(), secondary.is_empty()) {
        (true, _) => secondary.to_owned(),
        (_, true) => primary.to_owned(),
        (false, false) => format!("{primary} - {secondary}"),
    }
}

fn track_item_type(column: TrackColumn, value: u32) -> u32 {
    let secondary = match column {
        TrackColumn::Title => 0,
        TrackColumn::Album => item_type::ALBUM,
        TrackColumn::Genre => item_type::GENRE,
        TrackColumn::Artist => item_type::ARTIST,
        TrackColumn::Rating => item_type::RATING,
        TrackColumn::Duration => item_type::DURATION,
        TrackColumn::Bpm => item_type::TEMPO,
        TrackColumn::Label => item_type::LABEL,
        TrackColumn::Key => item_type::KEY,
        TrackColumn::Bitrate => item_type::BIT_RATE,
        TrackColumn::Color => item_type::COLOUR + value.min(8),
        TrackColumn::Comment => item_type::COMMENT,
        TrackColumn::OriginalArtist => item_type::ORIGINAL_ARTIST,
        TrackColumn::Remixer => item_type::REMIXER,
        TrackColumn::DjPlayCount => 0x2a,
        TrackColumn::DateAdded => item_type::DATE_ADDED,
    };
    (secondary << 8) | item_type::TITLE
}

/// The categories advertised by the capability mask in a root-menu request,
/// in rekordbox's display order.
pub fn root_menu(capabilities: u32) -> Vec<Item> {
    [
        (3, Item::heading(0x04, "TRACK", item_type::MENU_TRACK)),
        (10, Item::heading(0x0c, "KEY", item_type::MENU_KEY)),
        (4, Item::heading(0x06, "BPM", item_type::MENU_BPM)),
        (0, Item::heading(0x01, "GENRE", item_type::MENU_GENRE)),
        (1, Item::heading(0x02, "ARTIST", item_type::MENU_ARTIST)),
        (2, Item::heading(0x03, "ALBUM", item_type::MENU_ALBUM)),
        (
            26,
            Item::heading(0x1a, "MATCHING", item_type::MENU_MATCHING),
        ),
        (19, Item::heading(0x12, "SEARCH", item_type::MENU_SEARCH)),
        (
            16,
            Item::heading(0x05, "PLAYLIST", item_type::MENU_PLAYLIST),
        ),
        (18, Item::heading(0x16, "HISTORY", item_type::MENU_HISTORY)),
        (14, Item::heading(0x14, "BITRATE", item_type::MENU_BITRATE)),
        (12, Item::heading(0x0f, "COLOR", item_type::MENU_COLOR)),
        (
            15,
            Item::heading(0x15, "FILE NAME", item_type::MENU_FILE_NAME),
        ),
        (
            17,
            Item::heading(0x17, "HOT CUE BANK", item_type::MENU_HOT_CUE_BANK),
        ),
        (8, Item::heading(0x0a, "LABEL", item_type::MENU_LABEL)),
        (
            9,
            Item::heading(0x0b, "ORIGINAL ARTIST", item_type::MENU_ORIGINAL_ARTIST),
        ),
        (5, Item::heading(0x07, "RATING", item_type::MENU_RATING)),
        (7, Item::heading(0x09, "REMIXER", item_type::MENU_REMIXER)),
        (13, Item::heading(0x13, "TIME", item_type::MENU_TIME)),
        (6, Item::heading(0x08, "YEAR", item_type::MENU_YEAR)),
    ]
    .into_iter()
    .filter_map(|(bit, item)| (capabilities & (1 << bit) != 0).then_some(item))
    .collect()
}

/// The sort options a track list offers, in rekordbox's order. The id is
/// what a later track-menu request carries as its sort.
pub fn sort_menu(sorts: &[crate::catalog::Sort]) -> Vec<Item> {
    use crate::catalog::Sort;

    sorts
        .iter()
        .map(|sort| match sort {
            Sort::Default => Item::heading(0x00, "DEFAULT", item_type::MENU_DEFAULT),
            Sort::Alphabet => Item::heading(0x01, "ALPHABET", item_type::MENU_ALPHABET),
            Sort::Artist => Item::heading(0x02, "ARTIST", item_type::MENU_ARTIST),
            Sort::Album => Item::heading(0x03, "ALBUM", item_type::MENU_ALBUM),
            Sort::Bpm => Item::heading(0x04, "BPM", item_type::MENU_BPM),
            Sort::Rating => Item::heading(0x05, "RATING", item_type::MENU_RATING),
            Sort::Genre => Item::heading(0x06, "GENRE", item_type::GENRE),
            Sort::Label => Item::heading(0x0a, "LABEL", item_type::MENU_LABEL),
            Sort::Key => Item::heading(0x0c, "KEY", item_type::MENU_KEY),
            Sort::DjPlayCount => {
                Item::heading(0x10, "DJ PLAY COUNT", item_type::MENU_DJ_PLAY_COUNT)
            }
            Sort::DateAdded => Item::heading(0x11, "DATE ADDED", item_type::MENU_DATE_ADDED),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_columns_use_rekordbox_composite_types() {
        let columns = [
            (TrackColumn::Title, 0x0004),
            (TrackColumn::Album, 0x0204),
            (TrackColumn::Genre, 0x0604),
            (TrackColumn::Artist, 0x0704),
            (TrackColumn::Rating, 0x0a04),
            (TrackColumn::Duration, 0x0b04),
            (TrackColumn::Bpm, 0x0d04),
            (TrackColumn::Label, 0x0e04),
            (TrackColumn::Key, 0x0f04),
            (TrackColumn::Bitrate, 0x1004),
            (TrackColumn::Color, 0x1a04),
            (TrackColumn::Comment, 0x2304),
            (TrackColumn::OriginalArtist, 0x2804),
            (TrackColumn::Remixer, 0x2904),
            (TrackColumn::DjPlayCount, 0x2a04),
            (TrackColumn::DateAdded, 0x2e04),
        ];

        for (column, expected) in columns {
            let value = if column == TrackColumn::Color { 7 } else { 0 };
            assert_eq!(track_item_type(column, value), expected);
        }
    }

    #[test]
    fn every_track_column_carries_the_raw_database_key_id() {
        let mut row = TrackRow {
            id: 42,
            title: "Track".into(),
            secondary_text: "3A - 112.2 bpm".into(),
            column: TrackColumn::Key,
            column_value: 3_441_880_869,
            key: 5,
            key_id: 3_441_880_869,
            key_name: "Ebm".into(),
            bpm_x100: 11_220,
        };
        let item = Item::track(&row, 0, 0);

        assert_eq!(item.a, 3_441_880_869);
        assert_eq!(item.art, 3_441_880_869);
        assert_eq!(item.item_type, 0x0f04);
        assert_eq!(item.text2, "3A - 112.2 bpm");

        row.column = TrackColumn::Comment;
        row.column_value = row.id;
        row.secondary_text = "note".into();
        let item = Item::track(&row, 0, 0);

        assert_eq!(item.a, row.id);
        assert_eq!(item.art, 3_441_880_869);
        assert_eq!(item.item_type, 0x2304);

        row.column = TrackColumn::DateAdded;
        row.secondary_text = "2026-10-02".into();
        let item = Item::track(&row, 0, 0);

        assert_eq!(item.a, row.id);
        assert_eq!(item.item_type, 0x2e04);
    }

    #[test]
    fn rx3_key_and_bpm_sorts_use_opposite_composite_order() {
        let mut row = TrackRow {
            id: 42,
            title: "Track".into(),
            secondary_text: String::new(),
            column: TrackColumn::Bpm,
            column_value: 13_000,
            key: 5,
            key_id: 5001,
            key_name: "3A".into(),
            bpm_x100: 13_000,
        };

        row.format_sort_column();
        assert_eq!(row.secondary_text, "130.0 bpm - 3A");

        row.column = TrackColumn::Key;
        row.format_sort_column();
        assert_eq!(row.secondary_text, "3A - 130.0 bpm");
    }
}
