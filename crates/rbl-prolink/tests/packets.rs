//! Packet encoding, decoding, and the device table's expiry rules.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::net::Ipv4Addr;

use rbl_prolink::{
    device_name, packet_kind, AnnounceKind, Announcement, Claim, DeviceTable, DeviceType,
    KeepAlive, PacketError, KEEP_ALIVE_LEN, MAGIC, PEER_TIMEOUT_MS, REKORDBOX_DEVICE_NUMBER,
    REKORDBOX_NAME,
};

fn sample() -> KeepAlive {
    KeepAlive::rekordbox(
        [0x00, 0x1e, 0x1d, 0x11, 0x22, 0x33],
        Ipv4Addr::new(192, 168, 1, 42),
        2,
    )
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// rekordbox 7.2.11's keep-alive with one player on the network, verbatim
/// from the capture of 2026-09-12 (MAC 00:e0:4c:cf:63:2e, 192.168.1.14).
const CAPTURED_REKORDBOX_KEEP_ALIVE: &str =
    "5173707431576d4a4f4c060072656b6f7264626f78000000000000000000000001030036110100e04ccf632ec0a8010e020100000408";
/// The CDJ-3000's, from the same capture (player 1, 192.168.1.152).
const CAPTURED_CDJ_KEEP_ALIVE: &str =
    "5173707431576d4a4f4c060043444a2d333030300000000000000000000000000103003601012497ed0b4043c0a80198030000000164";
/// rekordbox's status beacon while the deck was master at 78.08 BPM.
const CAPTURED_REKORDBOX_STATUS: &str =
    "5173707431576d4a4f4c2972656b6f7264626f7800000000000000000000000101110038110000c00010000080001e80001000000009ff01";
/// The player's question about rekordbox's library slot, and rekordbox's
/// answer (38,681 tracks, 627 playlists), then the device-settings request
/// and response, all from the same capture (the CDJ-3000 emulator, EP122
/// firmware, which asks about slot `03`).
const CAPTURED_MEDIA_QUERY: &str =
    "5173707431576d4a4f4c0543444a2d33303030000000000000000000000000010001000cc0a801980000001100000003";
const CAPTURED_MEDIA_RESPONSE: &str =
    "5173707431576d4a4f4c0672656b6f7264626f780000000000000000000000010111009c000000110000000300720065006b006f007200640062006f007800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000009719000001010000027300000000000000000000000000000000";
const CAPTURED_DEVICE_SETTINGS_RESPONSE: &str =
    "5173707431576d4a4f4c4772656b6f7264626f7800000000000000000000000101110024110400001234567800000001010104010101000002000000000000000000000000000000";
/// The RX3's SOURCE-eligibility request and rekordbox's reply, captured from
/// a direct macOS-to-RX3 Link Export session on firmware 1.19.
const CAPTURED_DEVICE_PROPERTY_QUERY: &str =
    "5173707431576d4a4f4c3058444a2d5258330000000000000000000000000001030b0000";
const CAPTURED_DEVICE_PROPERTY_RESPONSE: &str =
    "5173707431576d4a4f4c3172656b6f7264626f78000000000000000000000001031100080600000000000000";
/// The packet rekordbox unicasts to a player that has just connected.
const CAPTURED_CONNECT_GREETING: &str =
    "5173707431576d4a4f4c1672656b6f7264626f7800000000000000000000000101110000000000000000000000000000";
/// rekordbox's answer (`11`) to a player's `10` identity packet, verbatim
/// from the 2026-09-13 capture on the emulator bridge (computer "chrisles-MBP",
/// UTF-16BE), zero-padded to 296 bytes. Without this the player never lists us.
const CAPTURED_CONNECT_IDENTITY: &str = "5173707431576d4a4f4c1172656b6f7264626f78000000000000000000000001011101041101000000630068007200690073006c00650073002d004d0042005000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

#[test]
fn rekordboxs_keep_alive_is_reproduced_byte_for_byte() {
    let alive = KeepAlive::rekordbox(
        [0x00, 0xe0, 0x4c, 0xcf, 0x63, 0x2e],
        Ipv4Addr::new(192, 168, 1, 14),
        2,
    );
    assert_eq!(alive.encode(), hex(CAPTURED_REKORDBOX_KEEP_ALIVE));
    assert_eq!(
        KeepAlive::decode(&hex(CAPTURED_REKORDBOX_KEEP_ALIVE)).unwrap(),
        alive
    );
}

#[test]
fn a_cdj_3000s_keep_alive_decodes_to_a_player() {
    let deck = KeepAlive::decode(&hex(CAPTURED_CDJ_KEEP_ALIVE)).unwrap();
    assert_eq!(deck.name, "CDJ-3000");
    assert_eq!(deck.device_number, 1);
    assert_eq!(deck.device_type, DeviceType::Cdj);
    assert_eq!(deck.ip, Ipv4Addr::new(192, 168, 1, 152));
    assert_eq!(deck.peers, 3);
    assert_eq!(deck.generation, 3);
    assert_eq!(deck.encode(), hex(CAPTURED_CDJ_KEEP_ALIVE));
}

#[test]
fn the_status_beacon_and_the_connect_greeting_match_the_capture() {
    let status = rbl_prolink::Status {
        name: REKORDBOX_NAME.to_owned(),
        device_number: REKORDBOX_DEVICE_NUMBER,
        bpm_x100: 0x1e80,
        beat: 1,
        master: false,
    };
    assert_eq!(status.encode(), hex(CAPTURED_REKORDBOX_STATUS));
    assert_eq!(
        rbl_prolink::connect_greeting(REKORDBOX_NAME, REKORDBOX_DEVICE_NUMBER),
        hex(CAPTURED_CONNECT_GREETING)
    );
}

#[test]
fn the_identity_reply_matches_the_capture() {
    let identity =
        rbl_prolink::connect_identity(REKORDBOX_NAME, REKORDBOX_DEVICE_NUMBER, "chrisles-MBP");
    assert_eq!(identity.len(), rbl_prolink::CONNECT_IDENTITY_LEN);
    assert_eq!(identity, hex(CAPTURED_CONNECT_IDENTITY));
}

#[test]
fn the_rx3_device_property_exchange_matches_the_capture() {
    let query =
        rbl_prolink::DevicePropertyQuery::decode(&hex(CAPTURED_DEVICE_PROPERTY_QUERY)).unwrap();
    assert_eq!(query.name, "XDJ-RX3");
    assert_eq!(query.requester, 0x0b);

    let response = rbl_prolink::DevicePropertyResponse {
        name: REKORDBOX_NAME.to_owned(),
        device_number: REKORDBOX_DEVICE_NUMBER,
    };
    assert_eq!(response.encode(), hex(CAPTURED_DEVICE_PROPERTY_RESPONSE));
}

#[test]
fn a_keep_alive_round_trips() {
    let original = sample();
    let bytes = original.encode();
    assert_eq!(bytes.len(), KEEP_ALIVE_LEN);
    assert_eq!(KeepAlive::decode(&bytes).unwrap(), original);
}

#[test]
fn every_packet_starts_with_the_dj_link_magic() {
    for bytes in [
        sample().encode(),
        Announcement {
            name: "rekordbox".into(),
            device_type: DeviceType::Rekordbox,
        }
        .encode(),
        Claim {
            stage: AnnounceKind::ClaimStage1,
            name: "rekordbox".into(),
            device_number: 17,
            mac: [1, 2, 3, 4, 5, 6],
            ip: Ipv4Addr::LOCALHOST,
            repeat: 1,
            device_type: DeviceType::Rekordbox,
        }
        .encode(),
    ] {
        assert_eq!(bytes.get(0..10), Some(&MAGIC[..]), "magic missing");
    }
}

#[test]
fn the_device_name_sits_at_a_fixed_offset_and_is_nul_padded() {
    let bytes = sample().encode();
    assert_eq!(device_name(&bytes).unwrap(), "rekordbox");
    // Twenty bytes, so the field is padded rather than truncated.
    assert_eq!(&bytes[0x0c..0x0c + 9], b"rekordbox");
    assert!(
        bytes[0x15..0x20].iter().all(|&b| b == 0),
        "name must be NUL-padded"
    );
}

#[test]
fn a_long_name_is_truncated_to_the_field_rather_than_overflowing() {
    let mut alive = sample();
    alive.name = "a-very-long-device-name-that-will-not-fit".into();
    let bytes = alive.encode();
    assert_eq!(
        bytes.len(),
        KEEP_ALIVE_LEN,
        "the packet must stay a fixed size"
    );
    assert_eq!(device_name(&bytes).unwrap().len(), 20);
}

#[test]
fn keep_alive_fields_land_where_the_protocol_says() {
    let bytes = sample().encode();
    assert_eq!(bytes[0x0a], 0x06, "kind: keep-alive");
    assert_eq!(bytes[0x21], 0x03, "generation");
    assert_eq!(bytes[0x34], DeviceType::Rekordbox.to_u8());
    assert_eq!(
        u16::from_be_bytes([bytes[0x22], bytes[0x23]]) as usize,
        KEEP_ALIVE_LEN
    );
    assert_eq!(bytes[0x24], REKORDBOX_DEVICE_NUMBER);
    assert_eq!(&bytes[0x26..0x2c], &[0x00, 0x1e, 0x1d, 0x11, 0x22, 0x33]);
    assert_eq!(&bytes[0x2c..0x30], &[192, 168, 1, 42]);
    assert_eq!(bytes[0x30], 2, "peer count");
}

#[test]
fn rubbish_is_rejected_rather_than_misread() {
    assert_eq!(KeepAlive::decode(&[]), Err(PacketError::TooShort(0)));
    assert_eq!(packet_kind(&[0; 4]), Err(PacketError::TooShort(4)));

    let mut wrong = sample().encode();
    wrong[0] = 0xff;
    assert_eq!(packet_kind(&wrong), Err(PacketError::BadMagic));
    assert_eq!(device_name(&wrong), Err(PacketError::BadMagic));
}

#[test]
fn a_packet_of_the_wrong_kind_is_refused() {
    let mut bytes = sample().encode();
    bytes[0x0a] = AnnounceKind::Announce.to_u8();
    assert_eq!(KeepAlive::decode(&bytes), Err(PacketError::WrongKind(0x0a)));
}

#[test]
fn announce_kinds_round_trip() {
    for kind in [
        AnnounceKind::ClaimStage1,
        AnnounceKind::ClaimStage2,
        AnnounceKind::ClaimFinal,
        AnnounceKind::KeepAlive,
        AnnounceKind::Conflict,
        AnnounceKind::Announce,
        AnnounceKind::Other(0x7f),
    ] {
        assert_eq!(AnnounceKind::from_u8(kind.to_u8()), kind);
    }
}

#[test]
fn device_types_round_trip() {
    for kind in [
        DeviceType::Cdj,
        DeviceType::Mixer,
        DeviceType::Rekordbox,
        DeviceType::AllInOne,
        DeviceType::Other(9),
    ] {
        assert_eq!(DeviceType::from_u8(kind.to_u8()), kind);
    }
}

#[test]
fn the_first_claim_stage_is_shorter_than_the_later_ones() {
    let base = Claim {
        stage: AnnounceKind::ClaimStage1,
        name: "rekordbox".into(),
        device_number: 17,
        mac: [1, 2, 3, 4, 5, 6],
        ip: Ipv4Addr::new(10, 0, 0, 5),
        repeat: 1,
        device_type: DeviceType::Rekordbox,
    };
    let first = base.encode();
    let second = Claim {
        stage: AnnounceKind::ClaimStage2,
        ..base.clone()
    }
    .encode();
    assert!(
        second.len() > first.len(),
        "later stages carry the IP and number"
    );
    assert_eq!(first[0x0a], 0x00);
    assert_eq!(second[0x0a], 0x02);
    // The device number being claimed appears in the later stages.
    assert!(second.contains(&17));
}

// ---- device table ----

#[test]
fn the_table_records_and_updates_a_peer() {
    let mut table = DeviceTable::new();
    table.observe(&sample(), 1_000);
    assert_eq!(table.len(), 1);
    assert_eq!(table.peers()[0].name, "rekordbox");

    // A second keep-alive from the same device updates rather than duplicates.
    let mut moved = sample();
    moved.ip = Ipv4Addr::new(192, 168, 1, 99);
    table.observe(&moved, 2_000);
    assert_eq!(table.len(), 1);
    assert_eq!(table.peers()[0].ip, Ipv4Addr::new(192, 168, 1, 99));
    assert_eq!(table.peers()[0].last_seen_ms, 2_000);
}

#[test]
fn occupancy_block_has_distinct_masks_counter_and_truncation_boundary() {
    use rbl_prolink::{ConnectionMode, NumberBlock, NumberProbe, PacketError};
    let mut wire = rbl_prolink::rekordbox_claim_stage2([1, 2, 3, 4, 5, 6],
        Ipv4Addr::new(192, 168, 1, 20), 99, 7);
    wire.resize(68, 0);
    wire[11] = 2;
    wire[34..36].copy_from_slice(&68_u16.to_be_bytes());
    wire[48] = 0b10;
    wire[51] = 0b0101;
    wire[67] = 255;
    let block = NumberBlock::decode(&wire).unwrap();
    assert_eq!(block.ip, Ipv4Addr::new(192, 168, 1, 20));
    assert_eq!(block.mac, [1, 2, 3, 4, 5, 6]);
    assert_eq!(block.counter, 255);
    assert!(!block.names(17, ConnectionMode::Wired));
    assert!(block.names(18, ConnectionMode::Wired));
    assert!(block.names(41, ConnectionMode::Wireless));
    assert!(!block.names(42, ConnectionMode::Wireless));
    assert!(block.names(43, ConnectionMode::Wireless));
    assert!(!block.names(44, ConnectionMode::Wireless));
    for number in 0..=255 {
        assert!(!block.names(number, ConnectionMode::Unknown));
    }
    assert_eq!(NumberProbe::decode(&wire), Err(PacketError::WrongSubtype(2)));
    for cut in 0..68 {
        assert_eq!(NumberBlock::decode(&wire[..cut]), Err(PacketError::TooShort(cut)));
    }
    wire[11] = 0;
    assert_eq!(NumberBlock::decode(&wire), Err(PacketError::WrongSubtype(0)));
    wire[11] = 2;
    wire[10] = 6;
    assert_eq!(NumberBlock::decode(&wire), Err(PacketError::WrongKind(6)));
    wire[0] = 0;
    assert_eq!(NumberBlock::decode(&wire), Err(PacketError::BadMagic));
}

#[test]
fn a_wrong_keepalive_subtype_cannot_mark_probing_occupancy() {
    let mut wire = hex(CAPTURED_CDJ_KEEP_ALIVE);
    wire[11] = 1;
    assert_eq!(rbl_prolink::KeepAlive::decode(&wire), Err(rbl_prolink::PacketError::WrongSubtype(1)));
}

#[test]
fn a_peer_that_goes_quiet_is_dropped() {
    let mut table = DeviceTable::new();
    table.observe(&sample(), 0);
    assert_eq!(table.expire(PEER_TIMEOUT_MS), 0, "still within the timeout");
    assert_eq!(table.len(), 1);
    assert_eq!(
        table.expire(PEER_TIMEOUT_MS + 1),
        1,
        "a player unplugged mid-set stops announcing"
    );
    assert!(table.is_empty());
}

#[test]
fn several_devices_are_tracked_separately() {
    let mut table = DeviceTable::new();
    for number in [1, 2, 17] {
        let mut alive = sample();
        alive.device_number = number;
        alive.name = format!("device-{number}");
        table.observe(&alive, 0);
    }
    assert_eq!(table.len(), 3);
    assert_eq!(
        table.free_device_number(17),
        18,
        "17 is taken, so pick the next free one"
    );
    assert_eq!(table.free_device_number(20), 20, "20 is free");
}

#[test]
fn an_empty_table_hands_back_the_preferred_number() {
    assert_eq!(
        DeviceTable::new().free_device_number(REKORDBOX_DEVICE_NUMBER),
        REKORDBOX_DEVICE_NUMBER
    );
}

#[test]
fn the_media_query_decodes_and_the_response_matches_the_capture() {
    let query = rbl_prolink::MediaQuery::decode(&hex(CAPTURED_MEDIA_QUERY)).unwrap();
    assert_eq!(query.name, "CDJ-3000");
    assert_eq!(query.from, Ipv4Addr::new(192, 168, 1, 152));
    assert_eq!(query.device_number, REKORDBOX_DEVICE_NUMBER);
    assert_eq!(query.slot, rbl_prolink::SLOT_REKORDBOX_LEGACY);

    let response = rbl_prolink::MediaResponse {
        name: REKORDBOX_NAME.to_owned(),
        device_number: REKORDBOX_DEVICE_NUMBER,
        slot: query.slot,
        tracks: 38_681,
        playlists: 627,
    };
    assert_eq!(response.encode(), hex(CAPTURED_MEDIA_RESPONSE));
}

/// A real CDJ-3000 (player 1 at 192.168.1.170, current firmware) asks about
/// slot `04`, not the emulator's `03`, and rekordbox 7.2 answers naming `04`
/// back with its counts (54 tracks, 2 playlists on the capture machine);
/// everything else in the answer is as it is for `03`. Captured 2026-09-13
/// on chris-win11. An answer that names `03` to this player is ignored, and
/// the player never lists the library.
const CAPTURED_MEDIA_QUERY_SLOT_4: &str =
    "5173707431576d4a4f4c0543444a2d33303030000000000000000000000000010001000cc0a801aa0000001100000004";
const CAPTURED_MEDIA_RESPONSE_SLOT_4: &str = "5173707431576d4a4f4c0672656b6f7264626f780000000000000000000000010111009c000000110000000400720065006b006f007200640062006f007800000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000036000001010000000200000000000000000000000000000000";

#[test]
fn a_current_cdj_3000_asks_about_slot_4_and_the_answer_names_it_back() {
    let query = rbl_prolink::MediaQuery::decode(&hex(CAPTURED_MEDIA_QUERY_SLOT_4)).unwrap();
    assert_eq!(query.name, "CDJ-3000");
    assert_eq!(query.from, Ipv4Addr::new(192, 168, 1, 170));
    assert_eq!(query.device_number, REKORDBOX_DEVICE_NUMBER);
    assert_eq!(query.slot, rbl_prolink::SLOT_REKORDBOX);
    assert_ne!(query.slot, rbl_prolink::SLOT_REKORDBOX_LEGACY);

    let response = rbl_prolink::MediaResponse {
        name: REKORDBOX_NAME.to_owned(),
        device_number: REKORDBOX_DEVICE_NUMBER,
        slot: query.slot,
        tracks: 54,
        playlists: 2,
    };
    assert_eq!(response.encode(), hex(CAPTURED_MEDIA_RESPONSE_SLOT_4));
}

#[test]
fn the_device_settings_response_matches_the_capture() {
    assert_eq!(
        rbl_prolink::device_settings_response(
            REKORDBOX_NAME,
            REKORDBOX_DEVICE_NUMBER,
            rbl_prolink::DeviceSettings::default(),
        ),
        hex(CAPTURED_DEVICE_SETTINGS_RESPONSE)
    );
}

#[test]
fn device_settings_response_encodes_each_decoded_field() {
    let response = rbl_prolink::device_settings_response(
        REKORDBOX_NAME,
        REKORDBOX_DEVICE_NUMBER,
        rbl_prolink::DeviceSettings {
            overview_waveform: rbl_prolink::OverviewWaveform::Full,
            waveform_color: rbl_prolink::WaveformColor::Rgb,
            key_display: rbl_prolink::KeyDisplay::Alphanumeric,
            waveform_position: rbl_prolink::WaveformPosition::Left,
        },
    );

    assert_eq!(&response[0x30..0x36], &[1, 2, 3, 1, 2, 2]);
}

#[test]
fn device_settings_deserialize_from_the_dj_system_values() {
    let settings: rbl_prolink::DeviceSettings = serde_json::from_value(serde_json::json!({
        "waveformColor": "3band",
        "waveformPosition": "left",
        "overviewWaveform": "full",
        "keyDisplay": "alphanumeric",
    }))
    .unwrap();

    assert_eq!(
        settings,
        rbl_prolink::DeviceSettings {
            overview_waveform: rbl_prolink::OverviewWaveform::Full,
            waveform_color: rbl_prolink::WaveformColor::ThreeBand,
            key_display: rbl_prolink::KeyDisplay::Alphanumeric,
            waveform_position: rbl_prolink::WaveformPosition::Left,
        }
    );
}

#[test]
fn alphanumeric_device_settings_change_only_the_key_display_byte() {
    let mut expected = hex(CAPTURED_DEVICE_SETTINGS_RESPONSE);
    expected[0x34] = 2;

    let response = rbl_prolink::device_settings_response(
        REKORDBOX_NAME,
        REKORDBOX_DEVICE_NUMBER,
        rbl_prolink::DeviceSettings {
            key_display: rbl_prolink::KeyDisplay::Alphanumeric,
            ..rbl_prolink::DeviceSettings::default()
        },
    );

    assert_eq!(response, expected);
}

#[test]
fn the_rekordbox_startup_ladder_matches_the_capture() {
    let mac = [0x00, 0xe0, 0x4c, 0xcf, 0x63, 0x2e];
    let ip = Ipv4Addr::new(192, 168, 1, 14);
    assert_eq!(
        rbl_prolink::rekordbox_claim_stage1(mac, 1),
        hex("5173707431576d4a4f4c000072656b6f7264626f7800000000000000000000000103002c010400e04ccf632e")
    );
    assert_eq!(
        rbl_prolink::rekordbox_claim_stage2(mac, ip, 0x11, 1),
        hex("5173707431576d4a4f4c020072656b6f7264626f78000000000000000000000001030032c0a8010e00e04ccf632e11010401")
    );
    assert_eq!(
        rbl_prolink::rekordbox_claim_stage2(mac, ip, 0x2c, 6),
        hex("5173707431576d4a4f4c020072656b6f7264626f78000000000000000000000001030032c0a8010e00e04ccf632e2c060401")
    );
    // 3 first-stage + 6 numbers × 6 counters.
    assert_eq!(rbl_prolink::rekordbox_startup_ladder(mac, ip).len(), 3 + 36);
}

/// rekordbox's status with nothing loaded (cold-start capture, 2026-09-12):
/// the master byte is 0x00 and the beat is 0.
const CAPTURED_REKORDBOX_STATUS_IDLE: &str =
    "5173707431576d4a4f4c2972656b6f7264626f7800000000000000000000000101110038110000c00010000000000000001000000009ff00";

#[test]
fn the_idle_status_matches_the_capture() {
    let status = rbl_prolink::Status {
        name: REKORDBOX_NAME.to_owned(),
        device_number: REKORDBOX_DEVICE_NUMBER,
        bpm_x100: 0,
        beat: 0,
        master: false,
    };
    assert_eq!(status.encode(), hex(CAPTURED_REKORDBOX_STATUS_IDLE));
}

/// rekordbox 7.2 as the network's tempo master at 130.00 BPM, verbatim from
/// the wire (2026-09-14, rekordbox on this Mac): the status flag is `0xe0`
/// (the master bit set, where a non-master sends `0xc0`) and `Mm` at `0x34`
/// is `0x01`. Everything else matches the mirror status.
const CAPTURED_MASTER_STATUS_BEAT_1: &str =
    "5173707431576d4a4f4c2972656b6f7264626f7800000000000000000000000101110038110000e000100000800032c8001000000109ff01";

#[test]
fn the_master_status_matches_the_capture() {
    let status = rbl_prolink::Status {
        name: REKORDBOX_NAME.to_owned(),
        device_number: REKORDBOX_DEVICE_NUMBER,
        bpm_x100: 13_000,
        beat: 1,
        master: true,
    };
    assert_eq!(status.encode(), hex(CAPTURED_MASTER_STATUS_BEAT_1));
    // The master bit and Mm are the only difference from a mirror status.
    let mirror = rbl_prolink::Status {
        master: false,
        ..status.clone()
    }
    .encode();
    let master = status.encode();
    let differing: Vec<usize> = (0..master.len())
        .filter(|&i| master[i] != mirror[i])
        .collect();
    assert_eq!(differing, vec![0x27, 0x34]);
}

/// rekordbox 7.2 broadcasting beat packets as master at 130.00 BPM, one per
/// beat across a whole bar, verbatim from the wire (2026-09-14). The six
/// timing fields track the beat within the bar; everything else is fixed.
const CAPTURED_BEATS: [(&str, u8); 4] = [
    ("5173707431576d4a4f4c2872656b6f7264626f780000000000000000000000010111003c000001cd0000039b000007360000073600000e6c00000e6cffffffffffffffffffffffffffffffffffffffffffffffff00100000000032c801000011", 1),
    ("5173707431576d4a4f4c2872656b6f7264626f780000000000000000000000010111003c000001cd0000039b000005680000073600000c9e00000e6cffffffffffffffffffffffffffffffffffffffffffffffff00100000000032c802000011", 2),
    ("5173707431576d4a4f4c2872656b6f7264626f780000000000000000000000010111003c000001cd0000039b0000039b0000073600000ad100000e6cffffffffffffffffffffffffffffffffffffffffffffffff00100000000032c803000011", 3),
    ("5173707431576d4a4f4c2872656b6f7264626f780000000000000000000000010111003c000001cd0000039b000001cd000007360000090300000e6cffffffffffffffffffffffffffffffffffffffffffffffff00100000000032c804000011", 4),
];

#[test]
fn the_beat_packet_is_rekordboxs_byte_for_byte_across_a_bar() {
    for (captured, beat) in CAPTURED_BEATS {
        let packet =
            rbl_prolink::beat_packet(REKORDBOX_NAME, REKORDBOX_DEVICE_NUMBER, 13_000, beat);
        assert_eq!(packet.len(), rbl_prolink::BEAT_LEN);
        assert_eq!(packet, hex(captured), "beat {beat}");
        assert_eq!(
            rbl_prolink::packet_kind(&packet).unwrap(),
            rbl_prolink::BEAT_KIND
        );
    }
}

/// rekordbox 7.2 telling player 1 (a real CDJ-3000 at 192.168.1.170) to load
/// track 19,925,719 from its own collection, and the same to player 2 for
/// track 73,561,055, verbatim from the wire (2026-09-13, rekordbox on
/// chris-win11; the player answered `1a` two milliseconds later and mounted
/// the export). Note the slot at `0x29`: `04`, where the djl-analysis figure
/// shows `03`; and rekordbox's own `01` at `0x20` and `32` at `0x4b`.
const CAPTURED_LOAD_TRACK_PLAYER_1: &str =
    "5173707431576d4a4f4c1972656b6f7264626f7800000000000000000000000101110034110000001104010001300ad700000032000000000000000000000000000000000000000000000032000000000000000000000000";
const CAPTURED_LOAD_TRACK_PLAYER_2: &str =
    "5173707431576d4a4f4c1972656b6f7264626f78000000000000000000000001011100341100000011040100046273df00000032000000000000000000000000010000000000000000000032000000000000000000000000";

#[test]
fn the_load_track_command_is_rekordboxs_byte_for_byte() {
    let to_player_1 =
        rbl_prolink::load_track_command(REKORDBOX_NAME, REKORDBOX_DEVICE_NUMBER, 1, 19_925_719);
    assert_eq!(to_player_1.len(), rbl_prolink::LOAD_TRACK_LEN);
    assert_eq!(to_player_1, hex(CAPTURED_LOAD_TRACK_PLAYER_1));
    let to_player_2 =
        rbl_prolink::load_track_command(REKORDBOX_NAME, REKORDBOX_DEVICE_NUMBER, 2, 73_561_055);
    assert_eq!(to_player_2, hex(CAPTURED_LOAD_TRACK_PLAYER_2));
}

/// The fields a caller varies, by offset, so a wrong id or player is named
/// rather than shown as a byte-string diff.
#[test]
fn the_load_track_command_names_the_track_and_the_player() {
    let packet =
        rbl_prolink::load_track_command(REKORDBOX_NAME, REKORDBOX_DEVICE_NUMBER, 0x02, 0x1234_5678);
    assert_eq!(packet[0x0a], rbl_prolink::LOAD_TRACK_KIND);
    assert_eq!(
        rbl_prolink::status_device_name(&packet).unwrap(),
        REKORDBOX_NAME
    );
    assert_eq!(packet[0x21], REKORDBOX_DEVICE_NUMBER, "our device number");
    assert_eq!(
        packet[0x28], REKORDBOX_DEVICE_NUMBER,
        "the track's source device"
    );
    assert_eq!(
        packet[0x29],
        rbl_prolink::SLOT_REKORDBOX,
        "the rekordbox slot"
    );
    assert_eq!(
        packet[0x2a],
        rbl_prolink::TRACK_TYPE_REKORDBOX,
        "a rekordbox track"
    );
    assert_eq!(
        &packet[0x2c..0x30],
        &0x1234_5678_u32.to_be_bytes(),
        "the track id"
    );
    assert_eq!(
        packet[0x40], 0x01,
        "the player to load onto, counted from zero"
    );
}

/// What a CDJ-3000 (EP122, player 3) sent back to our status port within a
/// millisecond of the load command, verbatim from the wire (2026-09-13,
/// verification/link/push-load-cdj3000-emu-20260913.pcap): kind `1a`, its own
/// name, and it then reported the track loaded from device 17 in its status.
/// A real CDJ-3000 (player 1) answered rekordbox the same way, its own number
/// in place of `03`.
const CAPTURED_LOAD_TRACK_ACK: &str =
    "5173707431576d4a4f4c1a43444a2d33303030000000000000000000000000010003000403010000";
const CAPTURED_LOAD_TRACK_ACK_PLAYER_1: &str =
    "5173707431576d4a4f4c1a43444a2d33303030000000000000000000000000010001000401010000";

#[test]
fn a_cdj_3000_acknowledges_a_load_track_command() {
    for (captured, player) in [
        (CAPTURED_LOAD_TRACK_ACK, 0x03),
        (CAPTURED_LOAD_TRACK_ACK_PLAYER_1, 0x01),
    ] {
        let bytes = hex(captured);
        assert_eq!(
            rbl_prolink::packet_kind(&bytes).unwrap(),
            rbl_prolink::LOAD_TRACK_ACK_KIND
        );
        assert_eq!(rbl_prolink::status_device_name(&bytes).unwrap(), "CDJ-3000");
        assert_eq!(bytes[0x21], player, "the player that accepted it");
    }
}

#[test]
fn load_track_response_preserves_raw_fields_and_rejects_malformed_frames() {
    use rbl_prolink::{LoadTrackResponse, PacketError};
    let mut wire = hex(CAPTURED_LOAD_TRACK_ACK);
    let response = LoadTrackResponse::decode(&wire).unwrap();
    assert_eq!(response.name, "CDJ-3000");
    assert_eq!(response.fields, [3, 1, 0, 0]);
    for cut in 0..40 {
        assert_eq!(LoadTrackResponse::decode(&wire[..cut]), Err(PacketError::TooShort(cut)));
    }
    wire[36..40].copy_from_slice(&[255, 0, 17, 42]);
    assert_eq!(LoadTrackResponse::decode(&wire).unwrap().fields, [255, 0, 17, 42]);
    wire[35] = 5;
    assert_eq!(LoadTrackResponse::decode(&wire), Err(PacketError::WrongLength(5)));
    wire[35] = 4;
    wire[10] = 0x19;
    assert_eq!(LoadTrackResponse::decode(&wire), Err(PacketError::WrongKind(0x19)));
    wire[0] = 0;
    assert_eq!(LoadTrackResponse::decode(&wire), Err(PacketError::BadMagic));
}

/// A DJM-V5's keep-alive, verbatim from the wire (2026-09-13, device 33 at
/// 192.168.1.66): it announces device type `03`, not the community-documented
/// `02`, and must still be read as a mixer or it shows up as a nameless
/// "device" instead of the MIXER the link strip draws.
const CAPTURED_DJM_V5_KEEP_ALIVE: &str =
    "5173707431576d4a4f4c0600444a4d2d56350000000000000000000000000000010200362102c83dfc1dfea2c0a80142040000000331";

#[test]
fn a_djm_v5_is_read_as_a_mixer() {
    let bytes = hex(CAPTURED_DJM_V5_KEEP_ALIVE);
    let mixer = KeepAlive::decode(&bytes).unwrap();
    assert_eq!(mixer.name, "DJM-V5");
    assert_eq!(mixer.device_number, 33);
    assert_eq!(mixer.device_type, DeviceType::Mixer);
    // The documented value still decodes the same way.
    assert_eq!(DeviceType::from_u8(0x02), DeviceType::Mixer);
}

#[test]
fn synthetic_members_do_not_own_or_refresh_received_keepalive_timers() {
    let mut table = DeviceTable::new();
    let mut primary = sample();
    primary.device_number = 9;
    table.observe_with_flags(&primary, 1_000, [0xa5, 0x5a]);
    let mut secondary = primary.clone();
    secondary.device_number = 10;
    table.observe_synthetic(&secondary, 1_000, Some([0xa5, 0x5a]));
    assert!(table.is_synthetic(10));
    assert_eq!(table.peers()[1].member_flags, Some([0xa5, 0x5a]));
    assert!(table.expire_received(7_000).is_empty());
    assert_eq!(table.expire_received(7_001), vec![9]);
    assert_eq!(table.peers()[0].device_number, 10);

    // A real keepalive promotes the identity to an independent timer owner.
    table.observe_with_flags(&secondary, 8_000, [1, 2]);
    assert!(!table.is_synthetic(10));
    secondary.ip = Ipv4Addr::new(192, 168, 1, 99);
    table.observe_synthetic(&secondary, 13_999, Some([3, 4]));
    assert_eq!(table.peers()[0].ip, secondary.ip);
    assert_eq!(table.peers()[0].member_flags, Some([3, 4]));
    assert_eq!(table.peers()[0].last_seen_ms, 8_000);
    assert!(table.expire_received(14_000).is_empty());
    assert_eq!(table.expire_received(14_001), vec![10]);

    // After the old timer has fired, virtual recreation does not rearm it.
    table.observe_synthetic(&secondary, 15_000, Some([5, 6]));
    assert!(table.is_synthetic(10));
    assert!(table.expire_received(u64::MAX).is_empty());
}

#[test]
fn pending_slot_timer_survives_removal_and_synthetic_recreation() {
    let mut table = DeviceTable::new();
    let mut secondary = sample(); secondary.device_number = 10;
    table.observe(&secondary, 0);
    table.remove_number(10);
    table.observe_synthetic(&secondary, 1_000, None);
    assert!(!table.is_synthetic(10));
    assert_eq!(table.peers()[0].last_seen_ms, 0);
    assert!(table.expire_received(6_000).is_empty());
    assert_eq!(table.expire_received(6_001), vec![10]);
    assert!(table.is_empty());

    // A timer firing while its slot is inactive is consumed, not suspended.
    table.observe(&secondary, 10_000);
    table.remove_number(10);
    assert!(table.expire_received(16_001).is_empty());
    table.observe_synthetic(&secondary, 17_000, None);
    assert!(table.is_synthetic(10));
    assert_eq!(table.peers()[0].last_seen_ms, 17_000);
    assert!(table.expire_received(u64::MAX).is_empty());

    // A new direct reception replaces the pending deadline, including after removal.
    table.observe(&secondary, 20_000);
    table.remove_number(10);
    table.observe(&secondary, 25_000);
    assert!(table.expire_received(26_001).is_empty());
    assert!(table.expire_received(31_000).is_empty());
    assert_eq!(table.expire_received(31_001), vec![10]);
}

#[test]
fn rejection_disconnect_is_the_complete_pre_clear_41_byte_frame() {
    assert_eq!(rbl_prolink::rejection_disconnect(18, Ipv4Addr::new(192,168,50,2)),
        hex("5173707431576d4a4f4c080072656b6f7264626f7800000000000000000000000103002912c0a83202"));
    assert_eq!(rbl_prolink::rejection_disconnect(41, Ipv4Addr::UNSPECIFIED),
        hex("5173707431576d4a4f4c080072656b6f7264626f780000000000000000000000010300292900000000"));
}
