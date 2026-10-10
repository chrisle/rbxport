//! `PIONEER/rekordbox/exportExt.pdb`: the second `DeviceSQL` database on a
//! stick, which carries the library's My Tags and which tracks carry them,
//! for a player's Track Filter.
//!
//! The tables are rekordbox's `DJDBEX*` tables, numbered in the order
//! rekordbox creates them [OBS static, rekordbox 7.2.19 arm64
//! `_edb_MAS_MODULE1_table_inits` @0x100a1a38c]:
//!
//! | type | table | here |
//! |---|---|---|
//! | 0 | `DJDBEXRECOMMENDLIKE` | empty |
//! | 1 | `DJDBEXHISTORY` | empty |
//! | 2 | `DJDBEXSONGHISTORY` | empty |
//! | 3 | `DJDBEXMYTAG`, the tags | one row per tag |
//! | 4 | `DJDBEXSONGMYTAG`, a track's tags | one row per tag on a track |
//! | 5 | `DJDBEXCUEOPTION` | empty |
//! | 6 | `DJDBEXCONTENTOPTION` | empty |
//! | 7 | `DJDBEXDBPROPERTYOPTION` | one row, the master id |
//! | 8 | `DJDBEXGENERALOPTION` | empty |
//!
//! Type 0 (the "recommend like" pairs of tracks) is not a My Tag table and
//! stays empty [OBS: the reporter's file has 14 type-0 rows, each a pair
//! of track ids; what makes rekordbox write them is [UNKNOWN]].
//!
//! Rows are laid out by rekordbox's `DeviceSQL` engine from each table's
//! columns (`__epl_DJDBEX*_create`), not by hand [OBS static, 7.2.19:
//! `_TE_ComputeRealPositions` @0x102f747b0, `_TE_RowFormSmgr` @0x102f7572c]:
//! a u32 header holding the row's index on its page `<< 21` and the table's
//! number `(49 + type) << 5`; then the fixed columns, 8-byte ones first,
//! then 4-, 2- and 1-byte ones, each group in reverse column order; then the
//! first string column at the next multiple of four; then, if there are
//! more string columns, one offset byte each, and their strings. The row
//! takes the space that sums to, rounded up to four. That rule reproduces
//! 79 of the 80 type-3 rows and the type-7 row of a rekordbox 7 stick
//! (2026-10-03) byte for byte, lengths included; the eightieth has one
//! stray byte past its last string [OBS]. It does not hold for type 4,
//! whose rows rekordbox writes with no header at all [OBS: the reporter's
//! rekordbox 7 `exportExt.pdb`, #316, below].
//!
//! - type 3, `DJDBEXMYTAG` (`ID`, `SEQ`, `ATTRIBUTE`, `RESERVED1` u8,
//!   `RESERVED2` u16, `PARENTID`, `RESERVED3`, `RESERVED4`, then the strings
//!   `RESERVED5`, `NAME`, `RESERVED6`): one row per `djmdMyTag` row, each
//!   category followed by the tags under it, in `Seq` order —
//!
//!   ```text
//!   00  u16  0x0680
//!   02  u16  the row's index on its page × 32, from 0 again on each page
//!   04  u32  0
//!   08  u32  0
//!   0c  u32  parent: the category's id, or 0 for a category
//!   10  u32  Seq − 1
//!   14  u32  id
//!   18  u8×4 0, 0, 0, then 1 for a category and 0 for a tag
//!   1c  u8   an empty short string (0x03)
//!   1d  u8   offset of the name
//!   1e  u8   offset of a second empty string, which follows the name
//!   1f       the name: short ASCII from here, or a pad byte and a long
//!            UTF-16 string from 0x20, without the NUL `export.pdb`'s carry
//!   ```
//!
//!   followed by the empty string and zeros to a length of
//!   `32 + name bytes + 8` rounded up to four — counted from 0x20 even
//!   when the ASCII name starts at 0x1f.
//!
//! - type 4, `DJDBEXSONGMYTAG`: one row per tag on a track, which the
//!   player reads by tag to list the tags that have tracks and by track to
//!   filter [OBS static, XDJ-RX3 1.20 `djeplGetMyTagItem` @0x0015f4e4 and
//!   `Dsql_SetRegistMyTagIDs` @0x0015f888]. 16 bytes, no row header and no
//!   page index [OBS: a rekordbox 7 export's `exportExt.pdb` (2026-10-04,
//!   sha1 `bc6c8351…`, posted on #316 by its reporter): all 572 rows, 48
//!   tracks and 50 tags] —
//!
//!   ```text
//!   00  u32  0
//!   04  u32  the track's id in export.pdb
//!   08  u32  the tag's id
//!   0c  u8   an empty short string (0x03), then three zero bytes
//!   ```
//!
//!   The rows go by track, track ids ascending, and each track's tags in
//!   the order type 3 lists them, each pair once [OBS, same file]. Its
//!   pages differ from type 3's: `0x20` holds 1 and `0x22` the last row's
//!   index, as on `export.pdb`'s large pages, and a page takes rows until
//!   it is full to the byte, 222 of them with 4 bytes free [OBS, same
//!   file; [`PageStyle::LastRowIndex`]].
//!
//! - type 7, one row: `00 07 00 00`, twenty zero bytes, the u32
//!   `exportLibrary.db` calls `myTagMasterDBID`, then an empty string, five
//!   offsets and the five empty strings they point at: 39 bytes, which the
//!   page counts as 60 used, so the row is padded to that.
//!
//! The XDJ-RX3 lists a tag under Track Filter only when a type-4 row ties it
//! to a track that exists [OBS static, `djeplGetMyTagItem`]; without those
//! rows a stick's tags never reach the player.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rbl_pdb::build::{long_utf16le, short_ascii, FileBuilder, PageStyle};

use crate::SourceMyTag;

/// The page size, `export.pdb`'s.
const PAGE_SIZE: usize = 4096;

/// The file, for these tags and the tracks that carry them: each entry of
/// `tagged` is a track's id in `export.pdb` and the library's ids of its
/// tags. Ids of tags not in `my_tags` are left out, as `exportLibrary.db`
/// leaves them out. A tag listed twice on one track gets one row here, as
/// in rekordbox's file (one row per pair), and because the XDJ-RX3 copies
/// a track's type-4 tags into a fixed buffer with no bound
/// (`Dsql_SetRegistMyTagIDs`); `exportLibrary.db` keeps such a repeat as
/// the library has it, which this change leaves alone.
/// `master_db_id` is the value the reference wrote into
/// both this file and `exportLibrary.db`'s `property` row; what it is
/// derived from is not known, so it is passed in.
#[must_use]
pub fn build(my_tags: &[SourceMyTag], tagged: &[(u32, &[u64])], master_db_id: u32) -> Vec<u8> {
    let mut file = FileBuilder::new(PAGE_SIZE);
    // rekordbox writes the master row first, then the tags [OBS: the
    // candidate for type 7 comes before type 3's second page].
    file.write_order(&[7, 3]);
    let empty: [Vec<u8>; 0] = [];
    file.add_table(0, &empty);
    file.add_table(1, &empty);
    file.add_table(2, &empty);
    file.add_table_numbered(3, &tag_rows(my_tags), |row, index| {
        row[2..4].copy_from_slice(&(index.wrapping_mul(32)).to_le_bytes());
    });
    file.add_table_styled(4, &song_tag_rows(my_tags, tagged), |_, _| {}, PageStyle::LastRowIndex);
    file.add_table(5, &empty);
    file.add_table(6, &empty);
    file.add_table(7, &[master_row(master_db_id)]);
    file.add_table(8, &empty);
    file.finish()
}

/// The tags in rekordbox's order: each category (by `Seq`) and then the
/// tags under it (by `Seq`). A tag whose category is not in the list is put
/// after the last category.
fn listed_order(my_tags: &[SourceMyTag]) -> Vec<&SourceMyTag> {
    let mut categories: Vec<&SourceMyTag> = my_tags.iter().filter(|t| t.attribute == 1).collect();
    categories.sort_by_key(|c| (c.seq, c.id));
    let mut order = Vec::with_capacity(my_tags.len());
    let mut placed = vec![false; my_tags.len()];
    for category in &categories {
        order.push(*category);
        let mut under: Vec<(usize, &SourceMyTag)> = my_tags
            .iter()
            .enumerate()
            .filter(|(_, t)| t.attribute != 1 && t.parent == category.id)
            .collect();
        under.sort_by_key(|(_, t)| (t.seq, t.id));
        for (index, tag) in under {
            placed[index] = true;
            order.push(tag);
        }
    }
    let mut orphans: Vec<&SourceMyTag> =
        my_tags.iter().enumerate().filter(|(i, t)| t.attribute != 1 && !placed[*i]).map(|(_, t)| t).collect();
    orphans.sort_by_key(|t| (t.seq, t.id));
    order.extend(orphans);
    order
}

/// The type-3 rows, one per tag in [`listed_order`].
fn tag_rows(my_tags: &[SourceMyTag]) -> Vec<Vec<u8>> {
    listed_order(my_tags).into_iter().map(tag_row).collect()
}

/// One type-4 row per tag on a track, in rekordbox's order: the tracks by
/// id, and each track's tags in the order type 3 lists them. A tag on a
/// track twice gets one row; ids that are not tags, or do not fit a u32,
/// get none.
fn song_tag_rows(my_tags: &[SourceMyTag], tagged: &[(u32, &[u64])]) -> Vec<Vec<u8>> {
    // Each tag's place in type 3, by the library's id.
    let listed: HashMap<u64, (usize, u32)> = listed_order(my_tags)
        .into_iter()
        .enumerate()
        .filter_map(|(at, tag)| u32::try_from(tag.id).ok().filter(|&id| id != 0).map(|id| (tag.id, (at, id))))
        .collect();
    let mut by_track: BTreeMap<u32, BTreeSet<(usize, u32)>> = BTreeMap::new();
    for &(track, tags) in tagged {
        if track == 0 {
            continue;
        }
        let known: Vec<(usize, u32)> = tags.iter().filter_map(|id| listed.get(id).copied()).collect();
        if !known.is_empty() {
            by_track.entry(track).or_default().extend(known);
        }
    }
    by_track
        .into_iter()
        .flat_map(|(track, tags)| tags.into_iter().map(move |(_, tag_id)| song_tag_row(track, tag_id)))
        .collect()
}

fn song_tag_row(track: u32, tag_id: u32) -> Vec<u8> {
    let mut row = vec![0_u8; 16];
    row[0x04..0x08].copy_from_slice(&track.to_le_bytes());
    row[0x08..0x0c].copy_from_slice(&tag_id.to_le_bytes());
    row[0x0c] = 0x03;
    row
}

fn tag_row(tag: &SourceMyTag) -> Vec<u8> {
    let category = tag.attribute == 1;
    let name: Vec<u8> = if tag.name.is_ascii() && tag.name.len() < 0x7e {
        short_ascii(&tag.name)
    } else {
        let mut long = long_utf16le(&tag.name);
        // `export.pdb`'s strings end in a NUL pair; these do not, and the
        // length counts what is there.
        long.truncate(long.len() - 2);
        let len = u16::try_from(long.len()).unwrap_or(u16::MAX);
        long[1..3].copy_from_slice(&len.to_le_bytes());
        long
    };
    // A short string sits right after the offsets; a long one starts on
    // the even byte after.
    let name_at: usize = if name[0] == 0x90 || name[0] == 0x40 { 0x20 } else { 0x1f };
    let end = name_at + name.len();
    let len = (0x20 + name.len() + 8).div_ceil(4) * 4;
    let mut row = vec![0_u8; len.max(end + 1)];
    row[0..2].copy_from_slice(&0x0680_u16.to_le_bytes());
    row[0x0c..0x10].copy_from_slice(&u32::try_from(if category { 0 } else { tag.parent }).unwrap_or(0).to_le_bytes());
    row[0x10..0x14].copy_from_slice(&tag.seq.saturating_sub(1).to_le_bytes());
    row[0x14..0x18].copy_from_slice(&u32::try_from(tag.id).unwrap_or(0).to_le_bytes());
    row[0x1b] = u8::from(category);
    row[0x1c] = 0x03;
    row[0x1d] = u8::try_from(name_at).unwrap_or(0);
    row[0x1e] = u8::try_from(end).unwrap_or(0);
    row[name_at..end].copy_from_slice(&name);
    row[end] = 0x03;
    row
}

fn master_row(master_db_id: u32) -> Vec<u8> {
    let mut row = vec![0_u8; 60];
    row[0..4].copy_from_slice(&[0x00, 0x07, 0x00, 0x00]);
    row[0x18..0x1c].copy_from_slice(&master_db_id.to_le_bytes());
    row[0x1c] = 0x03;
    row[0x1d..0x22].copy_from_slice(&[0x22, 0x23, 0x24, 0x25, 0x26]);
    row[0x22..0x27].fill(0x03);
    row
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn tag(id: u64, seq: u32, name: &str, attribute: u8, parent: u64) -> SourceMyTag {
        SourceMyTag { id, seq, name: name.to_owned(), attribute, parent }
    }

    #[test]
    fn a_category_row_is_the_reference_byte_for_byte() {
        // Page 8, row 0 of the reference: "Lexicon Tags", id 5, Seq 1.
        let expected: Vec<u8> = [
            "80 06 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 05 00 00 00 00 00 00 01 03 1f 2c 1b",
            "4c 65 78 69 63 6f 6e 20 54 61 67 73 03 00 00 00 00 00 00 00 00 00 00 00",
        ]
        .join(" ")
        .split(' ')
        .map(|h| u8::from_str_radix(h, 16).unwrap())
        .collect();
        assert_eq!(tag_row(&tag(5, 1, "Lexicon Tags", 1, 0)), expected);
    }

    #[test]
    fn a_tag_row_is_the_reference_byte_for_byte() {
        // Page 8, row 1: "Components ▶ Synth", id 4271045719, Seq 1, under 5.
        let mut expected: Vec<u8> = "80 06 20 00 00 00 00 00 00 00 00 00 05 00 00 00 00 00 00 00 57 fc 92 fe 00 00 00 00 03 20 48 00 90 28 00 00"
            .split(' ')
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        for unit in "Components ▶ Synth".encode_utf16() {
            expected.extend_from_slice(&unit.to_le_bytes());
        }
        expected.extend_from_slice(&[0x03, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(expected.len(), 80);
        let mut row = tag_row(&tag(4_271_045_719, 1, "Components ▶ Synth", 0, 5));
        // The index on the page is stamped by the file builder.
        row[2] = 0x20;
        assert_eq!(row, expected);
    }

    #[test]
    fn the_master_row_is_the_reference_byte_for_byte() {
        let expected: Vec<u8> = "00 07 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ff 4d f5 67 03 22 23 24 25 26 03 03 03 03 03"
            .split(' ')
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        assert_eq!(master_row(1_744_129_535)[..39], expected[..]);
        assert_eq!(master_row(1).len(), 60);
    }

    #[test]
    fn the_file_has_nine_tables_with_the_tags_under_their_categories_in_order() {
        let tags = vec![
            tag(11, 2, "Second", 1, 0),
            tag(5, 1, "First", 1, 0),
            tag(100, 2, "b", 0, 5),
            tag(101, 1, "a ▶ x", 0, 5),
            tag(102, 1, "c", 0, 11),
        ];
        let bytes = build(&tags, &[], 7);
        let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
        let census = pdb.census();
        assert_eq!(census.len(), 9);
        // Type 3 and type 7 by number; the names are `export.pdb`'s.
        let rows = pdb.rows(pdb.table(rbl_pdb::PageType::Albums).expect("type 3"));
        assert_eq!(rows.len(), 5);
        let ids: Vec<u32> = rows.iter().map(|&r| pdb.u4_at(r, 0x14)).collect();
        assert_eq!(ids, vec![5, 101, 100, 11, 102]);
        let indexes: Vec<u16> = rows.iter().map(|&r| pdb.u2_at(r, 2)).collect();
        assert_eq!(indexes, vec![0, 32, 64, 96, 128]);
        let master = pdb.rows(pdb.table(rbl_pdb::PageType::PlaylistTree).expect("type 7"));
        assert_eq!(master.len(), 1);
        assert_eq!(pdb.u4_at(master[0], 0x18), 7);
    }

    fn hex(text: &str) -> Vec<u8> {
        text.split_whitespace().map(|h| u8::from_str_radix(h, 16).unwrap()).collect()
    }

    #[test]
    fn track_tag_rows_are_the_reference_byte_for_byte() {
        // Rows of a rekordbox 7 export's exportExt.pdb (2026-10-04, sha1
        // bc6c8351..., posted on #316 by its reporter): type 4, the first
        // data page's rows 0 and 17 (tracks 2 and 3) and the last page's
        // row 127.
        assert_eq!(song_tag_row(2, 0x07ae_781a), hex("00 00 00 00 02 00 00 00 1a 78 ae 07 03 00 00 00"));
        assert_eq!(song_tag_row(3, 0x07ae_781a), hex("00 00 00 00 03 00 00 00 1a 78 ae 07 03 00 00 00"));
        assert_eq!(song_tag_row(0x31, 0x0e72_3f7d), hex("00 00 00 00 31 00 00 00 7d 3f 72 0e 03 00 00 00"));
    }

    #[test]
    fn tracks_tags_go_to_type_4_by_track_in_type_3_order_once_each_and_only_for_known_tags() {
        let tags = vec![
            tag(2, 2, "Mood", 1, 0),
            tag(1, 1, "Genre", 1, 0),
            tag(21, 1, "Dark", 0, 2),
            tag(12, 2, "Warm-up", 0, 1),
            tag(11, 1, "Peak", 0, 1),
        ];
        // Type 3 lists Genre, Peak, Warm-up, Mood, Dark.
        let first: Vec<u64> = vec![21, 12, 11, 12];
        let second: Vec<u64> = vec![12, 99];
        let none: Vec<u64> = Vec::new();
        let tagged: Vec<(u32, &[u64])> = vec![(9, &second), (7, &first), (10, &none)];
        let bytes = build(&tags, &tagged, 3);
        let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
        let rows = pdb.rows(pdb.table(rbl_pdb::PageType::Labels).expect("type 4"));
        let pairs: Vec<(u32, u32)> = rows.iter().map(|&r| (pdb.u4_at(r, 0x04), pdb.u4_at(r, 0x08))).collect();
        // Track 7 before 9; Peak, Warm-up, Dark; 99 is not a tag; no repeats.
        assert_eq!(pairs, vec![(7, 11), (7, 12), (7, 21), (9, 12)]);
        assert!(rows.iter().all(|&r| pdb.u4_at(r, 0) == 0 && pdb.u4_at(r, 0x0c) == 0x03));
        let offsets: Vec<usize> = rows.iter().map(|&r| r.offset - r.page_offset - 0x28).collect();
        assert_eq!(offsets, vec![0, 16, 32, 48]);
    }

    #[test]
    fn track_tag_pages_are_headed_and_filled_as_rekordboxs() {
        // The reference's 572 type-4 rows took pages of 222, 222 and 128
        // rows with these bytes at 0x18..0x24 [OBS, the #316 reporter's
        // rekordbox 7 exportExt.pdb, its pages 10, 21 and 23].
        let mut tags = vec![tag(1, 1, "Genre", 1, 0)];
        tags.extend((0..11_u32).map(|i| tag(100 + u64::from(i), i + 1, "t", 0, 1)));
        let all: Vec<u64> = (100..111).collect();
        let tagged: Vec<(u32, &[u64])> = (1..=52).map(|id| (id, all.as_slice())).collect();
        let bytes = build(&tags, &tagged, 3);
        let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
        let rows = pdb.rows(pdb.table(rbl_pdb::PageType::Labels).expect("type 4"));
        assert_eq!(rows.len(), 572, "the reader takes every row of each full page");
        let mut pages: Vec<usize> = rows.iter().map(|r| r.page_offset).collect();
        pages.dedup();
        let per_page: Vec<usize> = pages.iter().map(|&p| rows.iter().filter(|r| r.page_offset == p).count()).collect();
        assert_eq!(per_page, vec![222, 222, 128]);
        let header = |at: usize| bytes[at + 0x18..at + 0x24].to_vec();
        assert_eq!(header(pages[0]), hex("de c0 1b 24 04 00 e0 0d 01 00 dd 00"));
        assert_eq!(header(pages[1]), hex("de c0 1b 24 04 00 e0 0d 01 00 dd 00"));
        assert_eq!(header(pages[2]), hex("80 00 10 24 b8 06 00 08 01 00 7f 00"));
        // Each row group's two last words: the present mask, then 0 but
        // in the last group, which marks only the last row.
        let words = |at: usize, group: usize| bytes[at + 4096 - group * 0x24 - 4..at + 4096 - group * 0x24].to_vec();
        assert_eq!(words(pages[0], 0), hex("ff ff 00 00"));
        assert_eq!(words(pages[0], 13), hex("ff 3f 00 20"));
        assert_eq!(words(pages[2], 7), hex("ff ff 00 80"));
        let tracks: Vec<u32> = rows.iter().map(|&r| pdb.u4_at(r, 0x04)).collect();
        assert!(tracks.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn the_tag_and_master_tables_are_the_same_with_or_without_tracks() {
        let tags = vec![tag(5, 1, "Lexicon Tags", 1, 0), tag(6, 1, "Components ▶ Synth", 0, 5)];
        let carrying: Vec<u64> = vec![6];
        let with = build(&tags, &[(1, carrying.as_slice())], 9);
        let without = build(&tags, &[], 9);
        // Each data page's rows and row index: everything but the page's
        // own number, link and sequence, which move with the new table.
        let pages_of = |bytes: &[u8], kind: rbl_pdb::PageType| -> Vec<Vec<u8>> {
            let pdb = rbl_pdb::Pdb::parse(bytes).expect("parses");
            let mut pages: Vec<usize> = pdb.rows(pdb.table(kind).expect("table")).iter().map(|r| r.page_offset).collect();
            pages.dedup();
            pages.iter().map(|&at| bytes[at + 0x18..at + 4096].to_vec()).collect()
        };
        for kind in [rbl_pdb::PageType::Albums, rbl_pdb::PageType::PlaylistTree] {
            let before = pages_of(&without, kind);
            assert_eq!(before.len(), 1, "one page of {kind:?}");
            assert_eq!(pages_of(&with, kind), before, "{kind:?}");
        }
        let pdb = rbl_pdb::Pdb::parse(&without).expect("parses");
        assert!(pdb.rows(pdb.table(rbl_pdb::PageType::Labels).expect("type 4")).is_empty());
    }
}
