//! Writing an `exportLibrary.db` and reading it back.
//!
//! The schema and reference tables came from a real rekordbox-authored export;
//! these check that what we write matches its shape and reads back intact.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_onelibrary::build::{Builder, LookupTable, Track, DB_VERSION};
use rbl_onelibrary::{key, ExportLibrary};

/// Every table a real export carries, whether or not it holds rows.
const EXPECTED_TABLES: &[&str] = &[
    "album", "artist", "category", "color", "content", "cue", "genre", "history",
    "history_content", "hotCueBankList", "hotCueBankList_cue", "image", "key", "label",
    "menuItem", "myTag", "myTag_content", "playlist", "playlist_content", "property",
    "recommendedLike", "sort",
];

fn built() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exportLibrary.db");
    let mut builder = Builder::create(&path).expect("create");

    let artist = builder.intern(LookupTable::Artist, "ARTBAT").unwrap();
    let genre = builder.intern(LookupTable::Genre, "Melodic House").unwrap();
    let key_id = builder.intern(LookupTable::Key, "Am").unwrap();

    for i in 0..3 {
        builder
            .add_track(&Track {
                content_id: i64::from(i) + 1,
                title: format!("Track {i}"),
                artist_id: Some(artist),
                genre_id: Some(genre),
                key_id: Some(key_id),
                color_id: Some(4),
                bpm_x100: 12_800 + i64::from(i),
                length: 300,
                track_no: i64::from(i) + 1,
                path: format!("/Contents/ARTBAT/Track {i}.mp3"),
                file_name: format!("Track {i}.mp3"),
                file_size: 9_000_000,
                analysis_path: format!("/PIONEER/USBANLZ/P001/0000000{i}/ANLZ0000.DAT"),
                rating: 3,
                comment: "5A - Am - 128".to_owned(),
                date_added: "2026-09-07".to_owned(),
                ..Track::default()
            })
            .unwrap();
    }

    builder.add_playlist(1, "Friday", 0, 0).unwrap();
    for i in 0..3 {
        builder.add_to_playlist(1, i64::from(i) + 1, i64::from(i) + 1).unwrap();
    }
    builder.finish("RBXPORT", "2026-09-07", 0).unwrap();
    (dir, path)
}

#[test]
fn the_passphrase_derives_to_its_known_prefix() {
    // Three constants have to be intact for this: the blob, the XOR key, and
    // the base85 alphabet. A wrong one fails here rather than as an
    // unreadable database.
    let key = key::passphrase().expect("derive");
    assert!(key.starts_with("r8gd"), "{key}");
    assert_eq!(key.len(), 64);
    assert!(key.chars().all(|c| c.is_ascii_alphanumeric()), "{key}");
}

#[test]
fn deriving_the_passphrase_is_deterministic() {
    assert_eq!(key::passphrase().unwrap(), key::passphrase().unwrap());
}

#[test]
fn a_written_export_carries_every_table_a_real_one_does() {
    let (_dir, path) = built();
    let db = ExportLibrary::open_read_only(&path).expect("reopen");
    let mut tables = db.tables().unwrap();
    tables.sort();
    let mut expected: Vec<String> = EXPECTED_TABLES.iter().map(|s| (*s).to_owned()).collect();
    expected.sort();
    assert_eq!(tables, expected);
}

#[test]
fn the_file_is_encrypted_rather_than_plain_sqlite() {
    // A plain-SQLite export opens fine here and is unreadable on a player,
    // which is the worst possible way for this to go wrong.
    let (_dir, path) = built();
    let head = std::fs::read(&path).unwrap();
    assert!(!head.starts_with(b"SQLite format 3"), "the export was written unencrypted");
}

#[test]
fn tracks_read_back_exactly_as_written() {
    let (_dir, path) = built();
    let db = ExportLibrary::open_read_only(&path).unwrap();
    let conn = db.connection();

    assert_eq!(db.count("content").unwrap(), 3);
    let (title, bpm, path_text, rating, comment): (String, i64, String, i64, String) = conn
        .query_row(
            "SELECT title, bpmx100, path, rating, djComment FROM content WHERE content_id = 2",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(title, "Track 1");
    assert_eq!(bpm, 12_801);
    assert_eq!(path_text, "/Contents/ARTBAT/Track 1.mp3");
    assert_eq!(rating, 3, "three stars, stored as a star count");
    assert_eq!(comment, "5A - Am - 128");

    // The search column is filled from the title, as the reference does.
    let search: String = conn
        .query_row("SELECT titleForSearch FROM content WHERE content_id = 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(search, title);
}

#[test]
fn a_lookup_is_reused_rather_than_duplicated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exportLibrary.db");
    let mut builder = Builder::create(&path).unwrap();

    let first = builder.intern(LookupTable::Artist, "ARTBAT").unwrap();
    let again = builder.intern(LookupTable::Artist, "ARTBAT").unwrap();
    let other = builder.intern(LookupTable::Artist, "Meduza").unwrap();
    assert_eq!(first, again);
    assert_ne!(first, other);

    // An empty name is id 0, which is how the reference spells "none".
    assert_eq!(builder.intern(LookupTable::Album, "").unwrap(), 0);
    builder.finish("X", "2026-09-07", 0).unwrap();

    let db = ExportLibrary::open_read_only(&path).unwrap();
    assert_eq!(db.count("artist").unwrap(), 2);
    assert_eq!(db.count("album").unwrap(), 0);
}

#[test]
fn the_playlist_tree_reads_back_in_order() {
    let (_dir, path) = built();
    let db = ExportLibrary::open_read_only(&path).unwrap();
    let conn = db.connection();

    let (name, parent): (String, i64) = conn
        .query_row("SELECT name, playlist_id_parent FROM playlist WHERE playlist_id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(name, "Friday");
    assert_eq!(parent, 0, "the top level is 0 here, not the string root");

    let mut stmt = conn
        .prepare("SELECT content_id FROM playlist_content WHERE playlist_id = 1 ORDER BY sequenceNo")
        .unwrap();
    let order: Vec<i64> =
        stmt.query_map([], |r| r.get(0)).unwrap().filter_map(Result::ok).collect();
    assert_eq!(order, vec![1, 2, 3]);
}

#[test]
fn the_property_row_reports_what_was_written() {
    let (_dir, path) = built();
    let db = ExportLibrary::open_read_only(&path).unwrap();
    let (device, version, count, created): (String, String, i64, String) = db
        .connection()
        .query_row(
            "SELECT deviceName, dbVersion, numberOfContents, createdDate FROM property",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(device, "RBXPORT");
    assert_eq!(version, DB_VERSION);
    assert_eq!(count, 3, "the track count is written, not assumed");
    assert_eq!(created, "2026-09-07", "a date, not a timestamp");
}

#[test]
fn the_browse_menus_match_the_reference_export() {
    let (_dir, path) = built();
    let db = ExportLibrary::open_read_only(&path).unwrap();
    let conn = db.connection();

    assert_eq!(db.count("menuItem").unwrap(), 27);
    assert_eq!(db.count("category").unwrap(), 22);
    assert_eq!(db.count("sort").unwrap(), 17);
    assert_eq!(db.count("color").unwrap(), 8);

    // The names carry the annotation markers a player translates on.
    let name: String = conn
        .query_row("SELECT name FROM menuItem WHERE menuItem_id = 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "\u{FFFA}ARTIST\u{FFFB}");
    let kind: i64 = conn
        .query_row("SELECT kind FROM menuItem WHERE menuItem_id = 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kind, 129);

    // Colour ids line up with rekordbox's own order.
    let yellow: String = conn
        .query_row("SELECT name FROM color WHERE color_id = 4", [], |r| r.get(0))
        .unwrap();
    assert_eq!(yellow, "Yellow");
}

#[test]
fn an_export_is_never_written_over_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exportLibrary.db");
    std::fs::write(&path, b"someone else's export").unwrap();
    assert!(matches!(Builder::create(&path), Err(rbl_onelibrary::Error::Exists(_))));
    // And the file it refused to touch is untouched.
    assert_eq!(std::fs::read(&path).unwrap(), b"someone else's export");
}

#[test]
fn nothing_is_left_in_the_write_ahead_log() {
    // A stick whose pages live only in a WAL reads as an empty library on a
    // device that does not replay it.
    let (dir, path) = built();
    assert!(path.exists());
    for suffix in ["-wal", "-shm"] {
        let side = dir.path().join(format!("exportLibrary.db{suffix}"));
        assert!(!side.exists(), "{} was left behind", side.display());
    }
}

#[test]
fn text_that_would_break_naive_sql_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exportLibrary.db");
    let mut builder = Builder::create(&path).unwrap();
    let awkward = "O'Brien \"quoted\"; DROP TABLE content;-- とんかつ 🎧";
    let artist = builder.intern(LookupTable::Artist, awkward).unwrap();
    builder
        .add_track(&Track {
            content_id: 1,
            title: awkward.to_owned(),
            artist_id: Some(artist),
            ..Track::default()
        })
        .unwrap();
    builder.finish("X", "2026-09-07", 0).unwrap();

    let db = ExportLibrary::open_read_only(&path).unwrap();
    assert_eq!(db.count("content").unwrap(), 1, "the table survived");
    let title: String = db
        .connection()
        .query_row("SELECT title FROM content WHERE content_id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(title, awkward);
}

#[test]
fn the_stick_settings_read_back_as_the_reference_and_update_in_place() {
    use rbl_onelibrary::settings::StickSettings;

    let (dir, path) = built();
    let mut settings = StickSettings::read(&path).expect("read");
    assert_eq!(settings.device_name, "RBXPORT");
    // Untouched, a fresh export carries exactly the reference rows.
    let reference = StickSettings::default();
    assert_eq!(settings.categories, reference.categories);
    assert_eq!(settings.sorts, reference.sorts);
    assert_eq!(settings.colors, reference.colors);
    assert_eq!(settings.sub_column, reference.sub_column);
    assert_eq!(settings.categories.iter().find(|s| s.menu_item == 2).unwrap().name, "ARTIST");

    // Rename a colour, hide ARTIST, show GENRE first, pick a sub-column.
    settings.colors[0].name = "Vocal".to_owned();
    settings.device_name = "FRIDAY".to_owned();
    for slot in &mut settings.categories {
        match slot.menu_item {
            2 => {
                slot.visible = false;
                slot.seq = 0;
            }
            1 => {
                slot.visible = true;
                slot.seq = 1;
            }
            _ => {}
        }
    }
    settings.sub_column = Some(5);
    settings.write(&path).expect("write");

    let again = StickSettings::read(&path).expect("re-read");
    assert_eq!(again, settings);

    // And a database rebuilt from them keeps every one of the changes.
    let rebuilt = dir.path().join("rebuilt.db");
    let builder = Builder::create_with(&rebuilt, &again).expect("create_with");
    builder.finish(&again.device_name, "2026-09-09", 0).unwrap();
    let carried = StickSettings::read(&rebuilt).expect("read rebuilt");
    assert_eq!(carried, again);
}

#[test]
fn unfinished_builder_never_publishes_a_partial_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exportLibrary.db");
    let mut builder = Builder::create(&path).unwrap();
    builder.intern(LookupTable::Artist, "Uncommitted").unwrap();
    assert!(!path.exists());
    drop(builder);
    assert!(!path.exists());
    let builder = Builder::create(&path).unwrap();
    builder.finish("Device", "2026-09-21", 1).unwrap();
    assert!(ExportLibrary::open_read_only(&path).is_ok());
}
