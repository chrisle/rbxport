//! The analysis blobs against the bytes rekordbox 7.2.11 sent a CDJ-3000 for
//! the same track (`fixtures/`: the track's `PQTZ`, `PWAV`, `PWV2`, `PWV3`,
//! `PWV4` and `PQT2` sections, and the captured replies).
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use rbl_anlz::Anlz;
use rbl_index::Cue;
use rbl_link::blobs::{self, Analysis, ExtendedCue};

const DAT: &[u8] = include_bytes!("fixtures/ANLZ0001.DAT");
const EXT: &[u8] = include_bytes!("fixtures/ANLZ0001.EXT");

fn analysis() -> (Anlz, Anlz) {
    (rbl_anlz::parse(DAT).unwrap(), rbl_anlz::parse(EXT).unwrap())
}

#[test]
fn the_beat_grid_matches_the_capture() {
    let (dat, ext) = analysis();
    let a = Analysis {
        dat: Some(&dat),
        ext: Some(&ext),
        two_ex: None,
    };
    assert_eq!(
        a.beat_grid().unwrap(),
        include_bytes!("fixtures/captured-beat-grid.bin")
    );
}

#[test]
fn the_waveform_preview_matches_the_capture() {
    let (dat, ext) = analysis();
    let a = Analysis {
        dat: Some(&dat),
        ext: Some(&ext),
        two_ex: None,
    };
    assert_eq!(
        a.waveform_preview().unwrap(),
        include_bytes!("fixtures/captured-waveform-preview.bin")
    );
}

#[test]
fn the_waveform_detail_matches_the_capture() {
    let (dat, ext) = analysis();
    let a = Analysis {
        dat: Some(&dat),
        ext: Some(&ext),
        two_ex: None,
    };
    assert_eq!(
        a.waveform_detail().unwrap(),
        include_bytes!("fixtures/captured-waveform-detail.bin")
    );
}

#[test]
fn analysis_tags_match_the_capture_including_the_padding() {
    let (dat, ext) = analysis();
    let a = Analysis {
        dat: Some(&dat),
        ext: Some(&ext),
        two_ex: None,
    };
    // PWV4 is a multiple of four already; PQT2 is 810 bytes and goes out as 812.
    assert_eq!(
        a.tag(b"PWV4", b"EXT").unwrap(),
        include_bytes!("fixtures/captured-tag-PWV4.bin")
    );
    assert_eq!(
        a.tag(b"PQT2", b"EXT").unwrap(),
        include_bytes!("fixtures/captured-tag-PQT2.bin")
    );
    assert!(a.tag(b"PWV6", b"2EX").is_none(), "no .2EX here");
    assert!(
        a.tag(b"PSSI", b"EXT").is_none(),
        "not in the trimmed fixture"
    );
}

#[test]
fn the_vbr_compatibility_placeholder_keeps_its_existing_bytes() {
    // The old fixture normalized the final VBR scalar as alleged cue padding.
    // It is not a vendor oracle. Assert the retained zero-filled compatibility
    // policy directly, pending track-specific and unavailable-data evidence.
    assert_eq!(blobs::vbr_compatibility_blob(), vec![0; 1604]);
}

#[test]
fn the_extended_cue_list_matches_the_capture() {
    // The track's cues as `djmdCue` holds them: hot cues A–D and four
    // memory cues at the same places (rekordbox's auto cues). The capture's
    // hot cues are all colour 21, green.
    let cue = |slot: u8, ms: u32| ExtendedCue {
        position_ms: ms,
        out_ms: 0,
        hot_slot: slot,
        comment: "CUE(Auto)".into(),
        colour: if slot == 0 { 0 } else { 21 },
    };
    // Memory cues in rekordbox's table order, which the reply keeps.
    let cues = vec![
        cue(0, 2004),
        cue(0, 467),
        cue(0, 23521),
        cue(0, 201803),
        cue(1, 467),
        cue(2, 83),
        cue(3, 23521),
        cue(4, 201803),
    ];
    let (blob, count) = blobs::extended_cues_blob(&cues);
    assert_eq!(count, 8);
    assert_eq!(blob, include_bytes!("fixtures/captured-extended-cues.bin"));
}

#[test]
fn an_index_cue_becomes_an_extended_cue_by_its_hot_letter() {
    let hot_d = Cue {
        id: 1,
        position_ms: 100,
        out_ms: 0,
        kind: 5,
        colour: 0,
    };
    assert_eq!(ExtendedCue::from(&hot_d).hot_slot, 4, "kind 5 is D");
    let memory = Cue {
        id: 2,
        position_ms: 200,
        out_ms: 900,
        kind: 0,
        colour: 0,
    };
    let e = ExtendedCue::from(&memory);
    assert_eq!((e.hot_slot, e.out_ms), (0, 900));
    // A loop is marked 02 and carries its end.
    let (blob, _) = blobs::extended_cues_blob(&[e]);
    assert_eq!(blob[6], 2);
    assert_eq!(&blob[16..20], &900_u32.to_le_bytes());
}

#[test]
fn the_waveform_preview_sends_pwv2_as_height_alone() {
    // PWV2 written by this app before issue #278 carried whiteness and
    // zero columns; a Nexus player discards the whole preview reply when a
    // PWV2 byte is above 15 and reads a zero column as unfinished. The PWAV
    // columns keep their whiteness byte; a zero height rises to rekordbox's 2.
    let blob = blobs::waveform_preview_blob(&[0xa3, 0x20], &[0x48, 0xef, 0x00, 0x0f]);
    assert_eq!(&blob[..4], &[0x03, 0x05, 0x02, 0x01]);
    assert_eq!(&blob[4..8], &[8, 15, 1, 15]);
}

/// The four colour bytes of the only entry in a reply: code, then RGB,
/// after the comment and the word that follows it.
fn colour_of(blob: &[u8]) -> &[u8] {
    let comment = usize::from(u16::from_le_bytes([blob[0x48], blob[0x49]]));
    &blob[0x4e + comment..0x52 + comment]
}

#[test]
fn a_hot_cue_carries_its_own_colour() {
    // Issue 275: every hot cue was sent as green whatever its colour.
    let hot = |kind: u8, colour: u8| {
        ExtendedCue::from(&Cue {
            id: 1,
            position_ms: 100,
            out_ms: 0,
            kind,
            colour,
        })
    };
    for (kind, colour, expected) in [
        (1, 42, [42, 0xff, 0x00, 0x00]),
        (2, 9, [9, 0x00, 0xe0, 0xff]),
        (3, 30, [30, 0xe6, 0xff, 0x00]),
        (5, 56, [56, 0xb3, 0x00, 0xff]),
    ] {
        let (blob, _) = blobs::extended_cues_blob(&[hot(kind, colour)]);
        assert_eq!(colour_of(&blob), expected, "kind {kind}, colour {colour}");
    }
    // Without a colour, the code is 0 and the RGB the slot's default, as
    // the USB export writes it: A is palette entry 43.
    let (blob, _) = blobs::extended_cues_blob(&[hot(1, 0)]);
    assert_eq!(colour_of(&blob), [0, 0xff, 0x00, 0x17]);
    // A code past the 65-entry palette is sent the same way.
    let (blob, _) = blobs::extended_cues_blob(&[hot(1, 65)]);
    assert_eq!(colour_of(&blob), [0, 0xff, 0x00, 0x17]);
}

#[test]
fn a_memory_cue_carries_no_colour() {
    let memory = ExtendedCue::from(&Cue {
        id: 2,
        position_ms: 200,
        out_ms: 0,
        kind: 0,
        colour: 42,
    });
    let (blob, _) = blobs::extended_cues_blob(&[memory]);
    assert_eq!(colour_of(&blob), [0; 4]);
}
