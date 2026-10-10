//! How many rows a data page holds, as rekordbox and rbxport each write it.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_pdb::build::FileBuilder;
use rbl_pdb::{PageType, Pdb};

const PAGE: usize = 4096;
const ENTRIES: u32 = 600;

/// A playlist-entry row: entry index, track id, playlist id.
fn entry_row(index: u32) -> Vec<u8> {
    let mut row = (index + 1).to_le_bytes().to_vec();
    row.extend_from_slice(&(index + 1).to_le_bytes());
    row.extend_from_slice(&1_u32.to_le_bytes());
    row
}

/// A file whose playlist-entry table has pages of more than 255 rows, as a
/// stick of a few hundred entries does.
fn entries_file() -> Vec<u8> {
    let rows: Vec<Vec<u8>> = (0..ENTRIES).map(entry_row).collect();
    let mut file = FileBuilder::new(PAGE);
    file.add_table(8, &rows);
    file.finish()
}

/// The start of each playlist-entry data page holding more than 255 rows.
fn full_pages(bytes: &[u8]) -> Vec<usize> {
    (0..bytes.len() / PAGE)
        .map(|page| page * PAGE)
        .filter(|&at| {
            let page_type = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap());
            let data = bytes[at + 0x1b] & 0x40 == 0;
            let rows = u16::from_le_bytes([bytes[at + 0x22], bytes[at + 0x23]]);
            page_type == 8 && data && rows > 0xff && rows != 0x1fff
        })
        .collect()
}

fn entry_count(bytes: &[u8]) -> usize {
    let pdb = Pdb::parse(bytes).unwrap();
    pdb.playlist_entries(pdb.table(PageType::PlaylistEntries).unwrap()).len()
}

#[test]
fn rbxports_own_pages_of_more_than_255_rows_still_read_whole() {
    let bytes = entries_file();
    assert!(!full_pages(&bytes).is_empty(), "the test needs a page of more than 255 rows");
    assert_eq!(entry_count(&bytes), ENTRIES as usize);
}

/// rekordbox's header on a page of more than 255 rows [OBS: a rekordbox 7
/// stick, playlist-entry pages of 284 rows]: the index length in the low 13
/// bits of `0x18..0x1b` and the present rows in the top 11 (`1c 81 23` for
/// 284), `0x20` holding 1 and `0x22` the last row's index, 283. Reading
/// `0x22` as the count lost each such page's last entry, and the stick's
/// `export.pdb` and `exportLibrary.db` then looked as if they disagreed (#284).
#[test]
fn rekordboxs_packed_row_count_reads_every_row_of_a_full_page() {
    let mut bytes = entries_file();
    let pages = full_pages(&bytes);
    assert!(!pages.is_empty());
    for at in pages {
        let rows = u32::from(u16::from_le_bytes([bytes[at + 0x22], bytes[at + 0x23]]));
        let packed = rows | rows << 13;
        bytes[at + 0x18..at + 0x1b].copy_from_slice(&packed.to_le_bytes()[..3]);
        bytes[at + 0x20..at + 0x22].copy_from_slice(&1_u16.to_le_bytes());
        bytes[at + 0x22..at + 0x24].copy_from_slice(&u16::try_from(rows - 1).unwrap().to_le_bytes());
        if rows == 284 {
            assert_eq!(&bytes[at + 0x18..at + 0x1b], &[0x1c, 0x81, 0x23], "the bytes the rekordbox stick holds");
        }
    }
    let pdb = Pdb::parse(&bytes).unwrap();
    let entries = pdb.playlist_entries(pdb.table(PageType::PlaylistEntries).unwrap());
    assert_eq!(entries.len(), ENTRIES as usize, "no page loses its last row");
    let mut indices: Vec<u32> = entries.iter().map(|e| e.entry_index).collect();
    indices.sort_unstable();
    assert_eq!(indices, (1..=ENTRIES).collect::<Vec<_>>());
}

/// A rekordbox page whose index keeps deleted rows: 9 index entries, 8 of
/// them present [OBS: track pages on the same stick read `09 00 01`].
#[test]
fn a_deleted_row_keeps_its_index_entry_and_is_skipped() {
    let rows: Vec<Vec<u8>> = (0..9).map(entry_row).collect();
    let mut file = FileBuilder::new(PAGE);
    file.add_table(8, &rows);
    let mut bytes = file.finish();
    let at = (0..bytes.len() / PAGE)
        .map(|page| page * PAGE)
        .find(|&at| {
            u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap()) == 8
                && bytes[at + 0x1b] & 0x40 == 0
                && bytes[at + 0x18] == 9
        })
        .unwrap();
    let packed: u32 = 9 | 8 << 13;
    bytes[at + 0x18..at + 0x1b].copy_from_slice(&packed.to_le_bytes()[..3]);
    bytes[at + 0x20..at + 0x22].copy_from_slice(&0x1fff_u16.to_le_bytes());
    bytes[at + 0x22..at + 0x24].copy_from_slice(&0x1fff_u16.to_le_bytes());
    // Row 4's presence bit, in both copies of the group's mask.
    for mask_at in [at + PAGE - 4, at + PAGE - 2] {
        let mask = u16::from_le_bytes([bytes[mask_at], bytes[mask_at + 1]]) & !(1 << 4);
        bytes[mask_at..mask_at + 2].copy_from_slice(&mask.to_le_bytes());
    }
    let pdb = Pdb::parse(&bytes).unwrap();
    let entries = pdb.playlist_entries(pdb.table(PageType::PlaylistEntries).unwrap());
    let indices: Vec<u32> = entries.iter().map(|e| e.entry_index).collect();
    assert_eq!(indices, vec![1, 2, 3, 4, 6, 7, 8, 9]);
}
