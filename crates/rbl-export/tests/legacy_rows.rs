//! The parts of `export.pdb` that NXS-era players read and the CDJ-3000 does
//! not: each row's slot on its page (#182). The bytes are read straight off the pages, the way the
//! players' DeviceSQL engine reads them, not through `rbl_pdb`'s reader.
//!
//! Every stick here is written into a temporary directory.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use rbl_export::{export, SourcePlaylist, SourceTrack};

const PAGE: usize = 4096;

fn u2(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u4(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Every row of one table, as `(page, slot, row bytes from the row's start)`,
/// walked from the header's first page to its last.
fn rows(file: &[u8], page_type: u32) -> Vec<(u32, usize, &[u8])> {
    let tables = u4(file, 8) as usize;
    let entry = (0..tables).map(|i| 28 + i * 16).find(|&at| u4(file, at) == page_type).unwrap();
    let (mut page, last) = (u4(file, entry + 8), u4(file, entry + 12));
    let mut out = Vec::new();
    loop {
        let bytes = &file[page as usize * PAGE..(page as usize + 1) * PAGE];
        if bytes[0x1b] & 0x40 == 0 {
            let count = (u4(bytes, 0x18) & 0x1fff) as usize;
            for slot in 0..count {
                let base = PAGE - (slot / 16) * 0x24;
                assert_ne!(u2(bytes, base - 4) & (1 << (slot % 16)), 0, "a fresh export has no deleted rows");
                let offset = u2(bytes, base - 6 - 2 * (slot % 16)) as usize;
                out.push((page, slot, &bytes[0x28 + offset..]));
            }
        }
        if page == last {
            return out;
        }
        page = u4(bytes, 0x0c);
    }
}

fn track(dir: &Path, n: u64) -> SourceTrack {
    let path = dir.join(format!("source-{n}.mp3"));
    std::fs::write(&path, vec![n as u8; 2048]).unwrap();
    SourceTrack {
        id: n,
        source_path: path,
        title: format!("Track {n}"),
        artist: format!("Artist {n}"),
        album: format!("Album {n}"),
        genre: "House".into(),
        key: "Am".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        date_added: "2026-09-06".into(),
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

fn pdb(dest: &Path) -> Vec<u8> {
    std::fs::read(dest.join("PIONEER/rekordbox/export.pdb")).unwrap()
}

/// The first four bytes of the first three rows of a page rekordbox wrote, for
/// a track, an artist and an album [OBS: a stick rekordbox 7 wrote, pages 2,
/// 6 and 8]: the subtype, then the slot times 32.
const REKORDBOX_TRACK_HEADS: [[u8; 4]; 3] = [[0x24, 0, 0x00, 0], [0x24, 0, 0x20, 0], [0x24, 0, 0x40, 0]];
const REKORDBOX_ARTIST_HEADS: [[u8; 4]; 3] = [[0x60, 0, 0x00, 0], [0x60, 0, 0x20, 0], [0x60, 0, 0x40, 0]];
const REKORDBOX_ALBUM_HEADS: [[u8; 4]; 3] = [[0x80, 0, 0x00, 0], [0x80, 0, 0x20, 0], [0x80, 0, 0x40, 0]];

/// rekordbox numbers each row that opens with a subtype word by its slot,
/// from 0 on every page. rbxport wrote 0 on every row, so on a CDJ-900NXS's
/// DeviceSQL engine every row of a page claimed slot 0.
#[test]
fn rows_carry_their_slot_on_the_page_as_rekordboxs_do() {
    let (src, dest) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let tracks: Vec<SourceTrack> = (1..=40).map(|n| track(src.path(), n)).collect();
    let playlists = [SourcePlaylist { id: 1, name: "All".into(), track_indices: (0..40).collect(), ..Default::default() }];
    export(dest.path(), &tracks, &playlists).unwrap();
    let file = pdb(dest.path());

    for (page_type, heads) in [(0, REKORDBOX_TRACK_HEADS), (2, REKORDBOX_ARTIST_HEADS), (3, REKORDBOX_ALBUM_HEADS)] {
        let rows = rows(&file, page_type);
        let first: Vec<[u8; 4]> = rows.iter().take(3).map(|(_, _, row)| [row[0], row[1], row[2], row[3]]).collect();
        assert_eq!(first, heads, "table {page_type}");
        for (page, slot, row) in &rows {
            assert_eq!(u2(row, 2) as usize, slot * 32, "table {page_type}, page {page}, slot {slot}");
        }
    }
    let track_pages: std::collections::BTreeSet<u32> = rows(&file, 0).iter().map(|(page, _, _)| *page).collect();
    assert!(track_pages.len() > 1, "the numbering starts again on every page, so the tracks must span pages");

    let property = rows(&file, 19);
    assert_eq!(property.len(), 1);
    assert_eq!(u2(property[0].2, 2), 0);
}
