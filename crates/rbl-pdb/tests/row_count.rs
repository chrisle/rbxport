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

/// The row index length and present-row count packed into `0x18..0x1b`.
fn packed_counts(bytes: &[u8], at: usize) -> (u32, u32) {
    let packed = u32::from_le_bytes([bytes[at + 0x18], bytes[at + 0x19], bytes[at + 0x1a], 0]);
    (packed & 0x1fff, packed >> 13)
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

/// The start of every data page, of any table.
fn data_pages(bytes: &[u8]) -> Vec<usize> {
    (1..bytes.len() / PAGE).map(|page| page * PAGE).filter(|&at| bytes[at + 0x1b] & 0x40 == 0 && bytes[at + 0x1b] != 0).collect()
}

/// The start of each playlist-entry data page holding more than 255 rows.
fn full_pages(bytes: &[u8]) -> Vec<usize> {
    data_pages(bytes)
        .into_iter()
        .filter(|&at| {
            let page_type = u32::from_le_bytes(bytes[at + 8..at + 12].try_into().unwrap());
            page_type == 8 && packed_counts(bytes, at).0 > 0xff
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
/// 284), `0x20` holding 1 and `0x22` the last row's index, 283, and each row
/// group's second word 0 but the last group's, which holds the last row's
/// bit. rbxport now writes its own full pages that way (#349).
#[test]
fn full_pages_are_headed_as_rekordbox_heads_them() {
    let bytes = entries_file();
    let pages = full_pages(&bytes);
    assert!(!pages.is_empty());
    for at in pages {
        let (length, present) = packed_counts(&bytes, at);
        assert_eq!(length, present, "every row is present");
        assert_eq!(word(&bytes, at + 0x20), 1);
        assert_eq!(u32::from(word(&bytes, at + 0x22)), length - 1);
        let groups = length.div_ceil(16) as usize;
        for group in 0..groups {
            let second = word(&bytes, at + PAGE - group * 0x24 - 2);
            let expected = if group == groups - 1 { 1 << ((length - 1) % 16) } else { 0 };
            assert_eq!(second, expected, "group {group}'s second word");
        }
    }
    // The bytes the rekordbox stick holds on a page of 284 rows.
    let mut page = rbl_pdb::build::PageBuilder::new(PAGE, 1, 8, 0);
    for index in 0..284 {
        page.push_row(&entry_row(index));
    }
    let page = page.finish();
    assert_eq!(&page[0x18..0x1b], &[0x1c, 0x81, 0x23]);
    assert_eq!(&page[0x20..0x24], &[0x01, 0x00, 0x1b, 0x01]);
    assert_eq!(&page[PAGE - 0x24 * 17 - 4..PAGE - 0x24 * 17], &[0xff, 0x0f, 0x00, 0x08]);
}

/// What rekordbox's `DeviceSQL` does with a page's last-write record before
/// it adds or frees a row there [static: rekordbox 7.2.19
/// `_TE_PSmgrClearDirtyBits` @0x102f694d4; the same loop is where rekordbox
/// 6.8.6 crashed in #349's dump, `rekordbox.exe+0xe8f9c6`]: unless `0x20`
/// is `0x1fff`, start at row group `0x22 / 16` and clear each group's
/// second word, counting its bits off `0x20`, until `0x20` is zero.
/// `Some(groups visited)`, or `None` when the walk leaves the row index.
fn clear_dirty_bits_walk(bytes: &[u8], at: usize) -> Option<usize> {
    let mut count = word(bytes, at + 0x20) & 0x3fff;
    if count == 0x1fff {
        return Some(0);
    }
    let (length, _) = packed_counts(bytes, at);
    let groups = length.div_ceil(16) as usize;
    let mut group = usize::from(word(bytes, at + 0x22) & 0x3fff) / 16;
    let mut visited = 0;
    while count != 0 {
        if group >= groups {
            return None;
        }
        let bits = word(bytes, at + PAGE - group * 0x24 - 2).count_ones() as u16;
        count = count.wrapping_sub(bits) & 0x3fff;
        group += 1;
        visited += 1;
    }
    Some(visited)
}

/// rbxport put the row count in both `0x20` and `0x22` of a page of more
/// than 255 rows, so rekordbox's walk began past the page's last row group
/// and never found the bits it was counting: it ran off the page and
/// crashed, as rekordbox 6.8.6 did opening a stick with a few hundred
/// playlist entries (#349). Every data page rbxport writes must keep that
/// walk inside its own row index.
#[test]
fn rekordboxs_dirty_row_walk_stays_on_every_page() {
    let rows: Vec<Vec<u8>> = (0..ENTRIES).map(entry_row).collect();
    let short: Vec<Vec<u8>> = (0..40).map(entry_row).collect();
    let mut file = FileBuilder::new(PAGE);
    file.add_table(7, &short);
    file.add_table(8, &rows);
    file.add_table_styled(4, &rows, |_, _| {}, rbl_pdb::build::PageStyle::LastRowIndex);
    let bytes = file.finish();
    let pages = data_pages(&bytes);
    assert!(pages.len() >= 5);
    for at in pages {
        assert!(clear_dirty_bits_walk(&bytes, at).is_some(), "the walk leaves page {}", at / PAGE);
    }
}

/// A stick an older rbxport wrote: `0x18` held at `0xff` and the row count
/// in `0x22` on a page of more than 255 rows. rbxport still reads it whole.
#[test]
fn pages_an_older_rbxport_wrote_still_read_whole() {
    let mut bytes = entries_file();
    let pages = full_pages(&bytes);
    assert!(!pages.is_empty());
    for at in pages {
        let (rows, _) = packed_counts(&bytes, at);
        bytes[at + 0x18] = 0xff;
        bytes[at + 0x19] = u8::try_from((rows * 32) & 0xff).unwrap();
        bytes[at + 0x1a] = u8::try_from(rows / 8).unwrap();
        let rows = u16::try_from(rows).unwrap();
        bytes[at + 0x20..at + 0x22].copy_from_slice(&rows.to_le_bytes());
        bytes[at + 0x22..at + 0x24].copy_from_slice(&rows.to_le_bytes());
        assert!(clear_dirty_bits_walk(&bytes, at).is_none(), "the bytes rekordbox crashed on");
    }
    assert_eq!(entry_count(&bytes), ENTRIES as usize);
}

#[test]
fn every_entry_of_a_full_page_reads_once() {
    let bytes = entries_file();
    let pdb = Pdb::parse(&bytes).unwrap();
    let entries = pdb.playlist_entries(pdb.table(PageType::PlaylistEntries).unwrap());
    let mut indices: Vec<u32> = entries.iter().map(|e| e.entry_index).collect();
    indices.sort_unstable();
    assert_eq!(indices, (1..=ENTRIES).collect::<Vec<_>>(), "no page loses its last row");
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
