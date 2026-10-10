//! Long and UTF-16 strings in `export.pdb` rows, pinned to the bytes
//! rekordbox writes. A stick rbxport wrote with accented artist names froze a
//! CDJ-2000NXS (E-8709, #321); these rows differed from rekordbox's in two
//! ways: a counted `00 00` after every UTF-16 string, and long strings packed
//! at any offset instead of on a four-byte boundary of the row.
//!
//! The expected bytes are copied from a rekordbox-written `export.pdb`
//! [OBS: TRIODE2 stick, exported 2026-10-03; artist rows on pages 81 and
//! 88, album row on page 236, track row on page 240].
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_pdb::build::{device_sql_string, long_utf16le, push_device_sql_string, FileBuilder};
use rbl_pdb::rows::{album_row, artist_row, track_row, TrackInput, TRACK_STRINGS};
use rbl_pdb::{PageType, Pdb};

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace().map(|h| u8::from_str_radix(h, 16).unwrap()).collect()
}

#[test]
fn a_utf16_string_has_no_terminator() {
    // Artist "LÜRUM": length 0x0e is the four header bytes and five units.
    assert_eq!(long_utf16le("LÜRUM"), hex("90 0e 00 00 4c 00 dc 00 52 00 55 00 4d 00"));
    assert_eq!(long_utf16le(""), hex("90 04 00 00"));
}

#[test]
fn an_artist_with_a_utf16_name_is_rekordboxs_row() {
    // rekordbox: 60 00 e0 03 76 00 00 00 03 0c 00 00 90 0e 00 00 "LÜRUM".
    // Bytes 2..4 are the row's index on its page, stamped by the builder.
    let mut expected = hex("60 00 00 00 76 00 00 00 03 0c 00 00");
    expected.extend(long_utf16le("LÜRUM"));
    assert_eq!(artist_row(0x76, "LÜRUM"), expected);

    let mut expected = hex("60 00 00 00 12 01 00 00 03 0c 00 00");
    expected.extend(hex("90 16 00 00 44 00 4a 00 20 00 54 00 69 00 eb 00 73 00 74 00 6f 00"));
    assert_eq!(artist_row(0x112, "DJ Tiësto"), expected);
}

#[test]
fn an_artist_with_a_short_name_keeps_it_at_0x0a() {
    let mut expected = hex("60 00 00 00 05 00 00 00 03 0a");
    expected.extend(device_sql_string("Faustix"));
    assert_eq!(artist_row(5, "Faustix"), expected);
}

#[test]
fn an_album_with_a_utf16_name_is_rekordboxs_row() {
    let name = "Café Del Mar (Extended Mix)";
    let mut expected = hex("80 00 00 00 00 00 00 00 00 00 00 00 1c 01 00 00 00 00 00 00 03 18 00 00 90 3a 00 00");
    for unit in name.encode_utf16() {
        expected.extend(unit.to_le_bytes());
    }
    assert_eq!(album_row(0x11c, 0, name), expected);
}

#[test]
fn long_strings_in_a_track_row_start_on_four_byte_boundaries() {
    let input = TrackInput {
        id: 1,
        title: "¡Viva La Gloria!".to_owned(),
        filename: "04 ¡viva la gloria!.mp3".to_owned(),
        file_path: "/Contents/Green Day/21st Century Breakdown/04 ¡viva la gloria!.mp3".to_owned(),
        analyze_path: "/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT".to_owned(),
        date_added: "2023-08-06".to_owned(),
        comment: "x".repeat(0x80),
        ..TrackInput::default()
    };
    let row = track_row(&input);
    let mut long = 0;
    for slot in 0..TRACK_STRINGS {
        let at = usize::from(u16::from_le_bytes([row[0x5e + slot * 2], row[0x5f + slot * 2]]));
        if row[at] == 0x40 || row[at] == 0x90 {
            long += 1;
            assert_eq!(at % 4, 0, "slot {slot} starts at {at:#x}");
        }
    }
    assert_eq!(long, 4, "title, filename, path and the long comment");

    // rekordbox's title bytes, then the empty slot 18 string straight after:
    // `90 24 00 00 a1 00 56 00 ... 21 00 03`.
    let title_at = usize::from(u16::from_le_bytes([row[0x5e + 17 * 2], row[0x5f + 17 * 2]]));
    let mut expected = hex("90 24 00 00");
    for unit in "¡Viva La Gloria!".encode_utf16() {
        expected.extend(unit.to_le_bytes());
    }
    expected.push(0x03);
    assert_eq!(row[title_at..title_at + expected.len()], expected[..]);
    // The padding before an aligned string is zero.
    let filename_at = usize::from(u16::from_le_bytes([row[0x5e + 19 * 2], row[0x5f + 19 * 2]]));
    assert!(row[title_at + expected.len()..filename_at].iter().all(|&b| b == 0));

    // And it reads back.
    let mut file = FileBuilder::new(4096);
    file.add_table(0, &[row]);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    let read = &pdb.track_rows(pdb.table(PageType::Tracks).unwrap())[0];
    assert_eq!(read.title, "¡Viva La Gloria!");
    assert_eq!(read.file_path, input.file_path);
    assert_eq!(read.filename, input.filename);
    assert_eq!(read.comment, input.comment);
}

#[test]
fn artist_and_album_names_read_back() {
    let mut file = FileBuilder::new(4096);
    file.add_table(2, &[artist_row(1, "LÜRUM"), artist_row(2, "Faustix")]);
    file.add_table(3, &[album_row(3, 1, "Café Del Mar (Extended Mix)")]);
    let bytes = file.finish();
    let pdb = Pdb::parse(&bytes).unwrap();
    let artists = pdb.named_rows(pdb.table(PageType::Artists).unwrap());
    assert_eq!(artists[0].name, "LÜRUM");
    assert_eq!(artists[1].name, "Faustix");
    let albums = pdb.named_rows(pdb.table(PageType::Albums).unwrap());
    assert_eq!(albums[0].name, "Café Del Mar (Extended Mix)");
}

#[test]
fn a_short_string_is_appended_without_padding() {
    let mut row = vec![0_u8; 3];
    assert_eq!(push_device_sql_string(&mut row, "ab"), 3);
    assert_eq!(row.len(), 6);
    assert_eq!(push_device_sql_string(&mut row, "é"), 8);
    assert_eq!(row[6..8], [0, 0]);
}
