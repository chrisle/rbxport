//! Actual stock 6.8.7 DBH capture over generated WAVs. No USB/library writes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use rbl_export::cbm;
use std::collections::BTreeMap;

fn stock() -> BTreeMap<String, Vec<u8>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xdj-r1-stock-6.8.7");
    cbm::FILES
        .into_iter()
        .map(|name| (name.into(), std::fs::read(root.join(name)).unwrap()))
        .collect()
}

#[test]
fn stock_two_pcm_tracks_match_every_byte_except_per_file_timestamp() {
    // Inputs independently established from classic PDB and exported WAVs,
    // not reconstructed from Cbm. See the capture's provenance README.
    let tracks: Vec<_> = [(2, "alpha", 225, 529_278), (1, "beta", 300, 705_678)]
        .into_iter()
        .map(|(id, title, frames75, bytes)| cbm::Track {
            id,
            audio: format!("/Contents/UnknownArtist/UnknownAlbum/{title}.wav"),
            filename: format!("{title}.wav"),
            title: title.into(),
            artist: String::new(),
            artist_id: 0,
            album: String::new(),
            album_id: 0,
            genre: String::new(),
            key: String::new(),
            comment: String::new(),
            artwork: String::new(),
            track_number: 0,
            bpm: 15_000,
            hot_cue_auto_load: true,
            bytes,
            data_offset: 78,
            frames75,
            rate: 14_112,
            format: 0x20,
            emphasis: false,
        })
        .collect();
    let generated = cbm::build(
        &tracks,
        &[cbm::Playlist {
            id: 1,
            name: "R1 Comparison Test 20261108".into(),
            tracks: vec![1, 2],
        }],
        1,
        512,
        0,
    )
    .unwrap();
    let captured = stock();
    cbm::validate(&captured).unwrap();
    for name in cbm::FILES {
        let actual = &generated[name];
        let expected = &captured[name];
        assert_eq!(actual.len(), expected.len(), "{name} length");
        let difference = actual
            .iter()
            .zip(expected)
            .enumerate()
            .find(|(offset, (a, b))| !(16..20).contains(offset) && a != b)
            .map(|(offset, (a, b))| {
                format!("{name} offset {offset:#x}: generated {a:#x}, stock {b:#x}")
            });
        assert!(difference.is_none(), "{}", difference.unwrap_or_default());
    }
}

#[test]
fn category_content_end_rejects_wrong_reference_but_reads_legacy_zero() {
    let files = stock();
    let records = cbm::parse_contents(&files[cbm::FILES[0]]).unwrap().records;
    let mut category = files[cbm::FILES[1]].clone();
    category[4..8].copy_from_slice(&1_u32.to_le_bytes());
    assert!(cbm::parse_category(&category, &records)
        .unwrap_err()
        .to_string()
        .contains("content-table end"));
    category[4..8].fill(0);
    assert!(cbm::parse_category(&category, &records).is_ok());
    let empty = cbm::build(&[], &[], 0, 512, 0).unwrap();
    for file in empty.values() {
        assert_eq!(
            u32::from_le_bytes(file[4..8].try_into().unwrap()),
            u32::try_from(cbm::HEADER).unwrap()
        );
    }
}
