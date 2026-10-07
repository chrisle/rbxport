//! Intelligent playlists: the rule in `SmartList` decides the rows.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_index::testing::{add_smart_playlist, library_from, TestTrack};
use rbl_index::{smart::Date, SmartRule, SortColumn, TrackSource, ViewSpec};

fn tracks() -> Vec<TestTrack> {
    vec![
        TestTrack {
            id: 1,
            title: "Alpha",
            artist: "Daft Punk",
            album: "Discovery",
            genre: "House",
            album_artist: "Daft Punk",
            original_artist: "Sister Sledge",
            composer: "Thomas Bangalter",
            remixer: "",
            mix_name: "Album Mix",
            bpm_x100: 12_300,
            length_sec: 300,
            rating: 5,
            date_added: "2026-09-01",
            key: "Am",
            color: 1,
            year: 2001,
            play_count: 12,
            comment: "peak time",
            path: "/m/alpha.mp3",
            ..TestTrack::default()
        },
        TestTrack {
            id: 2,
            title: "Beta",
            artist: "Bicep",
            album: "Isles",
            genre: "Electronica",
            album_artist: "Bicep",
            original_artist: "",
            composer: "Bicep",
            remixer: "Four Tet",
            mix_name: "Club Mix",
            bpm_x100: 12_800,
            length_sec: 420,
            rating: 3,
            date_added: "2026-06-15",
            key: "F#m",
            color: 6,
            year: 2021,
            play_count: 0,
            comment: "",
            path: "/m/beta.mp3",
            ..TestTrack::default()
        },
        TestTrack {
            id: 3,
            title: "Gamma",
            artist: "Ben Böhmer",
            album: "Begin Again",
            genre: "Melodic House",
            album_artist: "Ben Böhmer",
            original_artist: "",
            composer: "Ben Böhmer",
            remixer: "",
            mix_name: "Original Mix",
            bpm_x100: 12_200,
            length_sec: 250,
            rating: 4,
            date_added: "2024-01-10",
            key: "Am",
            color: 0,
            year: 2021,
            play_count: 3,
            comment: "opener",
            path: "/m/gamma.mp3",
            ..TestTrack::default()
        },
    ]
}

fn rule(logic: u8, conditions: &[(&str, u8, &str, &str, &str)]) -> String {
    let mut xml = format!("<NODE Id=\"1\" LogicalOperator=\"{logic}\" AutomaticUpdate=\"1\">");
    for (property, operator, left, right, unit) in conditions {
        xml.push_str(&format!(
            "<CONDITION PropertyName=\"{property}\" Operator=\"{operator}\" ValueUnit=\"{unit}\" ValueLeft=\"{left}\" ValueRight=\"{right}\"/>"
        ));
    }
    xml.push_str("</NODE>");
    xml
}

fn rows_of(lib: &rbl_index::Library, xml: &str) -> Vec<u64> {
    let rule = SmartRule::parse(xml).expect("a rule");
    rule.evaluate_on(
        lib,
        &Date {
            year: 2026,
            month: 9,
            day: 17,
        },
    )
    .into_iter()
    .map(|row| lib.ids[row as usize])
    .collect()
}

#[test]
fn text_conditions_compare_without_case_or_accents() {
    let lib = library_from(&tracks());
    assert_eq!(
        rows_of(&lib, &rule(1, &[("genre", 8, "house", "", "")])),
        vec![1, 3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("artist", 1, "ben bohmer", "", "")])),
        vec![3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("name", 10, "B", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("album", 11, "S", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("albumArtist", 1, "bicep", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("originalArtist", 8, "sister", "", "")])),
        vec![1]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("producer", 1, "bicep", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("remixedBy", 8, "four", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("mixName", 10, "club", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("comments", 9, "peak", "", "")])),
        vec![3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("fileName", 8, "gamma", "", "")])),
        vec![3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("key", 1, "Am", "", "")])),
        vec![1, 3]
    );
}

#[test]
fn number_conditions_read_the_columns() {
    let lib = library_from(&tracks());
    assert_eq!(
        rows_of(&lib, &rule(1, &[("bpm", 5, "12200", "12500", "")])),
        vec![1, 3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("bpm", 3, "12500", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("rating", 3, "3", "", "")])),
        vec![1, 3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("rating", 1, "3", "", "")])),
        vec![2]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("duration", 4, "5:00", "", "")])),
        vec![3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("year", 1, "2021", "", "")])),
        vec![2, 3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("counter", 2, "0", "", "")])),
        vec![1, 3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("grouping", 1, "Aqua", "", "")])),
        vec![2]
    );
}

#[test]
fn date_conditions_count_back_from_today() {
    let lib = library_from(&tracks());
    assert_eq!(
        rows_of(&lib, &rule(1, &[("stockDate", 6, "1", "", "month")])),
        vec![1]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("stockDate", 6, "6", "", "months")])),
        Vec::<u64>::new()
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("stockDate", 7, "1", "", "year")])),
        vec![1, 2, 3]
    );
    assert_eq!(
        rows_of(&lib, &rule(1, &[("stockDate", 3, "2026-01-01", "", "")])),
        vec![1, 2]
    );
    assert_eq!(
        rows_of(
            &lib,
            &rule(1, &[("stockDate", 5, "2024-01-01", "2026-07-01", "")])
        ),
        vec![2, 3]
    );
}

#[test]
fn all_and_any_combine_and_invalid_rules_admit_nothing() {
    let lib = library_from(&tracks());
    let both = rule(
        1,
        &[("genre", 8, "house", "", ""), ("rating", 1, "5", "", "")],
    );
    assert_eq!(rows_of(&lib, &both), vec![1]);
    let either = rule(
        2,
        &[
            ("genre", 8, "electronica", "", ""),
            ("rating", 1, "5", "", ""),
        ],
    );
    assert_eq!(rows_of(&lib, &either), vec![1, 2]);
    // A My Tag is answered only by "contains" and "does not contain"; these
    // tracks carry none, so "=" admits nothing and the rule is understood.
    let tagged = rule(1, &[("myTag", 1, "7", "", "")]);
    assert!(rows_of(&lib, &tagged).is_empty());
    assert_eq!(SmartRule::parse(&tagged).unwrap().unsupported(), 0);
    let unknown = rule(1, &[("hotCueCount", 1, "7", "", "")]);
    assert!(rows_of(&lib, &unknown).is_empty());
    assert_eq!(SmartRule::parse(&unknown).unwrap().unsupported(), 1);
    assert!(rows_of(&lib, "<NODE LogicalOperator=\"1\"/>").is_empty());
    assert!(rows_of(&lib, "<NODE LogicalOperator=\"2\"/>").is_empty());
    assert!(rows_of(
        &lib,
        "<NODE LogicalOperator=\"1\"><CONDITION PropertyName=\"genre\" Operator=\"99\"/></NODE>"
    )
    .is_empty());
    assert!(SmartRule::parse(
        "<CONDITION PropertyName=\"genre\" Operator=\"1\" ValueLeft=\"House\"/><NODE LogicalOperator=\"1\"/>"
    )
    .is_none());
}

#[test]
fn a_smart_playlist_opens_as_a_view_and_sorts_and_searches_like_any_other() {
    let mut lib = library_from(&tracks());
    let index = add_smart_playlist(
        &mut lib,
        "Housey",
        &rule(1, &[("genre", 8, "house", "", "")]),
    );
    assert!(lib.playlists().is_smart(index));
    assert!(!lib.playlists().is_folder(index));
    assert!(lib.playlists().smart_rule(index).is_some());

    let view = lib.open_view(&ViewSpec {
        source: TrackSource::SmartPlaylist(index),
        sort: SortColumn::Title,
        descending: true,
        query: String::new(),
        filter: Default::default(),
    });
    let ids: Vec<u64> = view.rows.iter().map(|&r| lib.ids[r as usize]).collect();
    assert_eq!(ids, vec![3, 1]);

    let narrowed = lib.open_view(&ViewSpec {
        source: TrackSource::SmartPlaylist(index),
        sort: SortColumn::TrackNo,
        descending: false,
        query: "gamma".to_owned(),
        filter: Default::default(),
    });
    assert_eq!(narrowed.rows.len(), 1);

    // A rule that does not parse is a playlist with nothing in it.
    let broken = add_smart_playlist(&mut lib, "Broken", "not xml at all");
    assert!(lib.source_rows(&TrackSource::SmartPlaylist(broken)).is_empty());
}
