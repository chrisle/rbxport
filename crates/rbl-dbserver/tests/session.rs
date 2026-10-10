//! The menus a player is answered with, checked against rows rekordbox
//! 7.2.11 sent a CDJ-3000 (`docs/pre-release/verification/link/*-decoded.txt`).
//! Where a row depends on library data, a small catalog is built to hold
//! exactly what the captured row showed, so the bytes can be compared whole.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::sync::Arc;

use rbl_dbserver::catalog::{
    Analysis, Catalog, Edit, HotCueBank, HotCueBankCue, Query, RootCategory, Row, Sort,
    TrackColumn, TrackDetails, TrackScope, UsbCue,
};
use rbl_dbserver::item::{root_menu, TrackRow};
use rbl_dbserver::net::{Handler, Session};
use rbl_dbserver::session::CatalogHandler;
use rbl_dbserver::{kind, setup_request, Argument, Message};

/// The player's context word on its requests, as captured.
const CTX: u32 = 0x0101_0301;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// A library of one artist with one album and one track — the first rows
/// of the capture — plus the playlist folders it listed. The bool says
/// whether a player has loaded the track.
struct Small(bool);

const AALIYAH: u32 = 0x6d5c_28f2;
const ALBUM: u32 = 0xdc0d_0bca;
const GENRE: u32 = 0x8b0c_83f7;
const TRACK: u32 = 0x475f;

fn the_track() -> TrackRow {
    TrackRow {
        id: TRACK,
        title: "At Your Best (You Are Love)".into(),
        secondary_text: "Em - 156".into(),
        column: Default::default(),
        column_value: 0,
        key: 0x14,
        key_id: 0x1814_5d65,
        key_name: "D".into(),
        bpm_x100: 0x1e80,
    }
}

impl Catalog for Small {
    fn played(&self, track: u32) -> bool {
        self.0 && track == TRACK
    }
    fn list(&self, query: &Query) -> Vec<Row> {
        match query {
            Query::BpmBuckets => vec![Row::Date(7_800)],
            Query::Ratings => vec![Row::Date(1), Row::Date(0)],
            Query::Bitrates => vec![Row::Date(320), Row::Date(0)],
            Query::Colors => vec![
                Row::Named {
                    id: 1,
                    name: "Pink".into(),
                },
                Row::Named {
                    id: 8,
                    name: "Purple".into(),
                },
            ],
            Query::DurationMinutes => vec![Row::Date(11), Row::Date(0)],
            Query::ReleaseDecades => vec![Row::Date(2020), Row::Date(1990)],
            Query::ReleaseYears(2020) => vec![Row::Date(2024), Row::Date(2023)],
            Query::Genres(_) => vec![Row::Named {
                id: GENRE,
                name: "Pop".into(),
            }],
            Query::GenreArtists(_) => vec![Row::Named {
                id: AALIYAH,
                name: "Aaliyah".into(),
            }],
            Query::GenreArtistAlbums { .. } => vec![Row::Named {
                id: ALBUM,
                name: "Aaliyah".into(),
            }],
            Query::Labels(_) => vec![Row::Named {
                id: 0x1234,
                name: "Ablazing".into(),
            }],
            Query::LabelArtists(_) => vec![Row::Named {
                id: AALIYAH,
                name: "Aaliyah".into(),
            }],
            Query::LabelArtistAlbums { .. } => vec![Row::Named {
                id: ALBUM,
                name: "Aaliyah".into(),
            }],
            Query::Artists(_) => vec![Row::Named {
                id: AALIYAH,
                name: "Aaliyah".into(),
            }],
            Query::Albums(_) | Query::ArtistAlbums(_) => vec![Row::Named {
                id: ALBUM,
                name: "Aaliyah".into(),
            }],
            Query::Folder(0) => vec![
                Row::List {
                    id: 0xaa1f_785f,
                    name: "CURRENT".into(),
                    folder: true,
                    position: 1,
                },
                Row::List {
                    id: 0xffb7_d23b,
                    name: "NP3-TEST-MP3".into(),
                    folder: false,
                    position: 0xb,
                },
            ],
            Query::Histories => vec![Row::Named {
                id: 0x68ef_cd5c,
                name: "LINK HISTORY 2026-09-11".into(),
            }],
            Query::Years => vec![Row::Date(2026), Row::Date(2025)],
            Query::Months(2026) => vec![Row::Date(1), Row::Date(2)],
            Query::Tracks {
                scope: TrackScope::Album(ALBUM),
                ..
            } => vec![Row::Track {
                id: TRACK,
                position: 0x55,
            }],
            Query::Tracks { .. } => vec![Row::Track {
                id: TRACK,
                position: 0,
            }],
            _ => vec![],
        }
    }
    fn track_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
        (id == TRACK).then(|| {
            let mut track = the_track();
            if let Some(column) = column {
                track.column = column;
                match column {
                    TrackColumn::Artist => {
                        track.column_value = AALIYAH;
                        track.secondary_text = "Aaliyah".into();
                    }
                    TrackColumn::Bpm => {
                        track.column_value = track.bpm_x100;
                        track.secondary_text.clear();
                    }
                    TrackColumn::Title => {
                        track.column_value = 0;
                        track.secondary_text.clear();
                    }
                    _ => {}
                }
            }
            track
        })
    }
    fn file_name_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
        let mut track = self.track_row(id, column)?;
        track.title = "70 at your best (you are love).mp3".into();
        Some(track)
    }
    fn track(&self, id: u32) -> Option<TrackDetails> {
        (id == TRACK).then(|| TrackDetails {
            row: the_track(),
            comment: "Em - 156".into(),
            key_id: 0x1814_5d65,
            key_name: "11B".into(),
            artist_id: AALIYAH,
            artist: "Aaliyah".into(),
            duration_s: 0x122,
            genre_id: 0x8b0c_83f7,
            genre: "Pop".into(),
            date_added: "2023-08-06".into(),
            year: 0x140,
            bit_rate_kbps: 0x7ca,
            path: "/Volumes/SD/RB/Aaliyah/Unknown Album/70 at your best (you are love).mp3".into(),
            file_size: 0xb1_1858,
            file_type: 1,
            ..TrackDetails::default()
        })
    }
    fn hot_cue_banks(&self, parent: Option<u32>) -> Vec<HotCueBank> {
        if parent.is_none() {
            vec![HotCueBank {
                id: 42,
                name: "WARMUP".into(),
                folder: false,
            }]
        } else {
            Vec::new()
        }
    }
    fn hot_cue_bank_cues(&self, bank: u32) -> Vec<HotCueBankCue> {
        if bank == 42 {
            vec![HotCueBankCue {
                slot: 1,
                content: TRACK,
                in_ms: 1_000,
                out_ms: Some(2_000),
                color: 3,
                color_table_index: 21,
                active_loop: true,
                beat_loop_size: 0,
                cue_microsec: 0,
            }]
        } else {
            Vec::new()
        }
    }
    fn hot_cue_bank_tracks(&self, bank: u32) -> Vec<TrackRow> {
        if bank == 42 {
            vec![the_track()]
        } else {
            Vec::new()
        }
    }
    fn usb_cues(&self, track: u32) -> Vec<UsbCue> {
        if track == TRACK {
            vec![
                UsbCue {
                    slot: 1,
                    in_ms: 3_000,
                    out_ms: Some(4_000),
                    color_table_index: 21,
                },
                UsbCue {
                    slot: 0,
                    in_ms: 5_000,
                    out_ms: None,
                    color_table_index: 0,
                },
            ]
        } else {
            Vec::new()
        }
    }
    fn edit(&self, edit: &Edit) -> bool {
        matches!(edit, Edit::HotCueBankCue { bank: 42, .. })
    }
    fn artwork(&self, id: u32) -> Option<Vec<u8>> {
        (id == 0x14).then(|| vec![0xff, 0xd8, 0xff, 0xe1])
    }
    fn item_artwork(&self, id: u32) -> Option<Vec<u8>> {
        (id == 0x6272).then(|| vec![0xff, 0xd8, 0xff, 0xe0])
    }
    fn analysis(&self, track: u32, what: &Analysis) -> Option<Vec<u8>> {
        match what {
            Analysis::Tag { fourcc, extension }
                if track == TRACK && fourcc == b"PWV4" && extension == b"EXT" =>
            {
                Some(vec![b'P', b'W', b'V', b'4', 1, 2, 3])
            }
            _ => None,
        }
    }
}

fn session() -> Box<dyn Session> {
    session_with(Small(false))
}

fn session_with(catalog: Small) -> Box<dyn Session> {
    let handler = CatalogHandler::new(Arc::new(catalog));
    let mut session = handler.open();
    session.handle(&setup_request(1));
    session
}

fn numbers(kind: u16, tx: u32, args: &[u32]) -> Message {
    Message::new(
        tx,
        kind,
        args.iter().map(|&n| Argument::Number(n)).collect(),
    )
}

/// Asks for a menu and renders all of it; returns the items.
fn browse(session: &mut Box<dyn Session>, kind: u16, args: &[u32]) -> (u32, Vec<Message>) {
    let header = session.handle(&numbers(kind, 0x100, args));
    assert_eq!(header.len(), 1);
    assert_eq!(header[0].kind, rbl_dbserver::kind::MENU_HEADER);
    assert_eq!(header[0].arguments[0], Argument::Number(u32::from(kind)));
    let Argument::Number(count) = header[0].arguments[1] else {
        panic!()
    };
    let rendered = session.handle(&numbers(
        rbl_dbserver::kind::RENDER,
        0x101,
        &[args[0], 0, count, 0, count, 0xc, 1, 0],
    ));
    assert_eq!(
        rendered[0],
        Message::new(
            0x101,
            rbl_dbserver::kind::RENDER_HEADER,
            vec![Argument::Number(1), Argument::Number(0)]
        )
    );
    assert_eq!(
        rendered.last().unwrap().kind,
        rbl_dbserver::kind::MENU_FOOTER
    );
    (count, rendered[1..rendered.len() - 1].to_vec())
}

fn args(item: &Message) -> String {
    item.arguments
        .iter()
        .map(|a| match a {
            Argument::Number(n) => format!("{n:#x}"),
            Argument::String(s) => format!("{s:?}"),
            Argument::Blob(b) => format!("blob[{}]", b.len()),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn the_root_menu_matches_the_captured_cdj_3000_rekordbox_response() {
    let mut s = session();
    let (count, items) = browse(&mut s, kind::ROOT_MENU, &[CTX, 0, 0x5cf_ffff]);
    assert_eq!(count, 9);
    let actual: Vec<(u32, String, u32)> = items
        .iter()
        .map(|item| {
            let Argument::Number(id) = item.arguments[1] else {
                panic!()
            };
            let Argument::String(label) = &item.arguments[3] else {
                panic!()
            };
            let Argument::Number(item_type) = item.arguments[6] else {
                panic!()
            };
            (id, label.clone(), item_type)
        })
        .collect();
    let heading = |label: &str| format!("\u{fffa}{label}\u{fffb}");
    assert_eq!(
        actual,
        vec![
            (2, heading("ARTIST"), 0x81),
            (3, heading("ALBUM"), 0x82),
            (4, heading("TRACK"), 0x83),
            (12, heading("KEY"), 0x8b),
            (5, heading("PLAYLIST"), 0x84),
            (22, heading("HISTORY"), 0x95),
            (18, heading("SEARCH"), 0x91),
            (26, heading("MATCHING"), 0xaa),
            (27, heading("DATE ADDED"), 0x8c),
        ]
    );
    // The artist row exactly as captured (transaction 0x180 in the capture).
    let artist = items[0].clone();
    let mut captured = artist.clone();
    captured.transaction = 0x180;
    assert_eq!(
        captured.encode(),
        hex("11872349ae11000001801041010f101400000010060606020602060606060606060602061100000000110000000211000000122600000009fffa004100520054004900530054fffb000011000000022600000001000011000000811100000000110000000011000000001100000000110000000011000000001100000002260000000100001100000000")
    );
}

#[test]
fn the_root_menu_filters_configured_rows_with_the_players_mask() {
    let expected = [
        (1, 2),
        (2, 3),
        (3, 4),
        (10, 12),
        (16, 5),
        (18, 22),
        (19, 18),
        (26, 26),
        (24, 27),
    ];

    for (bit, id) in expected {
        let (count, items) = browse(&mut session(), kind::ROOT_MENU, &[CTX, 0, 1 << bit]);
        assert_eq!(count, 1, "capability bit {bit}");
        assert_eq!(items[0].arguments[1], Argument::Number(id));
    }

    let (count, items) = browse(&mut session(), kind::ROOT_MENU, &[CTX, 0, 1 << 0 | 1 << 23]);
    assert_eq!(count, 0);
    assert!(items.is_empty());
}

#[test]
fn the_legacy_root_mask_appends_hot_cue_bank_like_rekordbox() {
    let (count, items) = browse(&mut session(), kind::ROOT_MENU, &[CTX, 0, 0x00ff_ffff]);
    assert_eq!(count, 8);
    assert_eq!(items.last().unwrap().arguments[1], Argument::Number(23));
    assert_eq!(items.last().unwrap().arguments[6], Argument::Number(0x98));
}

#[test]
fn the_root_menu_uses_rekordboxs_special_disable_rules() {
    let category = |id, menu_item_id, disable| RootCategory {
        id,
        menu_item_id,
        disable,
        name: "TEST".into(),
        item_type: 0,
    };
    let categories = [
        category(1, 2, 2),
        category(2, 22, 2),
        category(3, 27, 2),
        category(4, 27, 3),
        category(5, 24, 0),
    ];
    let items = root_menu(&categories, (1 << 1) | (1 << 24) | (1 << 26));
    assert_eq!(
        items.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![2, 3]
    );
}

#[test]
fn numeric_filter_menus_use_the_captured_item_types() {
    let mut s = session();
    for (request, expected, item_type) in [
        (kind::BPM_MENU, vec![7_800], 0x0d),
        (kind::RATING_MENU, vec![1, 0], 0x0a),
        (kind::BITRATE_MENU, vec![320, 0], 0x10),
        (kind::TIME_MENU, vec![11, 0], 0x0b),
        (kind::RELEASE_DECADES, vec![2020, 1990], 0x11),
    ] {
        let (count, rows) = browse(&mut s, request, &[CTX, 0]);
        assert_eq!(count as usize, expected.len());
        assert_eq!(
            rows.iter()
                .map(|row| row.arguments[1].clone())
                .collect::<Vec<_>>(),
            expected
                .into_iter()
                .map(Argument::Number)
                .collect::<Vec<_>>()
        );
        assert!(rows
            .iter()
            .all(|row| row.arguments[6] == Argument::Number(item_type)));
    }

    let (_, colors) = browse(&mut s, kind::COLOR_MENU, &[CTX, 0]);
    assert_eq!(colors[0].arguments[3], Argument::String("Pink".into()));
    assert_eq!(colors[0].arguments[6], Argument::Number(0x14));
    assert_eq!(colors[1].arguments[3], Argument::String("Purple".into()));
    assert_eq!(colors[1].arguments[6], Argument::Number(0x1b));

    let (_, ranges) = browse(&mut s, kind::BPM_RANGES, &[CTX, 0, 7_800]);
    assert_eq!(ranges.len(), 7);
    assert_eq!(ranges[0].arguments[1], Argument::Number(0));
    assert_eq!(ranges[6].arguments[1], Argument::Number(6));
    assert!(ranges
        .iter()
        .all(|row| row.arguments[6] == Argument::Number(0x0d)));
}

#[test]
fn release_year_file_name_matching_and_label_paths_render() {
    let mut s = session();

    let (count, years) = browse(&mut s, kind::RELEASE_YEARS, &[CTX, 0, 2020]);
    assert_eq!(count, 3);
    assert_eq!(years[0].arguments[1], Argument::Number(u32::MAX));
    assert_eq!(years[0].arguments[6], Argument::Number(0xa0));
    assert_eq!(years[1].arguments[1], Argument::Number(2024));
    assert_eq!(years[1].arguments[6], Argument::Number(0x11));

    let (_, files) = browse(&mut s, kind::FILE_NAME_MENU, &[CTX, 0]);
    assert_eq!(
        files[0].arguments[3],
        Argument::String("70 at your best (you are love).mp3".into())
    );
    assert_eq!(browse(&mut s, kind::MATCHING_TRACKS, &[CTX, 0, TRACK]).0, 1);

    let (_, labels) = browse(&mut s, kind::LABEL_MENU, &[CTX, 0]);
    assert_eq!(labels[0].arguments[6], Argument::Number(0x0e));
    let (count, artists) = browse(&mut s, kind::LABEL_ARTISTS, &[CTX, 0, 0x1234]);
    assert_eq!(count, 1, "a single child does not gain an ALL row");
    assert_eq!(artists[0].arguments[6], Argument::Number(0x07));
    let (count, albums) = browse(
        &mut s,
        kind::LABEL_ARTIST_ALBUMS,
        &[CTX, 0, 0x1234, AALIYAH],
    );
    assert_eq!(count, 1);
    assert_eq!(albums[0].arguments[6], Argument::Number(0x02));
    assert_eq!(
        browse(
            &mut s,
            kind::LABEL_ARTIST_ALBUM_TRACKS,
            &[CTX, 0, 0x1234, AALIYAH, ALBUM],
        )
        .0,
        1
    );
}

#[test]
fn genre_browsing_drills_through_artist_album_and_tracks() {
    let mut s = session();

    let (count, genres) = browse(&mut s, kind::GENRE_MENU, &[CTX, 0]);
    assert_eq!(count, 1);
    assert_eq!(genres[0].arguments[1], Argument::Number(GENRE));
    assert_eq!(genres[0].arguments[6], Argument::Number(0x06));

    let (count, artists) = browse(&mut s, kind::GENRE_ARTISTS, &[CTX, 0, GENRE]);
    assert_eq!(count, 1);
    assert_eq!(artists[0].arguments[1], Argument::Number(AALIYAH));

    let (count, albums) = browse(&mut s, kind::GENRE_ARTIST_ALBUMS, &[CTX, 0, GENRE, AALIYAH]);
    assert_eq!(count, 1);
    assert_eq!(albums[0].arguments[1], Argument::Number(ALBUM));

    let (count, tracks) = browse(
        &mut s,
        kind::GENRE_ARTIST_ALBUM_TRACKS,
        &[CTX, 0, GENRE, AALIYAH, ALBUM],
    );
    assert_eq!(count, 1);
    assert_eq!(tracks[0].arguments[1], Argument::Number(TRACK));
}

#[test]
fn the_sort_menu_matches_the_capture() {
    let mut s = session();
    let (count, items) = browse(&mut s, kind::SORT_MENU, &[0x0105_0301, 0, 0]);
    assert_eq!(count, 7);
    assert_eq!(args(&items[0]), "0x0, 0x0, 0x14, \"\\u{fffa}DEFAULT\\u{fffb}\", 0x2, \"\", 0xa1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(args(&items[6]), "0x0, 0xc, 0xc, \"\\u{fffa}KEY\\u{fffb}\", 0x2, \"\", 0x8b, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
}

#[test]
fn the_sort_menu_uses_the_catalog_configuration() {
    struct ConfiguredCatalog;

    impl Catalog for ConfiguredCatalog {
        fn sorts(&self) -> Vec<Sort> {
            vec![Sort::DateAdded, Sort::Genre, Sort::DjPlayCount]
        }

        fn list(&self, _: &Query) -> Vec<Row> {
            Vec::new()
        }

        fn track_row(&self, _: u32, _: Option<TrackColumn>) -> Option<TrackRow> {
            None
        }

        fn track(&self, _: u32) -> Option<TrackDetails> {
            None
        }

        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }

        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }

        fn analysis(&self, _: u32, _: &Analysis) -> Option<Vec<u8>> {
            None
        }
    }

    let handler = CatalogHandler::new(Arc::new(ConfiguredCatalog));
    let mut session = handler.open();
    session.handle(&setup_request(1));
    let (count, items) = browse(&mut session, kind::SORT_MENU, &[CTX, 0, 0]);
    assert_eq!(count, 3);
    assert_eq!(items[0].arguments[1], Argument::Number(0x11));
    assert_eq!(
        items[0].arguments[3],
        Argument::String("\u{fffa}DATE ADDED\u{fffb}".to_owned())
    );
    assert_eq!(items[1].arguments[1], Argument::Number(0x06));
    assert_eq!(
        items[1].arguments[3],
        Argument::String("\u{fffa}GENRE\u{fffb}".to_owned())
    );
    assert_eq!(items[2].arguments[1], Argument::Number(0x10));
    assert_eq!(
        items[2].arguments[3],
        Argument::String("\u{fffa}DJ PLAY COUNT\u{fffb}".to_owned())
    );
    assert_eq!(items[0].arguments[6], Argument::Number(0x8c));
    assert_eq!(items[1].arguments[6], Argument::Number(0x06));
    assert_eq!(items[2].arguments[6], Argument::Number(0x97));
}

#[test]
fn the_key_menus_match_the_capture() {
    let mut s = session();
    let (count, items) = browse(&mut s, kind::KEY_MENU, &[CTX, 0]);
    assert_eq!(count, 24);
    assert_eq!(
        args(&items[0]),
        "0x0, 0x1, 0x8, \"Abm\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        args(&items[23]),
        "0x0, 0x18, 0x4, \"E\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    // The RX3 still uses the original two-level key menu in several browse
    // flows. It differs only by skipping the related-key distance selector.
    let (count, items) = browse(&mut s, kind::LEGACY_KEY_MENU, &[CTX, 0]);
    assert_eq!(count, 24);
    assert_eq!(items[0].arguments[1], Argument::Number(1));
    let (count, items) = browse(&mut s, kind::RELATED_KEYS, &[0x0102_0301, 0, 1]);
    assert_eq!(count, 3);
    assert_eq!(
        args(&items[0]),
        "0x0, 0x1, 0x8, \"Abm\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        args(&items[1]),
        "0x1, 0x1, 0xe, \"Abm, B\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(args(&items[2]), "0x2, 0x1, 0x22, \"Abm, B, Dbm, Ebm\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
}

#[test]
fn key_menus_use_catalog_display_names() {
    struct Alphanumeric;
    impl Catalog for Alphanumeric {
        fn key_name(&self, id: u32) -> String {
            let side = if id % 2 == 1 { 'A' } else { 'B' };
            format!("{}{side}", id.div_ceil(2))
        }
        fn list(&self, _: &Query) -> Vec<Row> {
            Vec::new()
        }
        fn track_row(&self, _: u32, _: Option<TrackColumn>) -> Option<TrackRow> {
            None
        }
        fn track(&self, _: u32) -> Option<TrackDetails> {
            None
        }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn analysis(&self, _: u32, _: &Analysis) -> Option<Vec<u8>> {
            None
        }
    }

    let handler = CatalogHandler::new(Arc::new(Alphanumeric));
    let mut session = handler.open();
    session.handle(&setup_request(1));

    let (_, keys) = browse(&mut session, kind::KEY_MENU, &[CTX, 0]);
    assert_eq!(keys[0].arguments[3], Argument::String("1A".to_owned()));
    assert_eq!(keys[23].arguments[3], Argument::String("12B".to_owned()));

    let (_, related) = browse(&mut session, kind::RELATED_KEYS, &[0x0102_0301, 0, 1]);
    assert_eq!(related[0].arguments[3], Argument::String("1A".to_owned()));
    assert_eq!(
        related[1].arguments[3],
        Argument::String("1A, 1B".to_owned())
    );
    assert_eq!(
        related[2].arguments[3],
        Argument::String("1A, 1B, 12A, 2A".to_owned())
    );
}

#[test]
fn rx3_legacy_key_tracks_are_exact_key_matches() {
    struct Spy(std::sync::Mutex<Option<Query>>);
    impl Catalog for Spy {
        fn list(&self, query: &Query) -> Vec<Row> {
            *self.0.lock().unwrap() = Some(query.clone());
            Vec::new()
        }
        fn track_row(&self, _: u32, _: Option<TrackColumn>) -> Option<TrackRow> {
            None
        }
        fn track(&self, _: u32) -> Option<TrackDetails> {
            None
        }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn analysis(&self, _: u32, _: &Analysis) -> Option<Vec<u8>> {
            None
        }
    }
    let catalog = Arc::new(Spy(std::sync::Mutex::new(None)));
    let mut s = CatalogHandler::new(Arc::clone(&catalog) as Arc<dyn Catalog>).open();
    s.handle(&setup_request(1));
    s.handle(&numbers(kind::LEGACY_KEY_TRACKS, 2, &[CTX, 0, 0x0c]));
    assert_eq!(
        *catalog.0.lock().unwrap(),
        Some(Query::Tracks {
            scope: TrackScope::Key {
                key: 0x0c,
                distance: 0,
            },
            sort: Sort::Default,
        })
    );
}

#[test]
fn artists_albums_and_their_tracks_are_shaped_as_captured() {
    let mut s = session();
    let (_, items) = browse(&mut s, kind::ARTIST_MENU, &[CTX, 0]);
    assert_eq!(args(&items[0]), "0x0, 0x6d5c28f2, 0x10, \"Aaliyah\", 0x2, \"\", 0x7, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");

    let (count, items) = browse(&mut s, kind::ARTIST_ALBUMS, &[CTX, 0, AALIYAH]);
    assert_eq!(count, 1, "a single album does not need an ALL row");
    assert_eq!(args(&items[0]), "0x0, 0xdc0d0bca, 0x10, \"Aaliyah\", 0x2, \"\", 0x2, 0x0, 0xdc0d0bca, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");

    // An artist's tracks carry flag 0x1000000; the album's the track number.
    let (_, items) = browse(
        &mut s,
        kind::ARTIST_ALBUM_TRACKS,
        &[CTX, 0, AALIYAH, 0xffff_ffff],
    );
    assert_eq!(args(&items[0]), "0x475f, 0x475f, 0x38, \"At Your Best (You Are Love)\", 0x12, \"Em - 156\", 0x2304, 0x1000000, 0x475f, 0x0, 0x100, 0x14, 0x18145d65, 0x4, \"D\", 0x1e80");
    let (_, items) = browse(&mut s, kind::ALBUM_TRACKS, &[CTX, 0, ALBUM]);
    assert_eq!(args(&items[0]), "0x475f, 0x475f, 0x38, \"At Your Best (You Are Love)\", 0x12, \"Em - 156\", 0x2304, 0x1000000, 0x475f, 0x55, 0x100, 0x14, 0x18145d65, 0x4, \"D\", 0x1e80");
    // In TRACK the flags are 0.
    let (_, items) = browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
    assert_eq!(args(&items[0]), "0x475f, 0x475f, 0x38, \"At Your Best (You Are Love)\", 0x12, \"Em - 156\", 0x2304, 0x0, 0x475f, 0x0, 0x100, 0x14, 0x18145d65, 0x4, \"D\", 0x1e80");
}

#[test]
fn render_override_selects_the_requested_track_column() {
    let mut s = session();
    let header = s.handle(&numbers(kind::TRACK_MENU, 1, &[CTX, 2]));
    assert_eq!(header[0].arguments[1], Argument::Number(1));

    let rendered = s.handle(&numbers(kind::RENDER, 2, &[CTX, 0, 1, 0, 1, 12, 1, 2]));
    let row = &rendered[1].arguments;
    assert_eq!(row[0], Argument::Number(AALIYAH));
    assert_eq!(row[5], Argument::String("Aaliyah".into()));
    assert_eq!(row[6], Argument::Number(0x0704));
    assert_eq!(row[12], Argument::Number(0x1814_5d65));
}

#[test]
fn rx3_render_uses_the_active_sort_as_the_track_column() {
    let mut s = session();
    let header = s.handle(&numbers(kind::TRACK_MENU, 1, &[CTX, 2]));
    assert_eq!(header[0].arguments[1], Argument::Number(1));

    let rendered = s.handle(&numbers(kind::RENDER, 2, &[CTX, 0, 1, 0, 1, 12]));
    let row = &rendered[1].arguments;
    assert_eq!(row[0], Argument::Number(AALIYAH));
    assert_eq!(row[5], Argument::String("Aaliyah".into()));
    assert_eq!(row[6], Argument::Number(0x0704));
    assert_eq!(row[12], Argument::Number(0x1814_5d65));
}

#[test]
fn rx3_bpm_sort_renders_bpm_before_key() {
    let mut s = session();
    let header = s.handle(&numbers(kind::TRACK_MENU, 1, &[CTX, 4]));
    assert_eq!(header[0].arguments[1], Argument::Number(1));

    let rendered = s.handle(&numbers(kind::RENDER, 2, &[CTX, 0, 1, 0, 1, 12]));
    let row = &rendered[1].arguments;
    assert_eq!(row[0], Argument::Number(0x1e80));
    assert_eq!(row[5], Argument::String("78.1 bpm - D".into()));
    assert_eq!(row[6], Argument::Number(0x0d04));
    assert_eq!(row[12], Argument::Number(0x1814_5d65));
    assert_eq!(row[15], Argument::Number(0x1e80));
}

#[test]
fn extended_render_without_an_override_uses_the_configured_column() {
    let mut s = session();
    let header = s.handle(&numbers(kind::TRACK_MENU, 1, &[CTX, 2]));
    assert_eq!(header[0].arguments[1], Argument::Number(1));

    let rendered = s.handle(&numbers(kind::RENDER, 2, &[CTX, 0, 1, 0, 1, 12, 1, 0]));
    let row = &rendered[1].arguments;
    assert_eq!(row[5], Argument::String("Em - 156".into()));
    assert_eq!(row[6], Argument::Number(0x2304));
}

#[test]
fn playlists_histories_and_dates_are_shaped_as_captured() {
    let mut s = session();
    let (_, items) = browse(&mut s, kind::PLAYLIST_MENU, &[CTX, 0, 0, 1]);
    assert_eq!(args(&items[0]), "0x0, 0xaa1f785f, 0x10, \"CURRENT\", 0x2, \"\", 0x1, 0x0, 0x0, 0x1, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(args(&items[1]), "0x0, 0xffb7d23b, 0x1a, \"NP3-TEST-MP3\", 0x2, \"\", 0x8, 0x0, 0x0, 0xb, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    // A playlist's tracks: flag 0x1000000 and the play order.
    let (_, items) = browse(&mut s, kind::PLAYLIST_MENU, &[CTX, 0, 0xffb7_d23b, 0]);
    assert!(args(&items[0]).contains("0x2304, 0x1000000, 0x475f, 0x0, 0x100"));
    // A history's tracks are all played.
    let (_, items) = browse(&mut s, kind::HISTORY_TRACKS, &[CTX, 0, 0x68ef_cd5c]);
    assert!(args(&items[0]).contains("0x2304, 0x100, 0x475f, 0x0, 0x100"));

    let (_, items) = browse(&mut s, kind::HISTORY_MENU, &[CTX, 0]);
    assert_eq!(args(&items[0]), "0x0, 0x68efcd5c, 0x30, \"LINK HISTORY 2026-09-11\", 0x2, \"\", 0x24, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");

    let (_, items) = browse(&mut s, kind::YEARS, &[CTX, 0]);
    assert_eq!(
        args(&items[0]),
        "0x0, 0x7ea, 0x2, \"\", 0x2, \"\", 0x2e, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    let (count, items) = browse(&mut s, kind::MONTHS, &[CTX, 0, 2026]);
    assert_eq!(count, 3);
    assert_eq!(args(&items[0]), "0x0, 0xffffffff, 0xc, \"\\u{fffa}ALL\\u{fffb}\", 0x2, \"\", 0xa0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(
        args(&items[1]),
        "0x0, 0x1, 0x2, \"\", 0x2, \"\", 0x2e, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
}

#[test]
fn metadata_and_track_info_have_the_captured_rows() {
    let mut s = session();
    let (count, items) = browse(&mut s, kind::METADATA, &[0x0102_0301, TRACK]);
    assert_eq!(count, 16);
    let rows: Vec<String> = items.iter().map(args).collect();
    assert_eq!(rows[0], "0x475f, 0x475f, 0x38, \"At Your Best (You Are Love)\", 0x12, \"Em - 156\", 0x2304, 0x0, 0x475f, 0x0, 0x100, 0x14, 0x18145d65, 0x4, \"D\", 0x1e80");
    assert_eq!(rows[1], "0x1, 0x6d5c28f2, 0x10, \"Aaliyah\", 0x2, \"\", 0x7, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(
        rows[2],
        "0x1, 0x0, 0x2, \"\", 0x2, \"\", 0x2, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[3],
        "0x0, 0x122, 0x2, \"\", 0x2, \"\", 0xb, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[4],
        "0x0, 0x1e80, 0x2, \"\", 0x2, \"\", 0xd, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[5],
        "0x1, 0x18145d65, 0x8, \"11B\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(rows[8], "0x0, 0x8b0c83f7, 0x8, \"Pop\", 0x2, \"\", 0x6, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(rows[9], "0x1, 0x475f, 0x16, \"2023-08-06\", 0x2, \"\", 0x2e, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(rows[10], "0x0, 0x475f, 0x12, \"Em - 156\", 0x2, \"\", 0x23, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(
        rows[11],
        "0x0, 0x140, 0x2, \"\", 0x2, \"\", 0x11, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[12],
        "0x0, 0x7ca, 0x2, \"\", 0x2, \"\", 0x10, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[15],
        "0x0, 0x0, 0x2, \"\", 0x2, \"\", 0x29, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );

    let (count, items) = browse(&mut s, kind::TRACK_INFO, &[0x0108_0301, TRACK]);
    assert_eq!(count, 7);
    let rows: Vec<String> = items.iter().map(args).collect();
    assert_eq!(rows[4], "0xb11858, 0x475f, 0x90, \"/Volumes/SD/RB/Aaliyah/Unknown Album/70 at your best (you are love).mp3\", 0x2, \"\", 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(
        rows[5],
        "0x0, 0x1, 0x2, \"\", 0x2, \"\", 0x2f, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[6],
        "0x0, 0x18145d65, 0x8, \"11B\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
}

/// After loading a track from a rekordbox source a CDJ-3000 asks for the KUVO
/// user info (`3006`) and waits on it: with no reply its next list request is
/// never sent ("Waiting…" after BACK, an 18 s timeout, two retries, the
/// source dropped). rekordbox 7.2.11 answers `4d02 [3006, 0, 160, blob]` and
/// the deck then asks for the delivery info (`2602`), a 13-row menu
/// (verification/link/kuvo-delivery-20260919.txt, frames 1407-1413).
#[test]
fn the_kuvo_user_info_and_delivery_info_are_answered_as_captured() {
    let mut s = session();
    let user = s.handle(&numbers(0x3006, 0xa2, &[0x0309_0301]));
    assert_eq!(user.len(), 1);
    // rekordbox's header, byte for byte; its blob carried its own account's
    // details (000482820000014d80042428…), ours is 160 zeros.
    let encoded = user[0].encode();
    let header = hex(
        "11872349ae11000000a2104d020f041400000004060606031100003006110000000011000000a014000000a0",
    );
    assert_eq!(&encoded[..header.len()], &header[..]);
    assert_eq!(encoded.len(), header.len() + 160);
    assert!(encoded[header.len()..].iter().all(|&b| b == 0));

    let (count, items) = browse(&mut s, 0x2602, &[0x0309_0301, TRACK]);
    assert_eq!(count, 13);
    let rows: Vec<String> = items.iter().map(args).collect();
    // The captured rows, with the capture's track ("Breaks 2", 0x18e460e, a
    // 13 s WAV at 140 BPM with no key, comment or label) read as ours.
    assert_eq!(
        rows[0],
        "0x0, 0x0, 0x2, \"\", 0x2, \"\", 0x36, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(rows[1], "0x0, 0x6d5c28f2, 0x10, \"Aaliyah\", 0x2, \"\", 0x7, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(
        rows[2],
        "0x0, 0x18145d65, 0x2, \"\", 0x2, \"\", 0xf, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[3],
        "0x0, 0x122, 0x2, \"\", 0x2, \"\", 0xb, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(rows[4], "0x475f, 0x475f, 0x38, \"At Your Best (You Are Love)\", 0x2, \"\", 0x4, 0x1000000, 0x475f, 0x0, 0x100, 0x0, 0x0, 0x2, \"\", 0x1e80");
    assert_eq!(rows[5], "0x0, 0x475f, 0x12, \"Em - 156\", 0x2, \"\", 0x23, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(
        rows[6],
        "0x0, 0x0, 0x2, \"\", 0x2, \"\", 0x2, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[7],
        "0x0, 0x1e80, 0x2, \"\", 0x2, \"\", 0xd, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[8],
        "0x0, 0x475f, 0x2, \"\", 0x2, \"\", 0x37, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[9],
        "0x0, 0x0, 0x2, \"\", 0x2, \"\", 0xe, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(
        rows[10],
        "0x0, 0x1, 0x2, \"\", 0x2, \"\", 0x12, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
    assert_eq!(rows[11], "0x0, 0x8b0c83f7, 0x8, \"Pop\", 0x2, \"\", 0x6, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0");
    assert_eq!(
        rows[12],
        "0x475f, 0x0, 0x2, \"\", 0x2, \"\", 0x4f, 0x0, 0x1, 0x0, 0x0, 0x0, 0x0, 0x2, \"\", 0x0"
    );
}

/// In the capture the track a player had loaded carried bit 0x100 in every
/// list and in its metadata; the tracks it had not, did not. A list that set
/// it on every row greyed the whole playlist on a CDJ.
#[test]
fn a_played_track_carries_the_played_bit_everywhere() {
    let mut s = session_with(Small(true));
    let (_, items) = browse(
        &mut s,
        kind::ARTIST_ALBUM_TRACKS,
        &[CTX, 0, AALIYAH, 0xffff_ffff],
    );
    assert!(args(&items[0]).contains("0x2304, 0x1000100, 0x475f, 0x0, 0x100"));
    let (_, items) = browse(&mut s, kind::PLAYLIST_MENU, &[CTX, 0, 0xffb7_d23b, 0]);
    assert!(args(&items[0]).contains("0x2304, 0x1000100, 0x475f, 0x0, 0x100"));
    let (_, items) = browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
    assert!(args(&items[0]).contains("0x2304, 0x100, 0x475f, 0x0, 0x100"));
    let (_, items) = browse(&mut s, kind::METADATA, &[0x0102_0301, TRACK]);
    assert!(args(&items[0]).contains("0x2304, 0x100, 0x475f, 0x0, 0x100"));
}

#[test]
fn artwork_and_tags_come_back_as_blobs_or_as_the_no_art_reply() {
    let mut s = session();
    let none = s.handle(&numbers(
        kind::ARTWORK,
        0x187,
        &[0x0108_0301, 0xdc0d_0bca, 1],
    ));
    assert_eq!(
        none[0].encode(),
        hex("11872349ae11000001871040020f04140000000406060603110000200311000000321100000000")
    );
    let some = s.handle(&numbers(kind::ARTWORK, 0x1a3, &[0x0108_0301, 0x6272, 1]));
    assert_eq!(args(&some[0]), "0x2003, 0x0, 0x4, blob[4]");
    // RX3 uses `0x2103` while loading: its content id is resolved as a menu
    // item's artwork, but the binary reply has the request kind it carried.
    let by_content = s.handle(&numbers(
        kind::CONTENT_ARTWORK,
        0x1a3,
        &[0x0108_0301, 0x6272, 0],
    ));
    assert_eq!(args(&by_content[0]), "0x2103, 0x0, 0x4, blob[4]");
    // Without the size argument the id is the title item's artwork field,
    // not the track's id.
    let by_field = s.handle(&numbers(kind::ARTWORK, 0x1a4, &[0x0108_0301, 0x14]));
    assert_eq!(args(&by_field[0]), "0x2003, 0x0, 0x4, blob[4]");
    let not_a_field = s.handle(&numbers(kind::ARTWORK, 0x1a5, &[0x0108_0301, 0x6272]));
    assert_eq!(args(&not_a_field[0]), "0x2003, 0x32, 0x0, blob[0]");

    let tag = s.handle(&numbers(
        kind::ANLZ_TAG,
        0x197,
        &[0x0108_0301, TRACK, 0x3456_5750, 0x54_5845],
    ));
    assert_eq!(tag[0].kind, kind::ANLZ_TAG_REPLY);
    assert_eq!(args(&tag[0]), "0x2c04, 0x0, 0x7, blob[7], 0x1");
    let missing = s.handle(&numbers(
        kind::ANLZ_TAG_2EX,
        0x198,
        &[0x0108_0301, TRACK, 0x3656_5750, 0x58_4532],
    ));
    assert_eq!(args(&missing[0]), "0x2d04, 0x32, 0x0, blob[0], 0x1");
    let grid = s.handle(&numbers(kind::BEAT_GRID, 0x199, &[0x0108_0301, TRACK]));
    assert_eq!(args(&grid[0]), "0x2204, 0x32, 0x0, blob[0], 0x0");
}

#[test]
fn rx3_hot_cue_bank_uses_its_menu_and_cue_envelopes() {
    let mut s = session();
    let banks = s.handle(&numbers(kind::HOT_CUE_BANK, 0x1c0, &[CTX, 0, 1]));
    assert_eq!(
        banks[0].arguments,
        vec![Argument::Number(0x2001), Argument::Number(1)]
    );
    let items = s.handle(&numbers(kind::RENDER, 0x1c1, &[CTX, 0, 8]));
    assert_eq!(items[1].arguments[1], Argument::Number(42));
    assert_eq!(items[1].arguments[6], Argument::Number(0x2b));
    let reply = s.handle(&numbers(kind::HOT_CUE_BANK_CUES, 0x1c2, &[CTX, 42]));
    assert_eq!(reply.len(), 1);
    assert_eq!(reply[0].kind, kind::HOT_CUE_BANK_REPLY);
    assert_eq!(
        args(&reply[0]),
        "0x2101, 0x0, 0x24, blob[36], 0x24, 0x1, 0x0, 0x8, blob[8], 0x0, blob[0]"
    );
    let Argument::Blob(record) = &reply[0].arguments[3] else {
        panic!("cue record")
    };
    assert_eq!(&record[..8], &[1, 1, 4, 0, 0x5f, 0x47, 0, 0]);
    let tracks = s.handle(&numbers(kind::HOT_CUE_BANK, 0x1c3, &[CTX, 42, 0]));
    assert_eq!(
        tracks[0].arguments,
        vec![Argument::Number(0x2001), Argument::Number(1)]
    );
    let track_items = s.handle(&numbers(kind::RENDER, 0x1c4, &[CTX, 0, 8]));
    assert_eq!(track_items[1].arguments[1], Argument::Number(TRACK));

    let mut changed_record = Vec::new();
    for word in [0x0004_0101, TRACK, 0, 150, 300, 0, 21, 0, 0] {
        changed_record.extend_from_slice(&word.to_le_bytes());
    }
    let changed = s.handle(&Message::new(
        0x1c5,
        kind::CHANGE_HOT_CUE_BANK,
        vec![
            Argument::Number(CTX),
            Argument::Number(42),
            Argument::Number(0x24),
            Argument::Blob(changed_record),
            Argument::Number(8),
            Argument::Blob([1_000_u32.to_le_bytes(), 2_000_u32.to_le_bytes()].concat()),
        ],
    ));
    assert_eq!(
        args(&changed[0]),
        "0x2201, 0x0, 0x48, blob[72], 0x24, 0x1, 0x1, 0x10, blob[16], 0x0, blob[0]"
    );
    let Argument::Blob(reloaded) = &changed[0].arguments[3] else {
        panic!("USB cue records")
    };
    assert_eq!(&reloaded[..8], &[1, 1, 1, 0, 0, 0, 0, 0]);
    let (decoded, used) = Message::decode(&reply[0].encode()).unwrap();
    assert_eq!(used, reply[0].encode().len());
    assert_eq!(decoded, reply[0]);

    // A successful bank edit reloads ordinary USB cues.  An uncued target
    // still receives the successful, empty `4702` envelope.
    let mut uncued_record = Vec::new();
    for word in [0x0004_0101, TRACK + 1, 0, 150, 300, 0, 21, 0, 0] {
        uncued_record.extend_from_slice(&word.to_le_bytes());
    }
    let uncued = s.handle(&Message::new(
        0x1c5,
        kind::CHANGE_HOT_CUE_BANK,
        vec![
            Argument::Number(CTX),
            Argument::Number(42),
            Argument::Number(0x24),
            Argument::Blob(uncued_record),
            Argument::Number(8),
            Argument::Blob([1_000_u32.to_le_bytes(), 2_000_u32.to_le_bytes()].concat()),
        ],
    ));
    assert_eq!(
        args(&uncued[0]),
        "0x2201, 0x0, 0x0, blob[0], 0x24, 0x0, 0x0, 0x0, blob[0], 0x0, blob[0]"
    );

    let missing = s.handle(&numbers(kind::HOT_CUE_BANK_CUES, 0x1c6, &[CTX, 999]));
    assert_eq!(
        args(&missing[0]),
        "0x2101, 0x0, 0x0, blob[0], 0x24, 0x0, 0x0, 0x0, blob[0], 0x0, blob[0]"
    );
    let (empty, used) = Message::decode(&missing[0].encode()).unwrap();
    assert_eq!(used, missing[0].encode().len());
    assert_eq!(empty, missing[0]);
    let wrong_context = s.handle(&numbers(kind::HOT_CUE_BANK_CUES, 0x1c6, &[CTX & !0xff, 42]));
    assert_eq!(
        args(&wrong_context[0]),
        "0x2101, 0x32, 0x0, blob[0], 0x24, 0x0, 0x0, 0x0, blob[0], 0x0, blob[0]"
    );
    let malformed = s.handle(&numbers(kind::CHANGE_HOT_CUE_BANK, 0x1c7, &[CTX, 42]));
    assert_eq!(
        args(&malformed[0]),
        "0x2201, 0x32, 0x0, blob[0], 0x24, 0x0, 0x0, 0x0, blob[0], 0x0, blob[0]"
    );
}

#[test]
fn a_page_of_a_long_list_is_the_window_asked_for() {
    struct Many;
    impl Catalog for Many {
        fn list(&self, _: &Query) -> Vec<Row> {
            (1..=100).map(|id| Row::Track { id, position: 0 }).collect()
        }
        fn track_row(&self, id: u32, _: Option<TrackColumn>) -> Option<TrackRow> {
            Some(TrackRow {
                id,
                title: format!("Track {id}"),
                ..TrackRow::default()
            })
        }
        fn track(&self, _: u32) -> Option<TrackDetails> {
            None
        }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn analysis(&self, _: u32, _: &Analysis) -> Option<Vec<u8>> {
            None
        }
    }
    let handler = CatalogHandler::new(Arc::new(Many));
    let mut s = handler.open();
    let header = s.handle(&numbers(kind::TRACK_MENU, 1, &[CTX, 0]));
    assert_eq!(header[0].arguments[1], Argument::Number(100));
    // The deck pages 25 at a time: offset 50, limit 25.
    let page = s.handle(&numbers(kind::RENDER, 2, &[CTX, 50, 25, 0, 100, 0xc, 1, 0]));
    assert_eq!(page.len(), 27);
    assert_eq!(
        page[0].arguments,
        vec![Argument::Number(1), Argument::Number(50)]
    );
    assert_eq!(page[1].arguments[0], Argument::Number(51));
    assert_eq!(page[25].arguments[0], Argument::Number(75));
    // Beyond the end clamps to the final row.
    let past = s.handle(&numbers(
        kind::RENDER,
        3,
        &[CTX, 100, 25, 0, 100, 0xc, 1, 0],
    ));
    assert_eq!(past.len(), 3);
    assert_eq!(past[0].arguments[1], Argument::Number(99));
    assert_eq!(past[1].arguments[0], Argument::Number(100));

    // An overrun is right-aligned to preserve the requested page size.
    let overrun = s.handle(&numbers(kind::RENDER, 4, &[CTX, 95, 25, 0, 100, 0xc, 1, 0]));
    assert_eq!(overrun.len(), 27);
    assert_eq!(overrun[0].arguments[1], Argument::Number(75));
    assert_eq!(overrun[1].arguments[0], Argument::Number(76));
    assert_eq!(overrun[25].arguments[0], Argument::Number(100));

    // A zero count renders one row, while the maximum offset gets no reply.
    let zero = s.handle(&numbers(kind::RENDER, 5, &[CTX, 0, 0, 0, 100, 0xc, 1, 0]));
    assert_eq!(zero.len(), 3);
    assert_eq!(zero[1].arguments[0], Argument::Number(1));

    let maximum = s.handle(&numbers(
        kind::RENDER,
        6,
        &[CTX, u32::MAX, 1, 0, 100, 0xc, 1, 0],
    ));
    assert!(maximum.is_empty());
}

#[test]
fn the_setup_reply_and_the_unknown_requests_answer_as_rekordbox_does() {
    let handler = CatalogHandler::new(Arc::new(Small(false)));
    let mut s = handler.open();
    let reply = s.handle(&setup_request(1));
    assert_eq!(
        reply[0].encode(),
        hex("11872349ae11fffffffe1000000f021400000002060611000000111100000014")
    );
    let after = s.handle(&numbers(0x3007, 0x17e, &[0x0108_0301, 0]));
    assert_eq!(
        after[0].encode(),
        hex("11872349ae110000017e1040000f021400000002060611000030071100000000")
    );
    browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
    let loaded = s.handle(&numbers(kind::ITEM_POSITION, 0x18d, &[CTX, TRACK, 0, 1]));
    assert_eq!(
        loaded[0].encode(),
        hex("11872349ae110000018d1040000f021400000002060611000031001100000000")
    );
    let matching = s.handle(&numbers(0x1017, 0x3cf, &[0x0102_0301, 0, TRACK]));
    assert_eq!(args(&matching[0]), "0x1017, 0x1");
}

#[test]
fn tracks_are_sorted_the_way_the_player_asked() {
    // The sort id travels from the request into the query the catalog sees.
    struct Spy(std::sync::Mutex<Option<Query>>);
    impl Catalog for Spy {
        fn list(&self, q: &Query) -> Vec<Row> {
            *self.0.lock().unwrap() = Some(q.clone());
            vec![]
        }
        fn track_row(&self, _: u32, _: Option<TrackColumn>) -> Option<TrackRow> {
            None
        }
        fn track(&self, _: u32) -> Option<TrackDetails> {
            None
        }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn analysis(&self, _: u32, _: &Analysis) -> Option<Vec<u8>> {
            None
        }
    }
    let spy = Arc::new(Spy(std::sync::Mutex::new(None)));
    let handler = CatalogHandler::new(Arc::clone(&spy) as Arc<dyn Catalog>);
    let mut s = handler.open();
    s.handle(&numbers(kind::TRACK_MENU, 1, &[CTX, 4]));
    assert_eq!(
        spy.0.lock().unwrap().clone(),
        Some(Query::Tracks {
            scope: TrackScope::All,
            sort: Sort::Bpm
        })
    );
    s.handle(&numbers(kind::KEY_TRACKS, 2, &[CTX, 0xc, 3, 2]));
    assert_eq!(
        spy.0.lock().unwrap().clone(),
        Some(Query::Tracks {
            scope: TrackScope::Key {
                key: 3,
                distance: 2
            },
            sort: Sort::Key
        })
    );
    s.handle(&Message::new(
        3,
        kind::SEARCH,
        vec![
            Argument::Number(CTX),
            Argument::Number(0),
            Argument::Number(8),
            Argument::String("ACID".into()),
            Argument::Number(0),
        ],
    ));
    assert_eq!(
        spy.0.lock().unwrap().clone(),
        Some(Query::Tracks {
            scope: TrackScope::Search("ACID".into()),
            sort: Sort::Default
        })
    );
    s.handle(&Message::new(
        4,
        kind::SEARCH_TRACK,
        vec![
            Argument::Number(CTX),
            Argument::Number(0),
            Argument::Number(8),
            Argument::String("ABC".into()),
        ],
    ));
    assert_eq!(
        spy.0.lock().unwrap().clone(),
        Some(Query::Tracks {
            scope: TrackScope::Search("ABC".into()),
            sort: Sort::Default
        })
    );
}

#[test]
fn the_extended_cue_reply_counts_its_entries_not_a_header_word() {
    // The 2b04 blob is entries concatenated, each led by its own little-endian
    // byte length; the reply's trailing argument is how many there are. A CDJ
    // reads that count to size its cue table, so it must be the entry count,
    // not the word at a fixed offset — which is a cue's own fields (here a
    // deceptively large 0xffff) and once made a deck fault on 65 535 cues.
    struct Cued;
    impl Catalog for Cued {
        fn list(&self, _: &Query) -> Vec<Row> {
            Vec::new()
        }
        fn track_row(&self, _: u32, _: Option<TrackColumn>) -> Option<TrackRow> {
            None
        }
        fn track(&self, _: u32) -> Option<TrackDetails> {
            None
        }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn analysis(&self, _: u32, what: &Analysis) -> Option<Vec<u8>> {
            match what {
                Analysis::ExtendedCueList => {
                    // Three 16-byte entries. Byte 4 of the first is 0xffff, the
                    // value the old code mistook for the count.
                    let mut blob = Vec::new();
                    for _ in 0..3 {
                        let mut e = vec![0_u8; 16];
                        e[0..4].copy_from_slice(&16_u32.to_le_bytes());
                        e[4..6].copy_from_slice(&0xffff_u16.to_le_bytes());
                        blob.extend_from_slice(&e);
                    }
                    Some(blob)
                }
                _ => None,
            }
        }
    }
    let handler = CatalogHandler::new(Arc::new(Cued));
    let mut s = handler.open();
    s.handle(&setup_request(1));
    let reply = s.handle(&numbers(
        kind::EXTENDED_CUES,
        0x1c0,
        &[0x0108_0301, TRACK, 0],
    ));
    assert_eq!(args(&reply[0]), "0x2b04, 0x0, 0x30, blob[48], 0x3");
}

#[test]
fn item_position_restores_categories_and_does_not_mark_tracks_played() {
    let mut s = session();
    browse(&mut s, kind::PLAYLIST_MENU, &[CTX, 0, 0, 1]);
    let response = s.handle(&numbers(kind::ITEM_POSITION, 4, &[CTX, 0xffb7_d23b, 0, 1]));
    assert_eq!(response[0].arguments[1], Argument::Number(1));
    let absent = s.handle(&numbers(kind::ITEM_POSITION, 5, &[CTX, 123, 0, 1]));
    assert_eq!(absent[0].arguments[1], Argument::Number(u32::MAX));
    let page = s.handle(&numbers(kind::RENDER, 6, &[CTX, 1, 1]));
    assert_eq!(page[1].arguments[1], Argument::Number(0xffb7_d23b));
}

#[test]
fn background_metadata_does_not_replace_the_browser_menu() {
    let mut s = session();
    browse(&mut s, kind::PLAYLIST_MENU, &[CTX, 0, 0, 1]);
    s.handle(&numbers(kind::METADATA, 2, &[0x0102_0301, TRACK]));
    let page = s.handle(&numbers(kind::RENDER, 3, &[CTX, 1, 1]));
    assert_eq!(page[1].arguments[1], Argument::Number(0xffb7_d23b));
    let info = s.handle(&numbers(kind::RENDER, 4, &[0x0102_0301, 0, 16]));
    assert_eq!(info.len(), 18);
}

#[test]
fn legacy_client_gets_a_bare_title_in_twelve_field_rows() {
    let mut s = session();
    let reply = s.handle(&numbers(kind::SETUP, 0xffff_fffe, &[5]));
    assert_eq!(
        reply[0].encode(),
        hex("11872349ae11fffffffe1040000f021400000002060611000000001100000011")
    );
    let (_, rows) = browse(&mut s, kind::METADATA, &[CTX, TRACK]);
    assert!(rows.iter().all(|row| row.arguments.len() == 12));
    assert_eq!(
        rows[0].arguments[6],
        Argument::Number(rbl_dbserver::item::item_type::TITLE)
    );
    assert_eq!(rows[0].arguments[3], Argument::String(the_track().title));
    assert_eq!(rows[6].arguments[1], Argument::Number(0x1814_5d65));
    assert_eq!(rows[6].arguments[3], Argument::String("11B".into()));
    assert_eq!(
        rows[6].arguments[6],
        Argument::Number(rbl_dbserver::item::item_type::KEY)
    );
    assert_eq!(rows[0].arguments[5], Argument::String(String::new()));
}

#[test]
fn filter_properties_use_the_expected_reply_and_do_not_clobber_browsing() {
    let mut s = session();
    browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
    let reply = s.handle(&numbers(0x3107, 3, &[0x0108_0301]));
    assert_eq!(reply[0].kind, 0x4004);
    assert_eq!(reply[0].arguments[1], Argument::Number(0));
    assert_eq!(reply[0].arguments[4], Argument::Number(4));
    let page = s.handle(&numbers(kind::RENDER, 4, &[CTX, 0, 1]));
    assert_eq!(page[1].arguments[1], Argument::Number(TRACK));
    // Select a BPM range excluding the fixture, then toggle filtering.
    let mut bytes = vec![1, 6, 2, 0];
    bytes.extend_from_slice(&12000_u32.to_le_bytes());
    bytes.extend_from_slice(&13000_u32.to_le_bytes());
    let update = Message::new(
        5,
        0x3207,
        vec![
            Argument::Number(CTX),
            Argument::Number(6),
            Argument::Number(0),
            Argument::Number(12),
            Argument::Blob(bytes),
        ],
    );
    assert_eq!(s.handle(&update)[0].arguments[1], Argument::Number(0));
    s.handle(&numbers(0x3007, 6, &[CTX, 1]));
    assert_eq!(browse(&mut s, kind::TRACK_MENU, &[CTX, 0]).0, 0);
    s.handle(&numbers(0x3007, 7, &[CTX, 0]));
    assert_eq!(browse(&mut s, kind::TRACK_MENU, &[CTX, 0]).0, 1);
}

#[test]
fn read_only_catalog_refuses_tag_and_rating_edits() {
    let mut s = session();
    for (kind, args) in [
        (0x3002, vec![CTX, TRACK, 1]),
        (0x3202, vec![CTX]),
        (0x2107, vec![CTX, TRACK, 5]),
    ] {
        assert_eq!(
            s.handle(&numbers(kind, 1, &args))[0].arguments[1],
            Argument::Number(1)
        );
    }
}

#[test]
fn tag_and_rating_edits_are_shared_and_acknowledged_after_the_catalog_changes() {
    use rbl_dbserver::catalog::Edit;
    #[derive(Default)]
    struct Editable(std::sync::Mutex<(Vec<u32>, u32, i16)>);
    impl Catalog for Editable {
        fn list(&self, q: &Query) -> Vec<Row> {
            if matches!(
                q,
                Query::Tracks {
                    scope: TrackScope::TagList,
                    ..
                }
            ) {
                return self
                    .0
                    .lock()
                    .unwrap()
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, &id)| Row::Track {
                        id,
                        position: i as u32 + 1,
                    })
                    .collect();
            }
            Small(false).list(q)
        }
        fn track_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
            Small(false).track_row(id, column)
        }
        fn track(&self, id: u32) -> Option<TrackDetails> {
            Small(false).track(id).map(|mut track| {
                track.rating = self.0.lock().unwrap().1;
                track
            })
        }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn analysis(&self, _: u32, _: &Analysis) -> Option<Vec<u8>> {
            None
        }
        fn tagged(&self, id: u32) -> bool {
            self.0.lock().unwrap().0.contains(&id)
        }
        fn grid_offset(&self, _: u32) -> i16 {
            self.0.lock().unwrap().2
        }
        fn edit(&self, edit: &Edit) -> bool {
            let mut state = self.0.lock().unwrap();
            match *edit {
                Edit::Tag { track, add: true } => {
                    if !state.0.contains(&track) {
                        state.0.push(track);
                    }
                }
                Edit::Tag { track, add: false } => state.0.retain(|&id| id != track),
                Edit::GridOffset { offset_ms, .. } => state.2 = offset_ms,
                Edit::ClearTags => state.0.clear(),
                Edit::Rating { stars, .. } => state.1 = u32::from(stars),
                Edit::HotCueBankCue { .. } => return false,
                Edit::HistoryAdd { .. }
                | Edit::HistoryRemove { .. }
                | Edit::HistoryDelete { .. }
                | Edit::SaveCue { .. }
                | Edit::DeleteCue { .. } => return false,
            }
            true
        }
    }
    let handler = CatalogHandler::new(Arc::new(Editable::default()));
    let mut one = handler.open();
    let mut two = handler.open();
    let added = one.handle(&numbers(0x3002, 1, &[CTX, TRACK, 1]));
    assert_eq!(added[0].arguments[1], Argument::Number(0));
    let (count, rows) = browse(&mut two, 0x100f, &[CTX, 0]);
    assert_eq!(count, 1);
    assert_eq!(rows[0].arguments[7], Argument::Number(0x0100_0001));
    let (_, metadata) = browse(&mut two, kind::METADATA, &[0x0102_0301, TRACK]);
    assert_eq!(
        metadata[0].arguments[7],
        Argument::Number(1),
        "loaded-track metadata must report tag membership too"
    );
    one.handle(&numbers(0x3002, 2, &[CTX, TRACK, 1]));
    assert_eq!(browse(&mut two, 0x100f, &[CTX, 0]).0, 1);
    one.handle(&numbers(0x2107, 3, &[CTX, TRACK, 4]));
    let (_, rows) = browse(&mut two, kind::METADATA, &[CTX, TRACK]);
    let rating = rows
        .iter()
        .find(|row| row.arguments[6] == Argument::Number(10))
        .unwrap();
    assert_eq!(rating.arguments[1], Argument::Number(4));
    assert_eq!(
        one.handle(&numbers(0x2107, 4, &[CTX, TRACK, 6]))[0].arguments[1],
        Argument::Number(1)
    );
    let saved = one.handle(&numbers(
        kind::SAVE_GRID_OFFSET,
        5,
        &[CTX, TRACK, 0xffff_fe2d],
    ));
    assert_eq!(saved[0].arguments[1], Argument::Number(0));
    let read = two.handle(&numbers(kind::GRID_OFFSET, 6, &[CTX, TRACK]));
    assert_eq!(read[0].arguments[1], Argument::Number(0xfe2d));
    one.handle(&numbers(0x3002, 7, &[CTX, TRACK, 0]));
    assert_eq!(browse(&mut two, 0x100f, &[CTX, 0]).0, 0);
    let (_, metadata) = browse(&mut two, kind::METADATA, &[0x0102_0301, TRACK]);
    assert_eq!(metadata[0].arguments[7], Argument::Number(0));
}

#[test]
fn history_commands_reach_the_catalog_and_only_the_removal_is_answered() {
    use rbl_dbserver::catalog::Edit;
    #[derive(Default)]
    struct Recording(std::sync::Mutex<Vec<Edit>>);
    impl Catalog for Recording {
        fn list(&self, q: &Query) -> Vec<Row> {
            Small(false).list(q)
        }
        fn track_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
            Small(false).track_row(id, column)
        }
        fn track(&self, id: u32) -> Option<TrackDetails> {
            Small(false).track(id)
        }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> {
            None
        }
        fn analysis(&self, _: u32, _: &Analysis) -> Option<Vec<u8>> {
            None
        }
        fn edit(&self, edit: &Edit) -> bool {
            self.0.lock().unwrap().push(edit.clone());
            !matches!(edit, Edit::HistoryRemove { track: 7 })
        }
    }
    let catalog = Arc::new(Recording::default());
    let mut s = CatalogHandler::new(Arc::clone(&catalog) as Arc<dyn Catalog>).open();
    browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
    // Sent and forgotten: no reply, and the menu being browsed stays.
    assert!(s
        .handle(&numbers(kind::INSERT_HISTORY, 2, &[CTX, TRACK]))
        .is_empty());
    let rendered = s.handle(&numbers(kind::RENDER, 3, &[CTX, 0, 1]));
    assert_eq!(rendered[1].arguments[1], Argument::Number(TRACK));
    assert!(s
        .handle(&numbers(kind::DELETE_HISTORY, 4, &[CTX, 0xffff_ffff]))
        .is_empty());
    // The RX3 changes on-air state without waiting for a response. Its
    // command must likewise leave the active browse menu intact.
    assert!(s
        .handle(&numbers(kind::SET_ON_AIR, 4, &[CTX, TRACK]))
        .is_empty());
    assert_eq!(
        s.handle(&numbers(kind::DELETE_HISTORY_TRACK, 5, &[CTX, TRACK]))[0].arguments,
        vec![Argument::Number(0x3401), Argument::Number(0)]
    );
    assert_eq!(
        s.handle(&numbers(kind::DELETE_HISTORY_TRACK, 6, &[CTX, 7]))[0].arguments,
        vec![Argument::Number(0x3401), Argument::Number(0xffff_ffff)]
    );
    // Not a rekordbox track (type 2, a USB track's): left alone, as rekordbox does.
    assert!(s
        .handle(&numbers(kind::INSERT_HISTORY, 7, &[0x0101_0302, TRACK]))
        .is_empty());
    assert_eq!(
        *catalog.0.lock().unwrap(),
        vec![
            Edit::HistoryAdd { track: TRACK },
            Edit::HistoryDelete {
                history: 0xffff_ffff
            },
            Edit::HistoryRemove { track: TRACK },
            Edit::HistoryRemove { track: 7 },
        ]
    );
}

#[test]
fn rx3_browse_type_fallback_is_database_backed_media() {
    let mut s = session();
    assert_eq!(
        s.handle(&numbers(kind::BROWSE_TYPE, 0x51, &[CTX, 2]))[0].arguments,
        vec![Argument::Number(0x3303), Argument::Number(1)]
    );
}

#[test]
fn rx3_scalar_track_and_mobile_queries_use_their_native_reply_shapes() {
    let mut idle = session();
    assert_eq!(
        idle.handle(&numbers(kind::TRACK_BPM, 0x52, &[CTX, TRACK]))[0].arguments,
        vec![Argument::Number(0x3008), Argument::Number(0x1e80)]
    );
    assert_eq!(
        idle.handle(&numbers(kind::TRACK_PLAY_STATE, 0x53, &[CTX, TRACK]))[0].arguments,
        vec![Argument::Number(0x3b03), Argument::Number(0)]
    );
    assert_eq!(
        idle.handle(&numbers(kind::LEGACY_KEY_TO_NEW_KEY, 0x56, &[CTX, 0x14]))[0].arguments,
        vec![Argument::Number(0x3a03), Argument::Number(0x14)]
    );
    assert_eq!(
        idle.handle(&numbers(kind::LEGACY_KEY_TO_NEW_KEY, 0x57, &[CTX, 25]))[0].arguments,
        vec![Argument::Number(0x3a03), Argument::Number(0)]
    );
    assert_eq!(
        idle.handle(&numbers(kind::CONTENT_NEW_KEY, 0x58, &[CTX, TRACK]))[0].arguments,
        vec![Argument::Number(0x3d03), Argument::Number(0x14)]
    );

    let mut loaded = session_with(Small(true));
    assert_eq!(
        loaded.handle(&numbers(kind::TRACK_PLAY_STATE, 0x54, &[CTX, TRACK]))[0].arguments,
        vec![Argument::Number(0x3b03), Argument::Number(0)]
    );
    let mobile = loaded.handle(&numbers(kind::REKORDBOX_MOBILE, 0x55, &[CTX]));
    assert_eq!(mobile[0].kind, kind::REKORDBOX_MOBILE_REPLY);
    assert_eq!(
        mobile[0].arguments,
        vec![
            Argument::Number(0),
            Argument::Number(2),
            Argument::String(String::new()),
        ]
    );
}

#[test]
fn rx3_content_tracks_is_the_sorted_all_tracks_alias() {
    let mut s = session();
    let regular = browse(&mut s, kind::TRACK_MENU, &[CTX, 4]);
    let content = browse(&mut s, kind::CONTENT_TRACKS, &[CTX, 4]);
    assert_eq!(content, regular);
}

#[test]
fn grid_offset_writes_do_not_claim_success_without_persistence() {
    let mut s = session();
    browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
    assert_eq!(
        s.handle(&numbers(kind::SAVE_GRID_OFFSET, 8, &[CTX, TRACK, 234]))[0].arguments,
        vec![Argument::Number(0x2605), Argument::Number(1)]
    );
    assert_eq!(
        s.handle(&numbers(kind::GRID_OFFSET, 9, &[CTX, TRACK]))[0].arguments,
        vec![Argument::Number(0x2804), Argument::Number(0)]
    );
    // An edit acknowledgement must not replace the browser's pending menu.
    let rendered = s.handle(&numbers(kind::RENDER, 10, &[CTX, 0, 1]));
    assert_eq!(rendered[1].arguments[1], Argument::Number(TRACK));
}

#[test]
fn unsupported_command_keeps_the_active_menu_and_echoes_its_kind_in_4003() {
    let mut s = session();
    let (count, expected) = browse(&mut s, kind::ROOT_MENU, &[CTX, 0, 0x5cf_ffff]);

    let unsupported = s.handle(&numbers(0x2fff, 0x8123, &[CTX, 0]));
    assert_eq!(
        unsupported,
        vec![Message::new(
            0x8123,
            kind::ERROR,
            vec![Argument::Number(0x2fff)],
        )]
    );

    let rendered = s.handle(&numbers(
        kind::RENDER,
        0x8124,
        &[CTX, 0, count, 0, count, 0xc, 1, 0],
    ));
    assert_eq!(rendered[0].kind, kind::RENDER_HEADER);
    let items = &rendered[1..rendered.len() - 1];
    assert_eq!(items.len(), expected.len());
    for (actual, expected) in items.iter().zip(expected) {
        assert_eq!(actual.kind, expected.kind);
        assert_eq!(actual.arguments, expected.arguments);
    }
    assert_eq!(
        rendered.last().map(|message| message.kind),
        Some(kind::MENU_FOOTER)
    );
}

/// Pass complete wire requests through the codec before the session dispatcher.
/// The synthetic unsupported-command fixtures use V4's source-proven envelope;
/// they are not a vendor capture of a player's reaction to that error.
fn exchange_wire(s: &mut Box<dyn Session>, request: &[u8]) -> Vec<Vec<u8>> {
    let (message, used) = Message::decode(request).unwrap();
    assert_eq!(used, request.len());
    assert_eq!(message.encode(), request);
    s.handle(&message).iter().map(Message::encode).collect()
}

#[test]
fn unsupported_command_wire_preserves_both_menu_locations_and_setup_modes() {
    const SECOND: u32 = 0x0102_0301;
    for (setup, reply, width) in [
        (
            "11872349ae11fffffffe1000000f011400000001061100000005",
            "11872349ae11fffffffe1040000f021400000002060611000000001100000011",
            12,
        ),
        (
            "11872349ae11fffffffe1000000f021400000002060611000000051100000014",
            "11872349ae11fffffffe1000000f021400000002060611000000111100000014",
            16,
        ),
    ] {
        let handler = CatalogHandler::new(Arc::new(Small(false)));
        let mut s = handler.open();
        assert_eq!(exchange_wire(&mut s, &hex(setup)), vec![hex(reply)]);
        // Keep different menus pending at the same time: an error must neither
        // replace its own location nor change the other location's rows.
        let root = numbers(kind::ROOT_MENU, 0x8100, &[CTX, 0, 0x5cf_ffff]);
        let tracks = numbers(kind::TRACK_MENU, 0x8101, &[SECOND, 0]);
        assert_eq!(
            exchange_wire(&mut s, &root.encode()),
            vec![hex("11872349ae11000081001040000f021400000002060611000010001100000009")]
        );
        assert_eq!(
            exchange_wire(&mut s, &tracks.encode()),
            vec![hex("11872349ae11000081011040000f021400000002060611000010041100000001")]
        );
        let renders = [
            numbers(kind::RENDER, 0x8124, &[CTX, 0, 9, 0, 9, 0xc, 1, 0]),
            numbers(kind::RENDER, 0x8124, &[SECOND, 0, 1, 0, 1, 0xc, 1, 0]),
        ];
        let before: Vec<_> = renders
            .iter()
            .map(|request| exchange_wire(&mut s, &request.encode()))
            .collect();
        assert_eq!(before[0].len(), 11);
        assert_eq!(before[1].len(), 3);
        for rows in &before {
            for row in &rows[1..rows.len() - 1] {
                assert_eq!(Message::decode(row).unwrap().0.arguments.len(), width);
            }
        }
        assert_eq!(
            Message::decode(&before[1][1]).unwrap().0.arguments[1],
            Argument::Number(TRACK)
        );
        // Same-location, other-location, foreign-location, and no-context
        // decoded unknown requests all have the same one-argument error.
        for request in [
            "11872349ae1100008123102fff0f021400000002060611010103011100000000",
            "11872349ae1100008123102fff0f021400000002060611010203011100000000",
            "11872349ae1100008123102fff0f021400000002060611010303011100000000",
            "11872349ae1100008123102fff0f001400000000",
        ] {
            assert_eq!(
                exchange_wire(&mut s, &hex(request)),
                vec![hex("11872349ae11000081231040030f011400000001061100002fff")]
            );
            for (render, expected) in renders.iter().zip(&before) {
                assert_eq!(exchange_wire(&mut s, &render.encode()), *expected);
            }
        }
        // Supported mobile queries and intentional silence must stay outside
        // the unknown-command route and leave both pending menus intact.
        assert_eq!(
            exchange_wire(
                &mut s,
                &hex("11872349ae1100008125103e030f011400000001061101010301"),
            ),
            vec![hex("11872349ae1100008125104b020f0314000000030606021100000000110000000226000000010000")]
        );
        for command in [kind::TEARDOWN, kind::SET_ON_AIR] {
            assert!(exchange_wire(&mut s, &numbers(command, 0x8126, &[CTX, 1]).encode()).is_empty());
        }
        for (render, expected) in renders.iter().zip(&before) {
            assert_eq!(exchange_wire(&mut s, &render.encode()), *expected);
        }
    }
}

#[test]
fn unsupported_command_keeps_filter_selection_and_enable_state() {
    let mut s = session();
    let mut property = vec![1, 6, 2, 0];
    property.extend_from_slice(&12_000_u32.to_le_bytes());
    property.extend_from_slice(&13_000_u32.to_le_bytes());
    let update = Message::new(
        0x8100,
        kind::FILTER_SET,
        vec![
            Argument::Number(CTX),
            Argument::Number(6),
            Argument::Number(0),
            Argument::Number(12),
            Argument::Blob(property),
        ],
    );
    exchange_wire(&mut s, &update.encode());
    exchange_wire(&mut s, &numbers(0x3007, 0x8101, &[CTX, 1]).encode());
    assert_eq!(browse(&mut s, kind::TRACK_MENU, &[CTX, 0]).0, 0);
    assert_eq!(
        exchange_wire(&mut s, &hex("11872349ae1100008123102fff0f011400000001061101010301")),
        vec![hex("11872349ae11000081231040030f011400000001061100002fff")]
    );
    assert_eq!(browse(&mut s, kind::TRACK_MENU, &[CTX, 0]).0, 0);
    exchange_wire(&mut s, &numbers(0x3007, 0x8102, &[CTX, 0]).encode());
    assert_eq!(browse(&mut s, kind::TRACK_MENU, &[CTX, 0]).0, 1);
    exchange_wire(&mut s, &numbers(0x3007, 0x8103, &[CTX, 1]).encode());
    assert_eq!(browse(&mut s, kind::TRACK_MENU, &[CTX, 0]).0, 0);
}

#[test]
fn played_state_wire_uses_context_four_and_preserves_pending_menu() {
    for played in [false, true] {
        let mut s = session_with(Small(played));
        browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
        let render = numbers(kind::RENDER, 0x8100, &[CTX, 0, 1]);
        let before = exchange_wire(&mut s, &render.encode());
        for location in [1, 2, 8] {
            for context in [0_u32, 1, 2, 3, 4, 5, 255] {
                let packed = (CTX & 0xff00_00ff) | (location << 16) | (context << 8);
                for track in [TRACK, 0, u32::MAX] {
                    let request = numbers(kind::TRACK_PLAY_STATE, 0x8130, &[packed, track]);
                    let scalar = if played && track == TRACK && context == 4 { "00000002" } else { "00000000" };
                    assert_eq!(exchange_wire(&mut s, &request.encode()),
                        vec![hex(&format!("11872349ae11000081301040000f02140000000206061100003b0311{scalar}"))]);
                    assert_eq!(exchange_wire(&mut s, &render.encode()), before);
                }
            }
        }
        for arguments in [vec![], vec![Argument::Number(0x0101_0401)],
            vec![Argument::String("context".into()), Argument::Number(TRACK)],
            vec![Argument::Number(0x0101_0401), Argument::String("track".into())]] {
            assert_eq!(exchange_wire(&mut s, &Message::new(0x8130, kind::TRACK_PLAY_STATE, arguments).encode()),
                vec![hex("11872349ae11000081301040000f02140000000206061100003b031100000000")]);
        }
        assert_eq!(exchange_wire(&mut s, &render.encode()), before);
    }
}

#[test]
fn filter_set_failure_is_32_and_keeps_existing_conditions_and_menu() {
    let mut s = session();
    browse(&mut s, kind::TRACK_MENU, &[CTX, 0]);
    let render = numbers(kind::RENDER, 0x8100, &[CTX, 0, 1]);
    let menu = exchange_wire(&mut s, &render.encode());
    let get = numbers(kind::FILTER_GET, 0x8101, &[CTX]);
    let before = exchange_wire(&mut s, &get.encode());
    let mut blob = vec![1, 6, 2, 0];
    blob.extend_from_slice(&12_000_u32.to_le_bytes());
    blob.extend_from_slice(&13_000_u32.to_le_bytes());
    let valid = vec![Argument::Number(CTX), Argument::Number(6), Argument::Number(0),
        Argument::Number(12), Argument::Blob(blob)];
    let mut invalid: Vec<Vec<Argument>> = vec![vec![]];
    for (index, value) in [(0, Argument::String("context".into())), (1, Argument::Number(99)), (2, Argument::Number(1)),
        (2, Argument::String("reserved".into())), (3, Argument::Number(3)),
        (3, Argument::Number(11)), (4, Argument::Blob(vec![1,6,2,0])),
        (4, Argument::String("blob".into()))] {
        let mut arguments = valid.clone();
        arguments[index] = value;
        invalid.push(arguments);
    }
    for arguments in invalid {
        let request = Message::new(0x8131, kind::FILTER_SET, arguments);
        assert_eq!(exchange_wire(&mut s, &request.encode()),
            vec![hex("11872349ae11000081311040000f021400000002060611000032071100000032")]);
        assert_eq!(exchange_wire(&mut s, &get.encode()), before);
        assert_eq!(exchange_wire(&mut s, &render.encode()), menu);
    }
    assert_eq!(exchange_wire(&mut s, &Message::new(0x8131, kind::FILTER_SET, valid).encode()),
        vec![hex("11872349ae11000081311040000f021400000002060611000032071100000000")]);
    assert_ne!(exchange_wire(&mut s, &get.encode()), before);
    assert_eq!(exchange_wire(&mut s, &render.encode()), menu);
    // Preserve the currently accepted typed foreign-context path without
    // inventing requester ownership or reconnect persistence semantics.
    let foreign = vec![Argument::Number(0x0208_0302), Argument::Number(6), Argument::Number(0),
        Argument::Number(4), Argument::Blob(vec![0, 0, 0, 0])];
    assert_eq!(exchange_wire(&mut s, &Message::new(0x8131, kind::FILTER_SET, foreign).encode()),
        vec![hex("11872349ae11000081311040000f021400000002060611000032071100000000")]);
}

#[test]
fn scalar_and_silent_notices_preserve_menus_for_both_setup_modes() {
    for extended in [false, true] {
        let handler = CatalogHandler::new(Arc::new(Small(false)));
        let mut s = handler.open();
        let setup = if extended { vec![5,20] } else { vec![5] };
        exchange_wire(&mut s, &numbers(kind::SETUP, 0xffff_fffe, &setup).encode());
        browse(&mut s, kind::ROOT_MENU, &[CTX, 0, 0x5cf_ffff]);
        let render = numbers(kind::RENDER, 0x8100, &[CTX, 0, 9]);
        let before = exchange_wire(&mut s, &render.encode());
        for command in [0x3402, 0x3903] {
            assert_eq!(exchange_wire(&mut s, &numbers(command, 0x8132, &[CTX]).encode()),
                vec![hex(&format!("11872349ae11000081321040000f0214000000020606110000{command:04x}1100000000"))]);
            assert_eq!(exchange_wire(&mut s, &render.encode()), before);
        }
        for command in [0x3203, 0x3503, kind::DELETE_HISTORY_TRACK] {
            // Foreign history context is intentionally silent and must not
            // turn into a generic error or affect a pending menu.
            assert!(exchange_wire(&mut s, &numbers(command, 0x8132, &[0x0101_0302, TRACK]).encode()).is_empty());
            assert_eq!(exchange_wire(&mut s, &render.encode()), before);
        }
    }
}

#[test]
fn my_setting_flag_scalar_masks_the_low_byte_and_preserves_both_menus() {
    const SECOND: u32 = 0x0102_0301;
    for extended in [false, true] {
        let handler = CatalogHandler::new(Arc::new(Small(false)));
        let mut s = handler.open();
        let setup = if extended { vec![5,20] } else { vec![5] };
        exchange_wire(&mut s, &numbers(kind::SETUP, 0xffff_fffe, &setup).encode());
        exchange_wire(&mut s, &numbers(kind::ROOT_MENU, 0x8100, &[CTX, 0, 0x5cf_ffff]).encode());
        exchange_wire(&mut s, &numbers(kind::TRACK_MENU, 0x8101, &[SECOND, 0]).encode());
        let renders = [numbers(kind::RENDER, 0x8102, &[CTX, 0, 9]),
            numbers(kind::RENDER, 0x8103, &[SECOND, 0, 1])];
        let before: Vec<_> = renders.iter().map(|request| exchange_wire(&mut s, &request.encode())).collect();
        assert_eq!(before[0].len(), 11);
        assert_eq!(before[1].len(), 3);
        // V4's case has no context condition. A typed foreign context remains
        // a scalar query, unlike the intentional foreign-history silence.
        for context in [CTX, SECOND, 0x0208_0302] {
            for (value, scalar) in [(0, 1), (1, 1), (2, 0), (255, 0),
                (256, 1), (257, 1), (0x10000, 1)] {
                let request = numbers(0x3c03, 0x8133, &[context, value]);
                assert_eq!(exchange_wire(&mut s, &request.encode()),
                    vec![hex(&format!("11872349ae11000081331040000f02140000000206061100003c03110000000{scalar}"))]);
                for (render, expected) in renders.iter().zip(&before) {
                    assert_eq!(exchange_wire(&mut s, &render.encode()), *expected);
                }
            }
        }
        for arguments in [vec![], vec![Argument::Number(CTX)],
            vec![Argument::String("context".into()), Argument::Number(0)],
            vec![Argument::Number(CTX), Argument::String("value".into())],
            vec![Argument::Number(CTX), Argument::Blob(vec![0])]] {
            let request = Message::new(0x8133, 0x3c03, arguments);
            assert_eq!(exchange_wire(&mut s, &request.encode()),
                vec![hex("11872349ae11000081331040030f011400000001061100003c03")]);
            for (render, expected) in renders.iter().zip(&before) {
                assert_eq!(exchange_wire(&mut s, &render.encode()), *expected);
            }
        }
    }
}

#[derive(Default)]
struct AnalysisBoundaryCatalog {
    edits: std::sync::atomic::AtomicUsize,
    reads: std::sync::atomic::AtomicUsize,
}

impl Catalog for AnalysisBoundaryCatalog {
    fn list(&self, query: &Query) -> Vec<Row> { Small(false).list(query) }
    fn track_row(&self, id: u32, column: Option<TrackColumn>) -> Option<TrackRow> {
        Small(false).track_row(id, column)
    }
    fn track(&self, id: u32) -> Option<TrackDetails> { Small(false).track(id) }
    fn artwork(&self, id: u32) -> Option<Vec<u8>> { Small(false).artwork(id) }
    fn item_artwork(&self, id: u32) -> Option<Vec<u8>> { Small(false).item_artwork(id) }
    fn analysis(&self, track: u32, what: &Analysis) -> Option<Vec<u8>> {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (track == TRACK && matches!(what, Analysis::Vbr)).then(|| vec![0; 1604])
    }
    fn edit(&self, _edit: &Edit) -> bool {
        self.edits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        false
    }
}

fn atom_write(command: u16, atom: &[u8; 4], extension: &[u8; 4], reserved: u32, length: u32) -> Message {
    let mut request = numbers(command, 0x8134, &[CTX, TRACK, u32::from_le_bytes(*atom),
        u32::from_le_bytes(*extension), reserved, length]);
    if command == 0x2905 { request.arguments.push(Argument::Number(u32::from(length != 0))); }
    request.arguments.push(Argument::Blob(vec![0xaa; length as usize]));
    request
}

#[test]
fn analysis_write_early_refusals_preserve_menus_without_storage_calls() {
    use std::sync::atomic::Ordering;
    const SECOND: u32 = 0x0102_0301;
    for extended in [false, true] {
        let catalog = Arc::new(AnalysisBoundaryCatalog::default());
        let handler = CatalogHandler::new(catalog.clone());
        let mut s = handler.open();
        let setup = if extended { vec![5,20] } else { vec![5] };
        exchange_wire(&mut s, &numbers(kind::SETUP, 0xffff_fffe, &setup).encode());
        exchange_wire(&mut s, &numbers(kind::ROOT_MENU, 0x8100, &[CTX, 0, 0x5cf_ffff]).encode());
        exchange_wire(&mut s, &numbers(kind::TRACK_MENU, 0x8101, &[SECOND, 0]).encode());
        let renders = [numbers(kind::RENDER, 0x8102, &[CTX, 0, 9]),
            numbers(kind::RENDER, 0x8103, &[SECOND, 0, 1])];
        let before: Vec<_> = renders.iter().map(|request| exchange_wire(&mut s, &request.encode())).collect();
        assert_eq!(before[0].len(), 11);
        assert_eq!(before[1].len(), 3);

        let mut cases = Vec::new();
        for command in [0x2805, 0x2905] {
            for length in [0, 1, 11, 12] {
                cases.push((atom_write(command, b"PQT2", b"EXT\0", 0, length),
                    if command == 0x2805 { (1..12).contains(&length) } else { length < 12 }));
            }
            for (atom, extension, reserved) in [
                (*b"PWV4", *b"EXT\0", 0), (*b"PQT2", *b"2EX\0", 0),
                (*b"PQT2", *b"EXTX", 0), (*b"PQT2", [0; 4], 0),
                (*b"PQT2", *b"EXT\0", 1),
            ] {
                cases.push((atom_write(command, &atom, &extension, reserved, 12), true));
            }
            for atom in [b"PVB2", b"PQT2", b"PQTZ"] {
                for extension in [b"DAT\0", b"EXT\0", b"ext\0"] {
                    cases.push((atom_write(command, atom, extension, 0, 12),
                        command == 0x2905 && (atom != b"PQT2" || extension == b"DAT\0")));
                }
            }
        }
        // A `2705` shorter than 0x38 is refused with 0x32. One of 0x38
        // whose hot cue slot (0xaaaa) is past H writes nothing and answers
        // with the track's cue list, which this catalog cannot read: the
        // same 0x32 envelope, after one read.
        for length in [0, 1, 55, 56] {
            let mut request = numbers(0x2705, 0x8134, &[CTX, TRACK, 0, length]);
            request.arguments.push(Argument::Blob(vec![0xaa; length as usize]));
            cases.push((request, true));
        }
        // OnWriteCmd and these early guards have no foreign-context gate.
        for context in [CTX, SECOND, 0x0208_0302] {
            for (request, refusal) in &cases {
                let mut request = request.clone();
                request.arguments[0] = Argument::Number(context);
                let expected = if !refusal {
                    hex(&format!("11872349ae11000081341040030f01140000000106110000{:04x}", request.kind))
                } else if request.kind == 0x2705 {
                    // Empty blob is declared but omitted after zero length.
                    hex("11872349ae1100008134104e020f05140000000506060603061100002705110000003211000000001100000000")
                } else {
                    hex(&format!("11872349ae11000081341040000f0214000000020606110000{:04x}1100000032", request.kind))
                };
                assert_eq!(exchange_wire(&mut s, &request.encode()), vec![expected], "{request:?}");
                for (render, expected) in renders.iter().zip(&before) {
                    assert_eq!(exchange_wire(&mut s, &render.encode()), *expected);
                }
            }
        }
        // RBX malformed-shape safety policy, not vendor refusal parity:
        // wrong-typed fields and missing blobs remain unsupported (4003),
        // except on a cue save, where a 4003 sends a CDJ-3000 into an
        // endless resend: that is refused with the 0x32 cue envelope.
        for command in [0x2705, 0x2805, 0x2905] {
            let valid = if command == 0x2705 {
                let mut request = numbers(command, 0x8134, &[CTX, TRACK, 0, 55]);
                request.arguments.push(Argument::Blob(vec![0xaa; 55]));
                request
            } else { atom_write(command, b"PQT2", b"EXT\0", 0, 11) };
            let mut malformed = vec![vec![], valid.arguments[..valid.arguments.len()-1].to_vec()];
            for index in 0..valid.arguments.len() {
                let mut arguments = valid.arguments.clone();
                arguments[index] = Argument::String("wrong type".into());
                malformed.push(arguments);
            }
            for arguments in malformed {
                let expected = if command == 0x2705 {
                    hex("11872349ae1100008134104e020f05140000000506060603061100002705110000003211000000001100000000")
                } else {
                    hex(&format!("11872349ae11000081341040030f01140000000106110000{command:04x}"))
                };
                assert_eq!(exchange_wire(&mut s, &Message::new(0x8134, command, arguments).encode()),
                    vec![expected]);
                for (render, expected) in renders.iter().zip(&before) {
                    assert_eq!(exchange_wire(&mut s, &render.encode()), *expected);
                }
            }
        }
        assert_eq!(catalog.edits.load(Ordering::Relaxed), 0);
        // The three 0x38-byte `2705` requests read the cue list; nothing else.
        assert_eq!(catalog.reads.load(Ordering::Relaxed), 3);
    }
}

#[test]
fn vbr_compatibility_placeholder_keeps_existing_complete_envelope_and_menu() {
    for extended in [false, true] {
        let handler = CatalogHandler::new(Arc::new(AnalysisBoundaryCatalog::default()));
        let mut s = handler.open();
        let setup = if extended { vec![5,20] } else { vec![5] };
        exchange_wire(&mut s, &numbers(kind::SETUP, 0xffff_fffe, &setup).encode());
        exchange_wire(&mut s, &numbers(kind::ROOT_MENU, 0x8100, &[CTX, 0, 0x5cf_ffff]).encode());
        let render = numbers(kind::RENDER, 0x8102, &[CTX, 0, 9]);
        let before = exchange_wire(&mut s, &render.encode());
        // Synthetic preservation fixture, not a vendor VBR-content oracle.
        let mut expected = hex("11872349ae11000081351045020f041400000004060606031100002504110000000011000006441400000644");
        expected.extend_from_slice(&[0; 1604]);
        assert_eq!(exchange_wire(&mut s, &numbers(kind::VBR, 0x8135, &[CTX, TRACK]).encode()), vec![expected]);
        assert_eq!(exchange_wire(&mut s, &render.encode()), before);
    }
}

#[test]
fn ready_open_uses_one_assigned_number_snapshot_without_a_fallback() {
    use std::sync::atomic::{AtomicU8, Ordering};
    let number = Arc::new(AtomicU8::new(0));
    let handler = CatalogHandler::new(Arc::new(Small(false))).with_device(Arc::clone(&number));
    assert!(handler.open_ready().is_none());
    assert!(!handler.serving());
    number.store(17, Ordering::Relaxed);
    let mut first = handler.open_ready().unwrap();
    number.store(18, Ordering::Relaxed);
    let mut second = handler.open_ready().unwrap();
    number.store(0, Ordering::Relaxed);
    assert!(handler.open_ready().is_none());
    // Already-open sessions retain their assigned snapshot. A different
    // reacquisition teardown policy remains an evidence task, not a guess.
    assert_eq!(exchange_wire(&mut first, &setup_request(1).encode()),
        vec![hex("11872349ae11fffffffe1000000f021400000002060611000000111100000014")]);
    assert_eq!(exchange_wire(&mut second, &setup_request(1).encode()),
        vec![hex("11872349ae11fffffffe1000000f021400000002060611000000121100000014")]);
}

#[test]
fn direct_database_socket_closes_unready_and_uses_negotiated_setup_identity() {
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, TcpStream};
    use std::sync::atomic::{AtomicU8, Ordering};
    let number = Arc::new(AtomicU8::new(0));
    let handler = Arc::new(CatalogHandler::new(Arc::new(Small(false))).with_device(Arc::clone(&number)));
    let bound = rbl_dbserver::net::Bound::start(handler, IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 0).unwrap();
    for assigned in [0, 17, 18, 0, 18] {
        number.store(assigned, Ordering::Relaxed);
        let mut client = TcpStream::connect(bound.database_address()).unwrap();
        client.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
        if assigned == 0 {
            let mut reply = [0; 64];
            assert!(matches!(client.read(&mut reply), Ok(0)), "unready sessions close without greeting or invented identity");
        } else {
            let mut request = rbl_dbserver::GREETING.to_vec();
            request.extend_from_slice(&setup_request(1).encode());
            client.write_all(&request).unwrap();
            let mut expected = rbl_dbserver::GREETING.to_vec();
            expected.extend_from_slice(&hex(&format!(
                "11872349ae11fffffffe1000000f021400000002060611000000{assigned:02x}1100000014")));
            let mut reply = vec![0; expected.len()];
            client.read_exact(&mut reply).unwrap();
            assert_eq!(reply, expected);
        }
    }
    bound.shutdown();
}
