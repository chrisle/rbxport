//! `PIONEER/rekordbox/exportExt.pdb`: the second `DeviceSQL` database on a
//! stick, which carries the library's My Tags for a player's tag browsing.
//!
//! Decoded from the 2026-09-17 parity reference (rekordbox 7.2.11, 99
//! tags, no track carrying one) [OBS]. Nine tables, types 0 to 8, laid out
//! like `export.pdb`'s: an index page each, a data page or an empty
//! candidate. Two hold rows:
//!
//! - type 3, the tags: one row per `djmdMyTag` row, each category followed
//!   by the tags under it, in `Seq` order —
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
//! - type 7, one row: `00 07 00 00`, twenty zero bytes, the u32
//!   `exportLibrary.db` calls `myTagMasterDBID`, then an empty string, five
//!   offsets and the five empty strings they point at: 39 bytes, which the
//!   page counts as 60 used, so the row is padded to that.
//!
//! Types 0–2, 4–6 and 8 were empty in the reference. Type 5 is where a
//! track's tags would go by `export.pdb`'s pattern of a members table two
//! after its names table [UNKNOWN: no captured export has a tagged
//! track]; nothing is written there.

use rbl_pdb::build::{long_utf16le, short_ascii, FileBuilder};

use crate::SourceMyTag;

/// The page size, `export.pdb`'s.
const PAGE_SIZE: usize = 4096;

/// The file, for these tags. `master_db_id` is the value the reference
/// wrote into both this file and `exportLibrary.db`'s `property` row; what
/// it is derived from is not known, so it is passed in.
#[must_use]
pub fn build(my_tags: &[SourceMyTag], master_db_id: u32) -> Vec<u8> {
    let mut file = FileBuilder::new(PAGE_SIZE);
    // rekordbox writes the master row first, then the tags [OBS: the
    // candidate for type 7 comes before type 3's second page].
    file.write_order(&[7, 3]);
    let empty: [Vec<u8>; 0] = [];
    file.add_table(0, &empty);
    file.add_table(1, &empty);
    file.add_table(2, &empty);
    file.add_table_numbered(3, &tag_rows(my_tags), |row, index| rbl_pdb::rows::set_index_shift(row, index));
    file.add_table(4, &empty);
    file.add_table(5, &empty);
    file.add_table(6, &empty);
    file.add_table(7, &[master_row(master_db_id)]);
    file.add_table(8, &empty);
    file.finish()
}

/// The tag rows in rekordbox's order: each category (by `Seq`) and then
/// the tags under it (by `Seq`). A tag whose category is not in the list
/// is put after the last category.
fn tag_rows(my_tags: &[SourceMyTag]) -> Vec<Vec<u8>> {
    let mut categories: Vec<&SourceMyTag> = my_tags.iter().filter(|t| t.attribute == 1).collect();
    categories.sort_by_key(|c| (c.seq, c.id));
    let mut rows = Vec::with_capacity(my_tags.len());
    let mut placed = vec![false; my_tags.len()];
    for category in &categories {
        rows.push(tag_row(category));
        let mut under: Vec<(usize, &SourceMyTag)> = my_tags
            .iter()
            .enumerate()
            .filter(|(_, t)| t.attribute != 1 && t.parent == category.id)
            .collect();
        under.sort_by_key(|(_, t)| (t.seq, t.id));
        for (index, tag) in under {
            placed[index] = true;
            rows.push(tag_row(tag));
        }
    }
    let mut orphans: Vec<&SourceMyTag> =
        my_tags.iter().enumerate().filter(|(i, t)| t.attribute != 1 && !placed[*i]).map(|(_, t)| t).collect();
    orphans.sort_by_key(|t| (t.seq, t.id));
    rows.extend(orphans.iter().map(|tag| tag_row(tag)));
    rows
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
        let bytes = build(&tags, 7);
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
}
