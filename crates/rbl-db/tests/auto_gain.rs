#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]
use rbl_db::{
    fixture::{self, track_id, Shape},
    write::{auto_gain::encode_auto_gain, Writer},
};
use rusqlite::params;

fn setup() -> (tempfile::TempDir, Writer) {
    let dir = tempfile::tempdir().unwrap();
    let loc = fixture::build(
        dir.path(),
        Shape {
            tracks: 3,
            playlists: 0,
            tracks_per_playlist: 0,
            history_sessions: 0,
            start_usn: 100,
        },
    )
    .unwrap();
    let writer = Writer::open(loc, dir.path().join("backups")).unwrap();
    // The minimal fixture omits mixer parameters; use the canonical schema, not a guessed row shape.
    let schema = include_str!("../src/master_schema.sql");
    let mixer_schema = schema
        .lines()
        .find(|line| line.starts_with("CREATE TABLE `djmdMixerParam`"))
        .unwrap();
    writer
        .library()
        .connection()
        .execute_batch(mixer_schema)
        .unwrap();
    for i in 0..2 {
        writer.library().connection().execute("INSERT INTO djmdMixerParam (ID,ContentID,GainHigh,GainLow,PeakHigh,PeakLow,created_at,updated_at,rb_local_usn) VALUES (?1,?2,16384,123,16900,456,'old','old',9000)", params![format!("m{i}"),track_id(i)]).unwrap();
    }
    (dir, writer)
}
fn halves(w: &Writer, i: usize) -> (Option<i64>, Option<i64>) {
    w.library()
        .connection()
        .query_row(
            "SELECT GainHigh,GainLow FROM djmdMixerParam WHERE ID=?1",
            [format!("m{i}")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}
#[test]
fn encoding_vectors_and_invalid_values() {
    assert_eq!(encode_auto_gain(0.0).unwrap(), (0x3f80, 0));
    assert_eq!(encode_auto_gain(6.0).unwrap(), (0x3fff, 0x64c1));
    assert_eq!(encode_auto_gain(-6.0).unwrap(), (0x3f00, 0x4dce));
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        10000.0,
        -10000.0,
    ] {
        assert!(encode_auto_gain(value).is_err());
    }
}
#[test]
fn batch_backup_exact_undo_redo_and_peak_preservation() {
    let (dir, mut w) = setup();
    let (changed, edit) = w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(1), 6.0)])
        .unwrap();
    assert_eq!(changed.rows, 2);
    assert!(changed.usn > 9000);
    assert_eq!(halves(&w, 0), (Some(16256), Some(0)));
    assert_eq!(
        rbl_db::write::backups_in(&dir.path().join("backups")).len(),
        1
    );
    let peak: (i64, i64, String) = w
        .library()
        .connection()
        .query_row(
            "SELECT PeakHigh,PeakLow,created_at FROM djmdMixerParam WHERE ID='m0'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(peak, (16900, 456, "old".into()));
    w.restore_auto_gain(&edit, true).unwrap();
    assert_eq!(halves(&w, 0), (Some(16384), Some(123)));
    w.restore_auto_gain(&edit, false).unwrap();
    assert_eq!(halves(&w, 1), (Some(16383), Some(25793)));
    let (_, noop) = w.set_auto_gain_with_undo(&[(track_id(0), 0.0)]).unwrap();
    assert!(noop.is_empty());
}
#[test]
fn batch_refuses_unknown_tracks_and_duplicate_ids_or_rows_atomically() {
    let (_dir, mut w) = setup();
    let old = halves(&w, 0);
    assert!(w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(99), 1.0)])
        .is_err());
    assert_eq!(halves(&w, 0), old);
    assert!(w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(0), 1.0)])
        .is_err());
    w.library().connection().execute("INSERT INTO djmdMixerParam (ID,ContentID,created_at,updated_at) VALUES ('duplicate',?1,'old','old')",[track_id(0)]).unwrap();
    assert!(w.set_auto_gain_with_undo(&[(track_id(0), 0.0)]).is_err());
}
#[test]
fn undo_preserves_nulls_and_conflicts_atomically() {
    let (_dir, mut w) = setup();
    w.library()
        .connection()
        .execute(
            "UPDATE djmdMixerParam SET GainHigh=NULL,GainLow=NULL WHERE ID='m0'",
            [],
        )
        .unwrap();
    let (_, edit) = w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(1), 6.0)])
        .unwrap();
    w.library()
        .connection()
        .execute("UPDATE djmdMixerParam SET GainLow=999 WHERE ID='m1'", [])
        .unwrap();
    assert!(w.restore_auto_gain(&edit, true).is_err());
    assert_eq!(halves(&w, 0), (Some(16256), Some(0)));
    w.library()
        .connection()
        .execute("UPDATE djmdMixerParam SET GainLow=25793 WHERE ID='m1'", [])
        .unwrap();
    w.restore_auto_gain(&edit, true).unwrap();
    assert_eq!(halves(&w, 0), (None, None));
}
#[test]
fn sql_failure_rolls_back_entire_batch() {
    let (_dir, mut w) = setup();
    let old = halves(&w, 0);
    w.library().connection().execute_batch("CREATE TRIGGER reject_m1 BEFORE UPDATE ON djmdMixerParam WHEN NEW.ID='m1' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(1), 6.0)])
        .is_err());
    assert_eq!(halves(&w, 0), old);
}

#[test]
fn deleted_track_and_replaced_row_refuse_without_partial_writes() {
    let (_dir, mut w) = setup();
    let old = halves(&w, 0);
    w.library()
        .connection()
        .execute(
            "UPDATE djmdContent SET rb_local_deleted=1 WHERE ID=?1",
            [track_id(1)],
        )
        .unwrap();
    assert!(w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(1), 6.0)])
        .is_err());
    assert_eq!(halves(&w, 0), old);
    let (_, edit) = w.set_auto_gain_with_undo(&[(track_id(0), 0.0)]).unwrap();
    w.library()
        .connection()
        .execute(
            "UPDATE djmdMixerParam SET ID='replacement' WHERE ID='m0'",
            [],
        )
        .unwrap();
    assert!(w.restore_auto_gain(&edit, true).is_err());
}

#[test]
fn a_missing_mixer_row_is_created_with_unity_peak_and_undoes_as_one_batch() {
    let (dir, mut w) = setup();
    let old = halves(&w, 0);
    let source_rows = content_rows(&w);
    let (changed, edit) = w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(2), 6.0)])
        .unwrap();
    assert_eq!(changed.rows, 2);
    assert!(changed.usn > 9000);
    let row: (String, String, i64, i64, i64, i64) = w.library().connection().query_row(
        "SELECT ID,UUID,GainHigh,GainLow,PeakHigh,PeakLow FROM djmdMixerParam WHERE ContentID=?1 AND rb_local_deleted=0",
        [track_id(2)], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
    ).unwrap();
    assert_eq!((row.2, row.3, row.4, row.5), (0x3fff, 0x64c1, 0x3f80, 0));
    for id in [&row.0, &row.1] {
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
    }
    assert_ne!(row.0, row.1);
    assert_eq!(
        rbl_db::write::backups_in(&dir.path().join("backups")).len(),
        1
    );
    assert_eq!(w.restore_auto_gain(&edit, true).unwrap().rows, 2);
    assert_eq!(halves(&w, 0), old);
    let deleted: i64 = w
        .library()
        .connection()
        .query_row(
            "SELECT rb_local_deleted FROM djmdMixerParam WHERE ID=?1",
            [&row.0],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(w.restore_auto_gain(&edit, false).unwrap().rows, 2);
    let identity: (String, String) = w
        .library()
        .connection()
        .query_row(
            "SELECT ID,UUID FROM djmdMixerParam WHERE ContentID=?1 AND rb_local_deleted=0",
            [track_id(2)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(identity, (row.0, row.1));
    assert_eq!(
        content_rows(&w),
        source_rows,
        "gain writes must not change track metadata or analysis associations"
    );
}

fn content_rows(w: &Writer) -> Vec<Vec<rusqlite::types::Value>> {
    let mut statement = w
        .library()
        .connection()
        .prepare("SELECT * FROM djmdContent ORDER BY ID")
        .unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            (0..columns)
                .map(|i| row.get(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn a_failed_new_row_rolls_back_existing_gain_and_the_counter() {
    let (_dir, mut w) = setup();
    let old = halves(&w, 0);
    let counter: i64 = w
        .library()
        .connection()
        .query_row("SELECT MAX(rb_local_usn) FROM djmdMixerParam", [], |r| {
            r.get(0)
        })
        .unwrap();
    w.library().connection().execute_batch("CREATE TRIGGER reject_new_mixer BEFORE INSERT ON djmdMixerParam BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(2), 6.0)])
        .is_err());
    assert_eq!(halves(&w, 0), old);
    let after: i64 = w
        .library()
        .connection()
        .query_row("SELECT MAX(rb_local_usn) FROM djmdMixerParam", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(after, counter);
    let rows: i64 = w
        .library()
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM djmdMixerParam WHERE ContentID=?1",
            [track_id(2)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn created_row_undo_preserves_outside_work_and_redo_refuses_a_replacement() {
    let (_dir, mut w) = setup();
    let old = halves(&w, 0);
    let (_, edit) = w
        .set_auto_gain_with_undo(&[(track_id(0), 0.0), (track_id(2), 6.0)])
        .unwrap();
    w.library()
        .connection()
        .execute(
            "UPDATE djmdMixerParam SET PeakLow=17 WHERE ContentID=?1",
            [track_id(2)],
        )
        .unwrap();
    assert!(w.restore_auto_gain(&edit, true).is_err());
    assert_eq!(
        halves(&w, 0),
        (Some(0x3f80), Some(0)),
        "a failed batch Undo cannot partially restore its existing row"
    );
    let live: i64 = w
        .library()
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM djmdMixerParam WHERE ContentID=?1 AND rb_local_deleted=0",
            [track_id(2)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(live, 1, "outside peak edits must not be soft-deleted");
    w.library()
        .connection()
        .execute(
            "UPDATE djmdMixerParam SET PeakLow=0 WHERE ContentID=?1",
            [track_id(2)],
        )
        .unwrap();
    w.restore_auto_gain(&edit, true).unwrap();
    assert_eq!(halves(&w, 0), old);
    w.library().connection().execute("INSERT INTO djmdMixerParam (ID,ContentID,created_at,updated_at) VALUES ('outside',?1,'outside','outside')",[track_id(2)]).unwrap();
    assert!(w.restore_auto_gain(&edit, false).is_err());
    assert_eq!(
        halves(&w, 0),
        old,
        "a conflicting replacement must refuse the entire Redo"
    );
}

#[test]
fn creating_a_missing_live_row_does_not_reuse_or_edit_a_tombstone() {
    let (_dir, mut w) = setup();
    w.library()
        .connection()
        .execute(
            "UPDATE djmdMixerParam SET rb_local_deleted=1 WHERE ID='m1'",
            [],
        )
        .unwrap();
    let (_, edit) = w.set_auto_gain_with_undo(&[(track_id(1), 0.0)]).unwrap();
    let id: String = w
        .library()
        .connection()
        .query_row(
            "SELECT ID FROM djmdMixerParam WHERE ContentID=?1 AND rb_local_deleted=0",
            [track_id(1)],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(id, "m1");
    assert_eq!(halves(&w, 1), (Some(16384), Some(123)));
    w.restore_auto_gain(&edit, true).unwrap();
    let live: i64 = w
        .library()
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM djmdMixerParam WHERE ContentID=?1 AND rb_local_deleted=0",
            [track_id(1)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(live, 0);
    w.restore_auto_gain(&edit, false).unwrap();
    let restored: String = w
        .library()
        .connection()
        .query_row(
            "SELECT ID FROM djmdMixerParam WHERE ContentID=?1 AND rb_local_deleted=0",
            [track_id(1)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(restored, id);
}

#[test]
fn usn_exhaustion_never_wraps_or_partially_publishes_a_batch() {
    for mixed in [false, true] {
        let (_dir, mut w) = setup();
        w.library()
            .connection()
            .execute(
                "UPDATE agentRegistry SET int_1=?1 WHERE registry_id='localUpdateCount'",
                [i64::MAX - 1],
            )
            .unwrap();
        let old = halves(&w, 0);
        let mut values = vec![(track_id(0), 0.0)];
        if mixed {
            values.push((track_id(2), 6.0));
        }
        let result = w.set_auto_gain_with_undo(&values);
        let expected_counter = if mixed {
            assert!(result.is_err());
            assert_eq!(halves(&w, 0), old);
            i64::MAX - 1
        } else {
            assert_eq!(result.unwrap().0.usn, i64::MAX);
            assert_eq!(halves(&w, 0), (Some(0x3f80), Some(0)));
            i64::MAX
        };
        let counter: i64 = w
            .library()
            .connection()
            .query_row(
                "SELECT int_1 FROM agentRegistry WHERE registry_id='localUpdateCount'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(counter, expected_counter);
        let added: i64 = w
            .library()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM djmdMixerParam WHERE ContentID=?1",
                [track_id(2)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(added, 0);
    }
}
