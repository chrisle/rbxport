//! The recorded session: rekordbox 7.2.11 serving a CDJ-3000 that connects,
//! browses, loads a track and plays it (`tests/corpus`, both directions,
//! captured 2026-09-12; see docs/pre-release/design-notes/link-export-capture.md).
//!
//! The codec must read every byte of both streams, and re-encode every
//! message to the bytes it came from: a framing change that breaks either
//! is wrong, whatever the spec seemed to say.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::BTreeMap;

use rbl_dbserver::{kind, Argument, Message, GREETING};

const PLAYER: &[u8] = include_bytes!("corpus/link-export.player.bin");
const REKORDBOX: &[u8] = include_bytes!("corpus/link-export.rekordbox.bin");

/// Every message in a stream, checked to re-encode byte for byte.
fn decode_stream(stream: &[u8]) -> Vec<Message> {
    assert!(
        stream.starts_with(GREETING),
        "a session opens with the greeting"
    );
    let mut at = GREETING.len();
    let mut out = Vec::new();
    while at < stream.len() {
        let (message, used) =
            Message::decode(&stream[at..]).unwrap_or_else(|e| panic!("at byte {at}: {e:?}"));
        assert_eq!(
            message.encode(),
            &stream[at..at + used],
            "message at byte {at} re-encodes as recorded"
        );
        at += used;
        out.push(message);
    }
    out
}

#[test]
fn both_streams_decode_whole_and_re_encode_byte_for_byte() {
    let requests = decode_stream(PLAYER);
    let replies = decode_stream(REKORDBOX);
    assert_eq!(requests.len(), 97);
    assert_eq!(replies.len(), 388);
}

#[test]
fn every_request_is_answered_on_its_own_transaction() {
    let requests = decode_stream(PLAYER);
    let mut replies: BTreeMap<u32, Vec<&Message>> = BTreeMap::new();
    let decoded = decode_stream(REKORDBOX);
    for reply in &decoded {
        replies.entry(reply.transaction).or_default().push(reply);
    }
    for request in &requests {
        let answers = replies
            .get(&request.transaction)
            .unwrap_or_else(|| panic!("{request:?} unanswered"));
        match request.kind {
            kind::SETUP => assert_eq!(answers[0].kind, kind::SETUP),
            kind::RENDER => {
                assert_eq!(answers[0].kind, kind::RENDER_HEADER);
                assert_eq!(answers.last().unwrap().kind, kind::MENU_FOOTER);
                assert!(answers[1..answers.len() - 1]
                    .iter()
                    .all(|m| m.kind == kind::MENU_ITEM));
            }
            k if k >> 12 == 2 && k != kind::METADATA && k != kind::TRACK_INFO => {
                // A blob reply: one message whose kind is the request's with
                // the high nibble raised, carrying the request kind first.
                assert_eq!(answers.len(), 1);
                assert_eq!(answers[0].arguments[0], Argument::Number(u32::from(k)));
            }
            _ => assert_eq!(answers[0].kind, kind::MENU_HEADER, "{request:?}"),
        }
    }
}

#[test]
fn a_menu_item_carries_sixteen_arguments() {
    let items: Vec<&Message> = decode_stream(REKORDBOX)
        .into_iter()
        .filter(|m| m.kind == kind::MENU_ITEM)
        .map(|m| Box::leak(Box::new(m)) as &Message)
        .collect();
    assert!(!items.is_empty());
    for item in items {
        assert_eq!(item.arguments.len(), 16);
        assert!(matches!(item.arguments[3], Argument::String(_)));
        assert!(matches!(item.arguments[5], Argument::String(_)));
        assert!(matches!(item.arguments[14], Argument::String(_)));
    }
}
