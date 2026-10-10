//! Cue points a player saves or deletes on a track it loaded over the link
//! (`2705`, and the legacy `2105`), decoded the way rekordbox 7.2.19 decodes
//! them and always answered with a cue list.
//!
//! The records here are built the way a CDJ-3000 3.20 builds them
//! (`track_info_repository` `sub_1280ba0`, `sub_12806b0` in the alphatheta-docs
//! decompile). rekordbox's side is `PSvDBMain::SavUsbCueExt` @`0x1018ca050`
//! and `PSvDBMain::SavUsbCue` @`0x1018c67d8` [OBS static].
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};

use rbl_dbserver::catalog::{
    Analysis, Catalog, Edit, HotCueColour, PlayerCue, Query, Row, TrackColumn, TrackDetails,
    UsbCue,
};
use rbl_dbserver::item::TrackRow;
use rbl_dbserver::net::{Handler, Session};
use rbl_dbserver::session::CatalogHandler;
use rbl_dbserver::{kind, setup_request, Argument, Message};

const CTX: u32 = 0x0101_0301;
const TRACK: u32 = 0x475f;

/// A track whose cues are whatever the test put there, and every edit asked
/// of it.
struct Recording {
    edits: Mutex<Vec<Edit>>,
    accept: bool,
    /// The extended cue list blob; `None` is a track the catalog cannot read.
    list: Option<Vec<u8>>,
}

impl Recording {
    fn new(list: Option<Vec<u8>>) -> Arc<Self> {
        Arc::new(Self { edits: Mutex::new(Vec::new()), accept: true, list })
    }
}

impl Catalog for Recording {
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
    fn analysis(&self, track: u32, what: &Analysis) -> Option<Vec<u8>> {
        match what {
            Analysis::ExtendedCueList if track == TRACK => self.list.clone(),
            _ => None,
        }
    }
    fn usb_cues(&self, track: u32) -> Vec<UsbCue> {
        if track == TRACK {
            vec![UsbCue { slot: 0, in_ms: 1_000, out_ms: None, color_table_index: 0 }]
        } else {
            Vec::new()
        }
    }
    fn edit(&self, edit: &Edit) -> bool {
        self.edits.lock().unwrap().push(edit.clone());
        self.accept
    }
}

fn session(catalog: Arc<Recording>) -> Box<dyn Session> {
    let mut s = CatalogHandler::new(catalog).open();
    s.handle(&setup_request(1));
    s
}

/// The one cue a CDJ-3000 3.20 sends in a `2705`, as `sub_1280ba0` lays it
/// out: the length, a `0x46`-byte head from offset 4, the comment in UTF-16
/// with its NUL, then for a hot cue a length word and `[0, r, g, b]`.
#[derive(Default)]
struct Cdj3000Cue<'a> {
    slot: u16,
    in_ms: u32,
    out_ms: Option<u32>,
    active: bool,
    memory_colour: u8,
    beats: Option<(u16, u16)>,
    comment: &'a str,
    rgb: [u8; 3],
}

impl Cdj3000Cue<'_> {
    fn record(&self) -> Vec<u8> {
        let comment: Vec<u8> = if self.comment.is_empty() {
            Vec::new()
        } else {
            self.comment.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
        };
        let hot = self.slot != 0;
        let mut head = vec![0_u8; 0x46];
        head[0..2].copy_from_slice(&self.slot.to_le_bytes());
        head[2..6].copy_from_slice(&(if self.out_ms.is_some() { 2_u32 } else { 1 }).to_le_bytes());
        head[6..8].copy_from_slice(&1000_u16.to_le_bytes());
        head[8..12].copy_from_slice(&self.in_ms.to_le_bytes());
        head[12..16].copy_from_slice(&self.out_ms.unwrap_or(u32::MAX).to_le_bytes());
        if self.active {
            head[0x14] = 4;
        }
        // No MPEG frame table: both pairs are all ones.
        head[0x20..0x30].fill(0xff);
        let options = 0x12 + comment.len() as u32 + if hot { 8 } else { 0 };
        head[0x30..0x34].copy_from_slice(&options.to_le_bytes());
        head[0x36] = self.memory_colour;
        if let Some((numerator, denominator)) = self.beats {
            head[0x3e..0x40].copy_from_slice(&numerator.to_le_bytes());
            head[0x40..0x42].copy_from_slice(&denominator.to_le_bytes());
        }
        head[0x44..0x46].copy_from_slice(&(comment.len() as u16).to_le_bytes());
        let mut body = head;
        body.extend_from_slice(&comment);
        if hot {
            body.extend_from_slice(&4_u32.to_le_bytes());
            body.extend_from_slice(&[0, self.rgb[0], self.rgb[1], self.rgb[2]]);
        }
        let pad = body.len() & 3;
        body.extend(std::iter::repeat_n(0, pad));
        let mut record = ((body.len() + 4) as u32).to_le_bytes().to_vec();
        record.extend_from_slice(&body);
        record
    }
}

fn save(op: u32, record: Vec<u8>) -> Message {
    Message::new(
        0x8134,
        kind::SAVE_EXTENDED_CUE,
        vec![
            Argument::Number(CTX),
            Argument::Number(TRACK),
            Argument::Number(op),
            Argument::Number(record.len() as u32),
            Argument::Blob(record),
        ],
    )
}

/// A two-entry extended list, as the catalog would hand it out.
fn two_cues() -> Vec<u8> {
    let mut blob = Vec::new();
    for _ in 0..2 {
        let mut entry = vec![0_u8; 0x38];
        entry[0..4].copy_from_slice(&0x38_u32.to_le_bytes());
        blob.extend_from_slice(&entry);
    }
    blob
}

fn the_list(reply: &[Message]) -> Vec<Argument> {
    assert_eq!(reply.len(), 1, "{reply:?}");
    assert_eq!(reply[0].kind, kind::EXTENDED_CUES_REPLY);
    assert_eq!(reply[0].transaction, 0x8134);
    reply[0].arguments.clone()
}

#[test]
fn a_memory_cue_saved_on_a_cdj_3000_reaches_the_library_and_is_answered_with_the_list() {
    let catalog = Recording::new(Some(two_cues()));
    let mut s = session(catalog.clone());
    let record = Cdj3000Cue { in_ms: 61_234, memory_colour: 3, comment: "drop", ..Default::default() }.record();
    let reply = s.handle(&save(1, record));
    // rekordbox answers a save with GetUsbCueExt: the request kind echoed,
    // status 0, then the list and its count.
    assert_eq!(
        the_list(&reply),
        vec![
            Argument::Number(0x2705),
            Argument::Number(0),
            Argument::Number(0x70),
            Argument::Blob(two_cues()),
            Argument::Number(2),
        ]
    );
    assert_eq!(
        *catalog.edits.lock().unwrap(),
        vec![Edit::SaveCue {
            track: TRACK,
            cue: PlayerCue {
                kind: 0,
                in_ms: 61_234,
                out_ms: None,
                // The player's 1-8 is stored as 0-7.
                colour: Some(2),
                hot_colour: HotCueColour::Index(21),
                comment: "drop".into(),
                beat_loop: None,
            },
        }]
    );
}

#[test]
fn hot_cues_and_loops_carry_their_slot_colour_and_beats() {
    let catalog = Recording::new(Some(two_cues()));
    let mut s = session(catalog.clone());
    // Hot cue D: Kind 5 (Kind 4 is not a hot slot). The CDJ-3000 sends the
    // colour as LED red, green and blue with a zero code.
    s.handle(&save(1, Cdj3000Cue { slot: 4, in_ms: 500, rgb: [0x00, 0x70, 0xff], ..Default::default() }.record()));
    // Hot loop H with a 4/1 beat length and no LED colour: the loop default.
    s.handle(&save(1, Cdj3000Cue { slot: 8, in_ms: 1_000, out_ms: Some(3_000), beats: Some((4, 1)), ..Default::default() }.record()));
    // A memory loop the player marked active is Kind 4.
    s.handle(&save(1, Cdj3000Cue { in_ms: 2_000, out_ms: Some(2_500), active: true, ..Default::default() }.record()));
    let edits = catalog.edits.lock().unwrap();
    let cues: Vec<&PlayerCue> = edits
        .iter()
        .map(|edit| match edit {
            Edit::SaveCue { track: TRACK, cue } => cue,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!((cues[0].kind, cues[0].out_ms, cues[0].hot_colour), (5, None, HotCueColour::Rgb([0x00, 0x70, 0xff])));
    assert_eq!(cues[0].colour, None);
    assert_eq!((cues[1].kind, cues[1].out_ms, cues[1].hot_colour), (9, Some(3_000), HotCueColour::Index(36)));
    assert_eq!(cues[1].beat_loop, Some((4, 1)));
    assert_eq!((cues[2].kind, cues[2].in_ms, cues[2].out_ms), (4, 2_000, Some(2_500)));
}

#[test]
fn a_deleted_cue_is_a_delete_edit() {
    let catalog = Recording::new(Some(two_cues()));
    let mut s = session(catalog.clone());
    let reply = s.handle(&save(0, Cdj3000Cue { slot: 1, in_ms: 500, ..Default::default() }.record()));
    assert_eq!(the_list(&reply)[1], Argument::Number(0));
    assert!(matches!(
        catalog.edits.lock().unwrap().as_slice(),
        [Edit::DeleteCue { track: TRACK, cue }] if cue.kind == 1 && cue.in_ms == 500
    ));
}

#[test]
fn frame_based_records_round_up_to_milliseconds_as_rekordbox_does() {
    // CmnFunc_Frm150ToMsec_InOoutConv: ceil(frames * 6.666666666666667),
    // the loop's out one frame later; time base 75 doubles first.
    for (timebase, frames_in, frames_out, in_ms, out_ms) in [
        (150_u16, 1_u32, 2_u32, 7_u32, 20_u32),
        (150, 3, 9, 20, 67),
        (150, 9_000, 9_149, 60_000, 61_000),
        (75, 1, 2, 14, 34),
    ] {
        let catalog = Recording::new(Some(two_cues()));
        let mut s = session(catalog.clone());
        let mut record = Cdj3000Cue { out_ms: Some(1), ..Default::default() }.record();
        record[0xa..0xc].copy_from_slice(&timebase.to_le_bytes());
        record[0xc..0x10].copy_from_slice(&frames_in.to_le_bytes());
        record[0x10..0x14].copy_from_slice(&frames_out.to_le_bytes());
        s.handle(&save(1, record));
        let edits = catalog.edits.lock().unwrap();
        match edits.as_slice() {
            [Edit::SaveCue { cue, .. }] => assert_eq!((cue.in_ms, cue.out_ms), (in_ms, Some(out_ms)), "{timebase} {frames_in}"),
            other => panic!("{other:?}"),
        };
    }
}

#[test]
fn records_rekordbox_leaves_alone_change_nothing_and_get_the_list() {
    let base = Cdj3000Cue { in_ms: 10, ..Default::default() }.record();
    let mut cases = Vec::new();
    // Hot cue past H.
    let mut past_h = base.clone();
    past_h[4..6].copy_from_slice(&9_u16.to_le_bytes());
    cases.push(past_h);
    // Neither a cue (1) nor a loop (2).
    for shape in [0_u8, 3] {
        let mut record = base.clone();
        record[6] = shape;
        cases.push(record);
    }
    // A time base other than 75, 150 and 1000.
    let mut timebase = base.clone();
    timebase[0xa..0xc].copy_from_slice(&100_u16.to_le_bytes());
    cases.push(timebase);
    for record in cases {
        let catalog = Recording::new(Some(two_cues()));
        let mut s = session(catalog.clone());
        let reply = s.handle(&save(1, record));
        assert_eq!(the_list(&reply)[4], Argument::Number(2));
        assert!(catalog.edits.lock().unwrap().is_empty());
    }
}

#[test]
fn a_cue_save_is_never_answered_with_4003() {
    // A CDJ-3000 answered 4003 resends the cue as a 2105, then again on
    // every further 4003, and its database client never moves on: the
    // stall of issue 281. Every case below answers 4e02 status 0x32.
    let refusal = vec![
        Argument::Number(0x2705),
        Argument::Number(0x32),
        Argument::Number(0),
        Argument::Blob(Vec::new()),
        Argument::Number(0),
    ];
    let good = Cdj3000Cue { in_ms: 10, ..Default::default() }.record();
    let mut requests = vec![
        // Shorter than rekordbox's 0x38.
        save(1, good[..0x37].to_vec()),
        // No record.
        Message::new(0x8134, kind::SAVE_EXTENDED_CUE, vec![Argument::Number(CTX), Argument::Number(TRACK), Argument::Number(1), Argument::Number(0)]),
        // A wrong-typed track.
        Message::new(0x8134, kind::SAVE_EXTENDED_CUE, vec![Argument::Number(CTX), Argument::String("x".into()), Argument::Number(1), Argument::Number(good.len() as u32), Argument::Blob(good.clone())]),
    ];
    let catalog = Recording::new(Some(two_cues()));
    let mut s = session(catalog.clone());
    for request in requests.drain(..) {
        let reply = s.handle(&request);
        assert_eq!(the_list(&reply), refusal, "{request:?}");
    }
    assert!(catalog.edits.lock().unwrap().is_empty());
    // The library refusing the write (rekordbox open, a restore pending).
    let refusing = Arc::new(Recording { edits: Mutex::new(Vec::new()), accept: false, list: Some(two_cues()) });
    let mut s = session(refusing.clone());
    assert_eq!(the_list(&s.handle(&save(1, good))), refusal);
    assert_eq!(refusing.edits.lock().unwrap().len(), 1);
}

#[test]
fn a_track_left_without_cues_is_answered_with_status_1() {
    // GetUsbCueExtSharedContent: status = count < 1, which a player reads as
    // success, so deleting a track's last cue is not reported as a failure.
    let catalog = Recording::new(Some(Vec::new()));
    let mut s = session(catalog);
    let reply = s.handle(&save(0, Cdj3000Cue { in_ms: 10, ..Default::default() }.record()));
    assert_eq!(
        the_list(&reply),
        vec![Argument::Number(0x2705), Argument::Number(1), Argument::Number(0), Argument::Blob(Vec::new()), Argument::Number(0)]
    );
    let read = s.handle(&Message::new(0x8134, kind::EXTENDED_CUES, vec![Argument::Number(CTX), Argument::Number(TRACK)]));
    assert_eq!(the_list(&read)[..2], [Argument::Number(0x2b04), Argument::Number(1)]);
}

/// The legacy record a CDJ-3000 falls back to (`sub_12806b0`): flags with
/// the slot in the top half, bit 0 a loop and `0x200` an active one, the
/// 1/150 s frames, and a sidecar with the milliseconds.
fn legacy(op: u32, flags: u32, in_ms: u32, out_ms: u32) -> Message {
    let mut record = vec![0_u8; 0x24];
    record[0..4].copy_from_slice(&flags.to_le_bytes());
    record[0xc..0x10].copy_from_slice(&(in_ms * 3 / 20).to_le_bytes());
    record[0x10..0x14].copy_from_slice(&(if flags & 1 != 0 { out_ms * 3 / 20 - 1 } else { u32::MAX }).to_le_bytes());
    let mut sidecar = in_ms.to_le_bytes().to_vec();
    sidecar.extend_from_slice(&out_ms.to_le_bytes());
    Message::new(
        0x8135,
        kind::SAVE_CUE,
        vec![
            Argument::Number(CTX),
            Argument::Number(TRACK),
            Argument::Number(op),
            Argument::Number(0x24),
            Argument::Blob(record),
            Argument::Number(8),
            Argument::Blob(sidecar),
        ],
    )
}

#[test]
fn a_legacy_cue_save_reaches_the_library_and_is_answered_with_the_legacy_list() {
    let catalog = Recording::new(Some(two_cues()));
    let mut s = session(catalog.clone());
    let reply = s.handle(&legacy(1, 2 << 16, 1_234, 0));
    assert_eq!(reply.len(), 1);
    assert_eq!(reply[0].kind, kind::HOT_CUE_BANK_REPLY);
    assert_eq!(reply[0].arguments[..2], [Argument::Number(0x2105), Argument::Number(0)]);
    s.handle(&legacy(0, 0x201, 2_000, 4_000));
    let edits = catalog.edits.lock().unwrap();
    assert_eq!(
        edits[0],
        Edit::SaveCue {
            track: TRACK,
            cue: PlayerCue { kind: 2, in_ms: 1_234, out_ms: None, colour: None, hot_colour: HotCueColour::Index(21), comment: String::new(), beat_loop: None },
        }
    );
    assert!(matches!(&edits[1], Edit::DeleteCue { cue, .. } if cue.kind == 4 && cue.out_ms == Some(4_000)));
}

#[test]
fn a_legacy_cue_save_rbx_cannot_read_still_gets_a_4702() {
    let catalog = Recording::new(Some(two_cues()));
    let mut s = session(catalog.clone());
    // Slot D does not fit the legacy record; rekordbox raises an internal
    // error and sends nothing, RBX answers with the failure envelope.
    for request in [
        legacy(1, 4 << 16, 1_000, 0),
        Message::new(0x8135, kind::SAVE_CUE, vec![Argument::Number(CTX), Argument::Number(TRACK), Argument::Number(1)]),
    ] {
        let reply = s.handle(&request);
        assert_eq!(reply.len(), 1);
        assert_eq!(reply[0].kind, kind::HOT_CUE_BANK_REPLY);
        assert_eq!(reply[0].arguments[1], Argument::Number(0x32));
    }
    assert!(catalog.edits.lock().unwrap().is_empty());
}
