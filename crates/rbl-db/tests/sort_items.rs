//! Sort Items (#443): a folder's children put in name order, in one write.
//!
//! Every test builds its own encrypted library in a tempdir; nothing here can
//! reach an installed library.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cmp::Ordering;

use rbl_db::fixture::{self, Shape};
use rbl_db::write::{Writer, ROOT};
use rbl_db::DbError;
use rusqlite::params;

struct Fixture {
    _dir: tempfile::TempDir,
    writer: Writer,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).expect("build the fixture");
    let writer = Writer::open(location, dir.path().join("backups")).expect("open for writing");
    Fixture { _dir: dir, writer }
}

/// Stands in for the collator, which lives in `rbl-index`: case aside.
fn by_name(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

impl Fixture {
    fn conn(&self) -> &rusqlite::Connection {
        self.writer.library().connection()
    }

    /// A parent's children, in the order the tree reads them.
    fn children(&self, parent: &str) -> Vec<String> {
        let mut stmt = self
            .conn()
            .prepare("SELECT ID FROM djmdPlaylist WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID")
            .unwrap();
        stmt.query_map(params![parent], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
    }

    fn names(&self, parent: &str) -> Vec<String> {
        let mut stmt = self
            .conn()
            .prepare("SELECT Name FROM djmdPlaylist WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID")
            .unwrap();
        stmt.query_map(params![parent], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
    }

    fn seqs(&self, parent: &str) -> Vec<i64> {
        let mut stmt = self
            .conn()
            .prepare("SELECT Seq FROM djmdPlaylist WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID")
            .unwrap();
        stmt.query_map(params![parent], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
    }

    /// A folder holding, in this order: "delta", a folder "Bravo" with two
    /// playlists of its own out of order, "alpha", "Charlie".
    fn unsorted_folder(&mut self) -> (String, String) {
        let home = self.writer.create_folder("Home", ROOT).unwrap();
        self.writer.create_playlist("delta", &home).unwrap();
        let bravo = self.writer.create_folder("Bravo", &home).unwrap();
        self.writer.create_playlist("zulu", &bravo).unwrap();
        self.writer.create_playlist("yankee", &bravo).unwrap();
        self.writer.create_playlist("alpha", &home).unwrap();
        self.writer.create_playlist("Charlie", &home).unwrap();
        (home, bravo)
    }
}

#[test]
fn sorts_one_level_with_folders_and_playlists_together() {
    let mut f = fixture();
    let (home, bravo) = f.unsorted_folder();
    let (changed, edit) = f.writer.sort_children_with_undo(&home, by_name).unwrap();
    assert!(!edit.is_empty());
    assert_eq!(changed.rows, 4);
    // rekordbox's sorter compares names only: the folder Bravo sorts among
    // the playlists rather than ahead of them.
    assert_eq!(f.names(&home), ["alpha", "Bravo", "Charlie", "delta"]);
    assert_eq!(edit.order(), f.children(&home).as_slice());
    // One level: the folder inside keeps its own order.
    assert_eq!(f.names(&bravo), ["zulu", "yankee"]);
    // Seq is contiguous from zero, as a move leaves it.
    assert_eq!(f.seqs(&home), [0, 1, 2, 3]);
}

#[test]
fn sorts_the_top_of_the_tree() {
    let mut f = fixture();
    f.writer.create_playlist("zz last", ROOT).unwrap();
    f.writer.create_playlist("AA first", ROOT).unwrap();
    f.writer.sort_children_with_undo(ROOT, by_name).unwrap();
    let names = f.names(ROOT);
    let mut expected = names.clone();
    expected.sort_by(|a, b| by_name(a, b));
    assert_eq!(names, expected);
    assert_eq!(names.first().map(String::as_str), Some("AA first"));
    assert_eq!(names.last().map(String::as_str), Some("zz last"));
}

#[test]
fn sorting_a_sorted_folder_again_writes_nothing() {
    let mut f = fixture();
    let (home, _) = f.unsorted_folder();
    f.writer.sort_children_with_undo(&home, by_name).unwrap();
    let order = f.children(&home);
    let usns: Vec<i64> = {
        let mut stmt = f.conn().prepare("SELECT rb_local_usn FROM djmdPlaylist WHERE ParentID = ?1 ORDER BY ID").unwrap();
        stmt.query_map(params![home], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
    };

    let (changed, again) = f.writer.sort_children_with_undo(&home, by_name).unwrap();
    assert!(again.is_empty(), "a sorted folder has nothing to undo");
    assert_eq!(changed.rows, 0);
    assert_eq!(f.children(&home), order);
    let usns_after: Vec<i64> = {
        let mut stmt = f.conn().prepare("SELECT rb_local_usn FROM djmdPlaylist WHERE ParentID = ?1 ORDER BY ID").unwrap();
        stmt.query_map(params![home], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
    };
    assert_eq!(usns_after, usns, "no row was touched");
}

#[test]
fn equal_names_keep_the_order_they_had() {
    let mut f = fixture();
    let home = f.writer.create_folder("Home", ROOT).unwrap();
    let b = f.writer.create_playlist("b", &home).unwrap();
    let first = f.writer.create_playlist("Same", &home).unwrap();
    let second = f.writer.create_playlist("same", &home).unwrap();
    f.writer.sort_children_with_undo(&home, by_name).unwrap();
    assert_eq!(f.children(&home), [b, first, second]);
}

#[test]
fn undo_puts_the_old_order_back_and_redo_sorts_again() {
    let mut f = fixture();
    let (home, _) = f.unsorted_folder();
    let before = f.children(&home);
    let (_, edit) = f.writer.sort_children_with_undo(&home, by_name).unwrap();
    let after = f.children(&home);

    f.writer.undo_sort(&edit).unwrap();
    assert_eq!(f.children(&home), before);
    assert_eq!(f.seqs(&home), [0, 1, 2, 3]);
    f.writer.redo_sort(&edit).unwrap();
    assert_eq!(f.children(&home), after);
}

#[test]
fn undo_is_refused_once_the_folder_holds_something_else() {
    let mut f = fixture();
    let (home, _) = f.unsorted_folder();
    let (_, edit) = f.writer.sort_children_with_undo(&home, by_name).unwrap();
    let extra = f.writer.create_playlist("added later", &home).unwrap();
    let now = f.children(&home);

    assert!(matches!(f.writer.undo_sort(&edit), Err(DbError::WriteRefused(_))));
    assert_eq!(f.children(&home), now, "nothing was written");
    assert!(now.contains(&extra));
}

#[test]
fn only_a_folder_or_the_top_can_be_sorted() {
    let mut f = fixture();
    let playlist = f.writer.create_playlist("Just a list", ROOT).unwrap();
    assert!(matches!(f.writer.sort_children_with_undo(&playlist, by_name), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.sort_children_with_undo("999999999", by_name), Err(DbError::WriteRefused(_))));
}

#[test]
fn a_failure_part_way_leaves_the_old_order() {
    let mut f = fixture();
    let (home, _) = f.unsorted_folder();
    let before = f.children(&home);
    let seqs = f.seqs(&home);
    // The third row's update fails, after two have been written in the
    // same transaction.
    let charlie: String = f
        .conn()
        .query_row("SELECT ID FROM djmdPlaylist WHERE Name = 'Charlie'", [], |r| r.get(0))
        .unwrap();
    f.conn()
        .execute_batch(&format!(
            "CREATE TRIGGER fail_sort BEFORE UPDATE OF Seq ON djmdPlaylist WHEN OLD.ID = '{charlie}'
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END"
        ))
        .unwrap();

    assert!(f.writer.sort_children_with_undo(&home, by_name).is_err());
    assert_eq!(f.children(&home), before);
    assert_eq!(f.seqs(&home), seqs);
}
