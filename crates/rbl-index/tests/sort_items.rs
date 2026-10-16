//! Sort Items (#443) end to end below the app: the index's collator decides
//! the order and the writer stores it, on a fixture library.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_db::fixture::{self, Shape};
use rbl_db::write::{Writer, ROOT};
use rbl_index::collate::NameCollator;
use rusqlite::params;

fn names(writer: &Writer, parent: &str) -> Vec<String> {
    let conn = writer.library().connection();
    let mut stmt = conn
        .prepare("SELECT Name FROM djmdPlaylist WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID")
        .unwrap();
    stmt.query_map(params![parent], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
}

#[test]
fn a_folder_takes_the_order_rekordboxs_collator_gives() {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).unwrap();
    let mut writer = Writer::open(location, dir.path().join("backups")).unwrap();
    let home = writer.create_folder("Home", ROOT).unwrap();
    // Case, accents, digits and punctuation: where a byte or lower-case
    // compare and ICU's tertiary order part ways.
    for name in ["Techno", "2 Warmup", "élan", "10 Peak", "ambient", "Ambient", "_inbox", "elan", "Zouk", "bass"] {
        writer.create_playlist(name, &home).unwrap();
    }
    writer.create_folder("Archive", &home).unwrap();

    let collator = NameCollator::new(Some("en"));
    let (_, edit) = writer.sort_children_with_undo(&home, |a, b| collator.compare(a, b)).unwrap();
    assert!(!edit.is_empty());
    assert_eq!(
        names(&writer, &home),
        ["_inbox", "10 Peak", "2 Warmup", "ambient", "Ambient", "Archive", "bass", "elan", "élan", "Techno", "Zouk"],
    );

    // Idempotent: a second sort finds it in order.
    let (changed, again) = writer.sort_children_with_undo(&home, |a, b| collator.compare(a, b)).unwrap();
    assert!(again.is_empty());
    assert_eq!(changed.rows, 0);
}
