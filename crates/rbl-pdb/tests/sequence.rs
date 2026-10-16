//! The header's sequence number (`0x14`) stays above every page's.
//!
//! A DeviceSQL reader that opens a file to write it (a player recording
//! history) rolls back every page stamped with the header's sequence, as an
//! unfinished transaction (rekordbox 7.2.19 `TE_DPgm_CheckAndRecoverPage`
//! @0x102f57e5c; XDJ-700 firmware 1.15 @0xa8e103f8). rekordbox's own exports
//! never have such a page: a fresh one carries one more than its newest page
//! [OBS rig exports: 21 over 20 in export.pdb, 6 over 5 in exportExt.pdb].
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_pdb::build::{device_sql_string, replace_single_page_table, replace_table, FileBuilder};
use rbl_pdb::rows::{playlist_entry_row, playlist_row};

const PAGE: usize = 4096;

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The sequence of every page on every table's chain, walked the way the
/// player's recovery walks it: from the first page to the empty candidate.
fn chain_sequences(bytes: &[u8]) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    for table in 0..word(bytes, 8) as usize {
        let entry = 0x1c + table * 16;
        let (page_type, candidate, first) = (word(bytes, entry), word(bytes, entry + 4), word(bytes, entry + 8));
        let mut page = first;
        let mut seen = 0;
        while page != candidate {
            let at = page as usize * PAGE;
            assert!(at + PAGE <= bytes.len(), "table {page_type}'s chain leaves the file before its candidate");
            out.push((page_type, page, word(bytes, at + 0x10)));
            page = word(bytes, at + 0x0c);
            seen += 1;
            assert!(seen < 100_000, "table {page_type}'s chain never reaches its candidate");
        }
    }
    out
}

fn assert_header_above_every_page(bytes: &[u8]) {
    let header = word(bytes, 0x14);
    let pages = chain_sequences(bytes);
    assert!(!pages.is_empty());
    for (page_type, page, sequence) in pages {
        assert!(sequence < header, "table {page_type} page {page} carries {sequence}, header {header}");
    }
}

fn genre_row(id: u32, name: &str) -> Vec<u8> {
    let mut row = id.to_le_bytes().to_vec();
    row.extend_from_slice(&device_sql_string(name));
    row
}

fn export_like() -> Vec<u8> {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(0, &[]);
    file.add_table(1, &(1..=400).map(|i| genre_row(i, &format!("Genre {i:04}"))).collect::<Vec<_>>());
    file.add_table(7, &[playlist_row(1, 0, 1, false, "One")]);
    file.add_table(8, &(1..=10).map(|i| playlist_entry_row(i, 1000 + i, 1)).collect::<Vec<_>>());
    file.add_table(18, &[genre_row(1, "x")]);
    file.add_table(19, &[genre_row(2, "y")]);
    file.finish()
}

#[test]
fn a_fresh_file_names_one_past_its_newest_page() {
    let bytes = export_like();
    assert_header_above_every_page(&bytes);
    let newest = chain_sequences(&bytes).into_iter().map(|(_, _, s)| s).max().unwrap();
    assert_eq!(word(&bytes, 0x14), newest + 1, "as rekordbox's fresh exports do");
}

#[test]
fn a_file_with_no_rows_still_names_a_sequence_above_its_index_pages() {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &[]);
    file.add_table(2, &[]);
    assert_header_above_every_page(&file.finish());
}

#[test]
fn replacing_a_table_keeps_the_header_above_every_page() {
    let before = export_like();
    let grown = replace_table(&before, 8, &(1..=2000).map(|i| playlist_entry_row(i, 1000 + i, 1)).collect::<Vec<_>>()).unwrap();
    assert_header_above_every_page(&grown);
    let emptied = replace_table(&grown, 8, &[]).unwrap();
    assert_header_above_every_page(&emptied);
}

#[test]
fn a_file_whose_header_names_a_page_is_moved_past_it_on_the_next_write() {
    // How files were written before: the header named the newest page.
    let mut old = export_like();
    let newest = chain_sequences(&old).into_iter().map(|(_, _, s)| s).max().unwrap();
    old[0x14..0x18].copy_from_slice(&newest.to_le_bytes());

    assert_header_above_every_page(&replace_table(&old, 7, &[playlist_row(1, 0, 1, false, "Renamed")]).unwrap());

    // Make the one-page table hold the header's sequence, then rewrite it in place.
    let entry = (0..word(&old, 8) as usize).map(|t| 0x1c + t * 16).find(|&e| word(&old, e) == 19).unwrap();
    let page = word(&old, entry + 12) as usize;
    old[page * PAGE + 0x10..page * PAGE + 0x14].copy_from_slice(&newest.to_le_bytes());
    let rewritten = replace_single_page_table(&old, 19, &[genre_row(3, "z")]).unwrap();
    assert_header_above_every_page(&rewritten);
    assert_eq!(&rewritten[0x18..], &{
        let mut expected = old.clone();
        let fresh = replace_single_page_table(&old, 19, &[genre_row(3, "z")]).unwrap();
        expected[page * PAGE..(page + 1) * PAGE].copy_from_slice(&fresh[page * PAGE..(page + 1) * PAGE]);
        expected
    }[0x18..], "only the header sequence and the page change");
}

#[test]
fn a_rekordbox_file_keeps_its_header_on_an_in_place_rewrite() {
    let bytes = export_like();
    let rewritten = replace_single_page_table(&bytes, 19, &[genre_row(3, "z")]).unwrap();
    assert_eq!(word(&rewritten, 0x14), word(&bytes, 0x14));
}
