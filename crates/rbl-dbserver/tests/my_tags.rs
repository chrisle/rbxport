//! My Tags in Track Filter over LINK, replayed from two captures of
//! rekordbox 7 serving an XDJ-RX3 (firmware 1.20), taken by the reporter of
//! #316 on 2026-10-10 (`tests/corpus/mytag-rx3-*`).
//!
//! `*.player.bin` is the player's whole request stream. `*.rekordbox.bin`
//! holds the rekordbox replies the replay compares: the setup reply, every
//! reply to `1015`/`3307`/`3407`/`3007`, the row renders of the My Tag
//! menus, and the counts of the track lists (`1105`, `1004`). The streams
//! were reassembled from the reporter's pktmon captures (TCP 64053 and
//! 60619), each segment taken once by sequence number.
//!
//! The My Tags here are the ones the capture lists, with the ids, names and
//! order rekordbox sent. The tracks are made up to give rekordbox's counts:
//! a 41-track playlist where 30 tracks carry `reese`, 26 `wubby`, 21 both
//! and one `reese` and `mc-bars`; 130 tracks in all, 35 of them `reese`,
//! seven of those at 170 BPM and the rest at 174. Each reply rbx gives to a
//! kept request must be the bytes rekordbox sent.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use rbl_dbserver::catalog::{
    Analysis, ArtworkSize, Catalog, MyTag, Query, Row, TrackColumn, TrackDetails, TrackScope,
};
use rbl_dbserver::item::TrackRow;
use rbl_dbserver::net::Session;
use rbl_dbserver::session::LinkSession;
use rbl_dbserver::{kind, Argument, Message, GREETING};

const PLAYLIST: u32 = 0xbbfb_546b;
const FOLDER: u32 = 0x7c15_7ab2;
const OTHER_PLAYLIST: u32 = 0x0736_200d;

const REESE: u32 = 0x0677_d658;
const WUBBY: u32 = 0x0e7a_8716;
const MC_BARS: u32 = 0x0be5_c94d;

/// The categories and the tags under them, in the order and with the ids
/// the captures show. Where a category lists more tags than the player
/// drew, the rest are stand-ins carried by some track; `unused` tags are
/// carried by none, as the two Traits and one Vocals tag rekordbox left out
/// of its counts (38 of 40, 7 of 8; the reporter's `exportExt.pdb`).
fn my_tags() -> Vec<MyTag> {
    let mut out = Vec::new();
    let mut category = |id: u32, seq: u32, name: &str, tags: &[(u32, &str)]| {
        out.push(MyTag { id, parent: 0, seq, name: name.into() });
        for (at, (tag, name)) in tags.iter().enumerate() {
            out.push(MyTag { id: *tag, parent: id, seq: at as u32 + 1, name: (*name).into() });
        }
    };
    category(
        1,
        1,
        "Sub-genre",
        &[
            (0x0fcc_f39f, "DnB Neuro"),
            (0x07ae_781a, "DnB Jump-up"),
            (0x0116_4f68, "DnB Jungle"),
            (0x0d40_2d69, "DnB Liquid"),
            (0x0917_59b2, "DnB Dancefloor"),
        ],
    );
    let mut traits: Vec<(u32, String)> = [
        (REESE, "reese"),
        (WUBBY, "wubby"),
        (0x024d_98b8, "growly"),
        (0x050f_ac24, "sub"),
        (0x06f9_9f77, "distorted"),
        (0x0ba0_f793, "gritty"),
        (0x067f_5448, "buzzy"),
        (0x05f5_5332, "roller"),
    ]
    .iter()
    .map(|(id, name)| (*id, (*name).to_owned()))
    .collect();
    traits.extend((0..32).map(|n| (0x7000_0000 + n, format!("trait {n}"))));
    let traits: Vec<(u32, &str)> = traits.iter().map(|(id, name)| (*id, name.as_str())).collect();
    category(2, 2, "Traits", &traits);
    category(
        3,
        3,
        "Vocals",
        &[
            (0x0e72_3f7d, "vocals"),
            // Unused, at its real Seq 2 in the reporter's djmdMyTag (#316
            // comment 2026-10-13T09:31Z): the rows after it are still
            // numbered 1, 2, ... in slot 9.
            (0x054e_6c12, "sung"),
            (MC_BARS, "mc-bars"),
            (0x0538_c9ea, "ragga"),
            (0x04b6_11c1, "one-shot"),
            (0x0991_213e, "full-vocal"),
            (0x07c7_3257, "us-vocal"),
            (0x0e5c_721e, "uk-vocal"),
        ],
    );
    out
}

/// Tags no track carries.
const UNUSED: [u32; 3] = [0x7000_001e, 0x7000_001f, 0x054e_6c12];

struct Library {
    playlist: Vec<u32>,
    all: Vec<u32>,
    tags: BTreeMap<u32, Vec<u32>>,
    bpm: BTreeMap<u32, u32>,
}

fn library() -> Library {
    let mut tags: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut bpm = BTreeMap::new();
    let all: Vec<u32> = (1..=130).collect();
    // Playlist: tracks 1-41. 1-21 reese+wubby, 22-30 reese, 31-35 wubby.
    for track in 1..=21 {
        tags.insert(track, vec![REESE, WUBBY]);
    }
    for track in 22..=30 {
        tags.insert(track, vec![REESE]);
    }
    for track in 31..=35 {
        tags.insert(track, vec![WUBBY]);
    }
    tags.get_mut(&22).unwrap().push(MC_BARS);
    // Outside the playlist: five more reese tracks.
    for track in 42..=46 {
        tags.insert(track, vec![REESE]);
    }
    // One track carries every other tag that is in use.
    let reese_like = [REESE, WUBBY, MC_BARS];
    let rest: Vec<u32> = my_tags()
        .iter()
        .filter(|t| t.parent != 0 && !reese_like.contains(&t.id) && !UNUSED.contains(&t.id))
        .map(|t| t.id)
        .collect();
    tags.insert(100, rest);
    let reese: Vec<u32> = tags
        .iter()
        .filter(|(_, t)| t.contains(&REESE))
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(reese.len(), 35);
    for track in &all {
        bpm.insert(*track, 12_800);
    }
    for (at, track) in reese.iter().enumerate() {
        bpm.insert(*track, if at < 7 { 17_000 } else { 17_400 });
    }
    Library { playlist: (1..=41).collect(), all, tags, bpm }
}

impl Catalog for Library {
    fn list(&self, query: &Query) -> Vec<Row> {
        let tracks = |ids: &[u32]| {
            ids.iter()
                .enumerate()
                .map(|(at, id)| Row::Track { id: *id, position: at as u32 + 1 })
                .collect()
        };
        let lists = |n: u32| {
            (0..n)
                .map(|at| Row::List { id: 0x100 + at, name: format!("list {at}"), folder: false, position: at })
                .collect()
        };
        match query {
            Query::Folder(0) => lists(5),
            Query::Folder(FOLDER) => lists(6),
            Query::Tracks { scope: TrackScope::Playlist(PLAYLIST), .. } => tracks(&self.playlist),
            Query::Tracks { scope: TrackScope::Playlist(OTHER_PLAYLIST), .. } => tracks(&self.all[..37]),
            Query::Tracks { scope: TrackScope::All, .. } => tracks(&self.all),
            _ => Vec::new(),
        }
    }

    fn track_row(&self, id: u32, _column: Option<TrackColumn>) -> Option<TrackRow> {
        let bpm_x100 = *self.bpm.get(&id)?;
        Some(TrackRow { id, title: format!("track {id}"), bpm_x100, ..TrackRow::default() })
    }

    fn track(&self, id: u32) -> Option<TrackDetails> {
        Some(TrackDetails { row: self.track_row(id, None)?, ..TrackDetails::default() })
    }

    fn artwork(&self, _: u32, _: ArtworkSize) -> Option<Vec<u8>> {
        None
    }

    fn analysis(&self, _track: u32, _what: &Analysis) -> Option<Vec<u8>> {
        None
    }

    fn my_tags(&self) -> Vec<MyTag> {
        my_tags()
    }

    fn track_my_tags(&self, track: u32) -> Vec<u32> {
        self.tags.get(&track).cloned().unwrap_or_default()
    }

    fn my_tags_in_use(&self) -> HashSet<u32> {
        self.tags.values().flatten().copied().collect()
    }
}

fn decode(stream: &[u8]) -> Vec<(Message, Vec<u8>)> {
    assert!(stream.starts_with(GREETING));
    let mut at = GREETING.len();
    let mut out = Vec::new();
    while at < stream.len() {
        let (message, used) = Message::decode(&stream[at..]).unwrap_or_else(|e| panic!("at {at}: {e:?}"));
        out.push((message, stream[at..at + used].to_vec()));
        at += used;
    }
    out
}

/// Replays a capture's requests and compares every kept reply byte for
/// byte. Returns how many requests of each kind were compared.
fn replay(player: &[u8], rekordbox: &[u8]) -> BTreeMap<u16, usize> {
    let mut recorded: BTreeMap<u32, Vec<Vec<u8>>> = BTreeMap::new();
    for (message, bytes) in decode(rekordbox) {
        recorded.entry(message.transaction).or_default().push(bytes);
    }
    let mut session = LinkSession::new(Arc::new(library()));
    let mut compared: BTreeMap<u16, usize> = BTreeMap::new();
    for (request, _) in decode(player) {
        let ours: Vec<Vec<u8>> = session.handle(&request).iter().map(Message::encode).collect();
        let Some(theirs) = recorded.get(&request.transaction) else {
            continue;
        };
        assert_eq!(
            ours.len(),
            theirs.len(),
            "{} {:#06x} {:?}: reply count",
            request.transaction,
            request.kind,
            request.arguments
        );
        for (a, b) in ours.iter().zip(theirs) {
            if a != b {
                let (ours, _) = Message::decode(a).unwrap();
                let (theirs, _) = Message::decode(b).unwrap();
                panic!(
                    "tx {:#x} {:#06x} {:?}:\n rbx       {ours:?}\n rekordbox {theirs:?}",
                    request.transaction, request.kind, request.arguments
                );
            }
        }
        *compared.entry(request.kind).or_default() += 1;
    }
    compared
}

#[test]
fn capture_one_one_tag_per_category_replays_byte_for_byte() {
    let compared = replay(
        include_bytes!("corpus/mytag-rx3-1.player.bin"),
        include_bytes!("corpus/mytag-rx3-1.rekordbox.bin"),
    );
    // Every My Tag menu and setter, the switch and the list counts.
    assert_eq!(compared.get(&kind::MY_TAG), Some(&48));
    assert_eq!(compared.get(&kind::FILTER_MY_TAG), Some(&6));
    assert_eq!(compared.get(&kind::FILTER_MY_TAG_ITEM), Some(&2));
    assert_eq!(compared.get(&kind::FILTER_SWITCH), Some(&28));
    assert_eq!(compared.get(&kind::PLAYLIST_MENU), Some(&69));
    assert_eq!(compared.get(&kind::RENDER), Some(&RENDERS_1));
}

/// The My Tag menus the player drew, each compared row for row.
const RENDERS_1: usize = 48;
const RENDERS_2: usize = 33;

#[test]
fn capture_two_and_or_untick_all_tracks_and_bpm_replays_byte_for_byte() {
    let compared = replay(
        include_bytes!("corpus/mytag-rx3-2.player.bin"),
        include_bytes!("corpus/mytag-rx3-2.rekordbox.bin"),
    );
    assert_eq!(compared.get(&kind::MY_TAG), Some(&33));
    assert_eq!(compared.get(&kind::FILTER_MY_TAG), Some(&3));
    assert_eq!(compared.get(&kind::FILTER_MY_TAG_ITEM), Some(&3));
    assert_eq!(compared.get(&kind::FILTER_SWITCH), Some(&28));
    assert_eq!(compared.get(&kind::PLAYLIST_MENU), Some(&40));
    assert_eq!(compared.get(&kind::TRACK_MENU), Some(&44));
    assert_eq!(compared.get(&kind::RENDER), Some(&RENDERS_2));
}

fn ask(session: &mut LinkSession, kind: u16, arguments: &[u32]) -> Vec<Message> {
    let message = Message::new(7, kind, arguments.iter().map(|n| Argument::Number(*n)).collect());
    session.handle(&message)
}

fn status(replies: &[Message]) -> Argument {
    replies[0].arguments[1].clone()
}

fn session() -> LinkSession {
    let mut session = LinkSession::new(Arc::new(library()));
    ask(&mut session, kind::SETUP, &[0x0b]);
    session
}

const CTX: u32 = 0x0b01_0401;

/// `PSvDBMain::SetFilterConditionMyTag` and `AddFilterConditionMyTag`
/// refuse with `0x32`: a second argument other than 0, a mode other than 16
/// or 17, a category that does not exist, and ticking tag `0xffffffff`.
#[test]
fn my_tag_setters_refuse_what_rekordbox_refuses() {
    let mut s = session();
    let refused = Argument::Number(0x32);
    let ok = Argument::Number(0);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 1, 2, 1, 16])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, 2, 1, 18])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, 9, 1, 16])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, REESE, 1, 16])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, 0, 1, 16])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, 2, 1, 17])), ok);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 1, 2, REESE, 1])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 9, REESE, 1])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, u32::MAX, 1])), refused);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, u32::MAX, 0])), ok);
    // Unticking a tag that is not ticked succeeds; so does ticking twice.
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, WUBBY, 0])), ok);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, REESE, 1])), ok);
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, REESE, 1])), ok);
    // A missing argument reads as 0, as rekordbox's zeroed request does.
    assert_eq!(status(&ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, 2, 1])), refused);
}

fn playlist_count(s: &mut LinkSession) -> Argument {
    status(&ask(s, kind::PLAYLIST_MENU, &[CTX, 0, PLAYLIST, 0]))
}

fn items(s: &mut LinkSession, location: u32, total: u32) -> Vec<Message> {
    let replies = ask(s, kind::RENDER, &[0x0b00_0401 | location << 16, 0, total, 0, total, 0]);
    replies.into_iter().filter(|m| m.kind == kind::MENU_ITEM).collect()
}

/// Ticking twice is one tick, the category's "all" untick clears it, and
/// "selected only" lists only what is ticked or on.
#[test]
fn ticks_are_a_set_and_selected_only_lists_what_is_ticked() {
    let mut s = session();
    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, REESE, 1]);
    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, REESE, 1]);
    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, WUBBY, 1]);
    ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, 2, 1, 16]);
    ask(&mut s, kind::FILTER_SWITCH, &[CTX, 1]);
    assert_eq!(playlist_count(&mut s), Argument::Number(21));
    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, REESE, 0]);
    assert_eq!(playlist_count(&mut s), Argument::Number(26));

    assert_eq!(status(&ask(&mut s, 0x1015, &[0x0b05_0401, 0, 2, 1])), Argument::Number(1));
    let ticked = items(&mut s, 5, 1);
    assert_eq!(ticked[0].arguments[1], Argument::Number(WUBBY));
    assert_eq!(ticked[0].arguments[6], Argument::Number(0x4b));
    assert_eq!(status(&ask(&mut s, 0x1015, &[0x0b07_0401, 0, 0, 1])), Argument::Number(1));
    assert_eq!(items(&mut s, 7, 1)[0].arguments[1], Argument::Number(2));

    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 2, u32::MAX, 0]);
    assert_eq!(status(&ask(&mut s, 0x1015, &[0x0b05_0401, 0, 2, 1])), Argument::Number(0));
    assert_eq!(playlist_count(&mut s), Argument::Number(41));
}

/// Slot 9 of a `0x1015` tag row is the row's position among the tags the
/// category lists, so an unused tag between used ones takes no number
/// [OBS #316: the reporter's Vocals has the unused "sung" at `Seq` 2 and
/// rekordbox sent "mc-bars", `Seq` 3, as 1]. "Selected only" keeps each
/// row's number [ASSUME static: rekordbox reads it from the row, never
/// from a counter in the loop that drops unticked rows].
#[test]
fn a_tag_rows_number_skips_unused_tags_and_survives_selected_only() {
    fn rows(replies: &[Message]) -> Vec<(String, u32)> {
        replies
            .iter()
            .map(|m| match (&m.arguments[3], &m.arguments[9]) {
                (Argument::String(name), Argument::Number(n)) => (name.clone(), *n),
                other => panic!("{other:?}"),
            })
            .collect()
    }
    let mut s = session();
    assert_eq!(status(&ask(&mut s, kind::MY_TAG, &[0x0b05_0401, 0, 3, 0])), Argument::Number(7));
    let listed = rows(&items(&mut s, 5, 7));
    let expected: Vec<(String, u32)> =
        ["vocals", "mc-bars", "ragga", "one-shot", "full-vocal", "us-vocal", "uk-vocal"]
            .iter()
            .zip(0..)
            .map(|(name, n)| ((*name).to_owned(), n))
            .collect();
    assert_eq!(listed, expected);

    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 3, MC_BARS, 1]);
    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 3, 0x07c7_3257, 1]);
    assert_eq!(status(&ask(&mut s, kind::MY_TAG, &[0x0b05_0401, 0, 3, 1])), Argument::Number(2));
    assert_eq!(
        rows(&items(&mut s, 5, 2)),
        vec![("mc-bars".to_owned(), 1), ("us-vocal".to_owned(), 5)]
    );
}

/// A category that is on with no tag ticked, or with more than fifty,
/// does not narrow the list (`TrackFilter::TrackFilter`,
/// `getFilteringResult`); the master switch off shows every track with
/// the conditions kept.
#[test]
fn an_empty_or_oversized_category_and_the_master_switch_leave_the_list_whole() {
    let mut s = session();
    ask(&mut s, kind::FILTER_SWITCH, &[CTX, 1]);
    ask(&mut s, kind::FILTER_MY_TAG, &[CTX, 0, 3, 1, 16]);
    assert_eq!(playlist_count(&mut s), Argument::Number(41));
    ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 3, MC_BARS, 1]);
    assert_eq!(playlist_count(&mut s), Argument::Number(1));
    ask(&mut s, kind::FILTER_SWITCH, &[CTX, 0]);
    assert_eq!(playlist_count(&mut s), Argument::Number(41));
    ask(&mut s, kind::FILTER_SWITCH, &[CTX, 1]);
    assert_eq!(playlist_count(&mut s), Argument::Number(1));
    for n in 0..50 {
        ask(&mut s, kind::FILTER_MY_TAG_ITEM, &[CTX, 0, 3, 0x6000_0000 + n, 1]);
    }
    // 51 ticked: rekordbox drops the condition.
    assert_eq!(playlist_count(&mut s), Argument::Number(41));
}

/// `0x1315`: the track's tags under their category headings, categories
/// and tags in `Seq` order, the category id first in every row.
#[test]
fn a_tracks_my_tags_come_under_their_categories() {
    let mut s = session();
    assert_eq!(status(&ask(&mut s, kind::MY_TAG_ON_TRACK, &[0x0b05_0401, 0, 22])), Argument::Number(4));
    let rows: Vec<(u32, u32, String, u32)> = items(&mut s, 5, 4)
        .iter()
        .map(|m| match (&m.arguments[0], &m.arguments[1], &m.arguments[3], &m.arguments[6]) {
            (Argument::Number(a), Argument::Number(id), Argument::String(name), Argument::Number(t)) => {
                (*a, *id, name.clone(), *t)
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (2, 2, "Traits".to_owned(), 1),
            (2, REESE, "reese".to_owned(), 0x4a),
            (3, 3, "Vocals".to_owned(), 1),
            (3, MC_BARS, "mc-bars".to_owned(), 0x4a),
        ]
    );
    assert_eq!(status(&ask(&mut s, kind::MY_TAG_ON_TRACK, &[0x0b05_0401, 0, 41])), Argument::Number(0));
}
