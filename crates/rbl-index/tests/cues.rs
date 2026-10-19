//! Cues read out of a real-schema library, and re-read one track at a time
//! after the writer changes them.
//!
//! The writer and the index share the fixture: a cue the writer adds has to
//! come back through `reload_cues_of` with its id, or the interface has
//! nothing to delete by. Everything here is a tempdir; nothing can reach the
//! installed library.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use rbl_db::fixture::{self, track_id, Shape};
use rbl_db::write::Writer;
use rbl_db::{Library as Db, OpenMode};
use rbl_index::{Cue, Cues};

struct Fixture {
    _dir: tempfile::TempDir,
    writer: Writer,
    library: rbl_index::Library,
}

fn open() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).expect("build the fixture");
    let writer =
        Writer::open(location.clone(), dir.path().join("backups")).expect("open for writing");
    let db = Db::open(location, OpenMode::ReadOnly).expect("open read-only");
    let (library, _stats) = rbl_index::load(&db).expect("index");
    Fixture { _dir: dir, writer, library }
}

impl Fixture {
    fn reload(&self, track: &str) {
        let db = Db::open(self.writer.library().location().clone(), OpenMode::ReadOnly).unwrap();
        rbl_index::reload_cues_of(&db, &self.library, track).unwrap();
    }

    fn cues(&self, track: &str) -> Vec<Cue> {
        let row = self.library.row_of(track).unwrap();
        self.library.cues_of(row)
    }
}

#[test]
fn a_fixture_starts_with_no_cues_and_a_written_one_is_read_back_with_its_id() {
    let mut f = open();
    let track = track_id(3);
    assert!(f.cues(&track).is_empty());

    let id = f.writer.add_cue(&track, 0, 12_345).unwrap();
    // Nothing moves until the track is re-read: the index is a snapshot.
    assert!(f.cues(&track).is_empty());

    f.reload(&track);
    let cues = f.cues(&track);
    assert_eq!(cues.len(), 1);
    assert_eq!(cues[0].id.to_string(), id, "the id the writer handed back");
    assert_eq!(cues[0].position_ms, 12_345);
    assert_eq!(cues[0].out_ms, 0, "a plain cue has no out point");
    assert!(cues[0].is_memory());
}

#[test]
fn a_loop_carries_its_out_point_and_a_hot_cue_its_letter() {
    let mut f = open();
    let track = track_id(0);
    f.writer.add_loop(&track, 0, 8_000, 16_000, 4).unwrap();
    f.writer.add_cue(&track, 5, 2_000).unwrap();
    f.reload(&track);

    let cues = f.cues(&track);
    assert_eq!(cues.len(), 2, "ordered by position, whatever order they were written in");
    assert_eq!(cues[0].position_ms, 2_000);
    assert_eq!(cues[0].hot_letter(), Some('D'));
    assert_eq!(cues[0].out_ms, 0);
    assert_eq!(cues[1].position_ms, 8_000);
    assert_eq!(cues[1].out_ms, 16_000);
    assert!(cues[1].is_memory());
}

#[test]
fn re_reading_one_track_leaves_the_others_where_they_were() {
    let mut f = open();
    let (a, b, c) = (track_id(1), track_id(2), track_id(3));
    f.writer.add_cue(&a, 0, 1_000).unwrap();
    f.writer.add_cue(&b, 0, 2_000).unwrap();
    f.writer.add_cue(&c, 0, 3_000).unwrap();
    for track in [&a, &b, &c] {
        f.reload(track);
    }

    // The middle track grows by two and then shrinks to none; its neighbours
    // must read the same before and after each splice.
    f.writer.add_cue(&b, 0, 2_500).unwrap();
    f.writer.add_cue(&b, 1, 2_250).unwrap();
    f.reload(&b);
    assert_eq!(f.cues(&a).len(), 1);
    assert_eq!(f.cues(&a)[0].position_ms, 1_000);
    assert_eq!(f.cues(&b).iter().map(|c| c.position_ms).collect::<Vec<_>>(), [2_000, 2_250, 2_500]);
    assert_eq!(f.cues(&c).len(), 1);
    assert_eq!(f.cues(&c)[0].position_ms, 3_000);

    for cue in f.cues(&b) {
        f.writer.delete_cue(&cue.id.to_string()).unwrap();
    }
    f.reload(&b);
    assert!(f.cues(&b).is_empty());
    assert_eq!(f.cues(&a)[0].position_ms, 1_000);
    assert_eq!(f.cues(&c)[0].position_ms, 3_000);
    assert_eq!(f.library.cues().len(), 2);
}

#[test]
fn a_moved_cue_is_re_sorted_into_place() {
    let mut f = open();
    let track = track_id(0);
    let early = f.writer.add_cue(&track, 0, 1_000).unwrap();
    f.writer.add_cue(&track, 0, 5_000).unwrap();
    f.reload(&track);
    assert_eq!(f.cues(&track)[0].id.to_string(), early);

    f.writer.move_cue(&early, 9_000).unwrap();
    f.reload(&track);
    let cues = f.cues(&track);
    assert_eq!(cues.iter().map(|c| c.position_ms).collect::<Vec<_>>(), [5_000, 9_000]);
    assert_eq!(cues[1].id.to_string(), early);
}

#[test]
fn a_track_the_index_does_not_hold_is_left_alone() {
    let f = open();
    let before = f.library.cues().len();
    f.reload("no-such-track");
    assert_eq!(f.library.cues().len(), before);
}

#[test]
fn a_library_built_without_cues_takes_its_first_track_s_cues() {
    // `testing::library_from` never touches the cue table, so the index is
    // empty rather than one-per-track; the first replace has to size it.
    use rbl_index::testing::{library_from, TestTrack};
    let lib = library_from(&[
        TestTrack { id: 1, title: "one", ..TestTrack::default() },
        TestTrack { id: 2, title: "two", ..TestTrack::default() },
        TestTrack { id: 3, title: "three", ..TestTrack::default() },
    ]);
    assert!(lib.cues_of(1).is_empty());
    lib.set_cues_of(1, vec![Cue { id: 7, position_ms: 500, out_ms: 0, kind: 0, colour: 0, memory_colour: None }]);
    assert_eq!(lib.cues_of(1).len(), 1);
    assert!(lib.cues_of(0).is_empty());
    assert!(lib.cues_of(2).is_empty());
    lib.set_cues_of(2, vec![Cue { id: 8, position_ms: 900, out_ms: 0, kind: 1, colour: 21, memory_colour: None }]);
    assert_eq!(lib.cues_of(1)[0].id, 7);
    assert_eq!(lib.cues_of(2)[0].id, 8);
    // Out of range is a no-op, not a panic.
    lib.set_cues_of(9, vec![Cue::default()]);
    assert_eq!(lib.cues().len(), 2);
    let table: &Cues = &lib.cues();
    assert!(!table.is_empty());
}

#[test]
fn a_track_s_cues_come_back_in_position_order_with_their_colour_index() {
    let mut f = open();
    let track = track_id(0);
    let b = f.writer.add_cue(&track, 2, 90_000).unwrap(); // hot cue B, later
    f.writer.set_cue_colour(&b, Some(46)).unwrap();
    f.writer.add_cue(&track, 1, 46).unwrap(); // hot cue A, first
    f.writer.add_cue(&track, 0, 46).unwrap(); // memory cue beside it
    f.writer.add_cue(&track_id(2), 6, 24).unwrap(); // hot cue E on another track
    f.reload(&track);
    f.reload(&track_id(2));

    let shape: Vec<(u32, Option<char>, u8)> =
        f.cues(&track).iter().map(|c| (c.position_ms, c.hot_letter(), c.colour)).collect();
    // The writer stores 0, no colour, on a new cue of either kind, as
    // rekordbox's deck does, and B's chosen index; the loader must hand
    // them back untouched.
    assert_eq!(shape, vec![(46, None, 0), (46, Some('A'), 0), (90_000, Some('B'), 46)]);

    assert!(f.cues(&track_id(1)).is_empty());
    let other = f.cues(&track_id(2));
    assert_eq!((other.len(), other[0].hot_letter(), other[0].colour), (1, Some('E'), 0));
}

#[test]
fn adding_a_cue_moves_the_content_version() {
    // The snapshot carries the cues, so a cue the app writes has to change the
    // number the snapshot is keyed to, or the next start would show the
    // library without it.
    let mut f = open();
    let version = |f: &Fixture| {
        let db = Db::open(f.writer.library().location().clone(), OpenMode::ReadOnly).unwrap();
        rbl_index::content_version(&db).unwrap()
    };
    let before = version(&f);
    f.writer.add_cue(&track_id(0), 1, 1_000).unwrap();
    assert_ne!(before, version(&f));
}

#[test]
fn a_track_s_hot_letters_read_in_letter_order_whatever_order_they_sit_in() {
    use rbl_index::testing::{library_from, TestTrack};
    let lib = library_from(&[
        TestTrack { id: 1, title: "one", ..TestTrack::default() },
        TestTrack { id: 2, title: "two", ..TestTrack::default() },
    ]);
    // Positioned so D comes before A, with a memory cue and a loop between.
    lib.set_cues_of(
        0,
        vec![
            Cue { id: 1, position_ms: 100, out_ms: 0, kind: Cue::kind_of_letter('D').unwrap(), colour: 0, memory_colour: None },
            Cue { id: 2, position_ms: 200, out_ms: 0, kind: Cue::MEMORY, colour: 0, memory_colour: None },
            Cue { id: 3, position_ms: 300, out_ms: 900, kind: Cue::kind_of_letter('A').unwrap(), colour: 0, memory_colour: None },
            Cue { id: 4, position_ms: 400, out_ms: 0, kind: Cue::kind_of_letter('P').unwrap(), colour: 0, memory_colour: None },
        ],
    );
    assert_eq!(lib.cues().hot_letters_of(0), "ADP");
    assert_eq!(lib.cues().hot_letters_of(1), "", "no cues, no letters");
    assert_eq!(lib.cues().hot_letters_of(9), "", "out of range is empty, not a panic");
    // Nothing here fills a slot twice — the pads only set an empty one — but
    // the library is not ours alone, so a doubled slot is one letter, not two.
    lib.set_cues_of(
        1,
        vec![
            Cue { id: 5, position_ms: 100, out_ms: 0, kind: 1, colour: 0, memory_colour: None },
            Cue { id: 6, position_ms: 200, out_ms: 0, kind: 1, colour: 0, memory_colour: None },
        ],
    );
    assert_eq!(lib.cues().hot_letters_of(1), "A");
}

/// An Active Loop is a memory loop whose `Kind` rekordbox sets to 4 (#345):
/// `MstSetActiveLoop` writes `CUEKIND_ACTIVELOOP`, 4, and the MEMORY panel
/// lists it with its loop icon lit [OBS static, rekordbox 7]. It is one of
/// the track's memory cues, keeps its out point, and has no hot-cue letter,
/// so nothing draws it as an unlettered hot cue.
#[test]
fn an_active_loop_is_a_memory_loop_with_no_hot_cue_letter() {
    let mut f = open();
    let track = track_id(2);
    let active = rbl_db::write::PlayerCue { kind: 4, in_ms: 200_000, out_ms: Some(201_875), ..Default::default() };
    f.writer.save_player_cue(&track, &active).unwrap();
    f.writer.add_loop(&track, 0, 30_000, 31_875, 4).unwrap();
    f.writer.add_cue(&track, 0, 10_000).unwrap();
    f.reload(&track);

    let cues = f.cues(&track);
    let read: Vec<_> = cues
        .iter()
        .map(|c| (c.position_ms, c.out_ms, c.is_memory(), c.is_active_loop(), c.hot_letter()))
        .collect();
    assert_eq!(
        read,
        vec![
            (10_000, 0, true, false, None),
            (30_000, 31_875, true, false, None),
            (200_000, 201_875, true, true, None),
        ]
    );
    assert_eq!(f.library.cues().hot_letters_of(f.library.row_of(&track).unwrap()), "");
}
