#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::{apply, apply_recorded, CueEdit, CueKind};
use crate::error::ErrorKind;
use crate::state::{AppState, LibraryEdit};
use rbl_db::fixture::{self, track_id, Shape};
use rbl_db::write::{PlayerCue, Writer};
use rbl_db::{Library, OpenMode};
use std::sync::{Arc, Barrier};

struct Fixture {
    root: tempfile::TempDir,
    writer: Writer,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let location = fixture::build(
            root.path(),
            Shape {
                tracks: 2,
                playlists: 0,
                history_sessions: 0,
                ..Shape::default()
            },
        )
        .unwrap();
        let mut writer = Writer::open(location, root.path().join("backups")).unwrap();
        writer.disable_automatic_backups();
        Self { root, writer }
    }
}
fn addition(track: &str, letter: char, looped: bool) -> CueEdit {
    if looped {
        CueEdit::AddLoop {
            track: track.into(),
            kind: CueKind::Hot(letter),
            in_ms: 1000,
            out_ms: 3000,
            beat_loop: Some((4, 1)),
        }
    } else {
        CueEdit::Add {
            track: track.into(),
            kind: CueKind::Hot(letter),
            position_ms: 2000,
        }
    }
}

#[test]
fn occupied_hot_slots_refuse_point_and_loop_adds_without_rows_or_undo_changes() {
    let mut fixture = Fixture::new();
    let track = track_id(0);
    for (letter, first_loop, next_loop) in [
        ('A', false, false),
        ('D', false, true),
        ('H', true, false),
        ('P', true, true),
    ] {
        let (_, revision) =
            apply_recorded(&mut fixture.writer, addition(&track, letter, first_loop)).unwrap();
        let before = fixture.writer.cue_rows(&track).unwrap();
        let changes = fixture.writer.library().connection().total_changes();
        let refused =
            apply_recorded(&mut fixture.writer, addition(&track, letter, next_loop)).unwrap_err();
        assert_eq!(refused.kind, ErrorKind::Conflict);
        assert_eq!(
            refused.message,
            format!("Hot Cue {letter} has been set since")
        );
        assert_eq!(fixture.writer.cue_rows(&track).unwrap(), before);
        assert_eq!(
            fixture.writer.library().connection().total_changes(),
            changes
        );
        // The refusal leaves the accepted edit's original revision usable.
        fixture.writer.undo_cue_revision(&revision).unwrap();
        fixture.writer.redo_cue_revision(&revision).unwrap();
    }
}

#[test]
fn freed_hot_slots_other_slots_and_original_validation_remain_available() {
    let mut fixture = Fixture::new();
    let track = track_id(0);
    let (first, _) = apply_recorded(&mut fixture.writer, addition(&track, 'A', false)).unwrap();
    apply_recorded(&mut fixture.writer, CueEdit::Delete { cue: first.cue }).unwrap();
    apply_recorded(&mut fixture.writer, addition(&track, 'A', true)).unwrap();
    apply_recorded(&mut fixture.writer, addition(&track, 'B', false)).unwrap();
    let before = fixture.writer.cue_rows(&track).unwrap();
    for edit in [
        CueEdit::AddLoop {
            track: track.clone(),
            kind: CueKind::Hot('A'),
            in_ms: 1000,
            out_ms: 1000,
            beat_loop: None,
        },
        CueEdit::AddLoop {
            track: track.clone(),
            kind: CueKind::Hot('A'),
            in_ms: 1000,
            out_ms: 3000,
            beat_loop: Some((0, 1)),
        },
    ] {
        assert_eq!(
            apply_recorded(&mut fixture.writer, edit).unwrap_err().kind,
            ErrorKind::ReadOnly
        );
        assert_eq!(fixture.writer.cue_rows(&track).unwrap(), before);
    }
    assert_eq!(
        apply_recorded(&mut fixture.writer, addition(&track, 'Q', false))
            .unwrap_err()
            .kind,
        ErrorKind::Malformed
    );
    fixture
        .writer
        .library()
        .connection()
        .execute(
            "UPDATE djmdContent SET rb_local_deleted = 1 WHERE ID = ?1",
            [&track],
        )
        .unwrap();
    let refused = apply_recorded(&mut fixture.writer, addition(&track, 'A', false)).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::ReadOnly);
    assert!(refused.message.contains("no track"));
    assert_eq!(fixture.writer.cue_rows(&track).unwrap(), before);
}

#[test]
fn canonical_hot_kinds_are_guarded_but_raw_and_explicit_player_persistence_are_unchanged() {
    let mut fixture = Fixture::new();
    let track = track_id(0);
    let first = apply(&mut fixture.writer, addition(&track, 'A', true)).unwrap();
    fixture.writer.set_cue_colour(&first.cue, Some(15)).unwrap();
    fixture
        .writer
        .set_cue_comment(&first.cue, "retained hot loop")
        .unwrap();
    // The index truncates a numeric REAL Kind to u8, so 1.75 still displays A.
    fixture
        .writer
        .library()
        .connection()
        .execute("UPDATE djmdCue SET Kind = 1.75 WHERE ID = ?1", [&first.cue])
        .unwrap();
    let db = Library::open(
        fixture.writer.library().location().clone(),
        OpenMode::ReadOnly,
    )
    .unwrap();
    let (index, _) = rbl_index::load(&db).unwrap();
    let cues = index.cues_of(index.row_of(&track).unwrap());
    assert_eq!(cues.first().unwrap().hot_letter(), Some('A'));
    let before = fixture.writer.cue_rows(&track).unwrap();
    assert_eq!(
        apply_recorded(&mut fixture.writer, addition(&track, 'A', false))
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(fixture.writer.cue_rows(&track).unwrap(), before);
    // Raw import/conversion helpers keep their existing caller-owned contract.
    fixture
        .writer
        .library()
        .connection()
        .execute("UPDATE djmdCue SET Kind = 1 WHERE ID = ?1", [&first.cue])
        .unwrap();
    apply(&mut fixture.writer, addition(&track, 'A', false)).unwrap();
    let duplicates = fixture.writer.cue_rows(&track).unwrap();
    assert_eq!(
        duplicates
            .iter()
            .filter(|row| row.hot_kind() == Some(1))
            .count(),
        2
    );
    assert_eq!(
        apply_recorded(&mut fixture.writer, addition(&track, 'A', true))
            .unwrap_err()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        fixture.writer.cue_rows(&track).unwrap(),
        duplicates,
        "no migration or implicit cleanup of old duplicates"
    );
    // The explicit player persist mode still replaces its occupied slot.
    let replacement = fixture
        .writer
        .save_player_cue(
            &track,
            &PlayerCue {
                kind: 1,
                in_ms: 9000,
                color_table_index: 36,
                comment: "explicit replacement".into(),
                ..PlayerCue::default()
            },
        )
        .unwrap();
    let rows = fixture.writer.cue_rows(&track).unwrap();
    assert_eq!(
        rows.iter().filter(|row| row.hot_kind() == Some(1)).count(),
        1
    );
    let current: (String, i64, String) = fixture.writer.library().connection().query_row(
        "SELECT ID, InMsec, Comment FROM djmdCue WHERE ContentID = ?1 AND Kind = 1 AND rb_local_deleted = 0",
        [&track], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(current, (replacement, 9000, "explicit replacement".into()));
}

#[test]
fn competing_normal_hot_adds_use_the_existing_edit_gate_and_record_one_undo_entry() {
    let fixture = Fixture::new();
    let location = fixture.writer.library().location().clone();
    let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    let state = Arc::new(AppState::with_backups(
        fixture.root.path().join("app-backups"),
    ));
    state.set_library(library, false, db.schema().db_version, 0, location);
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for looped in [false, true] {
        let state = Arc::clone(&state);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            // Same gate/write/history boundary as the normal edit_cues action.
            let _gate = state.edit_gate.lock();
            let result = state
                .write_history_then(
                    |writer| Ok(apply_recorded(writer, addition(&track_id(0), 'E', looped))),
                    |_, applied| Ok(applied),
                )
                .unwrap();
            result.map(|(change, revision)| {
                state
                    .edit_history
                    .lock()
                    .record(LibraryEdit::Cues(revision), "Set Hot Cue");
                change
            })
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|result| result.as_ref().err())
            .unwrap()
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(state.edit_history.lock().undo.len(), 1);
    assert_eq!(
        fixture
            .writer
            .cue_rows(&track_id(0))
            .unwrap()
            .iter()
            .filter(|row| row.hot_kind() == Some(6))
            .count(),
        1
    );
}

#[test]
fn existing_duplicate_hot_rows_keep_the_generic_exact_id_clear_contract() {
    let mut fixture = Fixture::new();
    let track = track_id(0);
    let first = apply(
        &mut fixture.writer,
        CueEdit::Add {
            track: track.clone(),
            kind: CueKind::Hot('E'),
            position_ms: 1000,
        },
    )
    .unwrap();
    let second = apply(
        &mut fixture.writer,
        CueEdit::Add {
            track: track.clone(),
            kind: CueKind::Hot('E'),
            position_ms: 2000,
        },
    )
    .unwrap();
    let (_, deletion) =
        apply_recorded(&mut fixture.writer, CueEdit::Delete { cue: first.cue }).unwrap();
    let remaining: Vec<String> = fixture
        .writer
        .library()
        .connection()
        .prepare(
            "SELECT ID FROM djmdCue WHERE ContentID = ?1 AND Kind = 6 AND rb_local_deleted = 0",
        )
        .unwrap()
        .query_map([&track], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(remaining, vec![second.cue]);
    assert_eq!(
        fixture
            .writer
            .cue_rows(&track)
            .unwrap()
            .iter()
            .filter(|row| row.hot_kind() == Some(6))
            .count(),
        1
    );
    // No migration: the existing history guard refuses to restore a second
    // occupant while E remains set. Slot-wide legacy clear is separate work.
    let before_undo = fixture.writer.cue_rows(&track).unwrap();
    let changes = fixture.writer.library().connection().total_changes();
    let refused = fixture.writer.undo_cue_revision(&deletion).unwrap_err();
    assert!(matches!(refused, rbl_db::DbError::Conflict(ref reason)
        if reason == "Hot Cue E has been set since"));
    assert_eq!(fixture.writer.cue_rows(&track).unwrap(), before_undo);
    assert_eq!(
        fixture.writer.library().connection().total_changes(),
        changes
    );
}
