//! Round-trips: what the builder writes, the reader must read back exactly.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_pdb::build::{device_sql_string, long_utf16le, short_ascii, FileBuilder, PageBuilder, PageStyle};
use rbl_pdb::{PageType, Pdb};

const PAGE: usize = 4096;

/// A genre row: u4 id then an inline string.
fn genre_row(id: u32, name: &str) -> Vec<u8> {
    let mut row = id.to_le_bytes().to_vec();
    row.extend_from_slice(&device_sql_string(name));
    row
}

#[test]
fn a_built_file_parses_back() {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &[genre_row(1, "House"), genre_row(2, "Techno")]);
    let bytes = file.finish();

    let pdb = Pdb::parse(&bytes).unwrap();
    assert_eq!(pdb.page_size, 4096);
    let table = pdb.table(PageType::Genres).unwrap();
    let rows = pdb.named_rows(table);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, 1);
    assert_eq!(rows[0].name, "House");
    assert_eq!(rows[1].name, "Techno");
}

#[test]
fn many_rows_span_groups_and_pages() {
    // More than 16 forces multiple row groups; enough bytes forces a new page.
    let names: Vec<String> = (0..300).map(|i| format!("Genre number {i:03}")).collect();
    let rows: Vec<Vec<u8>> = names
        .iter()
        .enumerate()
        .map(|(i, n)| genre_row(u32::try_from(i).unwrap() + 1, n))
        .collect();

    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &rows);
    let bytes = file.finish();

    let pdb = Pdb::parse(&bytes).unwrap();
    let table = pdb.table(PageType::Genres).unwrap();
    let read = pdb.named_rows(table);
    assert_eq!(read.len(), 300, "every row must survive paging and grouping");
    assert_eq!(read[0].name, "Genre number 000");
    assert_eq!(read[299].name, "Genre number 299");
    assert_eq!(read[299].id, 300);
}

#[test]
fn short_ascii_length_is_mangled_the_way_the_format_expects() {
    // "incremented, doubled, and incremented again"
    let encoded = short_ascii("abc");
    assert_eq!(encoded[0], ((3 + 1) * 2 + 1) as u8);
    assert_eq!(&encoded[1..], b"abc");

    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &[genre_row(1, "abc")]);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    assert_eq!(pdb.named_rows(pdb.table(PageType::Genres).unwrap())[0].name, "abc");
}

#[test]
fn utf16_strings_round_trip() {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &[genre_row(1, "Drum & Bass — Liquid ✨")]);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    assert_eq!(
        pdb.named_rows(pdb.table(PageType::Genres).unwrap())[0].name,
        "Drum & Bass — Liquid ✨"
    );
}

#[test]
fn the_encoder_picks_utf16_only_when_it_must() {
    assert_eq!(device_sql_string("plain")[0] % 2, 1, "ASCII uses the short form");
    assert_eq!(device_sql_string("é")[0], 0x90, "non-ASCII uses UTF-16");
    assert_eq!(long_utf16le("")[0], 0x90);
}

#[test]
fn an_empty_string_reads_back_as_empty() {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &[genre_row(9, "")]);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    let rows = pdb.named_rows(pdb.table(PageType::Genres).unwrap());
    assert_eq!(rows[0].id, 9);
    assert_eq!(rows[0].name, "");
}

#[test]
fn deleted_rows_are_skipped() {
    // Build a page by hand and clear one presence bit.
    let mut builder = PageBuilder::new(PAGE, 1, 1, 1);
    builder.push_row(&genre_row(1, "Kept"));
    builder.push_row(&genre_row(2, "Deleted"));
    builder.push_row(&genre_row(3, "AlsoKept"));
    let mut page = builder.finish();

    // Clear bit 1 in the first group's presence mask.
    let flags_at = PAGE - 4;
    let mut present = u16::from_le_bytes([page[flags_at], page[flags_at + 1]]);
    present &= !0b010;
    page[flags_at..flags_at + 2].copy_from_slice(&present.to_le_bytes());

    let mut file = vec![0_u8; PAGE];
    file[0x04..0x08].copy_from_slice(&(PAGE as u32).to_le_bytes());
    file[0x08..0x0c].copy_from_slice(&1_u32.to_le_bytes());
    file[28..32].copy_from_slice(&1_u32.to_le_bytes()); // page type genres
    file[36..40].copy_from_slice(&1_u32.to_le_bytes()); // first page
    file[40..44].copy_from_slice(&1_u32.to_le_bytes()); // last page
    file.extend_from_slice(&page);

    let pdb = Pdb::parse(&file).unwrap();
    let names: Vec<String> =
        pdb.named_rows(pdb.table(PageType::Genres).unwrap()).into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["Kept".to_owned(), "AlsoKept".to_owned()]);
}

#[test]
fn rejects_files_that_are_not_devicesql() {
    assert!(Pdb::parse(&[]).is_err());
    assert!(Pdb::parse(&[0; 16]).is_err());
    // A plausible size but a nonsense page size.
    let mut junk = vec![0_u8; 4096];
    junk[4..8].copy_from_slice(&12345_u32.to_le_bytes());
    assert!(Pdb::parse(&junk).is_err());
}

#[test]
fn a_truncated_file_does_not_panic() {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &[genre_row(1, "House")]);
    let bytes = file.finish();
    for cut in [PAGE, PAGE + 100, bytes.len() - 1] {
        let short = &bytes[..cut.min(bytes.len())];
        if let Ok(pdb) = Pdb::parse(short) {
            // Must not panic; returning fewer rows is fine.
            let _ = pdb.census();
        }
    }
}

#[test]
fn a_self_referential_page_chain_terminates() {
    let mut file = vec![0_u8; PAGE * 2];
    file[0x04..0x08].copy_from_slice(&(PAGE as u32).to_le_bytes());
    file[0x08..0x0c].copy_from_slice(&1_u32.to_le_bytes());
    file[28..32].copy_from_slice(&1_u32.to_le_bytes());
    file[36..40].copy_from_slice(&1_u32.to_le_bytes());
    file[40..44].copy_from_slice(&999_u32.to_le_bytes()); // last page never reached
    // Page 1 points at itself.
    file[PAGE + 0x0c..PAGE + 0x10].copy_from_slice(&1_u32.to_le_bytes());
    let pdb = Pdb::parse(&file).unwrap();
    let _ = pdb.census(); // must return, not hang
}

// ---- writing every table type ----

use rbl_pdb::rows::{
    album_row, artist_row, color_row, key_row, playlist_entry_row, playlist_row,
    simple_named_row, track_row, TrackInput,
};

#[test]
fn track_record_control_words_match_the_player_accepted_layout() {
    // Independent byte assertions: the semantic parser ignores these words
    // and previously round-tripped records that the CDJ displayed as empty.
    for (filename, format) in [
        ("track.mp3", 1_u16), ("TRACK.MP3", 1), ("track.m4a", 4),
        ("track.aac", 4), ("track.flac", 5), ("track.wav", 11),
        ("track.aif", 12), ("track.aiff", 12),
    ] {
        let row = track_row(&TrackInput { filename: filename.into(), ..Default::default() });
        assert_eq!(&row[..2], &[0x24, 0], "{filename}: track subtype");
        assert_eq!(&row[0x56..0x58], &[0x29, 0], "{filename}: record trailer");
        assert_eq!(&row[0x5a..0x5c], &format.to_le_bytes(), "{filename}: playback format");
        assert_eq!(&row[0x5c..0x5e], &[3, 0], "{filename}: record trailer");
    }
}

#[test]
fn a_written_track_reads_back_field_for_field() {
    let input = TrackInput {
        id: 42,
        artist_id: 7,
        album_id: 8,
        genre_id: 9,
        key_id: 10,
        label_id: 11,
        artwork_id: 12,
        color_id: 3,
        rating: 4,
        tempo_x100: 12_800,
        duration_sec: 257,
        year: 2026,
        bitrate: 320,
        sample_rate: 44_100,
        file_size: 10_485_760,
        track_number: 5,
        play_count: 6,
        title: "All U Need".into(),
        filename: "All U Need.mp3".into(),
        file_path: "/Contents/TRIODE/Single/All U Need.mp3".into(),
        analyze_path: "/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT".into(),
        comment: "8A - C - 128".into(),
        date_added: "2026-09-06".into(),
        release_date: "2026-08-29".into(),
        ..TrackInput::default()
    };

    let mut file = FileBuilder::new(PAGE);
    file.add_table(0, &[track_row(&input)]);
    let bytes = file.finish();

    let pdb = Pdb::parse(&bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(PageType::Tracks).unwrap());
    assert_eq!(rows.len(), 1);
    let t = &rows[0];

    assert_eq!(t.id, 42);
    assert_eq!(t.artist_id, 7);
    assert_eq!(t.album_id, 8);
    assert_eq!(t.genre_id, 9);
    assert_eq!(t.key_id, 10);
    assert_eq!(t.label_id, 11);
    assert_eq!(t.artwork_id, 12);
    assert_eq!(t.color_id, 3);
    assert_eq!(t.rating, 4);
    assert_eq!(t.tempo_x100, 12_800);
    assert_eq!(t.duration_sec, 257);
    assert_eq!(t.year, 2026);
    assert_eq!(t.bitrate, 320);
    assert_eq!(t.sample_rate, 44_100);
    assert_eq!(t.file_size, 10_485_760);
    assert_eq!(t.track_number, 5);
    assert_eq!(t.play_count, 6);
    assert_eq!(t.title, "All U Need");
    assert_eq!(t.filename, "All U Need.mp3");
    assert_eq!(t.file_path, "/Contents/TRIODE/Single/All U Need.mp3");
    assert_eq!(t.analyze_path, "/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT");
    assert_eq!(t.comment, "8A - C - 128");
    assert_eq!(t.date_added, "2026-09-06");
    assert_eq!(t.release_date, "2026-08-29");
}

#[test]
fn every_name_table_round_trips() {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(2, &[artist_row(1, "TRIODE"), artist_row(2, "ARTBAT")]);
    file.add_table(3, &[album_row(1, 1, "Single")]);
    file.add_table(1, &[simple_named_row(1, "House")]);
    file.add_table(4, &[simple_named_row(1, "Defected")]);
    file.add_table(5, &[key_row(1, "Am"), key_row(2, "Fm")]);
    file.add_table(6, &[color_row(1, "Pink"), color_row(2, "Red")]);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();

    let names = |kind| -> Vec<(u32, String)> {
        pdb.named_rows(pdb.table(kind).unwrap()).into_iter().map(|r| (r.id, r.name)).collect()
    };
    assert_eq!(names(PageType::Artists), vec![(1, "TRIODE".into()), (2, "ARTBAT".into())]);
    assert_eq!(names(PageType::Albums), vec![(1, "Single".into())]);
    assert_eq!(names(PageType::Genres), vec![(1, "House".into())]);
    assert_eq!(names(PageType::Labels), vec![(1, "Defected".into())]);
    assert_eq!(names(PageType::Keys), vec![(1, "Am".into()), (2, "Fm".into())]);
    assert_eq!(names(PageType::Colors), vec![(1, "Pink".into()), (2, "Red".into())]);
}

#[test]
fn playlists_and_their_entries_round_trip() {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(
        7,
        &[
            playlist_row(1, 0, 1, true, "USB"),
            playlist_row(2, 1, 1, false, "Melodic Vox"),
        ],
    );
    file.add_table(
        8,
        &[playlist_entry_row(1, 100, 2), playlist_entry_row(2, 101, 2)],
    );
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();

    let nodes = pdb.playlist_nodes(pdb.table(PageType::PlaylistTree).unwrap());
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].name, "USB");
    assert!(nodes[0].is_folder);
    assert_eq!(nodes[1].name, "Melodic Vox");
    assert!(!nodes[1].is_folder);
    assert_eq!(nodes[1].parent_id, 1);

    let entries = pdb.playlist_entries(pdb.table(PageType::PlaylistEntries).unwrap());
    assert_eq!(entries.len(), 2);
    assert_eq!((entries[0].entry_index, entries[0].track_id, entries[0].playlist_id), (1, 100, 2));
    assert_eq!(entries[1].track_id, 101);
}

#[test]
fn a_track_with_unicode_metadata_round_trips() {
    let input = TrackInput {
        id: 1,
        title: "Ébano — Tiësto Remix".into(),
        file_path: "/Contents/Tiësto/Álbum/Ébano.mp3".into(),
        ..TrackInput::default()
    };
    let mut file = FileBuilder::new(PAGE);
    file.add_table(0, &[track_row(&input)]);
    let pdb_bytes = file.finish();
    let pdb = Pdb::parse(&pdb_bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(PageType::Tracks).unwrap());
    assert_eq!(rows[0].title, "Ébano — Tiësto Remix");
    assert_eq!(rows[0].file_path, "/Contents/Tiësto/Álbum/Ébano.mp3");
}

#[test]
fn many_tracks_span_pages_and_all_survive() {
    let inputs: Vec<Vec<u8>> = (1..=250_u32)
        .map(|i| {
            track_row(&TrackInput {
                id: i,
                title: format!("Track number {i:03}"),
                file_path: format!("/Contents/A/B/track-{i:03}.mp3"),
                tempo_x100: 12_000 + i,
                ..TrackInput::default()
            })
        })
        .collect();

    let mut file = FileBuilder::new(PAGE);
    file.add_table(0, &inputs);
    let bytes = file.finish();

    let pdb = Pdb::parse(&bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(PageType::Tracks).unwrap());
    assert_eq!(rows.len(), 250, "every track must survive paging");
    assert_eq!(rows[0].title, "Track number 001");
    assert_eq!(rows[249].title, "Track number 250");
    assert_eq!(rows[249].tempo_x100, 12_250);
}

/// The playlist row rekordbox 7.2.11 wrote for NP3-TEST-MP3 [OBS 2026-09-17],
/// read back as a player would, and re-encoded to the same bytes.
#[test]
fn rekordbox_playlist_rows_read_and_write_the_same() {
    let real: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x1b, b'N', b'P', b'3', b'-', b'T', b'E', b'S', b'T', b'-', b'M', b'P',
        b'3',
    ];
    let ours = rbl_pdb::rows::playlist_row(1, 0, 0, false, "NP3-TEST-MP3");
    assert_eq!(ours, real, "the row a player reads must be rekordbox's");

    let mut file = rbl_pdb::build::FileBuilder::new(4096);
    file.add_table(7, &[real]);
    let bytes = file.finish();
    let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
    let table = pdb.table(rbl_pdb::PageType::PlaylistTree).expect("playlist table");
    let nodes = pdb.playlist_nodes(table);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].id, 1);
    assert_eq!(nodes[0].parent_id, 0);
    assert!(!nodes[0].is_folder);
    assert_eq!(nodes[0].name, "NP3-TEST-MP3");
}

/// The album and colour rows rekordbox 7.2.11 wrote [OBS 2026-09-17], read
/// back with the album's id where a player looks for it.
#[test]
fn rekordbox_album_and_colour_rows_read_and_write_the_same() {
    let album: Vec<u8> = vec![
        0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x03, 0x16, 0x13, b'F', b'r', b'e', b'a', b'k', b' ', b'E', b'P',
    ];
    assert_eq!(rbl_pdb::rows::album_row(1, 0, "Freak EP"), album);
    let pink: Vec<u8> = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x0b, b'P', b'i', b'n', b'k'];
    assert_eq!(rbl_pdb::rows::color_row(1, "Pink"), pink);

    let mut file = rbl_pdb::build::FileBuilder::new(4096);
    file.add_table(3, &[album]);
    file.add_table(6, &[pink]);
    let bytes = file.finish();
    let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
    let albums = pdb.named_rows(pdb.table(rbl_pdb::PageType::Albums).expect("albums"));
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].id, 1);
    assert_eq!(albums[0].name, "Freak EP");
    let colours = pdb.named_rows(pdb.table(rbl_pdb::PageType::Colors).expect("colours"));
    assert_eq!((colours[0].id, colours[0].name.as_str()), (1, "Pink"));
}

/// A path too long for the short form is written the way rekordbox 7.2.11
/// writes it [OBS 2026-09-17: `40 85 00 00` before a 129-byte path] and reads
/// back whole, without the byte before it or the bytes after it.
#[test]
fn a_long_ascii_string_reads_and_writes_as_rekordbox_does() {
    let path = "/Contents/NIIKO x SWAE, Honey & Badger/NIIKO X SWAE & Honey & Badger - Automatic/niiko x swae, honey & badger - automatic (ex.mp3";
    assert_eq!(path.len(), 129);
    let encoded = rbl_pdb::build::device_sql_string(path);
    assert_eq!(&encoded[..4], &[0x40, 0x85, 0x00, 0x00]);
    assert_eq!(encoded.len(), 133);

    let mut file = rbl_pdb::build::FileBuilder::new(4096);
    let mut row = rbl_pdb::rows::simple_named_row(1, "x");
    row.truncate(4);
    row.extend_from_slice(&encoded);
    row.extend_from_slice(b"ati"); // whatever follows must not be read as part of it
    file.add_table(1, &[row]);
    let bytes = file.finish();
    let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
    let rows = pdb.named_rows(pdb.table(rbl_pdb::PageType::Genres).expect("table"));
    assert_eq!(rows[0].name, path);
}


/// rekordbox's `exportExt.pdb` type-4 pages [OBS, a rekordbox 7 export
/// posted on #316]: full to the byte, `0x20` 1 and `0x22` the last row's
/// index, and the row groups' second word marking only the last row. The
/// reader takes every row of them, and the counted style is unchanged.
#[test]
fn last_row_index_pages_fill_up_and_read_back_whole() {
    let rows: Vec<Vec<u8>> = (0..500_u32).map(|i| [i.to_le_bytes(), [0; 4], [0; 4], [3, 0, 0, 0]].concat()).collect();
    let mut file = FileBuilder::new(PAGE);
    file.add_table_styled(4, &rows, |_, _| {}, PageStyle::LastRowIndex);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    let read = pdb.rows(pdb.table(PageType::Labels).unwrap());
    assert_eq!(read.len(), 500);
    let ids: Vec<u32> = read.iter().map(|&r| pdb.u4_at(r, 0)).collect();
    assert_eq!(ids, (0..500).collect::<Vec<u32>>());
    let mut pages: Vec<usize> = read.iter().map(|r| r.page_offset).collect();
    pages.dedup();
    let counts: Vec<usize> = pages.iter().map(|&p| read.iter().filter(|r| r.page_offset == p).count()).collect();
    assert_eq!(counts, vec![222, 222, 56]);
    assert_eq!(&bytes[pages[0] + 0x1c..pages[0] + 0x24], &[0x04, 0x00, 0xe0, 0x0d, 0x01, 0x00, 0xdd, 0x00]);
    assert_eq!(&bytes[pages[2] + 0x20..pages[2] + 0x24], &[0x01, 0x00, 0x37, 0x00]);
    // 56 rows: four groups, the last of eight rows.
    let last_group = pages[2] + PAGE - 3 * 0x24;
    assert_eq!(&bytes[last_group - 4..last_group], &[0xff, 0x00, 0x80, 0x00]);
    assert_eq!(&bytes[pages[2] + PAGE - 4..pages[2] + PAGE], &[0xff, 0xff, 0x00, 0x00]);

    // The same rows counted: the row count at 0x20, the mask twice, and
    // a new page before the last bytes run out.
    let mut file = FileBuilder::new(PAGE);
    file.add_table(4, &rows);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    let read = pdb.rows(pdb.table(PageType::Labels).unwrap());
    assert_eq!(read.len(), 500);
    let first = read[0].page_offset;
    let on_first = read.iter().filter(|r| r.page_offset == first).count();
    assert_eq!(on_first, 221);
    assert_eq!(&bytes[first + 0x20..first + 0x24], &[221, 0, 0, 0]);
    assert_eq!(&bytes[first + PAGE - 4..first + PAGE], &[0xff, 0xff, 0xff, 0xff]);
}
