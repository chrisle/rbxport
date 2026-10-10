//! Writer behaviour, against a fixture with the real schema.
//!
//! Every test builds its own encrypted library in a tempdir. Nothing here can
//! reach the installed library: `Library::open` refuses read-write on a real
//! install whenever `RBXPORT_TEST` is set, and a fixture is never marked as one.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_db::fixture::{self, playlist_id, track_id, Shape};
use rbl_db::write::{
    AnalysisRegistration, Changed, TrackField, Unsupported, Writer, ANALYSED_FULL, ATTRIBUTE_FOLDER,
    ATTRIBUTE_PLAYLIST, ROOT,
};
use rbl_db::{DbError, Library, OpenMode};
use rusqlite::params;

struct Fixture {
    _dir: tempfile::TempDir,
    writer: Writer,
}

fn fixture_with(shape: Shape) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), shape).expect("build the fixture");
    let writer = Writer::open(location, dir.path().join("backups")).expect("open for writing");
    Fixture { _dir: dir, writer }
}

fn fixture() -> Fixture {
    fixture_with(Shape::default())
}

impl Fixture {
    fn conn(&self) -> &rusqlite::Connection {
        self.writer.library().connection()
    }

    fn one<T: rusqlite::types::FromSql>(&self, sql: &str, args: &[&dyn rusqlite::ToSql]) -> T {
        self.conn().query_row(sql, args, |r| r.get(0)).unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.one(sql, &[])
    }

    /// The `TrackNo` sequence of a playlist, in order.
    fn track_numbers(&self, playlist: &str) -> Vec<i64> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT TrackNo FROM djmdSongPlaylist
                 WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo",
            )
            .unwrap();
        stmt.query_map(params![playlist], |r| r.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    }

    /// A parent's children, in the order the tree reads them.
    fn children(&self, parent: &str) -> Vec<String> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT ID FROM djmdPlaylist
                 WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID",
            )
            .unwrap();
        stmt.query_map(params![parent], |r| r.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    }

    /// The `Seq` run under a parent, to show it has no gaps or repeats.
    fn seqs(&self, parent: &str) -> Vec<i64> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT Seq FROM djmdPlaylist
                 WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID",
            )
            .unwrap();
        stmt.query_map(params![parent], |r| r.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    }

    /// The content ids of a playlist, in playing order.
    fn order(&self, playlist: &str) -> Vec<String> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT ContentID FROM djmdSongPlaylist
                 WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo",
            )
            .unwrap();
        stmt.query_map(params![playlist], |r| r.get(0))
            .unwrap()
            .filter_map(Result::ok)
            .collect()
    }
}

// ------------------------------------------------------------- new-row shape

#[test]
fn a_new_playlist_has_the_shape_rekordbox_gives_a_local_row() {
    // Every one of the 57 locally-created playlists in the reference library
    // carries rb_data_status 0 and a NULL usn. 256/257 are written by the
    // cloud sync, not by creation — a distinction two sample rows would miss.
    let mut f = fixture();
    let id = f.writer.create_playlist("New Set", ROOT).unwrap();

    let (status, local_status, deleted, synced): (i64, i64, i64, i64) = f
        .conn()
        .query_row(
            "SELECT rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced
             FROM djmdPlaylist WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((status, local_status, deleted, synced), (0, 0, 0, 0));

    let usn: Option<i64> = f.one("SELECT usn FROM djmdPlaylist WHERE ID = ?1", &[&id]);
    assert_eq!(usn, None, "usn is the sync's to assign, not ours");

    let local_usn: i64 = f.one("SELECT rb_local_usn FROM djmdPlaylist WHERE ID = ?1", &[&id]);
    assert!(local_usn > 1000, "must advance past the fixture's starting counter");

    let attribute: i64 = f.one("SELECT Attribute FROM djmdPlaylist WHERE ID = ?1", &[&id]);
    assert_eq!(attribute, ATTRIBUTE_PLAYLIST);
}

#[test]
fn a_new_row_carries_a_uuid_and_matching_timestamps() {
    let mut f = fixture();
    let id = f.writer.create_playlist("Set", ROOT).unwrap();
    let (uuid, created, updated): (String, String, String) = f
        .conn()
        .query_row(
            "SELECT UUID, created_at, updated_at FROM djmdPlaylist WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(uuid.len(), 36, "{uuid}");
    assert_eq!(uuid.split('-').count(), 5);
    assert_eq!(created, updated, "a row created now was not modified later");
    assert!(created.ends_with(" +00:00"), "{created}");
}

#[test]
fn a_folder_differs_from_a_playlist_only_by_its_attribute() {
    let mut f = fixture();
    let folder = f.writer.create_folder("Gigs", ROOT).unwrap();
    let attribute: i64 = f.one("SELECT Attribute FROM djmdPlaylist WHERE ID = ?1", &[&folder]);
    assert_eq!(attribute, ATTRIBUTE_FOLDER);
    // rb_data_status is *not* the folder marker, despite 257 appearing on
    // folders in the reference library: it appears under both Attribute values.
    let status: i64 = f.one("SELECT rb_data_status FROM djmdPlaylist WHERE ID = ?1", &[&folder]);
    assert_eq!(status, 0);
}

#[test]
fn ids_do_not_collide_with_rows_already_there() {
    let mut f = fixture();
    let mut made = std::collections::HashSet::new();
    for i in 0..40 {
        made.insert(f.writer.create_playlist(&format!("Set {i}"), ROOT).unwrap());
    }
    assert_eq!(made.len(), 40);
    for p in 0..3 {
        assert!(!made.contains(&playlist_id(p)), "reused a fixture id");
    }
}

// ------------------------------------------------------------------ the tree

#[test]
fn seq_appends_rather_than_assuming_a_base() {
    // Parents in the reference library start at Seq 0 or 1, so there is no
    // base to assume — a new child goes after the largest.
    let mut f = fixture();
    let a = f.writer.create_playlist("A", ROOT).unwrap();
    let b = f.writer.create_playlist("B", ROOT).unwrap();
    let seq_a: i64 = f.one("SELECT Seq FROM djmdPlaylist WHERE ID = ?1", &[&a]);
    let seq_b: i64 = f.one("SELECT Seq FROM djmdPlaylist WHERE ID = ?1", &[&b]);
    assert_eq!(seq_b, seq_a + 1);
    // The fixture already has three at root, seq 0..2.
    assert_eq!(seq_a, 3);
}

#[test]
fn a_playlist_can_be_created_inside_a_folder() {
    let mut f = fixture();
    let folder = f.writer.create_folder("Gigs", ROOT).unwrap();
    let inner = f.writer.create_playlist("Friday", &folder).unwrap();
    let parent: String = f.one("SELECT ParentID FROM djmdPlaylist WHERE ID = ?1", &[&inner]);
    assert_eq!(parent, folder);
}

#[test]
fn a_parent_that_does_not_exist_is_refused() {
    let mut f = fixture();
    let outcome = f.writer.create_playlist("Orphan", "no-such-folder");
    assert!(matches!(outcome, Err(DbError::WriteRefused(_))), "{outcome:?}");
    // And nothing was written.
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE Name = 'Orphan'"), 0);
}

#[test]
fn a_folder_cannot_be_moved_inside_itself() {
    // This detaches the whole subtree from the tree and it is never seen again.
    let mut f = fixture();
    let outer = f.writer.create_folder("Outer", ROOT).unwrap();
    let inner = f.writer.create_folder("Inner", &outer).unwrap();
    let deep = f.writer.create_folder("Deep", &inner).unwrap();

    assert!(matches!(f.writer.move_to(&outer, &outer, None), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.move_to(&outer, &inner, None), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.move_to(&outer, &deep, None), Err(DbError::WriteRefused(_))));
    // Moving the other way is fine.
    assert!(f.writer.move_to(&deep, ROOT, None).is_ok());
}

#[test]
fn a_node_takes_the_place_it_is_moved_to_among_its_siblings() {
    // Inside a folder of its own: the fixture's root already holds playlists,
    // and what is being pinned here is a whole sibling run.
    let mut f = fixture();
    let home = f.writer.create_folder("Home", ROOT).unwrap();
    let a = f.writer.create_playlist("A", &home).unwrap();
    let b = f.writer.create_playlist("B", &home).unwrap();
    let c = f.writer.create_playlist("C", &home).unwrap();
    assert_eq!(f.children(&home), vec![a.clone(), b.clone(), c.clone()]);

    // Last to first, which is the drag a tree makes most often.
    f.writer.move_to(&c, &home, Some(0)).unwrap();
    assert_eq!(f.children(&home), vec![c.clone(), a.clone(), b.clone()]);

    // Into the middle, counted after the node is lifted out: with C removed
    // the run is [A, B], so 1 puts it between them.
    f.writer.move_to(&c, &home, Some(1)).unwrap();
    assert_eq!(f.children(&home), vec![a.clone(), c.clone(), b.clone()]);

    // Past the end is the end, not a gap.
    f.writer.move_to(&a, &home, Some(99)).unwrap();
    assert_eq!(f.children(&home), vec![c.clone(), b.clone(), a.clone()]);

    // No index means appended, which is what it meant before there was one.
    f.writer.move_to(&c, &home, None).unwrap();
    assert_eq!(f.children(&home), vec![b, a, c]);

    // Seq is left contiguous from zero: rekordbox reads the order from it.
    assert_eq!(f.seqs(&home), vec![0, 1, 2]);
}

#[test]
fn a_node_moved_into_a_folder_takes_a_place_there() {
    let mut f = fixture();
    let folder = f.writer.create_folder("Folder", ROOT).unwrap();
    let first = f.writer.create_playlist("First", &folder).unwrap();
    let second = f.writer.create_playlist("Second", &folder).unwrap();
    let outside = f.writer.create_playlist("Outside", ROOT).unwrap();

    f.writer.move_to(&outside, &folder, Some(1)).unwrap();
    assert_eq!(f.children(&folder), vec![first, outside.clone(), second]);
    assert!(!f.children(ROOT).contains(&outside), "it left the root it was in");
    assert_eq!(f.seqs(&folder), vec![0, 1, 2]);
}

#[test]
fn renaming_changes_the_name_and_nothing_structural() {
    let mut f = fixture();
    let id = f.writer.create_playlist("Before", ROOT).unwrap();
    let seq_before: i64 = f.one("SELECT Seq FROM djmdPlaylist WHERE ID = ?1", &[&id]);
    let created: String = f.one("SELECT created_at FROM djmdPlaylist WHERE ID = ?1", &[&id]);

    let changed = f.writer.rename(&id, "After").unwrap();
    assert_eq!(changed.rows, 1);

    let (name, seq, created_now): (String, i64, String) = f
        .conn()
        .query_row(
            "SELECT Name, Seq, created_at FROM djmdPlaylist WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(name, "After");
    assert_eq!(seq, seq_before);
    assert_eq!(created_now, created, "created_at must not move");
}

#[test]
fn deleting_a_folder_takes_its_whole_subtree_and_the_memberships() {
    let mut f = fixture();
    let outer = f.writer.create_folder("Outer", ROOT).unwrap();
    let inner = f.writer.create_folder("Inner", &outer).unwrap();
    let list = f.writer.create_playlist("Deep list", &inner).unwrap();
    f.writer.add_tracks(&list, &[track_id(0), track_id(1)]).unwrap();

    f.writer.delete_playlist(&outer).unwrap();

    for id in [&outer, &inner, &list] {
        let deleted: i64 = f.one("SELECT rb_local_deleted FROM djmdPlaylist WHERE ID = ?1", &[id]);
        assert_eq!(deleted, 1, "{id} should be soft-deleted");
    }
    assert_eq!(
        f.count(&format!(
            "SELECT COUNT(*) FROM djmdSongPlaylist
             WHERE PlaylistID = '{list}' AND rb_local_deleted = 0"
        )),
        0
    );
}

#[test]
fn a_playlist_deletion_can_be_undone_and_redone_exactly() {
    let mut f = fixture();
    let before = f.children(ROOT);
    let outer = f.writer.create_folder("Outer", ROOT).unwrap();
    let inner = f.writer.create_folder("Inner", &outer).unwrap();
    let list = f.writer.create_playlist("Deep list", &inner).unwrap();
    let tracks = [track_id(0), track_id(1), track_id(2)];
    f.writer.add_tracks(&list, &tracks).unwrap();
    // This old tombstone must stay deleted when the later playlist deletion
    // is undone.
    f.writer.remove_tracks(&list, &[tracks[1].clone()]).unwrap();

    let (_, deletion) = f.writer.delete_playlist_with_undo(&outer).unwrap();
    assert_eq!(deletion.playlist_ids.len(), 3);
    assert_eq!(deletion.membership_ids.len(), 2);
    assert_eq!(f.children(ROOT), before);

    f.writer.restore_playlist(&deletion).unwrap();
    let mut restored_root = before.clone();
    restored_root.push(outer.clone());
    assert_eq!(f.children(ROOT), restored_root);
    assert_eq!(f.children(&outer), std::slice::from_ref(&inner));
    assert_eq!(f.children(&inner), std::slice::from_ref(&list));
    assert_eq!(f.order(&list), [tracks[0].clone(), tracks[2].clone()]);

    f.writer.redo_playlist_deletion(&deletion).unwrap();
    assert_eq!(f.children(ROOT), before);
    assert!(f.order(&list).is_empty());
}

#[test]
fn a_delete_is_always_soft() {
    // rekordbox's sync relies on the tombstone; a real DELETE loses it.
    let mut f = fixture();
    let before = f.count("SELECT COUNT(*) FROM djmdPlaylist");
    let id = f.writer.create_playlist("Doomed", ROOT).unwrap();
    f.writer.delete_playlist(&id).unwrap();
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist"), before + 1, "the row must remain");

    // And rb_data_status is untouched: all 919 deleted rows in the reference
    // library kept theirs.
    let status: i64 = f.one("SELECT rb_data_status FROM djmdPlaylist WHERE ID = ?1", &[&id]);
    assert_eq!(status, 0);
}

// ---------------------------------------------------------------- membership

#[test]
fn tracks_append_with_contiguous_numbering() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    let tracks: Vec<String> = (0..6).map(track_id).collect();
    let changed = f.writer.add_tracks(&list, &tracks).unwrap();
    assert_eq!(changed.rows, 6);
    assert_eq!(f.track_numbers(&list), vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(f.order(&list), tracks);
}

#[test]
fn a_track_listed_twice_in_one_drop_is_added_once() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    f.writer.add_tracks(&list, &[track_id(1)]).unwrap();
    let drop = [track_id(0), track_id(1), track_id(0), track_id(2)];
    let changed = f.writer.add_tracks(&list, &drop).unwrap();
    assert_eq!(changed.rows, 2);
    assert_eq!(f.order(&list), [track_id(1), track_id(0), track_id(2)]);
    assert_eq!(f.track_numbers(&list), vec![1, 2, 3]);
}

#[test]
fn a_membership_row_is_identified_by_a_uuid_not_a_number() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    f.writer.add_tracks(&list, &[track_id(0)]).unwrap();
    let id: String = f.one(
        "SELECT ID FROM djmdSongPlaylist WHERE PlaylistID = ?1",
        &[&list],
    );
    assert_eq!(id.len(), 36, "{id}");
    assert_eq!(id.split('-').count(), 5);
}

#[test]
fn adding_a_track_already_present_does_nothing() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    f.writer.add_tracks(&list, &[track_id(0), track_id(1)]).unwrap();
    let changed = f.writer.add_tracks(&list, &[track_id(0), track_id(2)]).unwrap();
    assert_eq!(changed.rows, 1, "only the new one");
    assert_eq!(f.order(&list), vec![track_id(0), track_id(1), track_id(2)]);
}

#[test]
fn adding_a_track_already_present_can_add_it_again() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    f.writer.add_tracks(&list, &[track_id(0), track_id(1)]).unwrap();
    let changed = f.writer.add_tracks_allowing(&list, &[track_id(0), track_id(2)], true).unwrap();
    assert_eq!(changed.rows, 2, "the repeat and the new one");
    assert_eq!(f.order(&list), vec![track_id(0), track_id(1), track_id(0), track_id(2)]);
}

#[test]
fn setting_a_playlists_tracks_replaces_them_in_the_order_given() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    f.writer.add_tracks(&list, &[track_id(0), track_id(1), track_id(2)]).unwrap();

    let wanted = vec![track_id(3), track_id(1), track_id(3)];
    let changed = f.writer.set_tracks(&list, &wanted).unwrap();

    assert_eq!(changed.rows, 2, "a repeated track is written once");
    assert_eq!(f.order(&list), vec![track_id(3), track_id(1)]);
    assert_eq!(f.track_numbers(&list), vec![1, 2]);
    assert_eq!(f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"), changed.usn);
    // The old rows are soft-deleted, as remove_tracks leaves them.
    assert_eq!(
        f.one::<i64>("SELECT COUNT(*) FROM djmdSongPlaylist WHERE PlaylistID = ?1 AND rb_local_deleted = 1", &[&list]),
        3
    );

    let usn = f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'");
    let again = f.writer.set_tracks(&list, &[track_id(3), track_id(1)]).unwrap();
    assert_eq!(again.rows, 0, "already holding exactly these writes nothing");
    assert_eq!(f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"), usn);
}

#[test]
fn a_set_of_a_playlists_tracks_that_fails_partway_leaves_the_old_ones() {
    // The old members are soft-deleted before the bad track is reached; the
    // refusal must roll that back, not leave the playlist empty.
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    let old = vec![track_id(2), track_id(0), track_id(1)];
    f.writer.add_tracks(&list, &old).unwrap();
    let usn = f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'");
    let rows = f.count("SELECT COUNT(*) FROM djmdSongPlaylist");

    let err = f.writer.set_tracks(&list, &[track_id(3), "no-such-track".to_owned()]).unwrap_err();

    assert!(matches!(err, DbError::WriteRefused(_)), "{err:?}");
    assert_eq!(f.order(&list), old);
    assert_eq!(f.track_numbers(&list), vec![1, 2, 3]);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongPlaylist"), rows);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongPlaylist WHERE rb_local_deleted = 1"), 0);
    assert_eq!(f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"), usn);
}

#[test]
fn an_intelligent_playlist_or_folder_has_no_tracks_to_set() {
    let mut f = fixture();
    let smart = f.writer.create_smart_playlist("Smart", ROOT, |_| "<NODE/>".to_owned()).unwrap();
    let folder = f.writer.create_folder("Crate", ROOT).unwrap();
    for node in [&smart, &folder] {
        let err = f.writer.set_tracks(node, &[track_id(0)]).unwrap_err();
        assert!(matches!(err, DbError::WriteRefused(_)), "{err:?}");
        assert_eq!(f.one::<i64>("SELECT COUNT(*) FROM djmdSongPlaylist WHERE PlaylistID = ?1", &[node]), 0);
    }
}

#[test]
fn removing_a_track_closes_the_gap_it_leaves() {
    // TrackNo is contiguous from 1 in every one of the 683 reference
    // playlists; a hole makes rekordbox render the playlist with a gap.
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    let tracks: Vec<String> = (0..5).map(track_id).collect();
    f.writer.add_tracks(&list, &tracks).unwrap();

    f.writer.remove_tracks(&list, &[track_id(1), track_id(3)]).unwrap();

    assert_eq!(f.track_numbers(&list), vec![1, 2, 3]);
    assert_eq!(f.order(&list), vec![track_id(0), track_id(2), track_id(4)]);
}

#[test]
fn reordering_puts_the_tracks_in_the_order_given() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    let tracks: Vec<String> = (0..4).map(track_id).collect();
    f.writer.add_tracks(&list, &tracks).unwrap();

    let wanted = vec![track_id(3), track_id(0), track_id(2), track_id(1)];
    f.writer.reorder(&list, &wanted).unwrap();
    assert_eq!(f.order(&list), wanted);
    assert_eq!(f.track_numbers(&list), vec![1, 2, 3, 4]);
}

#[test]
fn a_partial_reorder_keeps_the_tracks_it_did_not_mention() {
    // Dropping unmentioned tracks would silently empty a playlist when a caller
    // passes only the visible window.
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    let tracks: Vec<String> = (0..5).map(track_id).collect();
    f.writer.add_tracks(&list, &tracks).unwrap();

    f.writer.reorder(&list, &[track_id(4), track_id(3)]).unwrap();

    let order = f.order(&list);
    assert_eq!(order.len(), 5, "nothing may be dropped");
    assert_eq!(order.first(), Some(&track_id(4)));
    assert_eq!(order.get(1), Some(&track_id(3)));
    assert_eq!(f.track_numbers(&list), vec![1, 2, 3, 4, 5]);
}

#[test]
fn a_reorder_naming_a_track_that_is_not_there_ignores_it() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    f.writer.add_tracks(&list, &[track_id(0), track_id(1)]).unwrap();
    f.writer.reorder(&list, &[track_id(9), track_id(1), track_id(0)]).unwrap();
    assert_eq!(f.order(&list), vec![track_id(1), track_id(0)]);
}

#[test]
fn deleting_a_track_removes_it_from_every_playlist_and_renumbers_each() {
    let mut f = fixture();
    let a = f.writer.create_playlist("A", ROOT).unwrap();
    let b = f.writer.create_playlist("B", ROOT).unwrap();
    f.writer.add_tracks(&a, &[track_id(0), track_id(1), track_id(2)]).unwrap();
    f.writer.add_tracks(&b, &[track_id(1), track_id(3)]).unwrap();

    f.writer.delete_track(&track_id(1)).unwrap();

    assert_eq!(f.order(&a), vec![track_id(0), track_id(2)]);
    assert_eq!(f.track_numbers(&a), vec![1, 2]);
    assert_eq!(f.order(&b), vec![track_id(3)]);
    assert_eq!(f.track_numbers(&b), vec![1]);
    let deleted: i64 = f.one("SELECT rb_local_deleted FROM djmdContent WHERE ID = ?1",
                             &[&track_id(1)]);
    assert_eq!(deleted, 1);
}

// ------------------------------------------------------------------ metadata

#[test]
fn a_rating_is_stored_as_rekordbox_stores_it() {
    // Stars as a count, 0 to 5: the reference library's 269 rated rows hold
    // 1 to 5 and nothing else [OBS]. The multiples of 51 the XML export uses
    // do not belong here — stored, the index reads any of them as five stars.
    let mut f = fixture();
    for (stars, stored) in [(0_u8, 0_i64), (1, 1), (3, 3), (5, 5)] {
        f.writer.set_rating(&track_id(0), stars).unwrap();
        let value: i64 = f.one("SELECT Rating FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
        assert_eq!(value, stored, "{stars} stars");
    }
}

#[test]
fn an_impossible_rating_is_refused_rather_than_clamped() {
    let mut f = fixture();
    assert!(matches!(f.writer.set_rating(&track_id(0), 6), Err(DbError::WriteRefused(_))));
    let value: i64 = f.one("SELECT Rating FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert_eq!(value, 0, "the refused write must not have landed");
}

#[test]
fn a_comment_round_trips_including_awkward_text() {
    let mut f = fixture();
    for text in ["", "5A - Am - 128", "quote \" and ' apostrophe", "とんかつ 🎧", "a; DROP TABLE x;--"] {
        f.writer.set_comment(&track_id(2), text).unwrap();
        let stored: String = f.one("SELECT Commnt FROM djmdContent WHERE ID = ?1", &[&track_id(2)]);
        assert_eq!(stored, text);
    }
    // The injection attempt above must not have dropped anything.
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent"), 40);
}

#[test]
fn a_colour_can_be_set_and_cleared() {
    let mut f = fixture();
    f.writer.set_color(&track_id(3), Some("4")).unwrap();
    let value: Option<String> = f.one("SELECT ColorID FROM djmdContent WHERE ID = ?1", &[&track_id(3)]);
    assert_eq!(value.as_deref(), Some("4"));
    f.writer.set_color(&track_id(3), None).unwrap();
    let cleared: Option<String> = f.one("SELECT ColorID FROM djmdContent WHERE ID = ?1", &[&track_id(3)]);
    assert_eq!(cleared, None);
}

// --------------------------------------------------- the information panel

#[test]
fn a_plain_field_is_written_to_its_own_column() {
    let mut f = fixture();
    let t = track_id(4);
    f.writer.set_field(&t, TrackField::Title, "Renamed (Extended Mix)").unwrap();
    f.writer.set_field(&t, TrackField::Lyricist, "Words").unwrap();
    f.writer.set_field(&t, TrackField::Year, "2023").unwrap();
    f.writer.set_field(&t, TrackField::TrackNumber, " 7 ").unwrap();
    f.writer.set_field(&t, TrackField::DiscNumber, "2").unwrap();
    f.writer.set_field(&t, TrackField::PlayCount, "12").unwrap();
    let title: String = f.one("SELECT Title FROM djmdContent WHERE ID = ?1", &[&t]);
    let lyricist: String = f.one("SELECT Lyricist FROM djmdContent WHERE ID = ?1", &[&t]);
    let numbers: (i64, i64, i64, i64) = f
        .conn()
        .query_row(
            "SELECT ReleaseYear, TrackNo, DiscNo, DJPlayCount FROM djmdContent WHERE ID = ?1",
            params![t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(title, "Renamed (Extended Mix)");
    assert_eq!(lyricist, "Words");
    assert_eq!(numbers, (2023, 7, 2, 12));
}

#[test]
fn a_number_that_is_not_one_is_refused_rather_than_zeroed() {
    let mut f = fixture();
    let t = track_id(4);
    f.writer.set_field(&t, TrackField::Year, "2019").unwrap();
    for bad in ["", "abc", "-1", "20x", "10000"] {
        assert!(
            matches!(f.writer.set_field(&t, TrackField::Year, bad), Err(DbError::WriteRefused(_))),
            "{bad:?} must be refused"
        );
    }
    let year: i64 = f.one("SELECT ReleaseYear FROM djmdContent WHERE ID = ?1", &[&t]);
    assert_eq!(year, 2019, "a refused write leaves the year alone");
}

#[test]
fn a_reference_field_makes_its_lookup_row_once_and_shares_it() {
    let mut f = fixture();
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdArtist"), 0);

    f.writer.set_field(&track_id(0), TrackField::Artist, "TRIODE").unwrap();
    f.writer.set_field(&track_id(1), TrackField::Artist, "TRIODE").unwrap();
    f.writer.set_field(&track_id(1), TrackField::Remixer, "TRIODE").unwrap();
    f.writer.set_field(&track_id(2), TrackField::Composer, "Someone Else").unwrap();
    f.writer.set_field(&track_id(2), TrackField::OriginalArtist, "TRIODE").unwrap();

    // One artist row per distinct name, whatever column pointed at it.
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdArtist"), 2);
    let triode: String = f.one("SELECT ID FROM djmdArtist WHERE Name = 'TRIODE'", &[]);
    for (track, column) in [
        (track_id(0), "ArtistID"),
        (track_id(1), "ArtistID"),
        (track_id(1), "RemixerID"),
        (track_id(2), "OrgArtistID"),
    ] {
        let id: String =
            f.one(&format!("SELECT {column} FROM djmdContent WHERE ID = ?1"), &[&track]);
        assert_eq!(id, triode, "{column} of {track}");
    }

    // The new lookup row has the local-creation shape, as an import's does.
    let (status, usn, uuid): (i64, Option<i64>, String) = f
        .conn()
        .query_row(
            "SELECT rb_data_status, usn, UUID FROM djmdArtist WHERE ID = ?1",
            params![triode],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, 0);
    assert_eq!(usn, None);
    assert_eq!(uuid.len(), 36);
}

#[test]
fn album_genre_and_label_go_through_their_own_tables() {
    let mut f = fixture();
    let t = track_id(6);
    f.writer.set_field(&t, TrackField::Album, "An Album").unwrap();
    f.writer.set_field(&t, TrackField::Genre, "Tech House").unwrap();
    f.writer.set_field(&t, TrackField::Label, "Anjuna").unwrap();
    let (album, genre, label): (String, String, String) = f
        .conn()
        .query_row(
            "SELECT al.Name, g.Name, l.Name FROM djmdContent c
             JOIN djmdAlbum al ON al.ID = c.AlbumID
             JOIN djmdGenre g ON g.ID = c.GenreID
             JOIN djmdLabel l ON l.ID = c.LabelID
             WHERE c.ID = ?1",
            params![t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((album.as_str(), genre.as_str(), label.as_str()), ("An Album", "Tech House", "Anjuna"));
}

#[test]
fn an_emptied_reference_clears_to_null_and_leaves_the_lookup_row() {
    let mut f = fixture();
    let t = track_id(7);
    f.writer.set_field(&t, TrackField::Artist, "Gone Soon").unwrap();
    f.writer.set_field(&t, TrackField::Artist, "   ").unwrap();
    let artist: Option<String> = f.one("SELECT ArtistID FROM djmdContent WHERE ID = ?1", &[&t]);
    assert_eq!(artist, None, "NULL, as 3,942 of the reference library's artist-less tracks are");
    // Another track may still point at it; nothing is ever hard-deleted.
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdArtist WHERE Name = 'Gone Soon'"), 1);
}

#[test]
fn a_key_is_found_never_made() {
    let mut f = fixture();
    let stamp = rbl_core::time::now();
    f.conn()
        .execute(
            "INSERT INTO djmdKey (ID, ScaleName, Seq, created_at, updated_at) VALUES ('12', 'Fm', 7, ?1, ?1)",
            params![stamp],
        )
        .unwrap();
    let t = track_id(8);
    f.writer.set_field(&t, TrackField::Key, "Fm").unwrap();
    let key: String = f.one("SELECT KeyID FROM djmdContent WHERE ID = ?1", &[&t]);
    assert_eq!(key, "12");

    assert!(matches!(
        f.writer.set_field(&t, TrackField::Key, "H#m"),
        Err(DbError::WriteRefused(_))
    ));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey"), 1, "no key row is invented");
    let still: String = f.one("SELECT KeyID FROM djmdContent WHERE ID = ?1", &[&t]);
    assert_eq!(still, "12");

    f.writer.set_field(&t, TrackField::Key, "").unwrap();
    let cleared: Option<String> = f.one("SELECT KeyID FROM djmdContent WHERE ID = ?1", &[&t]);
    assert_eq!(cleared, None);
}

#[test]
fn a_detected_key_is_added_when_the_library_has_not_seen_it() {
    let mut f = fixture();
    let before: i64 = f.one("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]);

    f.writer.ensure_detected_key("Fm").unwrap();
    let key: (String, Option<i64>, i64, i64, i64, i64, Option<i64>) = f.conn().query_row(
        "SELECT ScaleName, Seq, rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, usn
         FROM djmdKey WHERE ScaleName = 'Fm'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
    ).unwrap();
    assert_eq!(key, ("Fm".to_owned(), None, 0, 0, 0, 0, None));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey WHERE ScaleName = 'Fm'"), 1);
    let after: i64 = f.one("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]);
    assert_eq!(after, before + 1);

    // A repeat analysis uses the first key row rather than accumulating
    // identical lookup records.
    f.writer.ensure_detected_key("Fm").unwrap();
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey WHERE ScaleName = 'Fm'"), 1);
}

#[test]
fn every_field_edit_bumps_the_usn_and_the_stamp() {
    let mut f = fixture();
    let t = track_id(9);
    let before: i64 = f.one("SELECT rb_local_usn FROM djmdContent WHERE ID = ?1", &[&t]);
    let changed = f.writer.set_field(&t, TrackField::Artist, "Anyone").unwrap();
    assert_eq!(changed.rows, 1);
    let (usn, updated, created): (i64, String, String) = f
        .conn()
        .query_row(
            "SELECT rb_local_usn, updated_at, created_at FROM djmdContent WHERE ID = ?1",
            params![t],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(usn > before);
    assert_eq!(usn, changed.usn);
    assert_ne!(updated, created, "updated_at moves, created_at does not");
    let counter: i64 =
        f.one("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]);
    assert_eq!(counter, usn);
}

#[test]
fn the_wire_names_round_trip() {
    for (name, field) in [
        ("title", TrackField::Title),
        ("artist", TrackField::Artist),
        ("album", TrackField::Album),
        ("year", TrackField::Year),
        ("trackNumber", TrackField::TrackNumber),
        ("discNumber", TrackField::DiscNumber),
        ("originalArtist", TrackField::OriginalArtist),
        ("composer", TrackField::Composer),
        ("remixer", TrackField::Remixer),
        ("lyricist", TrackField::Lyricist),
        ("playCount", TrackField::PlayCount),
        ("genre", TrackField::Genre),
        ("label", TrackField::Label),
        ("key", TrackField::Key),
        ("bpm", TrackField::Bpm),
    ] {
        assert_eq!(TrackField::parse(name), Some(field));
    }
    // What the panel shows read-only must not be reachable by name either.
    for refused in ["albumArtist", "mixName", "message", "hotCueAutoLoad", "publish", ""] {
        assert_eq!(TrackField::parse(refused), None, "{refused}");
    }
}

// ----------------------------------------------------------------- the USN

#[test]
fn every_write_advances_the_usn_and_the_registry_follows() {
    let mut f = fixture();
    let mut last = 0;
    for i in 0..5 {
        let id = f.writer.create_playlist(&format!("Set {i}"), ROOT).unwrap();
        let usn: i64 = f.one("SELECT rb_local_usn FROM djmdPlaylist WHERE ID = ?1", &[&id]);
        assert!(usn > last, "usn must advance: {usn} after {last}");
        last = usn;
        let counter: i64 = f.count(
            "SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
        );
        assert_eq!(counter, usn, "the registry must track the rows");
    }
}

#[test]
fn no_two_rows_share_a_usn() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    let tracks: Vec<String> = (0..8).map(track_id).collect();
    f.writer.add_tracks(&list, &tracks).unwrap();

    let mut stmt = f
        .conn()
        .prepare("SELECT rb_local_usn FROM djmdSongPlaylist WHERE PlaylistID = ?1")
        .unwrap();
    let usns: Vec<i64> = stmt
        .query_map(params![list], |r| r.get(0))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    let distinct: std::collections::HashSet<_> = usns.iter().collect();
    assert_eq!(distinct.len(), usns.len(), "a reused USN makes the sync skip a row");
}

#[test]
fn the_usn_starts_above_whatever_is_already_in_the_tables() {
    // The registry counter has been seen lagging the table maximum; taking the
    // larger of the two is what stops a USN being reused.
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape { start_usn: 10, ..Shape::default() }).unwrap();

    // Push a row's USN far past the registry counter, as a lagging counter does.
    {
        let library = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
        drop(library);
    }
    let conn = rusqlite::Connection::open(&location.master_db).unwrap();
    conn.pragma_update(None, "cipher", "sqlcipher").unwrap();
    conn.pragma_update(None, "legacy", 4).unwrap();
    conn.pragma_update(None, "key", &location.passphrase).unwrap();
    conn.execute("UPDATE djmdContent SET rb_local_usn = 999999 WHERE ID = ?1",
                 params![track_id(0)]).unwrap();
    drop(conn);

    let mut writer = Writer::open(location, dir.path().join("backups")).unwrap();
    let id = writer.create_playlist("After", ROOT).unwrap();
    let usn: i64 = writer
        .library()
        .connection()
        .query_row("SELECT rb_local_usn FROM djmdPlaylist WHERE ID = ?1", params![id], |r| r.get(0))
        .unwrap();
    assert!(usn > 999_999, "got {usn}, which reuses a USN already in the table");
}

// ---------------------------------------------------------------- the guards

#[test]
fn the_library_is_backed_up_before_the_first_write_and_only_once() {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).unwrap();
    let backups = dir.path().join("backups");
    let mut writer = Writer::open(location, &backups).unwrap();

    assert!(!backups.exists(), "opening alone must not back up");
    writer.create_playlist("First", ROOT).unwrap();
    let after_first = std::fs::read_dir(&backups).unwrap().count();
    assert_eq!(after_first, 1);

    for i in 0..3 {
        writer.create_playlist(&format!("More {i}"), ROOT).unwrap();
    }
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1, "once per session");
    assert!(writer.backed_up());
}

#[test]
fn a_writer_told_the_session_is_backed_up_takes_no_backup_of_its_own() {
    // The app opens a writer per edit; the second edit of a session must not
    // copy the library again.
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).unwrap();
    let backups = dir.path().join("backups");
    let mut first = Writer::open(location.clone(), &backups).unwrap();
    first.create_playlist("First", ROOT).unwrap();
    drop(first);
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1);

    let mut second = Writer::open(location, &backups).unwrap();
    assert!(!second.backed_up());
    second.mark_backed_up();
    second.create_playlist("Second", ROOT).unwrap();
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1, "the session's backup stands");
}

#[test]
fn a_backup_is_a_readable_library_in_its_own_right() {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).unwrap();
    let backups = dir.path().join("backups");
    let mut writer = Writer::open(location.clone(), &backups).unwrap();
    writer.create_playlist("Only in the live one", ROOT).unwrap();
    drop(writer);

    let backup = std::fs::read_dir(&backups).unwrap().next().unwrap().unwrap().path();
    let restored = Library::open(
        rbl_db::LibraryLocation { master_db: backup, ..location },
        OpenMode::ReadOnly,
    )
    .expect("the backup must open with the same passphrase");
    assert_eq!(restored.live_track_count().unwrap(), 40);
    // Taken before the write, so it must not contain it.
    let n: i64 = restored
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM djmdPlaylist WHERE Name = 'Only in the live one'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0, "the backup is the state before the write");
}

#[test]
fn a_refused_action_leaves_nothing_behind() {
    let mut f = fixture();
    let before: i64 = f.count("SELECT COUNT(*) FROM djmdPlaylist");
    let counter_before: i64 =
        f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'");

    assert!(f.writer.create_playlist("Nope", "missing").is_err());

    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist"), before);
    assert_eq!(
        f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"),
        counter_before,
        "a rolled-back transaction must not move the counter"
    );
}

#[test]
fn the_unsupported_edits_are_refused_with_a_reason() {
    let action = Unsupported::ContentCueOrFile;
    let error = Writer::refuse(action);
    let DbError::WriteRefused(reason) = error else {
        panic!("{action:?} should be a refusal");
    };
    assert!(!reason.is_empty());
    // The reason has to say what would settle it, or it is just a "no".
    assert!(
        reason.contains("recording") || reason.contains("not understood"),
        "{reason}"
    );
}

#[test]
fn only_the_named_columns_can_be_set() {
    // The column name is interpolated into SQL. It comes from this crate today,
    // but a future caller reaching it would be injecting into a real library.
    let mut f = fixture();
    assert!(f.writer.set_comment(&track_id(0), "fine").is_ok());
    // The allowlist is what makes that safe; prove it rejects rather than runs.
    let outcome = f.writer.rename("nope", "x");
    assert!(outcome.is_ok(), "a real column still works: {outcome:?}");
}

#[test]
fn a_changed_report_says_how_many_rows_moved() {
    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    assert_eq!(f.writer.add_tracks(&list, &[track_id(0), track_id(1)]).unwrap().rows, 2);
    assert_eq!(f.writer.rename(&list, "Renamed").unwrap().rows, 1);
    assert_eq!(
        f.writer.rename("no-such-playlist", "x").unwrap(),
        Changed { rows: 0, usn: f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'") },
        "renaming nothing changes nothing"
    );
}

#[test]
fn a_large_playlist_stays_consistent_through_many_edits() {
    let mut f = fixture_with(Shape { tracks: 200, ..Shape::default() });
    let list = f.writer.create_playlist("Big", ROOT).unwrap();
    let all: Vec<String> = (0..200).map(track_id).collect();
    f.writer.add_tracks(&list, &all).unwrap();

    // Remove every third track, then reverse what is left.
    let doomed: Vec<String> = (0..200).step_by(3).map(track_id).collect();
    f.writer.remove_tracks(&list, &doomed).unwrap();
    let mut remaining = f.order(&list);
    remaining.reverse();
    f.writer.reorder(&list, &remaining).unwrap();

    assert_eq!(f.order(&list), remaining);
    let expected: Vec<i64> = (1..=remaining.len() as i64).collect();
    assert_eq!(f.track_numbers(&list), expected, "numbering must stay 1..N");
}

// -------------------------------------------------------------- relocating

#[test]
fn a_track_can_be_pointed_at_a_moved_file() {
    let f_dir = tempfile::tempdir().unwrap();
    let moved = f_dir.path().join("moved elsewhere.mp3");
    std::fs::write(&moved, b"audio").unwrap();

    let mut f = fixture();
    f.writer.relocate(&track_id(0), &moved).unwrap();

    let (folder, name): (String, String) = f
        .conn()
        .query_row(
            "SELECT FolderPath, FileNameL FROM djmdContent WHERE ID = ?1",
            params![track_id(0)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(folder, moved.to_string_lossy());
    assert_eq!(name, "moved elsewhere.mp3");
}

#[test]
fn relocating_to_something_that_is_not_a_file_is_refused() {
    // Pointing a track at a directory, or at nothing, loses the old location
    // for no gain.
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture();
    let before: String =
        f.one("SELECT FolderPath FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);

    for bad in [dir.path().to_path_buf(), dir.path().join("no-such-file.mp3")] {
        assert!(matches!(f.writer.relocate(&track_id(0), &bad), Err(DbError::WriteRefused(_))));
    }
    let after: String =
        f.one("SELECT FolderPath FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert_eq!(after, before, "a refused relocate must not have moved anything");
}

#[test]
fn relocating_leaves_the_analysis_and_memberships_alone() {
    // Everything else keys off the track's id, so moving the audio must not
    // disturb it.
    let f_dir = tempfile::tempdir().unwrap();
    let moved = f_dir.path().join("elsewhere.mp3");
    std::fs::write(&moved, b"audio").unwrap();

    let mut f = fixture();
    let list = f.writer.create_playlist("Set", ROOT).unwrap();
    f.writer.add_tracks(&list, &[track_id(0), track_id(1)]).unwrap();
    // rekordbox's Relocate keeps a track's cues, beat grid and the rest
    // [OBS issue #201, the Help Center's Relocate article]: the cues are
    // `djmdCue` rows and the grid lives in the files `AnalysisDataPath` names.
    let cue = f.writer.add_cue(&track_id(0), 1, 12_345).unwrap();
    let analysis: Option<String> =
        f.one("SELECT AnalysisDataPath FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);

    f.writer.relocate(&track_id(0), &moved).unwrap();

    assert_eq!(f.order(&list), vec![track_id(0), track_id(1)]);
    let deleted: i64 =
        f.one("SELECT rb_local_deleted FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert_eq!(deleted, 0);
    let (owner, at, gone): (String, i64, i64) = f.conn().query_row(
        "SELECT ContentID, InMsec, rb_local_deleted FROM djmdCue WHERE ID = ?1",
        params![cue],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!((owner.as_str(), at, gone), (track_id(0).as_str(), 12_345, 0));
    let after: Option<String> =
        f.one("SELECT AnalysisDataPath FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert_eq!(after, analysis, "the grid's files stay the track's");
}

// ------------------------------------------------------------------- cues

#[test]
fn a_hot_cue_bank_slot_is_updated_atomically_and_missing_slots_are_refused() {
    let mut f = fixture();
    f.conn().execute_batch(
        "CREATE TABLE djmdSongHotCueBanklist (
             ID TEXT PRIMARY KEY, HotCueBanklistID TEXT, TrackNo INTEGER, ContentID TEXT,
             InMsec INTEGER, OutMsec INTEGER, Color INTEGER, ColorTableIndex INTEGER,
             ActiveLoop INTEGER, BeatLoopSize INTEGER, CueMicrosec INTEGER,
             rb_local_deleted INTEGER, rb_local_usn INTEGER, updated_at TEXT
         );
         INSERT INTO djmdSongHotCueBanklist VALUES
             ('bank-slot', '42', 1, '1', 100, NULL, 1, 2, 0, 0, 0, 0, 0, 'before');",
    ).unwrap();
    let cue = rbl_db::details::HotCueBankCue {
        slot: 1, content: 2, in_ms: 1_000, out_ms: Some(2_000), color: 3,
        color_table_index: 21, active_loop: true, beat_loop_size: 262_145, cue_microsec: 7,
    };
    let changed = f.writer.set_hot_cue_bank_cue("42", &cue).unwrap();
    assert_eq!(changed.rows, 1);
    let stored: (String, i64, Option<i64>, i64, i64, i64, i64, i64, i64, i64) = f.conn().query_row(
        "SELECT ContentID, InMsec, OutMsec, Color, ColorTableIndex, ActiveLoop,
                BeatLoopSize, CueMicrosec, rb_local_usn,
                CAST(updated_at <> 'before' AS INTEGER)
         FROM djmdSongHotCueBanklist WHERE ID = 'bank-slot'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?)),
    ).unwrap();
    assert_eq!(stored, ("2".into(), 1_000, Some(2_000), 3, 21, 1, 262_145, 7, changed.usn, 1));
    assert_eq!(f.one::<i64>("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]), changed.usn);
    let missing = rbl_db::details::HotCueBankCue { slot: 2, ..cue };
    assert!(matches!(f.writer.set_hot_cue_bank_cue("42", &missing), Err(DbError::WriteRefused(_))));
    assert_eq!(f.one::<i64>("SELECT COUNT(*) FROM djmdSongHotCueBanklist", &[]), 1);
}

#[test]
fn a_cue_is_written_in_the_shape_the_reference_library_shows() {
    // Every column here was settled by counting the reference library's
    // 1,040,598 cues rather than guessed. See the write module's docs.
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(0), 1, 45_000).unwrap();

    let (kind, in_ms, color, index, loop_size, active, out_ms): (
        i64, i64, i64, i64, Option<i64>, i64, Option<i64>,
    ) = f
        .conn()
        .query_row(
            "SELECT Kind, InMsec, Color, ColorTableIndex, BeatLoopSize, ActiveLoop, OutMsec
             FROM djmdCue WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!(kind, 1, "hot cue A");
    assert_eq!(in_ms, 45_000);
    assert_eq!(color, -1, "hot cues carry -1");
    assert_eq!(index, 21, "the default colour rekordbox writes");
    assert_eq!(loop_size, None, "not a loop");
    assert_eq!(active, 0);
    assert_eq!(out_ms, None);
}

#[test]
fn a_memory_cue_differs_from_a_hot_one_in_its_colour_columns() {
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(0), 0, 1_000).unwrap();
    let (color, index): (i64, i64) = f
        .conn()
        .query_row(
            "SELECT Color, ColorTableIndex FROM djmdCue WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    // 255 and 0, against -1 and 21 for a hot cue.
    assert_eq!(color, 255);
    assert_eq!(index, 0);
}

#[test]
fn a_cue_points_at_its_track_by_id_and_by_uuid() {
    // Both, or rekordbox's sync sees a cue with no owner.
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(2), 5, 10_000).unwrap();
    let (content, content_uuid): (String, String) = f
        .conn()
        .query_row(
            "SELECT ContentID, ContentUUID FROM djmdCue WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(content, track_id(2));
    let expected: String =
        f.one("SELECT UUID FROM djmdContent WHERE ID = ?1", &[&track_id(2)]);
    assert_eq!(content_uuid, expected);
}

#[test]
fn a_cue_carries_a_uuid_of_its_own_and_a_local_usn() {
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(0), 2, 5_000).unwrap();
    let (uuid, usn, sync_usn): (String, i64, Option<i64>) = f
        .conn()
        .query_row(
            "SELECT UUID, rb_local_usn, usn FROM djmdCue WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(uuid.len(), 36);
    assert!(usn > 1000);
    assert_eq!(sync_usn, None, "usn is the sync's to assign");
}

#[test]
fn kind_four_is_refused_because_rekordbox_does_not_use_it() {
    let mut f = fixture();
    assert!(matches!(f.writer.add_cue(&track_id(0), 4, 0), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.add_cue(&track_id(0), 18, 0), Err(DbError::WriteRefused(_))));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdCue"), 0);
}

#[test]
fn a_cue_on_a_track_that_is_not_there_is_refused() {
    let mut f = fixture();
    assert!(matches!(f.writer.add_cue("no-such-track", 1, 0), Err(DbError::WriteRefused(_))));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdCue"), 0);
}

#[test]
fn a_cue_moves_and_soft_deletes() {
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(0), 1, 1_000).unwrap();

    f.writer.move_cue(&id, 90_000).unwrap();
    let at: i64 = f.one("SELECT InMsec FROM djmdCue WHERE ID = ?1", &[&id]);
    assert_eq!(at, 90_000);

    f.writer.delete_cue(&id).unwrap();
    let deleted: i64 = f.one("SELECT rb_local_deleted FROM djmdCue WHERE ID = ?1", &[&id]);
    assert_eq!(deleted, 1);
    // Soft, as everywhere else: the row stays for the sync's sake.
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdCue"), 1);
}

#[test]
fn a_hot_cue_moves_to_a_slot_and_the_cues_between_shift_over() {
    let mut f = fixture();
    let a = f.writer.add_cue(&track_id(0), 1, 1_000).unwrap();
    let b = f.writer.add_cue(&track_id(0), 2, 2_000).unwrap();
    let c = f.writer.add_cue(&track_id(0), 3, 3_000).unwrap();
    let d = f.writer.add_cue(&track_id(0), 5, 4_000).unwrap();
    let other = f.writer.add_cue(&track_id(1), 1, 9_000).unwrap();
    let kinds = |f: &Fixture| -> Vec<i64> {
        [&a, &b, &c, &d].iter().map(|id| f.one("SELECT Kind FROM djmdCue WHERE ID = ?1", &[id])).collect()
    };

    // C to the front: A and B each move down one slot, D stays.
    f.writer.move_hot_cue(&c, 1).unwrap();
    assert_eq!(kinds(&f), [2, 3, 1, 5]);
    // The front cue to the end of the run: everything between moves up one.
    f.writer.move_hot_cue(&c, 5).unwrap();
    assert_eq!(kinds(&f), [1, 2, 5, 3]);
    // Another track's cue is never touched.
    let theirs: i64 = f.one("SELECT Kind FROM djmdCue WHERE ID = ?1", &[&other]);
    assert_eq!(theirs, 1);
    // Onto its own slot: nothing changes.
    let before = kinds(&f);
    f.writer.move_hot_cue(&c, 5).unwrap();
    assert_eq!(kinds(&f), before);
}

#[test]
fn a_memory_cue_has_no_slot_to_move_to_or_from() {
    let mut f = fixture();
    let memory = f.writer.add_cue(&track_id(0), 0, 1_000).unwrap();
    let hot = f.writer.add_cue(&track_id(0), 1, 2_000).unwrap();
    assert!(matches!(f.writer.move_hot_cue(&memory, 2), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.move_hot_cue(&hot, 0), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.move_hot_cue(&hot, 4), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.move_hot_cue("no-such-cue", 2), Err(DbError::WriteRefused(_))));
}

#[test]
fn every_hot_cue_slot_rekordbox_uses_can_be_written() {
    // 1-3 and 5-17: sixteen slots, A to P, with 4 unused.
    let mut f = fixture();
    for kind in [1_u8, 2, 3, 5, 6, 7, 8, 9, 10, 17] {
        f.writer.add_cue(&track_id(0), kind, u32::from(kind) * 1000).unwrap();
    }
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdCue WHERE rb_local_deleted = 0"), 10);
}

#[test]
fn a_cue_id_is_a_number_under_2_to_the_32_like_rekordbox_s_own() {
    // Every one of the reference library's 1,041,056 cue ids is a decimal
    // string, the largest 4,294,966,064, and not one is a UUID. The index
    // keeps them as u32, so a UUID here would be a cue that cannot be edited.
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(0), 0, 1_000).unwrap();
    let value: u64 = id.parse().expect("a decimal id");
    assert!(value > 0 && value < (1 << 32), "{id}");
    let looped = f.writer.add_loop(&track_id(0), 0, 2_000, 4_000, 4).unwrap();
    assert!(looped.parse::<u32>().is_ok(), "{looped}");
}

#[test]
fn a_cue_knows_its_track_until_it_is_deleted() {
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(4), 0, 1_000).unwrap();
    assert_eq!(f.writer.cue_owner(&id).unwrap(), Some(track_id(4)));
    assert_eq!(f.writer.cue_owner("no-such-cue").unwrap(), None);
    f.writer.delete_cue(&id).unwrap();
    assert_eq!(f.writer.cue_owner(&id).unwrap(), None, "a deleted cue has no owner to report");
}

#[test]
fn cue_colours_use_the_field_rekordbox_assigns_to_each_kind() {
    let mut f = fixture();
    let memory = f.writer.add_cue(&track_id(0), 0, 1_000).unwrap();
    let hot = f.writer.add_cue(&track_id(0), 1, 2_000).unwrap();
    f.writer.set_cue_colour(&memory, Some(3)).unwrap();
    f.writer.set_cue_colour(&hot, Some(46)).unwrap();
    fn columns(writer: &Writer, id: &str) -> (i64, i64) {
        writer.library().connection().query_row(
            "SELECT Color, ColorTableIndex FROM djmdCue WHERE ID=?1", [id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        ).unwrap()
    }
    assert_eq!(columns(&f.writer, &memory), (3, 0));
    assert_eq!(columns(&f.writer, &hot), (-1, 46));
    f.writer.set_cue_colour(&memory, None).unwrap();
    f.writer.set_cue_colour(&hot, None).unwrap();
    assert_eq!(columns(&f.writer, &memory), (255, 0));
    assert_eq!(columns(&f.writer, &hot), (-1, 21));
}

#[test]
fn a_cue_can_be_named_and_unnamed() {
    let mut f = fixture();
    let id = f.writer.add_cue(&track_id(0), 0, 1_000).unwrap();
    let comment = |writer: &Writer| -> String {
        writer.library().connection().query_row(
            "SELECT Comment FROM djmdCue WHERE ID=?1", [&id], |row| row.get(0),
        ).unwrap()
    };
    assert_eq!(comment(&f.writer), "");
    f.writer.set_cue_comment(&id, "Drop").unwrap();
    assert_eq!(comment(&f.writer), "Drop");
    f.writer.set_cue_comment(&id, "").unwrap();
    assert_eq!(comment(&f.writer), "");
}

// ----------------------------------------------------------------- import

/// A minimal but genuine WAV, so the tag reader has something real to open.
fn write_wav(path: &std::path::Path, seconds: u32) {
    let rate = 44_100_u32;
    let samples = rate * seconds;
    let data_len = samples * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1_u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(44 + data_len as usize, 0);
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_file_is_imported_in_the_shape_a_local_row_has() {
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Some Track.wav");
    write_wav(&path, 2);

    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();

    let (status, local_status, synced, usn): (i64, i64, i64, Option<i64>) = f
        .conn()
        .query_row(
            "SELECT rb_data_status, rb_local_data_status, rb_local_synced, usn
             FROM djmdContent WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    // The shape all 634 locally-created tracks in the reference library have.
    assert_eq!((status, local_status, synced), (0, 0, 0));
    assert_eq!(usn, None, "usn is the sync's to assign");
}

#[test]
fn an_import_carries_what_rekordbox_needs_to_open_the_file() {
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Some Track.wav");
    write_wav(&path, 2);

    let mut f = fixture();
    f.conn()
        .execute("UPDATE djmdProperty SET DeviceID = 'dev-1'", [])
        .unwrap();
    let db_id: String = f.one("SELECT DBID FROM djmdProperty", &[]);
    // A path with `..` in it is stored clean: rekordbox marks the raw form missing.
    let dotted = audio.path().join("sub").join("..").join("Some Track.wav");
    let id = f.writer.import_file(&dotted).unwrap();

    let (folder, file_type, depth, stock, created, master, song, device, hot, org, ext): (
        String, i64, i64, String, String, String, String, String, String, String, String,
    ) = f
        .conn()
        .query_row(
            "SELECT FolderPath, FileType, BitDepth, StockDate, DateCreated, MasterDBID, MasterSongID,
                    DeviceID, HotCueAutoLoad, OrgFolderPath, ExtInfo
             FROM djmdContent WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)),
        )
        .unwrap();
    assert_eq!(folder, path.to_string_lossy());
    assert_eq!((file_type, depth), (11, 16), "a 16-bit wav");
    assert_eq!(stock.len(), 10, "{stock}");
    assert_eq!(stock, created);
    assert_eq!((master, song, device), (db_id, id.clone(), "dev-1".to_owned()));
    assert_eq!((hot.as_str(), org.as_str(), ext.as_str()), ("on", "", "null"));
}

#[test]
fn an_imported_track_leaves_analysed_unset() {
    // Every track in the reference library has been analysed, so it cannot
    // show what the field holds before analysis. NULL asserts nothing.
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Track.wav");
    write_wav(&path, 1);

    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();
    let analysed: Option<i64> =
        f.one("SELECT Analysed FROM djmdContent WHERE ID = ?1", &[&id]);
    assert_eq!(analysed, None);
}

#[test]
fn an_untagged_file_takes_its_filename_as_its_title() {
    // A row with no title is unusable, and the filename is what someone
    // actually recognises.
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Bicep - Glue.wav");
    write_wav(&path, 1);

    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();
    let title: String = f.one("SELECT Title FROM djmdContent WHERE ID = ?1", &[&id]);
    assert_eq!(title, "Bicep - Glue");
}

#[test]
fn an_import_records_where_the_file_is_and_how_long_it_runs() {
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Track.wav");
    write_wav(&path, 3);

    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();
    let (folder, name, length): (String, String, i64) = f
        .conn()
        .query_row(
            "SELECT FolderPath, FileNameL, Length FROM djmdContent WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(folder, path.to_string_lossy());
    assert_eq!(name, "Track.wav");
    assert_eq!(length, 3);
}

#[test]
fn an_imported_file_is_found_again_by_its_path() {
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Track.wav");
    write_wav(&path, 1);
    let other = audio.path().join("Other.wav");
    write_wav(&other, 1);

    let mut f = fixture();
    assert_eq!(f.writer.track_id_at(&path).unwrap(), None);
    let id = f.writer.import_file(&path).unwrap();
    assert_eq!(f.writer.track_id_at(&path).unwrap(), Some(id));
    assert_eq!(f.writer.track_id_at(&other).unwrap(), None);
}

#[test]
fn importing_the_same_file_twice_is_refused() {
    // One file with two rows leaves every playlist pointing at the wrong one.
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Track.wav");
    write_wav(&path, 1);

    let mut f = fixture();
    f.writer.import_file(&path).unwrap();
    assert!(matches!(f.writer.import_file(&path), Err(DbError::WriteRefused(_))));
    assert_eq!(
        f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0"),
        41,
        "forty fixture tracks and the one import"
    );
}

#[test]
fn importing_something_that_is_not_audio_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("notes.txt");
    std::fs::write(&text, b"x").unwrap();

    let mut f = fixture();
    assert!(matches!(f.writer.import_file(&text), Err(DbError::WriteRefused(_))));
    assert!(matches!(
        f.writer.import_file(&dir.path().join("missing.wav")),
        Err(DbError::WriteRefused(_))
    ));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent"), 40);
}

#[test]
fn an_imported_track_can_go_straight_into_a_playlist() {
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Track.wav");
    write_wav(&path, 1);

    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();
    let list = f.writer.create_playlist("New", ROOT).unwrap();
    f.writer.add_tracks(&list, std::slice::from_ref(&id)).unwrap();
    assert_eq!(f.order(&list), vec![id]);
}

#[test]
fn a_loop_records_its_length_in_beats() {
    // BeatLoopSize is (beats << 16) | 1. Every value in the reference library
    // fits — 65537, 524289, 1048577, 2097153, 4194305 are 1, 8, 16, 32, 64 —
    // and the one loop whose track is still live carries 262145, four beats,
    // over an In/Out span measuring exactly four beats at its own BPM.
    let mut f = fixture();
    let id = f.writer.add_loop(&track_id(0), 1, 10_000, 11_739, 4).unwrap();

    let (out, size): (i64, i64) = f
        .conn()
        .query_row(
            "SELECT OutMsec, BeatLoopSize FROM djmdCue WHERE ID = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(out, 11_739);
    assert_eq!(size, 262_145, "four beats");
    assert_eq!(size >> 16, 4);
    assert_eq!(size & 0xFFFF, 1, "the low half is 1 on every value observed");
}

#[test]
fn every_loop_length_the_library_uses_round_trips() {
    let mut f = fixture();
    for (beats, expected) in [(1_u16, 65_537_i64), (8, 524_289), (16, 1_048_577),
                              (32, 2_097_153), (64, 4_194_305)] {
        let id = f.writer.add_loop(&track_id(1), 0, 0, 1000, beats).unwrap();
        let size: i64 = f.one("SELECT BeatLoopSize FROM djmdCue WHERE ID = ?1", &[&id]);
        assert_eq!(size, expected, "{beats} beats");
    }
}

#[test]
fn a_loop_with_no_stated_length_leaves_the_field_zero() {
    // Most of the library's loops do this: the length is implied by In and Out.
    let mut f = fixture();
    let id = f.writer.add_loop(&track_id(0), 0, 1000, 2000, 0).unwrap();
    let size: i64 = f.one("SELECT BeatLoopSize FROM djmdCue WHERE ID = ?1", &[&id]);
    assert_eq!(size, 0);
}

#[test]
fn a_loop_that_ends_before_it_starts_is_refused() {
    let mut f = fixture();
    assert!(matches!(f.writer.add_loop(&track_id(0), 1, 5000, 5000, 4), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.add_loop(&track_id(0), 1, 5000, 1000, 4), Err(DbError::WriteRefused(_))));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdCue"), 0);
}

// ---------------------------------------------------------------- analysis

#[test]
fn an_analysis_path_is_derived_from_the_uuid_and_kept_once_set() {
    let f = fixture();
    let track = track_id(1);
    f.conn()
        .execute(
            "UPDATE djmdContent SET UUID = 'a1b2c3d4-0000-4000-8000-000000000001', AnalysisDataPath = NULL WHERE ID = ?1",
            params![track],
        )
        .unwrap();
    assert_eq!(
        f.writer.analysis_data_path_for(&track).unwrap(),
        "/PIONEER/USBANLZ/a1b/2c3d4-0000-4000-8000-000000000001/ANLZ0000.DAT"
    );

    f.conn()
        .execute("UPDATE djmdContent SET AnalysisDataPath = '/PIONEER/USBANLZ/a1b/x/ANLZ0002.DAT' WHERE ID = ?1", params![track])
        .unwrap();
    assert_eq!(f.writer.analysis_data_path_for(&track).unwrap(), "/PIONEER/USBANLZ/a1b/x/ANLZ0002.DAT");

    f.conn().execute("UPDATE djmdContent SET UUID = NULL, AnalysisDataPath = NULL WHERE ID = ?1", params![track]).unwrap();
    assert!(matches!(f.writer.analysis_data_path_for(&track), Err(DbError::WriteRefused(_))));
}

#[test]
fn setting_the_bpm_writes_the_column_a_grid_edit_keeps_in_step() {
    let mut f = fixture();
    let track = track_id(1);
    let changed = f.writer.set_bpm_x100(&track, 12_850).unwrap();
    assert_eq!(changed.rows, 1);
    let (bpm, usn): (i64, i64) = f
        .conn()
        .query_row("SELECT BPM, rb_local_usn FROM djmdContent WHERE ID = ?1", params![track], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((bpm, usn), (12_850, changed.usn));
    assert_eq!(f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"), changed.usn);

    // Zero is not a tempo a grid can have, and nothing moves.
    let refused = f.writer.set_bpm_x100(&track, 0).unwrap_err();
    assert!(matches!(refused, DbError::WriteRefused(_)), "{refused}");
    assert_eq!(f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"), changed.usn);
}

#[test]
fn registering_an_analysis_sets_bpm_key_path_and_analysed_in_one_usn() {
    let mut f = fixture();
    let track = track_id(1);
    // Two rows named alike, as the reference library has: the one on more
    // tracks is the one rekordbox uses.
    for (id, n) in [("900001", 3), ("900002", 1)] {
        f.conn()
            .execute(
                "INSERT INTO djmdKey (ID, ScaleName, created_at, updated_at) VALUES (?1, 'Dbm', '2020-01-01', '2020-01-01')",
                params![id],
            )
            .unwrap();
        for k in 0..n {
            f.conn()
                .execute("UPDATE djmdContent SET KeyID = ?1 WHERE ID = ?2", params![id, track_id(2 + k + if id == "900001" { 0 } else { 3 })])
                .unwrap();
        }
    }
    let usn_before: i64 = f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'");

    let changed = f
        .writer
        .register_analysis(
            &track,
            &AnalysisRegistration { bpm_x100: 13600, key: Some("Dbm"), analysis_data_path: "/PIONEER/USBANLZ/a1b/x/ANLZ0000.DAT" },
        )
        .unwrap();
    assert_eq!(changed.rows, 1);
    assert!(changed.usn > usn_before);

    let (bpm, key, path, analysed, usn): (i64, String, String, i64, i64) = f
        .conn()
        .query_row(
            "SELECT BPM, KeyID, AnalysisDataPath, Analysed, rb_local_usn FROM djmdContent WHERE ID = ?1",
            params![track],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!((bpm, key.as_str(), path.as_str(), analysed), (13600, "900001", "/PIONEER/USBANLZ/a1b/x/ANLZ0000.DAT", ANALYSED_FULL));
    assert_eq!(usn, changed.usn);
    assert_eq!(f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"), changed.usn);

    // No key: the column is left as it was.
    f.writer
        .register_analysis(&track, &AnalysisRegistration { bpm_x100: 13700, key: None, analysis_data_path: "/PIONEER/USBANLZ/a1b/x/ANLZ0000.DAT" })
        .unwrap();
    let key: String = f.one("SELECT KeyID FROM djmdContent WHERE ID = ?1", &[&track]);
    assert_eq!(key, "900001");

    // A name no row carries is refused, and nothing moves.
    let counter: i64 = f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'");
    let refused = f
        .writer
        .register_analysis(&track, &AnalysisRegistration { bpm_x100: 1, key: Some("H#m"), analysis_data_path: "/x" })
        .unwrap_err();
    assert!(matches!(refused, DbError::WriteRefused(_)), "{refused}");
    assert_eq!(f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"), counter);
}

#[test]
fn an_intelligent_playlist_takes_no_tracks_by_hand() {
    let mut f = fixture();
    let id = f.writer.create_playlist("Rule", ROOT).unwrap();
    f.conn()
        .execute(
            "UPDATE djmdPlaylist SET Attribute = 4,
                SmartList = '<NODE Id=\"1\" LogicalOperator=\"1\" AutomaticUpdate=\"1\"><CONDITION PropertyName=\"rating\" Operator=\"1\" ValueUnit=\"\" ValueLeft=\"5\" ValueRight=\"\"/></NODE>'
             WHERE ID = ?1",
            [&id],
        )
        .unwrap();

    for result in [
        f.writer.add_tracks(&id, &[track_id(0)]).map(|_| ()),
        f.writer.remove_tracks(&id, &[track_id(0)]).map(|_| ()),
        f.writer.reorder(&id, &[track_id(0)]).map(|_| ()),
    ] {
        let Err(DbError::WriteRefused(reason)) = result else {
            panic!("a membership edit on a rule should be refused");
        };
        assert!(reason.contains("rule"), "{reason}");
    }
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongPlaylist WHERE PlaylistID = (SELECT ID FROM djmdPlaylist WHERE Name = 'Rule')"), 0);

    // The rule itself survives a rename, and the playlist can still go.
    f.writer.rename(&id, "Rule renamed").unwrap();
    let kept: String = f.one("SELECT SmartList FROM djmdPlaylist WHERE ID = ?1", &[&id]);
    assert!(kept.contains("PropertyName"));
    f.writer.delete_playlist(&id).unwrap();
}

#[test]
fn an_analysis_is_registered_on_the_track_with_the_usual_bookkeeping() {
    use rbl_db::write::{AnalysisWrite, ANALYSED_BY_THIS_APP};
    let mut f = fixture();
    let stamp = rbl_core::time::now();
    f.conn()
        .execute(
            "INSERT INTO djmdKey (ID, ScaleName, Seq, created_at, updated_at) VALUES ('12', 'Fm', 7, ?1, ?1)",
            [&stamp],
        )
        .unwrap();
    // A track rekordbox never analysed, and one it did.
    f.conn().execute("UPDATE djmdContent SET Analysed = 0, KeyID = NULL WHERE ID = ?1", [track_id(0)]).unwrap();
    let before: i64 = f.one("SELECT rb_local_usn FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);

    let changed = f
        .writer
        .set_analysis(
            &track_id(0),
            &AnalysisWrite { bpm_x100: 12_850, key: Some("Fm"), analysis_path: "/PIONEER/USBANLZ/P001/0000ABCD/ANLZ0000.DAT", length_sec: Some(312) },
        )
        .unwrap();
    assert_eq!(changed.rows, 1);
    assert!(changed.usn > before);
    let (bpm, key, path, length, analysed, updated): (i64, String, String, i64, i64, String) = f
        .conn()
        .query_row(
            "SELECT BPM, KeyID, AnalysisDataPath, Length, Analysed, AnalysisUpdated FROM djmdContent WHERE ID = ?1",
            [track_id(0)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    assert_eq!((bpm, key.as_str(), path.as_str(), length, analysed), (12_850, "12", "/PIONEER/USBANLZ/P001/0000ABCD/ANLZ0000.DAT", 312, ANALYSED_BY_THIS_APP));
    assert_eq!(updated, "1", "AnalysisUpdated is a revision counter, not a timestamp");
    assert_eq!(f.one::<i64>("SELECT ContentLink FROM djmdContent WHERE ID = ?1", &[&track_id(0)]), 2_885_120);
    let counter: i64 = f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'");
    assert_eq!(counter, changed.usn);

    // rekordbox's own value on an analysed track is kept, and an unknown key
    // name leaves the key as it was rather than inventing a djmdKey row.
    f.conn().execute("UPDATE djmdContent SET KeyID = '12', ContentLink = 3999246, AnalysisUpdated = '7' WHERE ID = ?1", [track_id(1)]).unwrap();
    f.writer
        .set_analysis(&track_id(1), &AnalysisWrite { bpm_x100: 12_000, key: Some("H#m"), analysis_path: "/PIONEER/USBANLZ/P001/00000002/ANLZ0000.DAT", length_sec: None })
        .unwrap();
    let (analysed, key, length): (i64, String, i64) = f
        .conn()
        .query_row("SELECT Analysed, KeyID, Length FROM djmdContent WHERE ID = ?1", [track_id(1)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!((analysed, key.as_str(), length), (105, "12", 300));
    assert_eq!(f.one::<String>("SELECT AnalysisUpdated FROM djmdContent WHERE ID = ?1", &[&track_id(1)]), "8");
    assert_eq!(f.one::<i64>("SELECT ContentLink FROM djmdContent WHERE ID = ?1", &[&track_id(1)]), 3_999_246);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey"), 1);

    // Releases before this fix wrote 1, which rekordbox uses only when the
    // files are missing. Re-analysis promotes it without losing the lock bit.
    f.conn().execute("UPDATE djmdContent SET Analysed = 129 WHERE ID = ?1", [track_id(2)]).unwrap();
    f.writer
        .set_analysis(&track_id(2), &AnalysisWrite { bpm_x100: 12_900, key: None, analysis_path: "/PIONEER/USBANLZ/old/path/ANLZ0000.DAT", length_sec: None })
        .unwrap();
    assert_eq!(f.one::<i64>("SELECT Analysed FROM djmdContent WHERE ID = ?1", &[&track_id(2)]), ANALYSED_BY_THIS_APP | 128);
}

#[test]
fn clearing_the_analysis_puts_the_row_back_to_unanalysed_and_lets_it_be_registered_again() {
    use rbl_db::write::{AnalysisWrite, ANALYSED_BY_THIS_APP};
    let mut f = fixture();
    let stamp = rbl_core::time::now();
    f.conn()
        .execute(
            "INSERT INTO djmdKey (ID, ScaleName, Seq, created_at, updated_at) VALUES ('12', 'Fm', 7, ?1, ?1)",
            [&stamp],
        )
        .unwrap();
    // The fixture's track is analysed by rekordbox: Analysed 105, a key, a path.
    f.conn()
        .execute("UPDATE djmdContent SET Analysed = 105, KeyID = '12', BPM = 12850 WHERE ID = ?1", [track_id(0)])
        .unwrap();
    let before: i64 = f.one("SELECT rb_local_usn FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);

    let changed = f.writer.clear_analysis(&track_id(0)).unwrap();
    assert_eq!(changed.rows, 1);
    assert!(changed.usn > before);
    let (bpm, key, path, analysed, updated, length): (i64, Option<String>, String, i64, Option<String>, i64) = f
        .conn()
        .query_row(
            "SELECT BPM, KeyID, AnalysisDataPath, Analysed, AnalysisUpdated, Length FROM djmdContent WHERE ID = ?1",
            [track_id(0)],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    assert_eq!((bpm, key, path.as_str(), analysed, updated), (0, None, "", 0, None));
    // The file's length is not the analysis's, and stays.
    assert_eq!(length, 300);
    let counter: i64 = f.count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'");
    assert_eq!(counter, changed.usn);

    // And the point of clearing it: `set_analysis` now writes its own
    // `Analysed`, where on the 105 above it would have kept rekordbox's.
    f.writer
        .set_analysis(
            &track_id(0),
            &AnalysisWrite { bpm_x100: 14_000, key: Some("Fm"), analysis_path: "/PIONEER/USBANLZ/P001/0000ABCD/ANLZ0000.DAT", length_sec: None },
        )
        .unwrap();
    let (bpm, analysed): (i64, i64) = f
        .conn()
        .query_row("SELECT BPM, Analysed FROM djmdContent WHERE ID = ?1", [track_id(0)], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((bpm, analysed), (14_000, ANALYSED_BY_THIS_APP));
}

#[test]
fn a_backup_is_listed_and_can_be_put_back() {
    use rbl_db::write::{backups_in, restore_backup};
    let mut f = fixture();
    let dir = f._dir.path().to_path_buf();
    let backups = dir.join("backups");
    assert!(backups_in(&backups).is_empty());

    // Taken on request, before any write.
    let copy = f.writer.back_up_now().unwrap();
    assert_eq!(backups_in(&backups), vec![copy.clone()]);

    // A write after it, then the backup put back: the write is gone.
    let id = f.writer.create_playlist("After the backup", ROOT).unwrap();
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE Name = 'After the backup'"), 1);
    let location = f.writer.library().location().clone();
    // The writer's handle goes; the directory stays.
    let Fixture { _dir: keep, writer } = f;
    drop(writer);
    restore_backup(&location, &copy).unwrap();
    let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let after: i64 = db
        .connection()
        .query_row("SELECT COUNT(*) FROM djmdPlaylist WHERE ID = ?1", [&id], |r| r.get(0))
        .unwrap();
    assert_eq!(after, 0, "the playlist made after the backup is gone");

    // Only this app's backups are accepted.
    let stray = dir.join("notes.db");
    std::fs::write(&stray, b"x").unwrap();
    assert!(matches!(restore_backup(&location, &stray), Err(DbError::WriteRefused(_))));
    drop(keep);
}

#[test]
fn a_rekordbox_xml_document_is_imported_with_its_playlists_and_cues() {
    use rbl_db::xml::{self, XmlLibrary};
    let audio = tempfile::tempdir().unwrap();
    let one = audio.path().join("One.wav");
    let two = audio.path().join("Two Ü.wav");
    write_wav(&one, 2);
    write_wav(&two, 2);
    let location = |p: &std::path::Path| {
        format!("file://localhost{}", p.to_string_lossy().bytes().map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => char::from(b).to_string(),
            _ => format!("%{b:02X}"),
        }).collect::<String>())
    };
    let doc = format!(
        r#"<?xml version="1.0"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="3">
        <TRACK TrackID="1" Name="One" Artist="A" Rating="153" Comments="hi" Location="{}">
          <POSITION_MARK Name="" Type="0" Start="1.5" Num="-1"/>
          <POSITION_MARK Name="" Type="0" Start="0.25" Num="0"/>
          <POSITION_MARK Name="" Type="4" Start="0.5" End="1.0" Num="2"/>
        </TRACK>
        <TRACK TrackID="2" Name="Two" Artist="B" Rating="0" Location="{}"/>
        <TRACK TrackID="3" Name="Gone" Artist="C" Rating="0" Location="file://localhost/nowhere/gone.wav"/>
        </COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1">
        <NODE Name="Sets" Type="0" Count="1"><NODE Name="Warm up" Type="1" KeyType="0" Entries="3">
        <TRACK Key="1"/><TRACK Key="2"/><TRACK Key="3"/></NODE></NODE></NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
        location(&one),
        location(&two),
    );
    let parsed = XmlLibrary::parse(&doc);
    assert_eq!(parsed.tracks.len(), 3);

    let mut f = fixture();
    let mut seen = Vec::new();
    let report = xml::import(&mut f.writer, &parsed, &mut |done, total| seen.push((done, total))).unwrap();
    assert_eq!((report.imported, report.existing, report.skipped.len(), report.playlists, report.cues), (2, 0, 1, 2, 3));
    assert_eq!(seen, vec![(1, 3), (2, 3), (3, 3)]);
    assert!(report.skipped[0].contains("gone.wav"));

    let (rating, comment): (i64, String) = f
        .conn()
        .query_row("SELECT Rating, Commnt FROM djmdContent WHERE FolderPath = ?1", [one.to_string_lossy()], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((rating, comment.as_str()), (3, "hi"));
    let kinds: Vec<(i64, i64, i64)> = {
        let mut stmt = f.conn().prepare("SELECT Kind, InMsec, OutMsec FROM djmdCue WHERE ContentID = (SELECT ID FROM djmdContent WHERE FolderPath = ?1) AND rb_local_deleted = 0 ORDER BY InMsec").unwrap();
        stmt.query_map([one.to_string_lossy()], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(-1)))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(kinds, vec![(1, 250, -1), (3, 500, 1000), (0, 1500, -1)]);
    let folder: String = f.one("SELECT ID FROM djmdPlaylist WHERE Name = 'Sets' AND Attribute = 1", &[]);
    let playlist_parent: String = f.one("SELECT ParentID FROM djmdPlaylist WHERE Name = 'Warm up'", &[]);
    assert_eq!(playlist_parent, folder);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongPlaylist WHERE rb_local_deleted = 0 AND PlaylistID = (SELECT ID FROM djmdPlaylist WHERE Name = 'Warm up')"), 2);

    // Importing again reuses the tracks and doubles no cues.
    let again = xml::import(&mut f.writer, &parsed, &mut |_, _| {}).unwrap();
    assert_eq!((again.imported, again.existing, again.cues), (0, 2, 0));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0 AND FolderPath LIKE '%One.wav'"), 1);
}

/// Issue #152: importing the same rekordbox.xml twice must not double the
/// library. The second run reuses the tracks, the folders and the playlists
/// the first one made, and adds no membership row twice.
#[test]
fn importing_the_same_rekordbox_xml_twice_doubles_nothing() {
    use rbl_db::xml::{self, XmlLibrary};
    let audio = tempfile::tempdir().unwrap();
    let one = audio.path().join("One.wav");
    let two = audio.path().join("Two.wav");
    let three = audio.path().join("Three.wav");
    for path in [&one, &two, &three] {
        write_wav(path, 2);
    }
    let location = |p: &std::path::Path| format!("file://localhost{}", p.to_string_lossy().replace(' ', "%20"));
    // Two sibling playlists share a name, as rekordbox allows; each keeps its
    // own tracks on every import rather than the two folding together.
    let doc = |extra: &str| {
        format!(
            r#"<?xml version="1.0"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="3">
            <TRACK TrackID="1" Name="One" Location="{}"><POSITION_MARK Name="" Type="0" Start="0.5" Num="0"/></TRACK>
            <TRACK TrackID="2" Name="Two" Location="{}"/>
            <TRACK TrackID="3" Name="Three" Location="{}"/>
            </COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="3">
            <NODE Name="Sets" Type="0" Count="2">
              <NODE Name="Warm up" Type="1" KeyType="0" Entries="2"><TRACK Key="1"/><TRACK Key="2"/>{extra}</NODE>
              <NODE Name="Inner" Type="0" Count="1"><NODE Name="Deep" Type="1" KeyType="0" Entries="1"><TRACK Key="3"/></NODE></NODE>
            </NODE>
            <NODE Name="Same" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE>
            <NODE Name="Same" Type="1" KeyType="0" Entries="1"><TRACK Key="2"/></NODE>
            </NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
            location(&one),
            location(&two),
            location(&three),
        )
    };
    let parsed = XmlLibrary::parse(&doc(""));

    let mut f = fixture();
    let before_tracks = f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0");
    let before_nodes = f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0");
    let before_members = f.count("SELECT COUNT(*) FROM djmdSongPlaylist WHERE rb_local_deleted = 0");

    let first = xml::import(&mut f.writer, &parsed, &mut |_, _| {}).unwrap();
    assert_eq!((first.imported, first.existing, first.playlists, first.cues), (3, 0, 6, 1));
    let snapshot = |f: &Fixture| {
        (
            f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0"),
            f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0"),
            f.count("SELECT COUNT(*) FROM djmdSongPlaylist WHERE rb_local_deleted = 0"),
            f.count("SELECT COUNT(*) FROM djmdCue WHERE rb_local_deleted = 0"),
        )
    };
    let after_first = snapshot(&f);
    assert_eq!(
        (after_first.0 - before_tracks, after_first.1 - before_nodes, after_first.2 - before_members),
        (3, 6, 5),
    );

    let names = xml::same_named_lists(f.writer.library(), &parsed).unwrap();
    assert_eq!(names, vec!["Sets", "Warm up", "Inner", "Deep", "Same", "Same"]);
    let second = xml::import(&mut f.writer, &parsed, &mut |_, _| {}).unwrap();
    assert_eq!((second.imported, second.existing, second.playlists, second.cues), (0, 3, 0, 0));
    assert_eq!(second.playlists_replaced, 6);
    assert_eq!(second.playlist_tracks, 0);
    assert_eq!(snapshot(&f), after_first, "a second import of the same file adds no row");
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'Sets'"), 1);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'Warm up'"), 1);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'Same'"), 2);

    // The two same-named siblings each still hold only their own track.
    let same: Vec<String> = f
        .children(ROOT)
        .into_iter()
        .filter(|id| f.one::<String>("SELECT Name FROM djmdPlaylist WHERE ID = ?1", &[id]) == "Same")
        .collect();
    let members = |f: &Fixture, playlist: &str| -> Vec<String> {
        let mut stmt = f
            .conn()
            .prepare(
                "SELECT c.FolderPath FROM djmdSongPlaylist s JOIN djmdContent c ON c.ID = s.ContentID
                 WHERE s.PlaylistID = ?1 AND s.rb_local_deleted = 0 ORDER BY s.TrackNo",
            )
            .unwrap();
        stmt.query_map([playlist], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(members(&f, &same[0]), vec![one.to_string_lossy().into_owned()]);
    assert_eq!(members(&f, &same[1]), vec![two.to_string_lossy().into_owned()]);

    // A newer export of the same collection with a track added to a playlist
    // replaces that playlist with the document's three tracks, and nothing
    // else.
    let grown = XmlLibrary::parse(&doc(r#"<TRACK Key="3"/>"#));
    let third = xml::import(&mut f.writer, &grown, &mut |_, _| {}).unwrap();
    assert_eq!((third.imported, third.playlists, third.playlists_replaced, third.playlist_tracks), (0, 0, 6, 3));
    let warm_up: String = f.one("SELECT ID FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'Warm up'", &[]);
    assert_eq!(
        members(&f, &warm_up),
        vec![
            one.to_string_lossy().into_owned(),
            two.to_string_lossy().into_owned(),
            three.to_string_lossy().into_owned()
        ],
    );
    assert_eq!(f.track_numbers(&warm_up), vec![1, 2, 3]);
}

/// Issue #152: re-importing an xml whose playlist lost a track and had the
/// rest reordered replaces the playlist with the document's, as rekordbox's
/// "Do you want to replace them with the one you're importing?" does: the
/// removed track leaves, the order is the document's, and no list is doubled.
/// Lists the document does not name, and intelligent playlists, are left
/// alone.
#[test]
fn reimporting_an_xml_replaces_the_same_named_playlist_with_the_documents() {
    use rbl_db::xml::{self, XmlLibrary};
    let audio = tempfile::tempdir().unwrap();
    let paths: Vec<std::path::PathBuf> = ["One", "Two", "Three"].iter().map(|n| audio.path().join(format!("{n}.wav"))).collect();
    for path in &paths {
        write_wav(path, 2);
    }
    let location = |p: &std::path::Path| format!("file://localhost{}", p.to_string_lossy().replace(' ', "%20"));
    let doc = |keys: &[u8]| {
        let members: String = keys.iter().map(|k| format!(r#"<TRACK Key="{k}"/>"#)).collect();
        format!(
            r#"<?xml version="1.0"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="3">
            <TRACK TrackID="1" Name="One" Location="{}"/>
            <TRACK TrackID="2" Name="Two" Location="{}"/>
            <TRACK TrackID="3" Name="Three" Location="{}"/>
            </COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="2">
            <NODE Name="Sets" Type="0" Count="1">
              <NODE Name="Warm up" Type="1" KeyType="0" Entries="{}">{members}</NODE>
            </NODE>
            <NODE Name="Smart" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE>
            </NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
            location(&paths[0]),
            location(&paths[1]),
            location(&paths[2]),
            keys.len(),
        )
    };

    let mut f = fixture();
    // An intelligent playlist named like a document playlist is never taken
    // for it.
    let smart = f.writer.create_smart_playlist("Smart", ROOT, |_| "<NODE/>".to_owned()).unwrap();
    let first = xml::import(&mut f.writer, &XmlLibrary::parse(&doc(&[1, 2, 3])), &mut |_, _| {}).unwrap();
    assert_eq!((first.playlists, first.playlists_replaced), (3, 0));
    let sets: String = f.one("SELECT ID FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'Sets'", &[]);
    let warm_up: String = f.one("SELECT ID FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'Warm up'", &[]);
    // A playlist the DJ made by hand in the imported folder.
    let own = f.writer.create_playlist("Mine", &sets).unwrap();
    f.writer.add_tracks(&own, &[f.one::<String>("SELECT ContentID FROM djmdSongPlaylist WHERE PlaylistID = ?1 AND TrackNo = 1 AND rb_local_deleted = 0", &[&warm_up])]).unwrap();

    // The next export dropped "Two" and put "Three" first.
    let newer = XmlLibrary::parse(&doc(&[3, 1]));
    assert_eq!(xml::same_named_lists(f.writer.library(), &newer).unwrap(), vec!["Sets", "Warm up", "Smart"]);
    let nodes = f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0");
    let second = xml::import(&mut f.writer, &newer, &mut |_, _| {}).unwrap();
    // "Smart" is the plain playlist the first import made beside the
    // intelligent one, replaced now; the intelligent one is never matched.
    assert_eq!((second.imported, second.existing, second.playlists, second.playlists_replaced), (0, 3, 0, 3));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0"), nodes);

    let members = |f: &Fixture, playlist: &str| -> Vec<String> {
        let mut stmt = f
            .conn()
            .prepare(
                "SELECT c.Title FROM djmdSongPlaylist s JOIN djmdContent c ON c.ID = s.ContentID
                 WHERE s.PlaylistID = ?1 AND s.rb_local_deleted = 0 ORDER BY s.TrackNo",
            )
            .unwrap();
        stmt.query_map([playlist], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    let current: String = f.one("SELECT ID FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'Warm up'", &[]);
    assert_eq!(current, warm_up, "the playlist is replaced where it stands, not doubled");
    let titles = members(&f, &warm_up);
    let title_of = |p: &std::path::Path| f.one::<String>("SELECT Title FROM djmdContent WHERE FolderPath = ?1 AND rb_local_deleted = 0", &[&p.to_string_lossy().into_owned()]);
    assert_eq!(titles, vec![title_of(&paths[2]), title_of(&paths[0])]);
    assert_eq!(f.track_numbers(&warm_up), vec![1, 2]);
    assert_eq!(members(&f, &own).len(), 1, "a list the document does not name is left alone");
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Attribute = 4 AND Name = 'Smart'"), 1);
    assert!(f.writer.library().connection().query_row("SELECT 1 FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0", [&smart], |_| Ok(())).is_ok());

    // The same document once more writes nothing.
    let usn: i64 = f.one("SELECT MAX(rb_local_usn) FROM djmdSongPlaylist", &[]);
    let third = xml::import(&mut f.writer, &newer, &mut |_, _| {}).unwrap();
    assert_eq!((third.playlists, third.playlist_tracks), (0, 0));
    assert_eq!(f.one::<i64>("SELECT MAX(rb_local_usn) FROM djmdSongPlaylist", &[]), usn);
}

/// A file named in the document by a path that is not in its plain form
/// (`/./`, `a/../`) is the library's track at the plain path: reused for the
/// playlist, not skipped as "already in the library".
#[test]
fn an_xml_track_at_an_unclean_path_is_reused_for_its_playlist() {
    use rbl_db::xml::{self, XmlLibrary};
    let audio = tempfile::tempdir().unwrap();
    let one = audio.path().join("One.wav");
    write_wav(&one, 2);
    std::fs::create_dir(audio.path().join("sub")).unwrap();
    let unclean = format!("{}/sub/../One.wav", audio.path().to_string_lossy());
    let doc = format!(
        r#"<?xml version="1.0"?><DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="1">
        <TRACK TrackID="1" Name="One" Location="file://localhost{unclean}"/>
        </COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="1">
        <NODE Name="List" Type="1" KeyType="0" Entries="1"><TRACK Key="1"/></NODE>
        </NODE></PLAYLISTS></DJ_PLAYLISTS>"#,
    );
    let parsed = XmlLibrary::parse(&doc);

    let mut f = fixture();
    let id = f.writer.import_file(&one).unwrap();
    let report = xml::import(&mut f.writer, &parsed, &mut |_, _| {}).unwrap();
    assert_eq!((report.imported, report.existing, report.skipped.len()), (0, 1, 0), "{:?}", report.skipped);
    let list: String = f.one("SELECT ID FROM djmdPlaylist WHERE rb_local_deleted = 0 AND Name = 'List'", &[]);
    let member: String = f.one("SELECT ContentID FROM djmdSongPlaylist WHERE PlaylistID = ?1 AND rb_local_deleted = 0", &[&list]);
    assert_eq!(member, id);
}

#[test]
fn a_bpm_typed_over_retimes_the_grid_and_sets_the_column() {
    let mut f = fixture();
    let location = f.writer.library().location().clone();
    // A DAT with a 120 BPM grid of eight beats, and a stale PQT2 beside it.
    let beats: Vec<rbl_anlz::Beat> = (0..8)
        .map(|i| rbl_anlz::Beat { beat_number: (i % 4) + 1, tempo_x100: 12_000, time_ms: 250 + u32::from(i) * 500 })
        .collect();
    let mut builder = rbl_anlz::AnlzBuilder::new();
    builder.path("/m/x.mp3").beat_grid(&beats).raw(rbl_core::FourCc::new(b"PQT2"), vec![0; 32], vec![0; 16]);
    let relative = "/PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT";
    let dat = location.share_root.join(relative.trim_start_matches('/'));
    std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
    std::fs::write(&dat, builder.finish()).unwrap();
    fixture::set_analysis_path(&location, 0, relative).unwrap();

    f.writer.set_field(&track_id(0), TrackField::Bpm, "128.5").unwrap();
    let bpm: i64 = f.one("SELECT BPM FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert_eq!(bpm, 12_850);
    let next: String = f.one("SELECT AnalysisDataPath FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert_ne!(next, relative);
    assert_eq!(rbl_anlz::Anlz::read(&dat).unwrap().beat_grid().unwrap(), beats);
    let rewritten = rbl_anlz::Anlz::read(&rbl_anlz::resolve(&location.share_root, &next)).unwrap();
    let grid = rewritten.beat_grid().unwrap();
    assert_eq!(grid.len(), 8);
    assert_eq!(grid[0].time_ms, 250, "the first beat stays where it was");
    assert_eq!(grid[1].time_ms, 250 + 467, "60000 / 128.5 is 466.9 ms");
    assert_eq!(grid[7].time_ms, 250 + (7.0_f64 * 6_000_000.0 / 12_850.0).round() as u32);
    assert!(grid.iter().all(|b| b.tempo_x100 == 12_850));
    assert_eq!(grid.iter().map(|b| b.beat_number).collect::<Vec<_>>(), vec![1, 2, 3, 4, 1, 2, 3, 4]);
    assert!(!rewritten.has_extended_grid(), "the stale PQT2 is dropped");
    assert_eq!(rewritten.path().as_deref(), Some("/m/x.mp3"), "the rest of the file is kept");

    // Nonsense and the impossible are refused before anything is touched.
    assert!(matches!(f.writer.set_field(&track_id(0), TrackField::Bpm, "fast"), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.set_field(&track_id(0), TrackField::Bpm, "1200"), Err(DbError::WriteRefused(_))));
    // A track with no analysis only gets the column.
    f.writer.set_field(&track_id(1), TrackField::Bpm, "90").unwrap();
    let plain: i64 = f.one("SELECT BPM FROM djmdContent WHERE ID = ?1", &[&track_id(1)]);
    assert_eq!(plain, 9_000);
}

#[test]
fn artwork_is_filed_in_the_share_tree_and_cleared_to_empty() {
    let mut f = fixture();
    let location = f.writer.library().location().clone();
    let image = f._dir.path().join("cover.png");
    std::fs::write(&image, b"\x89PNG not really").unwrap();

    f.writer.set_artwork(&track_id(0), Some(&image)).unwrap();
    let path: String = f.one("SELECT ImagePath FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert!(path.starts_with("/PIONEER/Artwork/") && path.ends_with("/artwork.png"), "{path}");
    let parts: Vec<&str> = path.split('/').collect();
    assert_eq!(parts.len(), 6, "{path}");
    assert_eq!(parts[3], &parts[4][..3], "the bucket is the uuid's first three characters");
    let filed = location.share_root.join(path.trim_start_matches('/'));
    assert_eq!(std::fs::read(&filed).unwrap(), b"\x89PNG not really");

    assert!(matches!(f.writer.set_artwork(&track_id(0), Some(&f._dir.path().join("x.txt"))), Err(DbError::WriteRefused(_))));

    f.writer.set_artwork(&track_id(0), None).unwrap();
    let cleared: String = f.one("SELECT ImagePath FROM djmdContent WHERE ID = ?1", &[&track_id(0)]);
    assert_eq!(cleared, "");
    assert!(filed.is_file(), "the file stays where it is");
    // Not a picture, so no small sizes beside it.
    assert!(!filed.with_file_name("artwork_s.jpg").exists());

    // A real picture gets the two sizes a stick takes: 240 and 80 square,
    // cut from the middle of a picture that is not.
    let real = f._dir.path().join("wide.png");
    let wide = image::RgbImage::from_fn(300, 100, |x, _| image::Rgb([u8::try_from(x % 256).unwrap_or(0), 0, 0]));
    wide.save(&real).unwrap();
    f.writer.set_artwork(&track_id(1), Some(&real)).unwrap();
    let path: String = f.one("SELECT ImagePath FROM djmdContent WHERE ID = ?1", &[&track_id(1)]);
    let filed = location.share_root.join(path.trim_start_matches('/'));
    for (name, side) in [("artwork_m.jpg", 240), ("artwork_s.jpg", 80)] {
        let small = image::open(filed.with_file_name(name)).unwrap();
        assert_eq!((small.width(), small.height()), (side, side), "{name}");
    }

    // A playlist takes a picture the same way.
    f.writer.set_playlist_artwork(&playlist_id(1), Some(&real)).unwrap();
    let path: String = f.one("SELECT ImagePath FROM djmdPlaylist WHERE ID = ?1", &[&playlist_id(1)]);
    assert!(path.starts_with("/PIONEER/Artwork/") && path.ends_with("/artwork.png"), "{path}");
    f.writer.set_playlist_artwork(&playlist_id(1), None).unwrap();
    let cleared: String = f.one("SELECT ImagePath FROM djmdPlaylist WHERE ID = ?1", &[&playlist_id(1)]);
    assert_eq!(cleared, "");
}

#[test]
fn a_play_goes_on_todays_session_and_can_be_taken_off_again() {
    let mut f = fixture();
    let today = rbl_core::time::local_date();
    f.writer.record_play(&track_id(3)).unwrap();
    f.writer.record_play(&track_id(4)).unwrap();
    f.writer.record_play(&track_id(3)).unwrap();

    let session: String = f.one(
        "SELECT ID FROM djmdHistory WHERE Name = ?1 AND Attribute = 0 AND rb_local_deleted = 0",
        &[&format!("HISTORY {today}")],
    );
    let (month_id, created): (String, String) = f
        .conn()
        .query_row("SELECT ParentID, DateCreated FROM djmdHistory WHERE ID = ?1", [&session], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(created.starts_with(&today) && created.len() == 19, "{created}");
    let (month_name, year_id): (String, String) = f
        .conn()
        .query_row("SELECT Name, ParentID FROM djmdHistory WHERE ID = ?1 AND Attribute = 1", [&month_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    let year_name: String = f.one("SELECT Name FROM djmdHistory WHERE ID = ?1 AND Attribute = 1 AND ParentID = 'root'", &[&year_id]);
    assert_eq!(year_name, &today[..4]);
    assert_eq!(month_name, today[5..7].trim_start_matches('0'));
    // A year already there is reused: the fixture's 2026 folder, when today is in it.
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdHistory WHERE Attribute = 1 AND ParentID = 'root' AND rb_local_deleted = 0"), if today.starts_with("2026") { 1 } else { 2 });

    let plays: Vec<(String, i64)> = {
        let mut stmt = f.conn().prepare("SELECT ContentID, TrackNo FROM djmdSongHistory WHERE HistoryID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo").unwrap();
        stmt.query_map([&session], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(plays, vec![(track_id(3), 1), (track_id(4), 2), (track_id(3), 3)]);
    let count: i64 = f.one("SELECT DJPlayCount FROM djmdContent WHERE ID = ?1", &[&track_id(3)]);
    assert_eq!(count, 2);

    let changed = f.writer.remove_from_history(&session, &[track_id(3)]).unwrap();
    assert_eq!(changed.rows, 2);
    let left: Vec<(String, i64)> = {
        let mut stmt = f.conn().prepare("SELECT ContentID, TrackNo FROM djmdSongHistory WHERE HistoryID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo").unwrap();
        stmt.query_map([&session], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(left, vec![(track_id(4), 1)], "the gap closes");
    assert!(matches!(f.writer.record_play("no-such-track"), Err(DbError::WriteRefused(_))));
}

#[test]
fn each_link_session_gets_its_own_history_in_todays_month_folder() {
    let mut f = fixture();
    let today = rbl_core::time::local_date();
    let first = f.writer.new_link_history().unwrap();
    let second = f.writer.new_link_history().unwrap();
    let third = f.writer.new_link_history().unwrap();
    let name = |id: &str| -> String { f.one("SELECT Name FROM djmdHistory WHERE ID = ?1", &[&id]) };
    // rekordbox's own numbering: the bare name, then (1), then one past the highest.
    assert_eq!(name(&first), format!("LINK HISTORY {today}"));
    assert_eq!(name(&second), format!("LINK HISTORY {today} (1)"));
    assert_eq!(name(&third), format!("LINK HISTORY {today} (2)"));

    let (month_id, attribute): (String, i64) = f
        .conn()
        .query_row("SELECT ParentID, Attribute FROM djmdHistory WHERE ID = ?1", [&first], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(attribute, 0);
    let (month_name, year_id): (String, String) = f
        .conn()
        .query_row("SELECT Name, ParentID FROM djmdHistory WHERE ID = ?1 AND Attribute = 1", [&month_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(month_name, today[5..7].trim_start_matches('0'));
    let year_name: String = f.one("SELECT Name FROM djmdHistory WHERE ID = ?1 AND Attribute = 1 AND ParentID = 'root'", &[&year_id]);
    assert_eq!(year_name, &today[..4]);
    // A play on the local HISTORY session shares the month folder.
    f.writer.record_play(&track_id(1)).unwrap();
    let played_in: String = f.one("SELECT ParentID FROM djmdHistory WHERE Name = ?1 AND rb_local_deleted = 0", &[&format!("HISTORY {today}")]);
    assert_eq!(played_in, month_id);
    let highest: i64 = f.one("SELECT MAX(rb_local_usn) FROM djmdHistory", &[]);
    let counter: i64 = f.one("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]);
    assert!(counter >= highest, "counter {counter} behind the rows' {highest}");
}

#[test]
fn plays_added_to_a_link_history_count_and_can_be_taken_off() {
    let mut f = fixture();
    let session = f.writer.new_link_history().unwrap();
    let before: i64 = f.one("SELECT COALESCE(DJPlayCount, 0) FROM djmdContent WHERE ID = ?1", &[&track_id(2)]);
    f.writer.add_to_history(&session, &track_id(2)).unwrap();
    f.writer.add_to_history(&session, &track_id(5)).unwrap();
    f.writer.add_to_history(&session, &track_id(2)).unwrap();
    let plays = |f: &Fixture| -> Vec<(String, i64)> {
        let mut stmt = f.conn().prepare("SELECT ContentID, TrackNo FROM djmdSongHistory WHERE HistoryID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo").unwrap();
        stmt.query_map([&session], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(plays(&f), vec![(track_id(2), 1), (track_id(5), 2), (track_id(2), 3)]);
    let after: i64 = f.one("SELECT DJPlayCount FROM djmdContent WHERE ID = ?1", &[&track_id(2)]);
    assert_eq!(after, before + 2);

    f.writer.remove_from_history(&session, &[track_id(2)]).unwrap();
    assert_eq!(plays(&f), vec![(track_id(5), 1)]);
    assert!(matches!(f.writer.add_to_history(&session, "no-such-track"), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.add_to_history("no-such-history", &track_id(2)), Err(DbError::WriteRefused(_))));
}

#[test]
fn my_tags_are_set_as_a_whole_and_read_back_on_the_details() {
    use rbl_db::fixture::{MY_TAG_PEAK, MY_TAG_WARM_UP};
    let mut f = fixture();
    let id = track_id(2);
    f.writer.set_my_tags(&id, &[MY_TAG_PEAK.to_owned(), MY_TAG_WARM_UP.to_owned()]).unwrap();
    let read = rbl_db::details::track_details(f.conn(), &id).unwrap().unwrap();
    assert_eq!(read.my_tags, vec![MY_TAG_PEAK.to_owned(), MY_TAG_WARM_UP.to_owned()]);

    f.writer.set_my_tags(&track_id(3), &[MY_TAG_PEAK.to_owned()]).unwrap();
    let second: i64 = f.one(
        "SELECT TrackNo FROM djmdSongMyTag WHERE MyTagID = ?1 AND ContentID = ?2 AND rb_local_deleted = 0",
        &[&MY_TAG_PEAK, &track_id(3)],
    );
    assert_eq!(second, 2, "each tag's list numbers on");

    // Taking one off soft-deletes it and leaves the other row untouched.
    let changed = f.writer.set_my_tags(&id, &[MY_TAG_WARM_UP.to_owned()]).unwrap();
    assert_eq!(changed.rows, 1);
    let read = rbl_db::details::track_details(f.conn(), &id).unwrap().unwrap();
    assert_eq!(read.my_tags, vec![MY_TAG_WARM_UP.to_owned()]);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongMyTag WHERE rb_local_deleted = 1"), 1);

    assert!(matches!(f.writer.set_my_tags(&id, &["nope".to_owned()]), Err(DbError::WriteRefused(_))));
    assert!(matches!(f.writer.set_my_tags("no-track", &[]), Err(DbError::WriteRefused(_))));
}

#[test]
fn the_tag_list_takes_tracks_on_the_end_and_closes_up_when_one_comes_off() {
    let mut f = fixture();
    let changed = f.writer.tag_list_add(&[track_id(3), track_id(1), track_id(3)]).unwrap();
    assert_eq!(changed.rows, 2, "a track already on the list is not added again");
    let counter_before: i64 =
        f.one("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]);
    // Numbered on from 1, with no update counter of their own, as rekordbox
    // writes them.
    let order = |f: &mut Fixture| -> Vec<(String, i64)> {
        let conn = f.conn();
        let mut stmt = conn
            .prepare("SELECT ContentID, TrackNo FROM djmdSongTagList WHERE rb_local_deleted = 0 ORDER BY TrackNo")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    };
    assert_eq!(order(&mut f), vec![(track_id(3), 1), (track_id(1), 2)]);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongTagList WHERE usn IS NULL AND rb_local_usn IS NULL"), 2);

    f.writer.tag_list_add(&[track_id(2)]).unwrap();
    f.writer.tag_list_remove(&[track_id(3)]).unwrap();
    assert_eq!(order(&mut f), vec![(track_id(1), 1), (track_id(2), 2)], "the gap closes");
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongTagList WHERE rb_local_deleted = 1"), 1);
    let counter_after: i64 =
        f.one("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]);
    assert_eq!(counter_before, counter_after, "the tag list does not move the update counter");

    f.writer.tag_list_clear().unwrap();
    assert!(order(&mut f).is_empty());
    assert!(matches!(f.writer.tag_list_add(&["no-such-track".to_owned()]), Err(DbError::WriteRefused(_))));
}

#[test]
fn reload_tag_reads_the_file_again_over_the_row() {
    use lofty::config::WriteOptions;
    use lofty::prelude::{ItemKey, TagExt};
    use lofty::tag::{Tag, TagType};

    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("Tagged.wav");
    write_wav(&path, 1);
    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();
    let title: String = f.one("SELECT Title FROM djmdContent WHERE ID = ?1", &[&id]);
    assert_eq!(title, "Tagged", "no tag, so the file's name");

    // The file gains tags after the import; Reload Tag brings them in.
    let mut tag = Tag::new(TagType::RiffInfo);
    tag.insert_text(ItemKey::TrackTitle, "Retitled".to_owned());
    tag.insert_text(ItemKey::TrackArtist, "Someone".to_owned());
    tag.insert_text(ItemKey::Genre, "House".to_owned());
    tag.insert_text(ItemKey::Comment, "a note".to_owned());
    tag.insert_text(ItemKey::TrackNumber, "7".to_owned());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    let changed = f.writer.reload_tags(&id).unwrap();
    assert!(changed >= 4, "{changed}");
    let read = rbl_db::details::track_details(f.conn(), &id).unwrap().unwrap();
    assert_eq!((read.title.as_str(), read.artist.as_str(), read.genre.as_str()), ("Retitled", "Someone", "House"));
    assert_eq!((read.comment.as_str(), read.track_number), ("a note", 7));
    assert!(matches!(f.writer.reload_tags("no-such-track"), Err(DbError::WriteRefused(_))));
}

/// Forty silent MPEG-1 Layer III frames, tagged with an `ID3v2` carrying
/// `items`: rekordbox reads a key from an MP3's `ID3v2` but not from a WAV's
/// `id3 ` chunk.
fn tagged_mp3(path: &std::path::Path, items: &[(lofty::prelude::ItemKey, &str)]) {
    use lofty::config::WriteOptions;
    use lofty::prelude::TagExt;
    use lofty::tag::{Tag, TagType};

    let mut out = Vec::new();
    for _ in 0..40 {
        let start = out.len();
        out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        out.resize(start + 417, 0);
    }
    std::fs::write(path, out).unwrap();
    if items.is_empty() {
        return;
    }
    let mut tag = Tag::new(TagType::Id3v2);
    for (key, value) in items {
        tag.insert_text(key.clone(), (*value).to_owned());
    }
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

fn key_of(f: &Fixture, id: &str) -> Option<String> {
    f.one("SELECT k.ScaleName FROM djmdContent c LEFT JOIN djmdKey k ON k.ID = c.KeyID WHERE c.ID = ?1", &[&id])
}

#[test]
fn an_imported_files_key_tag_points_the_track_at_a_key_row() {
    use lofty::prelude::ItemKey;

    let audio = tempfile::tempdir().unwrap();
    let mut f = fixture();
    let keys_before = f.count("SELECT COUNT(*) FROM djmdKey");

    // A key the library has not seen gets its row, as rekordbox's "2A" did.
    let first = audio.path().join("first.mp3");
    tagged_mp3(&first, &[(ItemKey::TrackTitle, "Keyed"), (ItemKey::InitialKey, "2A")]);
    let a = f.writer.import_file(&first).unwrap();
    assert_eq!(key_of(&f, &a).as_deref(), Some("2A"));

    // A second file with that key shares the row rather than adding one.
    let second = audio.path().join("second.mp3");
    tagged_mp3(&second, &[(ItemKey::TrackTitle, "Keyed"), (ItemKey::InitialKey, "2A")]);
    let b = f.writer.import_file(&second).unwrap();
    assert_eq!(key_of(&f, &b).as_deref(), Some("2A"));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey WHERE ScaleName = '2A'"), 1);

    // No key tag, no key: nothing is invented, and no row is made.
    let plain = audio.path().join("plain.mp3");
    tagged_mp3(&plain, &[]);
    let c = f.writer.import_file(&plain).unwrap();
    assert_eq!(key_of(&f, &c), None);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey"), keys_before + 1);

    // Reload Tag takes a key the file gained after the import...
    tagged_mp3(&plain, &[(ItemKey::TrackTitle, "Plain"), (ItemKey::InitialKey, "Fm")]);
    f.writer.reload_tags(&c).unwrap();
    assert_eq!(key_of(&f, &c).as_deref(), Some("Fm"));
    // ...and keeps the key it has when the file names none:
    // convertTagData copies the tag's key only when it is not empty.
    tagged_mp3(&plain, &[(ItemKey::TrackTitle, "Plain")]);
    f.writer.reload_tags(&c).unwrap();
    assert_eq!(key_of(&f, &c).as_deref(), Some("Fm"));
}

#[test]
fn a_wavs_id3_chunk_gives_an_imported_track_no_key() {
    use lofty::config::WriteOptions;
    use lofty::prelude::{ItemKey, TagExt};
    use lofty::tag::{Tag, TagType};

    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("chunk.wav");
    write_wav(&path, 1);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::InitialKey, "2A".to_owned());
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();
    assert_eq!(key_of(&f, &id), None);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey WHERE ScaleName = '2A'"), 0);
}

#[test]
fn a_bpm_tag_is_not_read_on_import_or_reload() {
    use lofty::prelude::ItemKey;

    // rekordbox's convertTagData never reads the tag's BPM: it comes from
    // analysis, so an unanalysed import has none.
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("bpm.mp3");
    tagged_mp3(&path, &[(ItemKey::TrackTitle, "Tempo"), (ItemKey::Bpm, "128")]);
    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();
    let bpm = |f: &Fixture| f.one::<Option<i64>>("SELECT BPM FROM djmdContent WHERE ID = ?1", &[&id]).unwrap_or(0);
    assert_eq!(bpm(&f), 0);
    f.writer.reload_tags(&id).unwrap();
    assert_eq!(bpm(&f), 0);
}

#[test]
fn importing_a_file_already_in_the_library_leaves_its_row_alone() {
    use lofty::prelude::ItemKey;

    // rekordbox's addTrack stops at isCollectionSong before reading any tag.
    let audio = tempfile::tempdir().unwrap();
    let path = audio.path().join("again.mp3");
    tagged_mp3(&path, &[(ItemKey::TrackTitle, "First"), (ItemKey::InitialKey, "2A")]);
    let mut f = fixture();
    let id = f.writer.import_file(&path).unwrap();

    tagged_mp3(&path, &[(ItemKey::TrackTitle, "Second"), (ItemKey::InitialKey, "9B")]);
    assert!(matches!(f.writer.import_file(&path), Err(DbError::WriteRefused(_))));
    assert_eq!(f.one::<String>("SELECT Title FROM djmdContent WHERE ID = ?1", &[&id]), "First");
    assert_eq!(key_of(&f, &id).as_deref(), Some("2A"));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdKey WHERE ScaleName = '9B'"), 0);
}

#[test]
fn embedded_cover_is_registered_with_thumbnails_and_never_replaces_custom_art() {
    use lofty::config::WriteOptions;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::prelude::TagExt;
    use lofty::tag::{Tag, TagType};

    let mut f = fixture();
    let path = f._dir.path().join("cover.wav");
    write_wav(&path, 1);
    let id = f.writer.import_file(&path).unwrap();
    assert!(!f.writer.import_artwork(&id).unwrap(), "untagged audio has no cover");
    let png = f._dir.path().join("front.png");
    image::RgbImage::from_pixel(12, 8, image::Rgb([10, 100, 200])).save(&png).unwrap();
    let bytes = std::fs::read(&png).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(MimeType::Png), None, bytes.clone()));
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    assert!(f.writer.import_artwork(&id).unwrap());
    let relative: String = f.one("SELECT ImagePath FROM djmdContent WHERE ID = ?1", &[&id]);
    let filed = rbl_anlz::resolve(&f.writer.library().location().share_root, &relative);
    assert_eq!(std::fs::read(&filed).unwrap(), bytes);
    for (name, size) in [("artwork_s.jpg", 80), ("artwork_m.jpg", 240)] {
        let thumb = image::open(filed.with_file_name(name)).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (size, size));
    }
    assert!(!f.writer.import_artwork(&id).unwrap(), "repeat analysis keeps the existing cover");
    f.writer.set_artwork(&id, Some(&png)).unwrap();
    let custom: String = f.one("SELECT ImagePath FROM djmdContent WHERE ID = ?1", &[&id]);
    assert!(!f.writer.import_artwork(&id).unwrap());
    assert_eq!(f.one::<String>("SELECT ImagePath FROM djmdContent WHERE ID = ?1", &[&id]), custom);
}

#[test]
fn usb_cues_replace_old_cues_and_preserve_loop_details() {
    let mut f = fixture();
    let track = track_id(0).to_string();
    f.writer.add_cue(&track, 1, 100).unwrap();
    let cue = rbl_anlz::CueEntry { hot_cue: 4, kind: 2, time_ms: 1000, loop_time_ms: 5000, color_id: 0, comment: Some("USB loop".into()), color_code: Some(21), rgb: None };
    f.writer.import_usb_cues(&track, &[cue], 12800).unwrap();
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdCue WHERE rb_local_deleted=0"), 1);
    let values: (i64, i64, String) = f.conn().query_row("SELECT Kind, OutMsec, Comment FROM djmdCue WHERE rb_local_deleted=0", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(values, (5, 5000, "USB loop".into()));
    assert_eq!(f.one::<i64>("SELECT BPM FROM djmdContent WHERE ID=?1", &[&track]), 12800);
}

#[test]
fn usb_history_is_repeat_safe_and_appends_new_plays() {
    let mut f = fixture();
    let track = track_id(0).to_string();
    let uuid = "00000000-0000-4000-8000-000000000001";
    assert_eq!(f.writer.import_usb_history("USB session", uuid, &[track.clone(), track.clone()]).unwrap(), 2);
    assert_eq!(f.writer.import_usb_history("USB session", uuid, &[track.clone(), track.clone()]).unwrap(), 0);
    assert_eq!(f.writer.import_usb_history("USB session", uuid, &[track.clone(), track.clone(), track]).unwrap(), 1);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdSongHistory WHERE rb_local_deleted=0 AND HistoryID IN (SELECT ID FROM djmdHistory WHERE UUID='00000000-0000-4000-8000-000000000001')"), 3);
}

#[test]
fn relocation_rolls_back_both_columns_when_the_counter_write_fails() {
    let mut f = fixture();
    let before: (String, String) = f.conn().query_row("SELECT FolderPath, FileNameL FROM djmdContent WHERE ID=?1", [track_id(0)], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let path = f._dir.path().join("moved.wav");
    std::fs::write(&path, b"audio").unwrap();
    f.conn().execute_batch("CREATE TRIGGER fail_counter BEFORE UPDATE ON agentRegistry BEGIN SELECT RAISE(ABORT, 'injected failure'); END").unwrap();
    assert!(f.writer.relocate(&track_id(0), &path).is_err());
    let after: (String, String) = f.conn().query_row("SELECT FolderPath, FileNameL FROM djmdContent WHERE ID=?1", [track_id(0)], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(before, after);
}

#[test]
fn reload_tags_rolls_back_earlier_fields_and_new_lookup_rows() {
    use lofty::{config::WriteOptions, prelude::{ItemKey, TagExt}, tag::{Tag, TagType}};
    let mut f = fixture();
    let path = f._dir.path().join("tags.wav");
    write_wav(&path, 1);
    let id = f.writer.import_file(&path).unwrap();
    let counter: i64 = f.one("SELECT int_1 FROM agentRegistry WHERE registry_id='localUpdateCount'", &[]);
    let mut tags = Tag::new(TagType::RiffInfo);
    tags.insert_text(ItemKey::TrackTitle, "Changed".into());
    tags.insert_text(ItemKey::TrackArtist, "New rollback artist".into());
    tags.save_to_path(&path, WriteOptions::default()).unwrap();
    f.conn().execute_batch("CREATE TRIGGER fail_artist BEFORE UPDATE OF ArtistID ON djmdContent BEGIN SELECT RAISE(ABORT, 'injected failure'); END").unwrap();
    assert!(f.writer.reload_tags(&id).is_err());
    assert_eq!(f.one::<String>("SELECT Title FROM djmdContent WHERE ID=?1", &[&id]), "tags");
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdArtist WHERE Name='New rollback artist'"), 0);
    assert_eq!(f.one::<i64>("SELECT int_1 FROM agentRegistry WHERE registry_id='localUpdateCount'", &[]), counter);
}

#[test]
fn bpm_failure_keeps_the_old_grid_and_database_reference() {
    let mut f = fixture();
    let location = f.writer.library().location().clone();
    let relative = "/PIONEER/USBANLZ/test/ANLZ0000.DAT";
    let dat = rbl_anlz::resolve(&location.share_root, relative);
    std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
    let mut builder = rbl_anlz::AnlzBuilder::new();
    builder.beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12000, time_ms: 0 }]);
    let before = builder.finish();
    std::fs::write(&dat, &before).unwrap();
    fixture::set_analysis_path(&location, 0, relative).unwrap();
    let bpm: i64 = f.one("SELECT BPM FROM djmdContent WHERE ID=?1", &[&track_id(0)]);
    f.conn().execute_batch("CREATE TRIGGER fail_bpm BEFORE UPDATE OF BPM ON djmdContent BEGIN SELECT RAISE(ABORT, 'injected failure'); END").unwrap();
    assert!(f.writer.set_bpm(&track_id(0), "130").is_err());
    assert_eq!(std::fs::read(dat).unwrap(), before);
    assert_eq!(f.one::<String>("SELECT AnalysisDataPath FROM djmdContent WHERE ID=?1", &[&track_id(0)]), relative);
    assert_eq!(f.one::<i64>("SELECT BPM FROM djmdContent WHERE ID=?1", &[&track_id(0)]), bpm);
}

#[test]
fn writable_connections_require_durable_commits() {
    let f = fixture();
    assert_eq!(f.one::<i64>("PRAGMA synchronous", &[]), 3);
    assert_eq!(f.one::<i64>("PRAGMA fullfsync", &[]), 1);
    assert_eq!(f.one::<i64>("PRAGMA read_uncommitted", &[]), 0);
}

// Spawned by abrupt_process_exit_recovers_without_partial_rows. exit() skips
// Rust Drop, including Transaction's rollback and Connection's clean close.
#[test]
fn crash_writer_child() {
    let Some(root) = std::env::var_os("RBL_CRASH_TEST_ROOT") else { return; };
    let root = std::path::PathBuf::from(root);
    let location = rbl_db::LibraryLocation { master_db: root.join("master.db"), share_root: root.join("share"), passphrase: fixture::FIXTURE_PASSPHRASE.into(), is_real_install: false };
    let mut db = Library::open(location, OpenMode::ReadWrite).unwrap();
    db.connection().pragma_update(None, "cache_size", 1).unwrap();
    let tx = db.connection_mut().transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
    tx.execute("UPDATE djmdContent SET Rating=5, rb_local_usn=99999 WHERE ID=?1", [track_id(0)]).unwrap();
    tx.execute("UPDATE djmdContent SET Commnt=printf('%16384s', 'crash test')", []).unwrap();
    if std::env::var_os("RBL_CRASH_TEST_COMMIT").is_some() {
        tx.execute("UPDATE agentRegistry SET int_1=99999 WHERE registry_id='localUpdateCount'", []).unwrap();
        tx.commit().unwrap();
    }
    std::process::exit(91);
}

#[test]
fn abrupt_process_exit_recovers_without_partial_rows() {
    for mode in ["DELETE", "WAL"] {
        for commit in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let location = fixture::build(dir.path(), Shape::default()).unwrap();
            let db = Library::open(location.clone(), OpenMode::ReadWrite).unwrap();
            db.connection().pragma_update(None, "journal_mode", mode).unwrap();
            let before: i64 = db.connection().query_row("SELECT Rating FROM djmdContent WHERE ID=?1", [track_id(0)], |r| r.get(0)).unwrap();
            drop(db);
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "crash_writer_child", "--nocapture"]).env("RBL_CRASH_TEST_ROOT", dir.path());
            child.env_remove("RBL_CRASH_TEST_COMMIT");
            if commit { child.env("RBL_CRASH_TEST_COMMIT", "1"); }
            assert_eq!(child.output().unwrap().status.code(), Some(91));
            let db = Library::open(location, OpenMode::ReadOnly).unwrap();
            let rating: i64 = db.connection().query_row("SELECT Rating FROM djmdContent WHERE ID=?1", [track_id(0)], |r| r.get(0)).unwrap();
            let counter: i64 = db.connection().query_row("SELECT int_1 FROM agentRegistry WHERE registry_id='localUpdateCount'", [], |r| r.get(0)).unwrap();
            let integrity: String = db.connection().query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
            assert_eq!(rating, if commit { 5 } else { before });
            assert_eq!(counter, if commit { 99999 } else { Shape::default().start_usn });
            assert_eq!(integrity, "ok");
        }
    }
}

#[test]
fn backup_is_standalone_and_includes_committed_wal_pages() {
    let mut f = fixture();
    f.conn().execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
    f.conn().execute("UPDATE djmdContent SET Rating=5 WHERE ID=?1", [track_id(0)]).unwrap();
    let backup = f.writer.back_up_now().unwrap();
    assert!(!std::path::PathBuf::from(format!("{}-wal", backup.display())).exists());
    let mut location = f.writer.library().location().clone();
    location.master_db = backup;
    let db = Library::open(location, OpenMode::ReadOnly).unwrap();
    let rating: i64 = db.connection().query_row("SELECT Rating FROM djmdContent WHERE ID=?1", [track_id(0)], |r| r.get(0)).unwrap();
    assert_eq!(rating, 5);
    assert_eq!(db.connection().query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
}

#[test]
fn invalid_backup_preserves_live_database_and_wal() {
    let f = fixture();
    f.conn().execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
    f.conn().execute("UPDATE djmdContent SET Rating=4 WHERE ID=?1", [track_id(0)]).unwrap();
    let backup = f._dir.path().join("master-corrupt.db");
    std::fs::write(&backup, b"not a database").unwrap();
    let location = f.writer.library().location();
    let wal = std::path::PathBuf::from(format!("{}-wal", location.master_db.display()));
    let before = std::fs::read(&wal).unwrap();
    assert!(rbl_db::write::restore_backup(location, &backup).is_err());
    assert_eq!(std::fs::read(wal).unwrap(), before);
    assert_eq!(f.one::<i64>("SELECT Rating FROM djmdContent WHERE ID=?1", &[&track_id(0)]), 4);
}

#[test]
fn grid_lock_preserves_other_flags_and_grid_revision_uses_reference_character() {
    let mut f = fixture();
    let id = track_id(1);
    f.conn().execute("UPDATE djmdContent SET Analysed=53, AnalysisUpdated='9' WHERE ID=?1", [&id]).unwrap();
    f.writer.set_analysis_lock(&id,true).unwrap();
    assert_eq!(f.one::<i64>("SELECT Analysed FROM djmdContent WHERE ID=?1", &[&id]),181);
    f.writer.set_analysis_lock(&id,false).unwrap();
    assert_eq!(f.one::<i64>("SELECT Analysed FROM djmdContent WHERE ID=?1", &[&id]),53);
    f.writer.save_grid_revision(&id,12800).unwrap();
    assert_eq!(f.one::<String>("SELECT AnalysisUpdated FROM djmdContent WHERE ID=?1", &[&id]),":");
    assert_eq!(f.one::<i64>("SELECT BPM FROM djmdContent WHERE ID=?1", &[&id]),12800);
}

#[test]
fn reload_tag_reads_a_cloud_tracks_local_copy() {
    use lofty::config::WriteOptions;
    use lofty::prelude::{ItemKey, TagExt};
    use lofty::tag::{Tag, TagType};

    let audio = tempfile::tempdir().unwrap();
    let local = audio.path().join("Local.wav");
    write_wav(&local, 1);
    let mut tag = Tag::new(TagType::RiffInfo);
    tag.insert_text(ItemKey::TrackTitle, "From The Local Copy".to_owned());
    tag.save_to_path(&local, WriteOptions::default()).unwrap();

    let mut f = fixture();
    let id = rbl_db::fixture::track_id(0);
    // Uploaded from this machine through Cloud Library Sync: FolderPath is
    // a cloud path no file answers to, OrgFolderPath is the real file.
    f.conn()
        .execute(
            "UPDATE djmdContent SET FolderPath = '/contents_1739239895/a/missing.wav', OrgFolderPath = ?1, \
             ContentLink = ?2, ServiceID = ?3, DeviceID = 'own' WHERE ID = ?4",
            rusqlite::params![local.to_string_lossy(), rbl_db::track_path::CLOUD_SHARED, rbl_db::track_path::SERVICE_GOOGLE_DRIVE, id],
        )
        .unwrap();
    f.conn().execute("UPDATE djmdProperty SET DeviceID = 'own'", []).unwrap();
    f.writer.library().set_dropbox_folder(None);

    let changed = f.writer.reload_tags(&id).unwrap();
    assert!(changed >= 1, "{changed}");
    let title: String = f.one("SELECT Title FROM djmdContent WHERE ID = ?1", &[&id]);
    assert_eq!(title, "From The Local Copy");
}

/// The member paths of a playlist, as file names, in `TrackNo` order.
fn member_files(f: &Fixture, playlist: &str) -> Vec<String> {
    let mut stmt = f
        .conn()
        .prepare(
            "SELECT c.FolderPath FROM djmdSongPlaylist s JOIN djmdContent c ON c.ID = s.ContentID
             WHERE s.PlaylistID = ?1 AND s.rb_local_deleted = 0 ORDER BY s.TrackNo",
        )
        .unwrap();
    stmt.query_map(params![playlist], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(Result::ok)
        .map(|p| std::path::Path::new(&p).file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

/// A folder dropped onto the Playlists root, the way rekordbox 7.2.19 handles
/// one (`TreeViewer::treeMessageImportExternalFoldersToList`) [OBS, static]:
/// one playlist named after the folder, at the end of the target, its
/// subfolders flattened into it in walk order, new files imported and files
/// already in the library reused.
#[test]
fn a_dropped_folder_becomes_one_flat_playlist_named_after_it() {
    let audio = tempfile::tempdir().unwrap();
    let folder = audio.path().join("Friday Set");
    std::fs::create_dir_all(folder.join("B Side/Deeper")).unwrap();
    for name in ["b.wav", "A.wav", "B Side/2.wav", "B Side/1.wav", "B Side/Deeper/x.wav"] {
        write_wav(&folder.join(name), 1);
    }
    std::fs::write(folder.join("cover.jpg"), b"x").unwrap();

    let mut f = fixture();
    // One of them is in the library already.
    let known = f.writer.import_file(&folder.join("b.wav")).unwrap();
    let before = f.children(ROOT);

    let mut budget = rbl_db::import::WalkBudget::new(16, 1000);
    let files = rbl_db::import::audio_files_in(&folder, &mut budget);
    let outcome = f.writer.import_folder_as_playlist("Friday Set", ROOT, &files, None, None).unwrap();

    let playlist = outcome.playlist.clone().expect("a playlist is made");
    assert_eq!(outcome.conflict, None);
    assert_eq!(outcome.existing, vec![known]);
    assert_eq!(outcome.imported.len(), 4);
    assert!(outcome.skipped.is_empty());

    let after = f.children(ROOT);
    assert_eq!(after[..before.len()], before[..], "existing order kept");
    assert_eq!(after.last(), Some(&playlist), "appended at the end of the target");
    assert_eq!(after.len(), before.len() + 1, "subfolders are not made into playlists");
    assert_eq!(
        f.one::<String>("SELECT Name FROM djmdPlaylist WHERE ID = ?1", &[&playlist]),
        "Friday Set"
    );
    assert_eq!(
        f.one::<i64>("SELECT Attribute FROM djmdPlaylist WHERE ID = ?1", &[&playlist]),
        ATTRIBUTE_PLAYLIST
    );
    assert_eq!(member_files(&f, &playlist), ["A.wav", "1.wav", "2.wav", "x.wav", "b.wav"]);
    assert_eq!(f.track_numbers(&playlist), [1, 2, 3, 4, 5]);
}

#[test]
fn a_folder_dropped_on_a_playlist_folder_lands_inside_it() {
    let audio = tempfile::tempdir().unwrap();
    write_wav(&audio.path().join("t.wav"), 1);
    let mut f = fixture();
    let crate_folder = f.writer.create_folder("Crates", ROOT).unwrap();
    let files = vec![audio.path().join("t.wav")];
    let outcome = f.writer.import_folder_as_playlist("Techno", &crate_folder, &files, None, None).unwrap();
    let playlist = outcome.playlist.unwrap();
    assert_eq!(f.children(&crate_folder), [playlist]);
}

#[test]
fn a_folder_without_audio_makes_nothing() {
    let mut f = fixture();
    let before = f.children(ROOT);
    let outcome = f.writer.import_folder_as_playlist("Artwork", ROOT, &[], None, None).unwrap();
    assert_eq!(outcome, rbl_db::write::FolderPlaylist::default());
    assert_eq!(f.children(ROOT), before);
}

/// rekordbox asks before replacing a same-named list
/// (`TreeViewer::showReplaceListAlert`) [OBS, static]; nothing is written
/// until the caller comes back with the answer.
#[test]
fn a_name_clash_writes_nothing_until_the_replacement_is_agreed() {
    let audio = tempfile::tempdir().unwrap();
    write_wav(&audio.path().join("t.wav"), 1);
    let files = vec![audio.path().join("t.wav")];
    let mut f = fixture();
    let clash = playlist_id(0);
    let before = f.children(ROOT);
    let tracks_before = f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0");

    let asked = f.writer.import_folder_as_playlist("Playlist 0", ROOT, &files, None, None).unwrap();
    assert_eq!(asked.conflict, Some(clash.clone()));
    assert_eq!(asked.playlist, None);
    assert_eq!(f.children(ROOT), before);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0"), tracks_before);

    let replaced = f.writer.import_folder_as_playlist("Playlist 0", ROOT, &files, Some(&clash), None).unwrap();
    let playlist = replaced.playlist.unwrap();
    assert_eq!(replaced.conflict, None);
    assert_eq!(
        f.one::<i64>("SELECT rb_local_deleted FROM djmdPlaylist WHERE ID = ?1", &[&clash]),
        1,
        "the old list is gone"
    );
    assert_eq!(f.writer.child_named(ROOT, "Playlist 0").unwrap(), Some(playlist.clone()));
    assert_eq!(member_files(&f, &playlist), ["t.wav"]);
}

/// Several folders in one drop all go to the drop's one insert index, so a
/// later folder lands before an earlier one, and replacing a clash that sat
/// before that index lowers it for the rest of the drop. rekordbox 7.2.19
/// [OBS, static]: `treeMessageImportExternalFoldersToList` (0x1015677ec) reads
/// the index once; `rekordboxDBController::createNewList` (0x1017e6808)
/// appends each list and `movePlaylist`s it to that index;
/// `checkSameNameList` (0x10155d5a8) takes one off it after deleting a list
/// that was before it (0x10155d748..0x10155d75c).
#[test]
fn folders_dropped_together_share_the_drop_index_like_rekordbox() {
    let audio = tempfile::tempdir().unwrap();
    write_wav(&audio.path().join("t.wav"), 1);
    let files = vec![audio.path().join("t.wav")];
    let mut f = fixture();
    let crate_folder = f.writer.create_folder("Crates", ROOT).unwrap();
    let old_1 = f.writer.create_playlist("Old 1", &crate_folder).unwrap();
    let old_2 = f.writer.create_playlist("Old 2", &crate_folder).unwrap();
    let name = |f: &Fixture, id: &String| f.one::<String>("SELECT Name FROM djmdPlaylist WHERE ID = ?1", &[id]);
    let names = |f: &Fixture| f.children(&crate_folder).iter().map(|id| name(f, id)).collect::<Vec<_>>();

    // A drop of `A` then `B` onto the middle of the `Crates` row.
    let a = f.writer.import_folder_as_playlist("A", &crate_folder, &files, None, None).unwrap();
    assert_eq!(a.at, Some(2), "the end of the target when the drop was made");
    let b = f.writer.import_folder_as_playlist("B", &crate_folder, &files, None, a.at).unwrap();
    assert_eq!(b.at, Some(2));
    assert_eq!(names(&f), ["Old 1", "Old 2", "B", "A"]);

    // A drop of `Old 1` (replaced) then `C`: the index starts at 4, drops to 3
    // when `Old 1` at 0 is deleted, and `C` goes in before the new `Old 1`.
    let asked = f.writer.import_folder_as_playlist("Old 1", &crate_folder, &files, None, None).unwrap();
    assert_eq!((asked.conflict.as_ref(), asked.at), (Some(&old_1), Some(4)));
    let replaced =
        f.writer.import_folder_as_playlist("Old 1", &crate_folder, &files, Some(&old_1), asked.at).unwrap();
    assert_eq!(replaced.at, Some(3));
    let c = f.writer.import_folder_as_playlist("C", &crate_folder, &files, None, replaced.at).unwrap();
    assert_eq!(c.at, Some(3));
    assert_eq!(names(&f), ["Old 2", "B", "A", "C", "Old 1"]);
    assert_eq!(f.children(&crate_folder)[0], old_2);
}

// ------------------------------------------------------------ batch writes

/// What a track row says about its file and its artist, album, genre and key,
/// by name rather than by the random ids two libraries would not share.
fn described(f: &Fixture) -> Vec<Vec<Option<String>>> {
    let mut stmt = f
        .conn()
        .prepare(
            "SELECT c.FolderPath, c.FileNameL, c.Title, CAST(c.Length AS TEXT), CAST(c.BitRate AS TEXT),
                    CAST(c.FileType AS TEXT), a.Name, al.Name, g.Name, k.ScaleName
             FROM djmdContent c
             LEFT JOIN djmdArtist a ON a.ID = c.ArtistID
             LEFT JOIN djmdAlbum al ON al.ID = c.AlbumID
             LEFT JOIN djmdGenre g ON g.ID = c.GenreID
             LEFT JOIN djmdKey k ON k.ID = c.KeyID
             WHERE c.rb_local_deleted = 0 AND c.FolderPath LIKE '%/imports/%'
             ORDER BY c.FileNameL",
        )
        .unwrap();
    stmt.query_map([], |r| (0..10).map(|i| r.get::<_, Option<String>>(i)).collect())
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_batch_import_leaves_the_rows_that_importing_one_at_a_time_does() {
    use lofty::prelude::ItemKey;
    use rbl_db::write::ImportOutcome;

    let root = tempfile::tempdir().unwrap();
    let audio = root.path().join("imports");
    std::fs::create_dir(&audio).unwrap();
    let tags = |artist: &'static str, key: &'static str| {
        vec![
            (ItemKey::TrackTitle, "Title"), (ItemKey::TrackArtist, artist),
            (ItemKey::AlbumTitle, "Album"), (ItemKey::Genre, "Techno"), (ItemKey::InitialKey, key),
        ]
    };
    let shared_a = audio.join("a1.mp3");
    let shared_b = audio.join("a2.mp3");
    let shared_c = audio.join("a3.mp3");
    let other = audio.join("b.mp3");
    let plain = audio.join("plain.mp3");
    let wav = audio.join("c.wav");
    let notes = audio.join("notes.txt");
    tagged_mp3(&shared_a, &tags("Same Artist", "2A"));
    tagged_mp3(&shared_b, &tags("Same Artist", "2A"));
    tagged_mp3(&shared_c, &tags("Same Artist", "2A"));
    tagged_mp3(&other, &tags("Other Artist", "Fm"));
    tagged_mp3(&plain, &[]);
    write_wav(&wav, 1);
    std::fs::write(&notes, b"not audio").unwrap();

    // One at a time, as an import has always gone.
    let mut one_by_one = fixture();
    for file in [&shared_a, &shared_b, &shared_c, &other, &plain, &wav] {
        one_by_one.writer.import_file(file).unwrap();
    }
    assert!(matches!(one_by_one.writer.import_file(&notes), Err(DbError::WriteRefused(_))));

    // In one batch, with a file the library already holds, a file that is not
    // audio, and one path listed twice.
    let mut batched = fixture();
    let held = batched.writer.import_file(&wav).unwrap();
    let usn_before: i64 = batched.one(
        "SELECT COALESCE(int_1, 0) FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
        &[],
    );
    let outcomes = batched
        .writer
        .import_files(&[
            shared_a.clone(), shared_b, shared_c, other, plain, wav, notes, shared_a.clone(),
        ])
        .unwrap();
    assert_eq!(outcomes.len(), 8);
    let added: Vec<&String> = outcomes
        .iter()
        .filter_map(|o| if let ImportOutcome::Added(id) = o { Some(id) } else { None })
        .collect();
    assert_eq!(added.len(), 5, "five new files: the wav was held already");
    assert_eq!(outcomes[5], ImportOutcome::Existing(held));
    assert!(matches!(outcomes[6], ImportOutcome::Refused(_)));
    assert_eq!(&outcomes[7], &ImportOutcome::Existing(added[0].clone()), "a path listed twice is one row");

    assert_eq!(described(&batched), described(&one_by_one));
    // Three files share an artist, an album, a genre and a key: one row each.
    for (table, column, name) in [
        ("djmdArtist", "Name", "Same Artist"), ("djmdAlbum", "Name", "Album"),
        ("djmdGenre", "Name", "Techno"), ("djmdKey", "ScaleName", "2A"),
    ] {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE {column} = '{name}' AND rb_local_deleted = 0");
        assert_eq!(batched.count(&sql), 1, "{table} {name}");
    }

    // Every new row has a USN of its own, above the counter's value before,
    // and the counter has moved to the highest.
    let mut usns: Vec<i64> = added
        .iter()
        .map(|id| batched.one("SELECT rb_local_usn FROM djmdContent WHERE ID = ?1", &[*id]))
        .collect();
    usns.sort_unstable();
    usns.dedup();
    assert_eq!(usns.len(), 5);
    assert!(usns[0] > usn_before);
    let counter: i64 = batched.one(
        "SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
        &[],
    );
    assert!(counter >= *usns.last().unwrap());
}

#[test]
fn removing_tracks_in_a_batch_leaves_what_removing_them_one_at_a_time_does() {
    let removed: Vec<String> = [2, 3, 7].into_iter().map(track_id).collect();

    let build = || {
        let mut f = fixture();
        let a = f.writer.create_playlist("A", ROOT).unwrap();
        let b = f.writer.create_playlist("B", ROOT).unwrap();
        let first: Vec<String> = (0..10).map(track_id).collect();
        let second: Vec<String> = [7, 1, 3, 9, 5].into_iter().map(track_id).collect();
        f.writer.add_tracks(&a, &first).unwrap();
        f.writer.add_tracks(&b, &second).unwrap();
        (f, a, b)
    };

    let (mut one_by_one, a1, b1) = build();
    for track in &removed {
        one_by_one.writer.delete_track(track).unwrap();
    }

    let (mut batched, a2, b2) = build();
    let usn_before: i64 = batched.one(
        "SELECT COALESCE(int_1, 0) FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
        &[],
    );
    // The fixture library already files these tracks in playlists of its own,
    // so the memberships are counted rather than assumed.
    let memberships: i64 = removed
        .iter()
        .map(|t| batched.one::<i64>(
            "SELECT COUNT(*) FROM djmdSongPlaylist WHERE rb_local_deleted = 0 AND ContentID = ?1",
            &[t],
        ))
        .sum();
    assert!(memberships >= 5);
    let changed = batched.writer.delete_tracks(&removed).unwrap();
    // Three tracks, and every membership they had.
    assert_eq!(changed.rows, 3 + memberships as usize);

    assert_eq!(batched.order(&a2), one_by_one.order(&a1));
    assert_eq!(batched.order(&b2), one_by_one.order(&b1));
    assert_eq!(batched.track_numbers(&a2), (1..=7).collect::<Vec<_>>());
    assert_eq!(batched.track_numbers(&b2), (1..=3).collect::<Vec<_>>());
    assert_eq!(
        batched.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 1"),
        one_by_one.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 1"),
    );

    // Every row it changed carries a USN above the counter's value before, and
    // no two share one; the counter has moved to the highest.
    let mut usns: Vec<i64> = {
        let mut stmt = batched
            .conn()
            .prepare(
                "SELECT rb_local_usn FROM djmdSongPlaylist WHERE rb_local_usn > ?1
                 UNION ALL SELECT rb_local_usn FROM djmdContent WHERE rb_local_usn > ?1",
            )
            .unwrap();
        stmt.query_map(params![usn_before], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    };
    let rows = usns.len();
    usns.sort_unstable();
    usns.dedup();
    assert_eq!(usns.len(), rows, "no two rows share a USN");
    let counter: i64 = batched.one(
        "SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
        &[],
    );
    assert_eq!(counter, *usns.last().unwrap());
}

/// The library a long job leaves, checked whole: no live playlist entry
/// points at a removed track, every playlist numbers its tracks 1..n, and
/// the update counter is at the highest USN any row carries.
fn assert_consistent(f: &Fixture) {
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM djmdSongPlaylist s JOIN djmdContent c ON c.ID = s.ContentID
             WHERE s.rb_local_deleted = 0 AND c.rb_local_deleted = 1",
        ),
        0,
        "no live playlist entry points at a removed track",
    );
    let playlists: Vec<String> = {
        let mut stmt = f.conn().prepare("SELECT ID FROM djmdPlaylist WHERE rb_local_deleted = 0").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    };
    for playlist in &playlists {
        let numbers = f.track_numbers(playlist);
        assert_eq!(numbers, (1..=numbers.len() as i64).collect::<Vec<_>>(), "playlist {playlist} numbered 1..n");
    }
    let highest: i64 = f.one(
        "SELECT MAX(u) FROM (SELECT MAX(rb_local_usn) AS u FROM djmdContent
                             UNION ALL SELECT MAX(rb_local_usn) FROM djmdSongPlaylist)",
        &[],
    );
    let counter: i64 = f.one("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", &[]);
    assert!(counter >= highest, "counter {counter} below a row's USN {highest}");
}

/// #291: Remove from Collection runs as a status-bar job, one
/// `delete_tracks` call (one transaction) per slice of 100, and Stop lands
/// between slices. A stop after two slices of 495 tracks leaves those 200
/// removed from the collection and from every playlist, the other 295 as
/// they were, and nothing half-done; removing the rest later finishes it.
#[test]
fn a_removal_stopped_between_slices_leaves_a_consistent_library() {
    let mut f = fixture_with(Shape { tracks: 495, ..Shape::default() });
    let all: Vec<String> = (0..495).map(track_id).collect();
    let big = f.writer.create_playlist("Big", ROOT).unwrap();
    f.writer.add_tracks(&big, &all).unwrap();
    let small = f.writer.create_playlist("Small", ROOT).unwrap();
    let mixed: Vec<String> = [150, 250, 50, 494].into_iter().map(track_id).collect();
    f.writer.add_tracks(&small, &mixed).unwrap();

    let slices: Vec<&[String]> = all.chunks(100).collect();
    for slice in &slices[..2] {
        f.writer.delete_tracks(slice).unwrap();
    }

    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 1"), 200);
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0"), 295);
    assert_eq!(f.order(&big), all[200..].to_vec());
    assert_eq!(f.order(&small), vec![track_id(250), track_id(494)]);
    assert_consistent(&f);

    for slice in &slices[2..] {
        f.writer.delete_tracks(slice).unwrap();
    }
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0"), 0);
    assert!(f.order(&big).is_empty());
    assert_consistent(&f);
}

/// #291: Import runs as a status-bar job, one `import_files` call (one
/// transaction) per slice of 100 files. A stop after three slices of 495
/// leaves exactly those 300 files as whole track rows and none of the rest;
/// importing all 495 again takes the 300 as already held and adds the 195,
/// ending where one uninterrupted import ends.
#[test]
fn an_import_stopped_between_slices_leaves_whole_rows_and_can_be_finished() {
    use rbl_db::write::ImportOutcome;

    let root = tempfile::tempdir().unwrap();
    let audio = root.path().join("imports");
    std::fs::create_dir(&audio).unwrap();
    let files: Vec<std::path::PathBuf> = (0..495)
        .map(|i| {
            let path = audio.join(format!("t{i:03}.mp3"));
            tagged_mp3(&path, &[]);
            path
        })
        .collect();

    let mut stopped = fixture();
    let before = stopped.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0");
    for slice in files.chunks(100).take(3) {
        let outcomes = stopped.writer.import_files(slice).unwrap();
        assert!(outcomes.iter().all(|o| matches!(o, ImportOutcome::Added(_))));
    }
    assert_eq!(stopped.count("SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0"), before + 300);
    assert_eq!(described(&stopped).len(), 300);
    assert_consistent(&stopped);

    let again = stopped.writer.import_files(&files).unwrap();
    let held = again.iter().filter(|o| matches!(o, ImportOutcome::Existing(_))).count();
    let added = again.iter().filter(|o| matches!(o, ImportOutcome::Added(_))).count();
    assert_eq!((held, added), (300, 195));
    assert_consistent(&stopped);

    let mut whole = fixture();
    whole.writer.import_files(&files).unwrap();
    assert_eq!(described(&stopped), described(&whole));
}

// ------------------------------------------------- cues a player saves (LINK)

fn player_cue(kind: u8, in_ms: u32, out_ms: Option<u32>) -> rbl_db::write::PlayerCue {
    rbl_db::write::PlayerCue { kind, in_ms, out_ms, colour: None, color_table_index: 21, comment: String::new(), beat_loop: None }
}

/// Kind, InMsec, OutMsec (-1 for none), Color, ColorTableIndex, Comment,
/// BeatLoopSize of a track's live cues.
type CueRow = (i64, i64, i64, i64, i64, String, Option<i64>);

fn live_cues(f: &Fixture, track: &str) -> Vec<CueRow> {
    let mut stmt = f
        .conn()
        .prepare(
            "SELECT Kind, InMsec, COALESCE(OutMsec, -1), Color, ColorTableIndex, Comment, BeatLoopSize FROM djmdCue
             WHERE ContentID = ?1 AND rb_local_deleted = 0 ORDER BY Kind, InMsec",
        )
        .unwrap();
    stmt.query_map([track], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn a_players_memory_cue_and_loop_are_added_with_their_colour_name_and_length() {
    // rekordbox's ReceiveSetCueEvent adds a memory cue without looking for
    // one already there; a second at the same place is a second row.
    let mut f = fixture();
    let track = track_id(0);
    let mut named = player_cue(0, 61_234, None);
    named.colour = Some(2);
    named.comment = "drop".into();
    f.writer.save_player_cue(&track, &named).unwrap();
    f.writer.save_player_cue(&track, &named).unwrap();
    let mut looped = player_cue(0, 1_000, Some(3_000));
    looped.beat_loop = Some((4, 1));
    f.writer.save_player_cue(&track, &looped).unwrap();
    assert_eq!(
        live_cues(&f, &track),
        vec![
            (0, 1_000, 3_000, 255, 0, String::new(), Some(262_145)),
            (0, 61_234, -1, 2, 0, "drop".into(), None),
            (0, 61_234, -1, 2, 0, "drop".into(), None),
        ]
    );
}

#[test]
fn a_players_hot_cue_replaces_the_one_in_its_slot_only() {
    // ReceiveSetCueEvent calls ReceiveDeleteCueEvent (vtable +0x158) on a
    // hot cue first: the slot's old cue goes, the other slots stay.
    let mut f = fixture();
    let track = track_id(0);
    let old = f.writer.add_cue(&track, 5, 9_000).unwrap();
    f.writer.add_cue(&track, 1, 1_000).unwrap();
    f.writer.add_cue(&track_id(1), 5, 9_000).unwrap();
    let mut d = player_cue(5, 4_000, None);
    d.color_table_index = 5;
    f.writer.save_player_cue(&track, &d).unwrap();
    assert_eq!(
        live_cues(&f, &track),
        vec![(1, 1_000, -1, -1, 21, String::new(), None), (5, 4_000, -1, -1, 5, String::new(), None)]
    );
    let deleted: i64 = f.one("SELECT rb_local_deleted FROM djmdCue WHERE ID = ?1", &[&old]);
    assert_eq!(deleted, 1, "soft-deleted, as rekordbox's own deletes are");
    assert_eq!(live_cues(&f, &track_id(1)).len(), 1, "another track's slot D is untouched");
}

#[test]
fn a_players_delete_takes_a_hot_cue_by_slot_and_a_memory_cue_by_its_times() {
    let mut f = fixture();
    let track = track_id(0);
    f.writer.add_cue(&track, 2, 1_000).unwrap();
    f.writer.add_cue(&track, 0, 5_000).unwrap();
    f.writer.add_loop(&track, 0, 5_000, 6_000, 0).unwrap();
    // A hot cue goes by its slot, wherever it is.
    assert_eq!(f.writer.delete_player_cue(&track, &player_cue(2, 77, None)).unwrap().rows, 1);
    // A memory cue needs its in and out to match: the loop at 5 s is not
    // the cue point at 5 s.
    assert_eq!(f.writer.delete_player_cue(&track, &player_cue(0, 5_000, Some(6_500))).unwrap().rows, 0);
    assert_eq!(f.writer.delete_player_cue(&track, &player_cue(0, 5_000, None)).unwrap().rows, 1);
    assert_eq!(live_cues(&f, &track), vec![(0, 5_000, 6_000, 255, 0, String::new(), Some(0))]);
    // Nothing left to match is not an error.
    assert_eq!(f.writer.delete_player_cue(&track, &player_cue(3, 0, None)).unwrap().rows, 0);
}

#[test]
fn a_players_active_memory_loop_is_kind_4_and_deleted_as_one() {
    // SavUsbCueExt stores an active memory loop as Kind 4, and a delete of
    // a memory loop that finds no Kind 0 looks for Kind 4.
    let mut f = fixture();
    let track = track_id(0);
    f.writer.save_player_cue(&track, &player_cue(4, 2_000, Some(2_500))).unwrap();
    assert_eq!(live_cues(&f, &track), vec![(4, 2_000, 2_500, 255, 0, String::new(), None)]);
    assert_eq!(f.writer.delete_player_cue(&track, &player_cue(0, 2_000, Some(2_500))).unwrap().rows, 1);
    assert!(live_cues(&f, &track).is_empty());
}

#[test]
fn a_players_cue_rekordbox_would_not_store_is_refused() {
    let mut f = fixture();
    let track = track_id(0);
    for cue in [
        player_cue(18, 0, None),
        player_cue(4, 0, None),
        player_cue(0, 5_000, Some(5_000)),
        rbl_db::write::PlayerCue { colour: Some(8), ..player_cue(0, 0, None) },
        rbl_db::write::PlayerCue { color_table_index: 0, ..player_cue(1, 0, None) },
    ] {
        assert!(matches!(f.writer.save_player_cue(&track, &cue), Err(DbError::WriteRefused(_))), "{cue:?}");
    }
    assert!(matches!(f.writer.save_player_cue("no-such-track", &player_cue(0, 0, None)), Err(DbError::WriteRefused(_))));
    assert_eq!(f.count("SELECT COUNT(*) FROM djmdCue"), 0);
}
