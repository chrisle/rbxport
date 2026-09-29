//! Byte-level tests for the database-server message codec.
//!
//! The framing is checked against bytes captured from rekordbox 7.2.11
//! serving a CDJ-3000 (`verification/link`, 2026-09-12); the rest against
//! the encoding rules that capture established.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use rbl_dbserver::{
    kind, menu_footer, menu_header, setup_reply, setup_request, Argument, DbError, Message, MAGIC,
    SETUP_TXID,
};

/// Header as measured: `11` magic, `11` transaction, `10` kind, `0f` count,
/// then the tag list as a blob (`14`, length = count, one tag byte each).
const HEADER_LEN: usize = 1 + 4 + 1 + 4 + 1 + 2 + 1 + 1 + 1 + 4;

/// The player's setup message, verbatim from the capture
/// (`verification/link`, rekordbox 7.2.11 ↔ CDJ-3000, 2026-09-12).
const CAPTURED_SETUP: &str = "11872349ae11fffffffe1000000f021400000002060611000000011100000014";
/// rekordbox's reply to it: our device number is 0x11.
const CAPTURED_SETUP_REPLY: &str =
    "11872349ae11fffffffe1000000f021400000002060611000000111100000014";
/// The root-menu request: three numbers.
const CAPTURED_ROOT_MENU: &str =
    "11872349ae110000017f1010000f031400000003060606110101030111000000001105cfffff";
/// rekordbox's "no artwork" reply: four arguments declared, the empty
/// trailing blob not sent at all.
const CAPTURED_NO_ARTWORK: &str =
    "11872349ae11000001871040020f04140000000406060603110000200311000000321100000000";

/// A header declaring the given tag bytes and nothing after it, so a test
/// can append hand-built argument fields.
fn header_declaring(kind: u16, tags: &[u8]) -> Vec<u8> {
    let mut bytes = Message::new(1, kind, vec![]).encode();
    bytes[14] = u8::try_from(tags.len()).unwrap();
    bytes[16..20].copy_from_slice(&u32::try_from(tags.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(tags);
    bytes
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn header_layout_is_exact() {
    let bytes = Message::new(0x1234_5678, kind::ROOT_MENU, vec![]).encode();
    assert_eq!(
        bytes.len(),
        HEADER_LEN,
        "an argument-free message is header only"
    );
    assert_eq!(bytes[0], 0x11);
    assert_eq!(&bytes[1..5], &MAGIC.to_be_bytes());
    assert_eq!(bytes[5], 0x11);
    assert_eq!(&bytes[6..10], &0x1234_5678_u32.to_be_bytes());
    assert_eq!(bytes[10], 0x10);
    assert_eq!(&bytes[11..13], &kind::ROOT_MENU.to_be_bytes());
    assert_eq!(bytes[13], 0x0f);
    assert_eq!(bytes[14], 0, "argument count");
    assert_eq!(bytes[15], 0x14, "the tag list is itself a blob field");
    assert_eq!(
        &bytes[16..20],
        &0_u32.to_be_bytes(),
        "one tag byte an argument"
    );
}

#[test]
fn the_captured_setup_exchange_decodes_and_re_encodes_byte_for_byte() {
    let (setup, used) = Message::decode(&hex(CAPTURED_SETUP)).unwrap();
    assert_eq!(used, hex(CAPTURED_SETUP).len());
    assert_eq!(setup, setup_request(1));
    assert_eq!(setup.encode(), hex(CAPTURED_SETUP));

    let (reply, _) = Message::decode(&hex(CAPTURED_SETUP_REPLY)).unwrap();
    assert_eq!(reply, setup_reply(SETUP_TXID, 0x11));
    assert_eq!(reply.encode(), hex(CAPTURED_SETUP_REPLY));
}

#[test]
fn the_captured_root_menu_request_decodes_and_re_encodes_byte_for_byte() {
    let (request, used) = Message::decode(&hex(CAPTURED_ROOT_MENU)).unwrap();
    assert_eq!(used, hex(CAPTURED_ROOT_MENU).len());
    assert_eq!(request.transaction, 0x17f);
    assert_eq!(request.kind, kind::ROOT_MENU);
    assert_eq!(
        request.arguments,
        vec![
            Argument::Number(0x0101_0301),
            Argument::Number(0),
            Argument::Number(0x05cf_ffff)
        ]
    );
    assert_eq!(request.encode(), hex(CAPTURED_ROOT_MENU));
}

#[test]
fn an_empty_trailing_blob_is_absent_on_the_wire_and_present_when_decoded() {
    let bytes = hex(CAPTURED_NO_ARTWORK);
    let (reply, used) = Message::decode(&bytes).unwrap();
    assert_eq!(used, bytes.len(), "the message ends after the third number");
    assert_eq!(
        reply.arguments,
        vec![
            Argument::Number(0x2003),
            Argument::Number(0x32),
            Argument::Number(0),
            Argument::Blob(vec![])
        ]
    );
    assert_eq!(
        reply.encode(),
        bytes,
        "and an empty trailing blob is not written"
    );
    // With another message following, the decoder must not eat its magic.
    let mut two = bytes.clone();
    two.extend_from_slice(&bytes);
    let (messages, consumed) = Message::decode_all(&two);
    assert_eq!(messages.len(), 2);
    assert_eq!(consumed, two.len());
}

#[test]
fn argument_tags_differ_between_the_header_list_and_the_value() {
    let message = Message::new(
        7,
        kind::SEARCH,
        vec![
            Argument::String("a".into()),
            Argument::Blob(vec![9]),
            Argument::Number(5),
        ],
    );
    let bytes = message.encode();
    // Header list: string 02, blob 03, number 06.
    assert_eq!(bytes[14], 3);
    assert_eq!(&bytes[16..20], &3_u32.to_be_bytes());
    assert_eq!(&bytes[20..23], &[0x02, 0x03, 0x06]);
    // First value field begins right after the tag list with the string tag 0x26.
    assert_eq!(bytes[HEADER_LEN + 3], 0x26);
}

#[test]
fn round_trips_every_argument_kind() {
    let message = Message::new(
        42,
        kind::METADATA,
        vec![
            Argument::Number(0x0102_0304),
            Argument::String("Take Me Home".into()),
            Argument::Blob(vec![0, 1, 2, 253, 254, 255]),
            Argument::String(String::new()),
        ],
    );
    let bytes = message.encode();
    let (back, used) = Message::decode(&bytes).unwrap();
    assert_eq!(back, message);
    assert_eq!(used, bytes.len(), "decode consumes exactly the message");
}

#[test]
fn strings_are_utf16be_with_a_trailing_nul_counted_in_the_length() {
    let bytes = Message::new(1, kind::SEARCH, vec![Argument::String("Hi".into())]).encode();
    let field = &bytes[HEADER_LEN + 1..];
    assert_eq!(field[0], 0x26);
    assert_eq!(
        &field[1..5],
        &3_u32.to_be_bytes(),
        "two characters plus the NUL"
    );
    assert_eq!(&field[5..11], &[0x00, b'H', 0x00, b'i', 0x00, 0x00]);
}

#[test]
fn non_ascii_survives_the_utf16_round_trip() {
    // Includes a character outside the BMP, which is a surrogate pair in UTF-16.
    for text in ["Björk", "とんかつ", "Ø", "emoji 🎧 here"] {
        let message = Message::new(1, kind::SEARCH, vec![Argument::String(text.into())]);
        let (back, _) = Message::decode(&message.encode()).unwrap();
        assert_eq!(back, message, "{text}");
    }
}

#[test]
fn decodes_the_narrow_number_tags_a_player_may_send() {
    // We always emit 0x11 (four bytes), but a client may use 0x0f or 0x10.
    let mut bytes = header_declaring(kind::RENDER, &[0x06, 0x06]);
    bytes.extend_from_slice(&[0x0f, 0x7b]); // one byte: 123
    bytes.extend_from_slice(&[0x10, 0x01, 0x00]); // two bytes: 256

    let (message, used) = Message::decode(&bytes).unwrap();
    assert_eq!(
        message.arguments,
        vec![Argument::Number(123), Argument::Number(256)]
    );
    assert_eq!(used, bytes.len());
}

#[test]
fn several_messages_in_one_segment_are_all_decoded() {
    let sent = vec![
        menu_header(3, 0x1105, 128),
        Message::new(
            3,
            kind::MENU_ITEM,
            vec![Argument::String("Melodic Vox".into())],
        ),
        menu_footer(3),
    ];
    let mut wire = Vec::new();
    for message in &sent {
        wire.extend_from_slice(&message.encode());
    }

    let (got, used) = Message::decode_all(&wire);
    assert_eq!(got, sent);
    assert_eq!(used, wire.len());
}

#[test]
fn a_message_split_across_segments_is_left_for_the_next_read() {
    let whole = Message::new(
        9,
        kind::MENU_ITEM,
        vec![Argument::String("Tech House".into())],
    );
    let wire = {
        let mut w = menu_header(9, 0x1105, 1).encode();
        w.extend_from_slice(&whole.encode());
        w
    };
    let first = menu_header(9, 0x1105, 1).encode().len();

    // Every split point inside the second message must yield exactly one
    // complete message and leave the partial bytes behind.
    for cut in first + 1..wire.len() {
        let (got, used) = Message::decode_all(&wire[..cut]);
        assert_eq!(got.len(), 1, "cut at {cut}");
        assert_eq!(used, first, "cut at {cut} consumes only the whole message");

        // Feeding the remainder afterwards recovers the rest.
        let (rest, _) = Message::decode_all(&wire[used..]);
        assert_eq!(rest, vec![whole.clone()]);
    }
}

#[test]
fn truncation_is_an_error_rather_than_a_panic() {
    let bytes = Message::new(1, kind::METADATA, vec![Argument::String("Angels".into())]).encode();
    for cut in 0..bytes.len() {
        match Message::decode(&bytes[..cut]) {
            Err(DbError::Truncated { .. }) => {}
            other => panic!("cut at {cut} gave {other:?}"),
        }
    }
}

#[test]
fn a_blob_length_beyond_the_buffer_is_rejected() {
    // A number before the blob saying it is huge, then the blob claims so too.
    let mut bytes = header_declaring(kind::ANLZ_TAG, &[0x06, 0x03]);
    bytes.push(0x11);
    bytes.extend_from_slice(&0xffff_ffff_u32.to_be_bytes());
    bytes.push(0x14);
    bytes.extend_from_slice(&0xffff_ffff_u32.to_be_bytes()); // lies about its size
    assert!(matches!(
        Message::decode(&bytes),
        Err(DbError::Truncated { .. })
    ));
}

#[test]
fn a_string_length_beyond_the_buffer_is_rejected() {
    let mut bytes = header_declaring(kind::SEARCH, &[0x02]);
    bytes.push(0x26);
    bytes.extend_from_slice(&0x4000_0000_u32.to_be_bytes());
    // Must not overflow when the count is doubled to a byte length.
    assert!(matches!(
        Message::decode(&bytes),
        Err(DbError::Truncated { .. })
    ));
}

#[test]
fn a_foreign_packet_is_not_mistaken_for_a_message() {
    let mut bytes = Message::new(1, kind::ROOT_MENU, vec![]).encode();
    bytes[1] = 0x51; // the Pro DJ Link magic starts here
    assert!(matches!(Message::decode(&bytes), Err(DbError::BadMagic(_))));
}

#[test]
fn an_impossible_argument_count_is_rejected() {
    let bytes = header_declaring(kind::ROOT_MENU, &[0x06; 33]);
    assert_eq!(Message::decode(&bytes), Err(DbError::TooManyArguments(33)));
}

#[test]
fn an_unknown_field_tag_is_reported_not_guessed() {
    let mut bytes = header_declaring(kind::ROOT_MENU, &[0x06]);
    bytes.push(0x99);
    assert_eq!(Message::decode(&bytes), Err(DbError::UnknownTag(0x99)));
}

#[test]
fn decode_all_stops_at_garbage_rather_than_looping() {
    let mut wire = menu_header(1, 0x1000, 0).encode();
    wire.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22]);
    let (got, used) = Message::decode_all(&wire);
    assert_eq!(got.len(), 1);
    assert_eq!(used, menu_header(1, 0x1000, 0).encode().len());
}

#[test]
fn setup_uses_the_reserved_transaction_id() {
    let message = setup_request(2);
    assert_eq!(message.transaction, SETUP_TXID);
    assert_eq!(message.kind, kind::SETUP);
    assert_eq!(
        message.arguments,
        vec![
            Argument::Number(2),
            Argument::Number(rbl_dbserver::SETUP_MAGIC)
        ]
    );
    assert_eq!(SETUP_TXID, 0xffff_fffe);
}

#[test]
fn only_device_numbers_one_to_six_are_answerable() {
    // A real server silently ignores 7 and above: the connection succeeds and
    // then nothing ever arrives, so we reject it where it can be explained.
    for device in 1..=6 {
        assert!(
            rbl_dbserver::is_answerable_device(device),
            "device {device}"
        );
    }
    for device in [0_u8, 7, 8, 17, 255] {
        assert!(
            !rbl_dbserver::is_answerable_device(device),
            "device {device}"
        );
    }
}

#[test]
fn the_port_query_string_is_nul_terminated() {
    assert_eq!(
        rbl_dbserver::PORT_QUERY_REQUEST,
        b"\x00\x00\x00\x0fRemoteDBServer\0"
    );
    assert_eq!(rbl_dbserver::PORT_QUERY, 12_523);
}

#[test]
fn a_twelve_byte_zero_padded_tag_list_is_accepted_too() {
    // What alphatheta-connect sends: the count says two, the list is twelve.
    let mut bytes = Message::new(1, kind::ROOT_MENU, vec![]).encode();
    bytes[14] = 2;
    bytes[16..20].copy_from_slice(&12_u32.to_be_bytes());
    bytes.extend_from_slice(&[0x06, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x11, 0, 0, 0, 7, 0x11, 0, 0, 0, 9]);
    let (message, used) = Message::decode(&bytes).unwrap();
    assert_eq!(
        message.arguments,
        vec![Argument::Number(7), Argument::Number(9)]
    );
    assert_eq!(used, bytes.len());
}
