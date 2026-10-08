//! The `property` table (page type 19): rekordbox's bytes, and the
//! writer and reader agreeing with them.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_pdb::build::{replace_single_page_table, FileBuilder};
use rbl_pdb::rows::{property_row, PdbProperty};
use rbl_pdb::Pdb;

const PAGE: usize = 4096;

/// The live row rekordbox 7.2.14 wrote with "Background Color : Device
/// Library" set to Yellow [OBS 2026-10-08]. It was row 2 of its page, so its
/// index shift is `0x40`.
const REKORDBOX_YELLOW: [u8; 56] = [
    0x80, 0x02, 0x40, 0x00, 0x25, 0x05, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x17, 0x32, 0x30, 0x32,
    0x36, 0x2d, 0x30, 0x35, 0x2d, 0x30, 0x38, 0x19, 0x1e, 0x0b, 0x31, 0x30, 0x30, 0x30, 0x25, 0x43,
    0x68, 0x72, 0x69, 0x73, 0x74, 0x6f, 0x70, 0x68, 0x65, 0x72, 0x27, 0x73, 0x20, 0x55, 0x53, 0x42,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The row rekordbox 7.2.11 wrote for a blank stick, which rbxport used as
/// a fixed template before the fields were known [OBS 2026-09-17].
const REKORDBOX_BLANK: [u8; 40] = [
    0x80, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x17, 0x32, 0x30, 0x32,
    0x36, 0x2d, 0x30, 0x39, 0x2d, 0x31, 0x37, 0x19, 0x1e, 0x0b, 0x31, 0x30, 0x30, 0x30, 0x03, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn stick(color: u8) -> PdbProperty {
    PdbProperty {
        device_name: "Christopher's USB".to_owned(),
        contents: 1317,
        created_date: "2026-05-08".to_owned(),
        background_color: color,
        ..PdbProperty::default()
    }
}

fn file_with(rows: &[Vec<u8>]) -> Vec<u8> {
    let mut file = FileBuilder::new(PAGE);
    file.add_table(19, rows);
    file.finish()
}

#[test]
fn the_row_is_rekordboxs_bytes_apart_from_the_index_shift() {
    let row = property_row(&stick(4)).unwrap();
    let mut expected = REKORDBOX_YELLOW;
    expected[2] = 0; // row 0 of its page, not row 2
    assert_eq!(row, expected.to_vec());
}

#[test]
fn a_blank_stick_gets_rekordboxs_blank_row() {
    let blank = PdbProperty { created_date: "2026-09-17".to_owned(), ..PdbProperty::default() };
    assert_eq!(property_row(&blank).unwrap(), REKORDBOX_BLANK.to_vec());
}

#[test]
fn a_date_that_is_not_ten_ascii_bytes_is_refused() {
    let mut bad = stick(0);
    bad.created_date = "2026-5-8".to_owned();
    assert!(property_row(&bad).is_none());
}

#[test]
fn rekordboxs_row_reads_back() {
    let pdb_bytes = file_with(&[REKORDBOX_YELLOW.to_vec()]);
    let pdb = Pdb::parse(&pdb_bytes).unwrap();
    assert_eq!(pdb.property(), Some(stick(4)));
}

#[test]
fn a_written_row_reads_back_with_any_name() {
    for name in ["", "TEST", "Stick ä", &"x".repeat(200)] {
        let property = PdbProperty { device_name: name.to_owned(), ..stick(7) };
        let bytes = file_with(&[property_row(&property).unwrap()]);
        assert_eq!(Pdb::parse(&bytes).unwrap().property(), Some(property), "{name:?}");
    }
}

#[test]
fn the_row_can_be_replaced_in_place() {
    let before = file_with(&[property_row(&stick(4)).unwrap()]);
    let after = replace_single_page_table(&before, 19, &[property_row(&stick(7)).unwrap()]).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(Pdb::parse(&after).unwrap().property(), Some(stick(7)));
}

#[test]
fn a_file_without_the_row_has_no_property() {
    let bytes = file_with(&[]);
    assert_eq!(Pdb::parse(&bytes).unwrap().property(), None);
    let mut file = FileBuilder::new(PAGE);
    file.add_table(1, &[]);
    assert_eq!(Pdb::parse(&file.finish()).unwrap().property(), None);
}
