//! Replacing one table's rows inside a finished file: the table must read
//! back as the new rows, and no byte of any other table may move.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_pdb::build::{device_sql_string, replace_table, FileBuilder};
use rbl_pdb::rows::{playlist_entry_row, playlist_row};
use rbl_pdb::{PageType, Pdb};

const PAGE: usize = 4096;

fn genre_row(id: u32, name: &str) -> Vec<u8> {
    let mut row = id.to_le_bytes().to_vec();
    row.extend_from_slice(&device_sql_string(name));
    row
}

fn entries(count: u32, playlist: u32) -> Vec<Vec<u8>> {
    (1..=count).map(|i| playlist_entry_row(i, 1000 + i, playlist)).collect()
}

/// A file shaped like an export: tables before and after the two playlist
/// tables, so a replacement that strays shows up in its neighbours.
fn file_with(entry_count: u32) -> Vec<u8> {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &(1..=400).map(|i| genre_row(i, &format!("Genre {i:04}"))).collect::<Vec<_>>());
    file.add_table(7, &[playlist_row(1, 0, 1, false, "One"), playlist_row(2, 0, 2, true, "Folder")]);
    file.add_table(8, &entries(entry_count, 1));
    file.add_table(4, &[genre_row(9, "Label")]);
    file.finish()
}

/// Every page that does not belong to `page_type`, by index, from a file.
fn other_pages(bytes: &[u8], page_type: u32) -> Vec<(usize, Vec<u8>)> {
    bytes
        .chunks(PAGE)
        .enumerate()
        .skip(1)
        .filter(|(_, page)| page.iter().any(|&b| b != 0) && u32::from_le_bytes(page[8..12].try_into().unwrap()) != page_type)
        .map(|(i, page)| (i, page.to_vec()))
        .collect()
}

fn read_entries(bytes: &[u8]) -> Vec<(u32, u32, u32)> {
    let pdb = Pdb::parse(bytes).unwrap();
    let table = pdb.table(PageType::PlaylistEntries).unwrap();
    pdb.playlist_entries(table).into_iter().map(|e| (e.entry_index, e.track_id, e.playlist_id)).collect()
}

fn assert_neighbours_intact(before: &[u8], after: &[u8], page_type: u32) {
    for (index, page) in other_pages(before, page_type) {
        assert_eq!(&after[index * PAGE..(index + 1) * PAGE], &page[..], "page {index} of another table changed");
    }
    let pdb = Pdb::parse(after).unwrap();
    let genres = pdb.named_rows(pdb.table(PageType::Genres).unwrap());
    assert_eq!(genres.len(), 400);
    assert_eq!(genres[399].name, "Genre 0400");
    let nodes = pdb.playlist_nodes(pdb.table(PageType::PlaylistTree).unwrap());
    assert_eq!(nodes.len(), 2);
}

#[test]
fn a_table_grows_onto_new_pages_and_reads_back() {
    let before = file_with(10);
    // 12-byte rows: a few hundred to a page, so this needs several.
    let rows = entries(2000, 1);
    let after = replace_table(&before, 8, &rows).expect("replaced");
    assert_eq!(read_entries(&after).len(), 2000);
    assert_eq!(read_entries(&after)[1999], (2000, 3000, 1));
    assert_neighbours_intact(&before, &after, 8);
}

#[test]
fn a_table_shrinks_and_the_pages_it_left_hold_nothing() {
    let before = file_with(2000);
    let after = replace_table(&before, 8, &entries(3, 1)).expect("replaced");
    assert_eq!(read_entries(&after), vec![(1, 1001, 1), (2, 1002, 1), (3, 1003, 1)]);
    assert_neighbours_intact(&before, &after, 8);
    let pdb = Pdb::parse(&after).unwrap();
    let table = pdb.table(PageType::PlaylistEntries).unwrap();
    // One data page after the index page.
    assert_ne!(table.first_page, table.last_page);
    let in_chain = |i: u32| i == table.first_page || i == table.last_page;
    let leftover = after
        .chunks(PAGE)
        .enumerate()
        .filter(|(i, page)| {
            u32::from_le_bytes(page[8..12].try_into().unwrap()) == 8 && !in_chain(u32::try_from(*i).unwrap()) && page.iter().any(|&b| b != 0)
        })
        .count();
    assert_eq!(leftover, 0, "no page of the old chain may still claim the table");
}

#[test]
fn a_table_empties_and_fills_again() {
    let before = file_with(5);
    let empty = replace_table(&before, 8, &[]).expect("emptied");
    assert!(read_entries(&empty).is_empty());
    let pdb = Pdb::parse(&empty).unwrap();
    let table = pdb.table(PageType::PlaylistEntries).unwrap();
    assert_eq!(table.first_page, table.last_page, "an empty table is its index page alone");
    assert_neighbours_intact(&before, &empty, 8);

    let refilled = replace_table(&empty, 8, &entries(4, 2)).expect("refilled");
    assert_eq!(read_entries(&refilled), vec![(1, 1001, 2), (2, 1002, 2), (3, 1003, 2), (4, 1004, 2)]);
    assert_neighbours_intact(&before, &refilled, 8);
}

#[test]
fn the_header_counts_the_pages_and_sequence_it_used() {
    let before = file_with(10);
    let after = replace_table(&before, 8, &entries(2000, 1)).unwrap();
    let word = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    assert!(word(&after, 0x14) > word(&before, 0x14), "pages written take later sequence numbers");
    assert!(word(&after, 0x0c) > word(&before, 0x0c), "new pages come from the unused end");
    // Every page the table now holds lies below the next unused page.
    let pdb = Pdb::parse(&after).unwrap();
    let table = *pdb.table(PageType::PlaylistEntries).unwrap();
    assert!(table.last_page < word(&after, 0x0c));
}

#[test]
fn a_rename_of_one_row_leaves_the_other_tables_alone() {
    let before = file_with(10);
    let after = replace_table(&before, 7, &[playlist_row(1, 0, 1, false, "Renamed"), playlist_row(2, 0, 2, true, "Folder")]).unwrap();
    let pdb = Pdb::parse(&after).unwrap();
    let nodes = pdb.playlist_nodes(pdb.table(PageType::PlaylistTree).unwrap());
    assert_eq!(nodes[0].name, "Renamed");
    assert_eq!(read_entries(&after), read_entries(&before));
    for (index, page) in other_pages(&before, 7) {
        assert_eq!(&after[index * PAGE..(index + 1) * PAGE], &page[..]);
    }
}

#[test]
fn a_playlist_row_reads_back_byte_for_byte_in_every_string_form() {
    let long = "L".repeat(200);
    let rows = vec![
        playlist_row(1, 0, 1, false, "Short"),
        playlist_row(2, 0, 2, true, &long),
        playlist_row(3, 2, 1, false, "Café ☕"),
        playlist_row(4, 2, 2, false, ""),
    ];
    let mut file = FileBuilder::new(PAGE);
    file.add_table(7, &rows);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    let table = pdb.table(PageType::PlaylistTree).unwrap();
    let read: Vec<Vec<u8>> = pdb.rows(table).into_iter().map(|row| pdb.playlist_row_bytes(row).unwrap()).collect();
    assert_eq!(read, rows);
}

#[test]
fn a_file_whose_free_pages_hold_data_is_refused() {
    let mut before = file_with(10);
    // Point the entries table's candidate at the genres' first data page:
    // taking it would overwrite another table.
    let pdb = Pdb::parse(&before).unwrap();
    let genres_data = pdb.table(PageType::Genres).unwrap().first_page + 1;
    let entry_at = (0..4).map(|i| 28 + i * 16).find(|&at| before[at] == 8).unwrap();
    before[entry_at + 4..entry_at + 8].copy_from_slice(&genres_data.to_le_bytes());
    assert!(replace_table(&before, 8, &entries(2000, 1)).is_none());
}

#[test]
fn a_missing_table_or_broken_chain_is_refused() {
    let before = file_with(10);
    assert!(replace_table(&before, 12, &[]).is_none(), "no such table");
    let mut broken = before.clone();
    let pdb = Pdb::parse(&broken).unwrap();
    let table = *pdb.table(PageType::PlaylistEntries).unwrap();
    // The index page now points at a genres page.
    let at = table.first_page as usize * PAGE + 0x0c;
    broken[at..at + 4].copy_from_slice(&2_u32.to_le_bytes());
    assert!(replace_table(&broken, 8, &entries(3, 1)).is_none());
}
