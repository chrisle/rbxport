//! View semantics: ordering, search, windowing and the edge cases that would
//! otherwise show up as a panic in front of the user.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use rbl_index::testing::{add_playlist, library_from, TestTrack};
use rbl_index::{SortColumn, TrackSource, ViewSpec};

fn track(id: u64, title: &'static str, artist: &'static str, bpm: u32) -> TestTrack {
    TestTrack {
        id,
        title,
        artist,
        bpm_x100: bpm,
        length_sec: 200 + id as u32,
        ..TestTrack::default()
    }
}

fn sample() -> Vec<TestTrack> {
    vec![
        track(1, "Zebra", "ARTBAT", 12800),
        track(2, "apple", "Meduza", 13000),
        track(3, "Ébano", "Tujamo", 12400),
        track(4, "Banana", "artbat", 0),
        track(5, "Cherry (Extended Mix)", "Kryder", 14000),
    ]
}

fn spec(sort: SortColumn, descending: bool, query: &str) -> ViewSpec {
    ViewSpec { source: TrackSource::Collection, sort, descending, query: query.to_owned(), filter: Default::default() }
}

#[test]
fn sorts_keys_by_their_names_with_the_sharps_apart_and_blanks_last() {
    let keyed: Vec<TestTrack> = [(1, "Fm"), (2, "F#"), (3, ""), (4, "F"), (5, "F#m"), (6, "Abm"), (7, "A")]
        .into_iter()
        .map(|(id, key)| TestTrack { id, title: "t", key, ..TestTrack::default() })
        .collect();
    let lib = library_from(&keyed);
    let view = lib.open_view(&spec(SortColumn::Key, false, ""));
    let keys: Vec<&str> = view.rows.iter().map(|&r| lib.key_name(r)).collect();
    // The general fold drops `#`, which used to put F and F# on top of each other.
    assert_eq!(keys, ["A", "Abm", "F", "F#", "F#m", "Fm", ""]);
}

#[test]
fn sorts_keys_round_the_camelot_wheel_for_the_alphanumeric_display() {
    let keyed: Vec<TestTrack> = [(1, "A"), (2, "Abm"), (3, "B"), (4, "Ebm"), (5, "E"), (6, "Odd"), (7, "G#m")]
        .into_iter()
        .map(|(id, key)| TestTrack { id, title: "t", key, ..TestTrack::default() })
        .collect();
    let lib = library_from(&keyed);
    let view = lib.open_view(&spec(SortColumn::KeyCamelot, false, ""));
    let keys: Vec<&str> = view.rows.iter().map(|&r| lib.key_name(r)).collect();
    // 1A, 1A (an enharmonic spelling), 1B, 2A, 11B, 12B, and what is not a key.
    assert_eq!(keys, ["Abm", "G#m", "B", "Ebm", "A", "E", "Odd"]);
    let view = lib.open_view(&spec(SortColumn::KeyCamelot, true, ""));
    assert_eq!(lib.key_name(view.rows[0]), "Odd", "descending is the same order reversed");
}

#[test]
fn sorts_by_title_case_and_accent_insensitively() {
    let lib = library_from(&sample());
    let view = lib.open_view(&spec(SortColumn::Title, false, ""));
    let titles: Vec<&str> = view.rows.iter().map(|&r| lib.title.get(r as usize)).collect();
    assert_eq!(titles, ["apple", "Banana", "Cherry (Extended Mix)", "Ébano", "Zebra"]);
}

#[test]
fn sorts_by_comment_case_and_accent_insensitively() {
    let tracks = ["Zebra", "apple", "Ébano", "Banana"]
        .into_iter()
        .enumerate()
        .map(|(index, comment)| TestTrack {
            id: index as u64 + 1,
            title: "track",
            comment,
            ..TestTrack::default()
        })
        .collect::<Vec<_>>();
    let lib = library_from(&tracks);
    let view = lib.open_view(&spec(SortColumn::Comment, false, ""));
    let comments: Vec<&str> = view.rows.iter().map(|&row| lib.comment.get(row as usize)).collect();
    assert_eq!(comments, ["apple", "Banana", "Ébano", "Zebra"]);
}

#[test]
fn descending_is_the_exact_reverse_of_ascending() {
    let lib = library_from(&sample());
    let asc = lib.open_view(&spec(SortColumn::Title, false, ""));
    let desc = lib.open_view(&spec(SortColumn::Title, true, ""));
    let mut reversed = desc.rows.clone();
    reversed.reverse();
    assert_eq!(asc.rows, reversed);
}

#[test]
fn sorts_numerically_not_lexically() {
    let lib = library_from(&sample());
    let view = lib.open_view(&spec(SortColumn::Bpm, false, ""));
    let bpms: Vec<u32> = view.rows.iter().map(|&r| lib.bpm_x100[r as usize]).collect();
    assert_eq!(bpms, [0, 12400, 12800, 13000, 14000]);
}

#[test]
fn search_is_case_and_accent_insensitive() {
    let lib = library_from(&sample());
    assert_eq!(lib.open_view(&spec(SortColumn::Title, false, "ARTBAT")).len(), 2);
    assert_eq!(lib.open_view(&spec(SortColumn::Title, false, "ebano")).len(), 1);
    assert_eq!(lib.open_view(&spec(SortColumn::Title, false, "ÉBANO")).len(), 1);
}

#[test]
fn search_requires_every_token() {
    let lib = library_from(&sample());
    assert_eq!(lib.open_view(&spec(SortColumn::Title, false, "cherry extended")).len(), 1);
    assert_eq!(lib.open_view(&spec(SortColumn::Title, false, "cherry zebra")).len(), 0);
}

#[test]
fn search_ignores_punctuation() {
    let lib = library_from(&sample());
    assert_eq!(lib.open_view(&spec(SortColumn::Title, false, "(extended mix)")).len(), 1);
}

#[test]
fn refining_a_view_matches_searching_from_scratch() {
    let lib = library_from(&sample());
    let broad = lib.open_view(&spec(SortColumn::Title, false, "a"));
    let refined = lib.refine(&broad, "artbat");
    let direct = lib.open_view(&spec(SortColumn::Title, false, "artbat"));
    let mut a = refined.rows.clone();
    let mut b = direct.rows.clone();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b);
}

#[test]
fn clearing_the_query_restores_the_previous_set() {
    let lib = library_from(&sample());
    let broad = lib.open_view(&spec(SortColumn::Title, false, "a"));
    assert_eq!(lib.refine(&broad, "").rows, broad.rows);
}

#[test]
fn window_clamps_instead_of_panicking() {
    let lib = library_from(&sample());
    let view = lib.open_view(&spec(SortColumn::Title, false, ""));
    assert_eq!(view.window(0, 2).len(), 2);
    assert_eq!(view.window(3, 100).len(), 2);
    assert_eq!(view.window(500, 64).len(), 0);
    assert_eq!(view.window(0, 0).len(), 0);
    assert_eq!(view.window(usize::MAX, 64).len(), 0);
}

#[test]
fn ids_in_range_works_in_either_direction_and_clamps() {
    let lib = library_from(&sample());
    let view = lib.open_view(&spec(SortColumn::Title, false, ""));
    let forward = lib.ids_in_range(&view, 1, 3);
    let backward = lib.ids_in_range(&view, 3, 1);
    assert_eq!(forward, backward);
    assert_eq!(forward.len(), 3);
    assert_eq!(lib.ids_in_range(&view, 0, 999).len(), 5);
}

#[test]
fn a_playlist_view_holds_only_its_own_rows() {
    let mut lib = library_from(&sample());
    let pl = add_playlist(&mut lib, "Set", &[4, 0, 2]);
    let view = lib.open_view(&ViewSpec {
        source: TrackSource::Playlist(pl),
        sort: SortColumn::Title,
        descending: false,
        query: String::new(),
        filter: Default::default(),
    });
    let titles: Vec<&str> = view.rows.iter().map(|&r| lib.title.get(r as usize)).collect();
    assert_eq!(titles, ["Cherry (Extended Mix)", "Ébano", "Zebra"]);
}

#[test]
fn searching_within_a_playlist_stays_within_it() {
    let mut lib = library_from(&sample());
    let pl = add_playlist(&mut lib, "Set", &[0, 1]);
    let view = lib.open_view(&ViewSpec {
        source: TrackSource::Playlist(pl),
        sort: SortColumn::Title,
        descending: false,
        query: "artbat".to_owned(),
        filter: Default::default(),
    });
    // Row 3 ("Banana" / "artbat") also matches, but is not in this playlist.
    assert_eq!(view.len(), 1);
}

#[test]
fn an_unknown_playlist_index_yields_an_empty_view_rather_than_panicking() {
    let lib = library_from(&sample());
    let view = lib.open_view(&ViewSpec {
        source: TrackSource::Playlist(999),
        sort: SortColumn::Title,
        descending: false,
        query: String::new(),
        filter: Default::default(),
    });
    assert!(view.is_empty());
}

#[test]
fn an_empty_library_sorts_and_searches_without_panicking() {
    let lib = library_from(&[]);
    let view = lib.open_view(&spec(SortColumn::Title, false, ""));
    assert!(view.is_empty());
    assert!(lib.open_view(&spec(SortColumn::Bpm, true, "anything")).is_empty());
    assert!(lib.ids_in_range(&view, 0, 10).is_empty());
    assert_eq!(view.window(0, 64).len(), 0);
}

#[test]
fn every_sort_column_produces_a_full_permutation() {
    let lib = library_from(&sample());
    for column in [
        SortColumn::TrackNo, SortColumn::Title, SortColumn::Artist, SortColumn::Album,
        SortColumn::Genre, SortColumn::Label, SortColumn::Key, SortColumn::Bpm,
        SortColumn::Duration, SortColumn::Rating, SortColumn::PlayCount, SortColumn::DateAdded, SortColumn::ReleaseDate,
    ] {
        for descending in [false, true] {
            let view = lib.open_view(&spec(column, descending, ""));
            let mut rows = view.rows.clone();
            rows.sort_unstable();
            rows.dedup();
            assert_eq!(rows.len(), 5, "{column:?} desc={descending} dropped or duplicated rows");
        }
    }
}

#[test]
fn hot_cue_letters_follow_rekordbox_s_kind_numbering() {
    use rbl_index::Cue;
    // 1,2,3 then 5 — kind 4 is unused, which is why D is 5. Counted across all
    // 1,040,598 cues in the reference library.
    let letter = |kind: u8| Cue { kind, ..Cue::default() }.hot_letter();
    assert_eq!(letter(0), None, "kind 0 is a memory cue");
    assert_eq!(letter(1), Some('A'));
    assert_eq!(letter(2), Some('B'));
    assert_eq!(letter(3), Some('C'));
    assert_eq!(letter(4), None, "kind 4 is not used");
    assert_eq!(letter(5), Some('D'));
    assert_eq!(letter(6), Some('E'));
    assert_eq!(letter(9), Some('H'));
    // rekordbox 7 has sixteen hot cues, not the eight recorded before.
    assert_eq!(letter(10), Some('I'));
    assert_eq!(letter(17), Some('P'));
    assert_eq!(letter(18), None, "past the sixteenth");
    assert_eq!(letter(255), None);
}

#[test]
fn a_letter_round_trips_through_the_kind_it_is_stored_as() {
    use rbl_index::Cue;
    for letter in 'A'..='P' {
        let kind = Cue::kind_of_letter(letter).expect("a slot rekordbox has");
        assert_ne!(kind, 4, "kind 4 is unused");
        assert_eq!(Cue { kind, ..Cue::default() }.hot_letter(), Some(letter));
    }
    assert_eq!(Cue::kind_of_letter('a'), Some(1), "case does not matter");
    assert_eq!(Cue::kind_of_letter('Q'), None, "rekordbox 7 stops at P");
    assert_eq!(Cue::kind_of_letter('1'), None);
    assert_eq!(Cue::kind_of_letter('é'), None);
}

#[test]
fn a_memory_cue_is_distinguishable_from_a_hot_one() {
    use rbl_index::Cue;
    assert!(Cue { kind: 0, ..Cue::default() }.is_memory());
    assert!(!Cue { kind: 1, ..Cue::default() }.is_memory());
}

#[test]
fn a_playlist_keeps_its_own_order_when_sorting_is_off() {
    // `TrackNo` means "the order this view produced", not a column to rank by.
    // Ranking by it puts a playlist into collection order, so turning sorting
    // off would silently rearrange somebody's set — a one-way door.
    let mut library = library_from(&sample());
    // Deliberately not ascending row order: that is the whole point.
    let members = vec![4_u32, 1, 3, 0];
    add_playlist(&mut library, "Set", &members);

    let natural = ViewSpec {
        source: TrackSource::Playlist(0),
        sort: SortColumn::TrackNo,
        descending: false,
        query: String::new(),
        filter: Default::default(),
    };
    assert_eq!(library.open_view(&natural).rows, members, "the membership order, untouched");

    let backwards = ViewSpec { descending: true, ..natural.clone() };
    assert_eq!(library.open_view(&backwards).rows, vec![0_u32, 3, 1, 4]);

    // And sorting by something real still sorts it.
    let by_title = ViewSpec { sort: SortColumn::Title, ..natural };
    assert_ne!(library.open_view(&by_title).rows, members);
}

#[test]
fn the_collection_in_its_own_order_is_the_row_order() {
    let library = library_from(&sample());
    let rows = library.open_view(&spec(SortColumn::TrackNo, false, "")).rows;
    assert_eq!(rows, vec![0_u32, 1, 2, 3, 4]);
}
