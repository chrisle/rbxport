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
//! stays empty.
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
//! stray byte past its last string [OBS]. No captured stick has a type-4
//! row, so the one below follows from the rule and its columns [ASSUME:
//! unverified on hardware].
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
//! - type 4, `DJDBEXSONGMYTAG` (`MYTAGID` u32, `CONTENTID` i32,
//!   `RESERVED1` u32, `RESERVED2` string): one row per tag on a track,
//!   which the player reads by tag to list the tags that have tracks and by
//!   track to filter [OBS static, XDJ-RX3 1.20 `djeplGetMyTagItem`
//!   @0x0015f4e4 and `Dsql_SetRegistMyTagIDs` @0x0015f888] —
//!
//!   ```text
//!   00  u16  0x06a0
//!   02  u16  the row's index on its page × 32
//!   04  u32  0
//!   08  u32  the track's id in export.pdb
//!   0c  u32  the tag's id
//!   10  u8   an empty short string (0x03)
//!   ```
//!
//!   20 bytes with the padding. rekordbox inserts the rows a tag at a time
//!   (`db::extPutMyTagOnTrack` @0x1019b7b74: the tag id, the track, 0 and
//!   an empty string); the order on a stick is [ASSUME] not significant,
//!   as the player looks rows up through its hash indexes on both ids.
//!
//! - type 7, one row: `00 07 00 00`, twenty zero bytes, the u32
//!   `exportLibrary.db` calls `myTagMasterDBID`, then an empty string, five
//!   offsets and the five empty strings they point at: 39 bytes, which the
//!   page counts as 60 used, so the row is padded to that.
//!
//! The XDJ-RX3 lists a tag under Track Filter only when a type-4 row ties it
//! to a track that exists [OBS static, `djeplGetMyTagItem`]; without those
//! rows a stick's tags never reach the player.

use std::collections::BTreeSet;

use rbl_pdb::build::{long_utf16le, short_ascii, FileBuilder};

use crate::SourceMyTag;

/// The page size, `export.pdb`'s.
const PAGE_SIZE: usize = 4096;

/// The file, for these tags and the tracks that carry them: each entry of
/// `tagged` is a track's id in `export.pdb` and the library's ids of its
/// tags. Ids of tags not in `my_tags` are left out, as `exportLibrary.db`
/// leaves them out. `master_db_id` is the value the reference wrote into
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
    file.add_table_numbered(4, &song_tag_rows(my_tags, tagged), |row, index| {
        row[2..4].copy_from_slice(&(index.wrapping_mul(32)).to_le_bytes());
    });
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

/// The table number rekordbox's engine gives `DJDBEXSONGMYTAG` (`49 +
/// type`, one after `DJDBEXMYTAG`'s 52), in a row header's bits 5 to 20.
const SONG_MY_TAG_HEADER: u16 = 53 << 5;

/// One type-4 row per tag on a track: the tags in the order type 3 lists
/// them, and under each the tracks in the order given.
fn song_tag_rows(my_tags: &[SourceMyTag], tagged: &[(u32, &[u64])]) -> Vec<Vec<u8>> {
    let mut rows = Vec::new();
    let mut written: BTreeSet<(u32, u32)> = BTreeSet::new();
    for tag in listed_order(my_tags) {
        let Ok(tag_id) = u32::try_from(tag.id) else { continue };
        if tag_id == 0 {
            continue;
        }
        for &(track, tags) in tagged {
            if track != 0 && tags.contains(&tag.id) && written.insert((tag_id, track)) {
                rows.push(song_tag_row(tag_id, track));
            }
        }
    }
    rows
}

fn song_tag_row(tag_id: u32, track: u32) -> Vec<u8> {
    let mut row = vec![0_u8; 20];
    row[0..2].copy_from_slice(&SONG_MY_TAG_HEADER.to_le_bytes());
    row[0x08..0x0c].copy_from_slice(&track.to_le_bytes());
    row[0x0c..0x10].copy_from_slice(&tag_id.to_le_bytes());
    row[0x10] = 0x03;
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

    #[test]
    fn a_track_tag_row_is_rekordboxs_layout_for_its_columns() {
        // DJDBEXSONGMYTAG is (MYTAGID u32, CONTENTID i32, RESERVED1 u32,
        // RESERVED2 string): the u32s in reverse column order after the
        // header, then the one string, empty, at 0x10, padded to four.
        let row = song_tag_row(4_271_045_719, 0x0102_0304);
        let expected: Vec<u8> = "a0 06 00 00 00 00 00 00 04 03 02 01 57 fc 92 fe 03 00 00 00"
            .split(' ')
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        assert_eq!(row, expected);
    }

    #[test]
    fn the_header_numbers_follow_the_tables_rekordbox_wrote() {
        // The reference's type-3 rows start 0x0680 and its type-7 row 0x0700:
        // (49 + type) << 5, so type 4 is 0x06a0.
        let category = tag_row(&tag(5, 1, "x", 1, 0));
        assert_eq!(u16::from_le_bytes([category[0], category[1]]), 0x0680);
        let master = master_row(1);
        assert_eq!(u16::from_le_bytes([master[0], master[1]]), 0x0700);
        assert_eq!(SONG_MY_TAG_HEADER, (49 + 4) << 5);
    }

    #[test]
    fn tracks_tags_go_to_type_4_by_tag_once_each_and_only_for_known_tags() {
        let tags = vec![
            tag(1, 1, "Genre", 1, 0),
            tag(12, 2, "Warm-up", 0, 1),
            tag(11, 1, "Peak", 0, 1),
        ];
        let first: Vec<u64> = vec![12, 11, 12];
        let second: Vec<u64> = vec![12, 99];
        let none: Vec<u64> = Vec::new();
        let tagged: Vec<(u32, &[u64])> = vec![(7, &first), (9, &second), (10, &none)];
        let bytes = build(&tags, &tagged, 3);
        let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
        let rows = pdb.rows(pdb.table(rbl_pdb::PageType::Labels).expect("type 4"));
        let pairs: Vec<(u32, u32)> = rows.iter().map(|&r| (pdb.u4_at(r, 0x0c), pdb.u4_at(r, 0x08))).collect();
        // Peak (Seq 1) before Warm-up (Seq 2); 99 is not a tag; no repeats.
        assert_eq!(pairs, vec![(11, 7), (12, 7), (12, 9)]);
        let headers: Vec<u16> = rows.iter().map(|&r| pdb.u2_at(r, 0)).collect();
        assert_eq!(headers, vec![0x06a0; 3]);
        let indexes: Vec<u16> = rows.iter().map(|&r| pdb.u2_at(r, 2)).collect();
        assert_eq!(indexes, vec![0, 32, 64]);
        assert!(rows.iter().all(|&r| pdb.u4_at(r, 4) == 0 && pdb.u1_at(r, 0x10) == 0x03));
    }

    #[test]
    fn many_track_tags_fill_pages_with_the_index_starting_again() {
        let tags = vec![tag(1, 1, "Genre", 1, 0), tag(11, 1, "Peak", 0, 1)];
        let one: Vec<u64> = vec![11];
        let tagged: Vec<(u32, &[u64])> = (1..=600).map(|id| (id, one.as_slice())).collect();
        let bytes = build(&tags, &tagged, 3);
        let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
        let rows = pdb.rows(pdb.table(rbl_pdb::PageType::Labels).expect("type 4"));
        assert_eq!(rows.len(), 600);
        let tracks: Vec<u32> = rows.iter().map(|&r| pdb.u4_at(r, 0x08)).collect();
        assert_eq!(tracks, (1..=600).collect::<Vec<u32>>());
        // Every page's first row is index 0 again.
        let firsts = rows.iter().filter(|&&r| pdb.u2_at(r, 2) == 0).count();
        assert!(firsts > 1, "600 rows take more than one page");
        assert!(rows.iter().all(|&r| r.offset - r.page_offset - 0x28 + 20 <= 4096));
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
