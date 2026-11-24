//! The eight track colours' names, read with the library out of a
//! real-schema fixture: a renamed colour reads as its own name, a NULL or
//! empty `Commnt` as the stock one. Written to a temporary fixture only.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used)]

use rbl_db::fixture::{self, Shape};
use rbl_db::{Library as Db, OpenMode};

#[test]
fn the_index_reads_each_colours_name_and_falls_back_to_the_stock_one() {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).expect("build the fixture");
    let db = Db::open(location.clone(), OpenMode::ReadWrite).expect("open for writing");
    for (id, name) in [("1", Some("Opener")), ("8", Some(" Closer ")), ("2", None), ("3", Some(""))] {
        db.connection().execute("UPDATE djmdColor SET Commnt = ?1 WHERE ID = ?2", rusqlite::params![name, id]).unwrap();
    }
    drop(db);

    let db = Db::open(location, OpenMode::ReadOnly).expect("open read-only");
    let (library, _stats) = rbl_index::load(&db).expect("index");
    assert_eq!(
        library.color_names(),
        ["Opener", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", " Closer "]
    );
    assert_eq!(library.color_name(0), "");
    assert_eq!(library.color_name(9), "");

    // What a startup served from the snapshot reads in its place.
    let mut restored = rbl_index::Library::default();
    assert_eq!(restored.color_name(1), "Pink");
    restored.set_color_names(rbl_index::read_color_names(&db).unwrap());
    assert_eq!(restored.color_name(1), "Opener");
}

#[test]
fn existing_snapshot_layout_stays_compatible_and_names_are_refreshed_from_db() {
    use rbl_index::cache::{decode, encode, Fingerprint, FORMAT};
    let directory = tempfile::tempdir().unwrap();
    let location = fixture::build(directory.path(), Shape::default()).unwrap();
    let db = Db::open(location.clone(), OpenMode::ReadWrite).unwrap();
    let (mut original, _) = rbl_index::load(&db).unwrap();
    let fingerprint = Fingerprint { format: FORMAT, db_len: 1, db_modified_ns: 1, wal_len: 0,
        wal_modified_ns: 0, db_version: 6000, content: 1, database: 1 };
    let bytes = encode(&original, fingerprint);
    original.set_color_names(std::array::from_fn(|_| "not part of snapshot".to_owned()));
    assert_eq!(encode(&original, fingerprint), bytes);
    db.connection().execute("UPDATE djmdColor SET Commnt='Fresh name' WHERE ID='1'", []).unwrap();
    let mut restored = decode(&bytes, fingerprint).unwrap();
    restored.set_color_names(rbl_index::read_color_names(&db).unwrap());
    assert_eq!(restored.color_name(1), "Fresh name");
    assert_eq!(restored.len(), original.len());
}
