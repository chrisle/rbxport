//! `PSSI` (song structure) and `PVDI` (vocals): the two tags whose layout is
//! decided by a mask and a header word rather than by the section framing.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_anlz::{AnlzBuilder, Mood};
use rbl_core::FourCc;

const MASK_BASE: [u8; 19] = [
    0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3, 0xE8,
    0xE9, 0xF4, 0xE1,
];

/// One 24-byte phrase entry, addressed by the offsets the real files use.
fn entry(index: u16, beat: u16, kind: u16, k1: u8, k2: u8, k3: u8, fill: Option<u16>) -> Vec<u8> {
    let mut e = vec![0_u8; 24];
    e[0..2].copy_from_slice(&index.to_be_bytes());
    e[2..4].copy_from_slice(&beat.to_be_bytes());
    e[4..6].copy_from_slice(&kind.to_be_bytes());
    e[0x07] = k1;
    e[0x09] = k2;
    e[0x13] = k3;
    if let Some(at) = fill {
        e[0x15] = 1;
        e[0x16..0x18].copy_from_slice(&at.to_be_bytes());
    }
    e
}

/// The tag body: everything from byte `0x12`, which is the part the mask
/// covers.
fn body(mood: u16, end_beat: u16, bank: u8, entries: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0_u8; 14];
    b[0..2].copy_from_slice(&mood.to_be_bytes());
    b[8..10].copy_from_slice(&end_beat.to_be_bytes());
    b[0x0c] = bank;
    for e in entries {
        b.extend_from_slice(e);
    }
    b
}

fn mask(body: &mut [u8], len_entries: u16) {
    for (at, byte) in body.iter_mut().enumerate() {
        *byte ^= MASK_BASE[at % 19].wrapping_add(len_entries as u8);
    }
}

/// A `PSSI` section split the way rekordbox 7 writes it locally: `len_header`
/// 32, so the whole tag header including the mood lands in `Section::header`.
fn pssi_local(mood: u16, end_beat: u16, bank: u8, entries: &[Vec<u8>], masked: bool) -> Vec<u8> {
    let len_entries = entries.len() as u16;
    let mut b = body(mood, end_beat, bank, entries);
    if masked {
        mask(&mut b, len_entries);
    }
    let mut header = Vec::new();
    header.extend_from_slice(&24_u32.to_be_bytes());
    header.extend_from_slice(&len_entries.to_be_bytes());
    let mut all = header;
    all.extend_from_slice(&b);

    let mut builder = AnlzBuilder::new();
    // 20 header bytes puts the split at 0x20, where the entries start.
    builder.raw(FourCc::new(b"PSSI"), all[..20].to_vec(), all[20..].to_vec());
    builder.finish()
}

/// The same tag split the way an exported `.EXT` carries it: `len_header` 20,
/// so the mood and everything after it is payload.
fn pssi_exported(mood: u16, entries: &[Vec<u8>], masked: bool) -> Vec<u8> {
    let len_entries = entries.len() as u16;
    let mut b = body(mood, 0, 0, entries);
    if masked {
        mask(&mut b, len_entries);
    }
    let mut header = Vec::new();
    header.extend_from_slice(&24_u32.to_be_bytes());
    header.extend_from_slice(&len_entries.to_be_bytes());

    let mut builder = AnlzBuilder::new();
    builder.raw(FourCc::new(b"PSSI"), header, b);
    builder.finish()
}

fn labels(bytes: &[u8]) -> Vec<&'static str> {
    rbl_anlz::parse(bytes).unwrap().phrases().unwrap().iter().map(|p| p.label).collect()
}

fn versioned_pssi(version: u16, entries: &[Vec<u8>], masked: bool) -> Vec<u8> {
    let count = u16::try_from(entries.len()).unwrap();
    let mut b = body(1, 129, 0, entries);
    if masked {
        for (at, byte) in b.iter_mut().enumerate() {
            *byte ^= MASK_BASE[(at + usize::from(version)) % MASK_BASE.len()]
                .wrapping_add(count as u8);
        }
    }
    let mut raw = version.to_be_bytes().to_vec();
    raw.extend_from_slice(&24_u16.to_be_bytes());
    raw.extend_from_slice(&count.to_be_bytes());
    raw.extend_from_slice(&b);
    let mut builder = AnlzBuilder::new();
    builder.raw(FourCc::new(b"PSSI"), raw[..20].to_vec(), raw[20..].to_vec());
    builder.finish()
}

#[test]
fn version_one_phrases_survive_plain_and_masked_reads() {
    let entries = [entry(1, 1, 1, 1, 0, 0, None), entry(2, 65, 3, 0, 0, 0, None)];
    for masked in [false, true] {
        let anlz = rbl_anlz::parse(&versioned_pssi(1, &entries, masked)).unwrap();
        let phrases = anlz.phrases().unwrap();
        assert_eq!(phrases.iter().map(|p| (p.beat, p.label)).collect::<Vec<_>>(), [(1, "INTRO 1"), (65, "DOWN")]);
    }
}

#[test]
fn version_one_export_mask_and_edits_retain_the_version_and_phrases() {
    let entries = [entry(1, 1, 1, 1, 0, 0, None), entry(2, 65, 3, 0, 0, 0, None)];
    let plain = rbl_anlz::parse(&versioned_pssi(1, &entries, false)).unwrap();
    let masked = rbl_anlz::parse(&versioned_pssi(1, &entries, true)).unwrap();
    let section = &plain.sections[0];
    assert_eq!(section.with_export_phrase_mask(), masked.sections[0]);
    assert_eq!(masked.sections[0].with_export_phrase_mask(), masked.sections[0]);
    let edited = masked.sections[0].with_phrase_edit(rbl_anlz::PhraseEdit::Cut { beat: 33 }).unwrap();
    assert_eq!(&edited.header[..4], &[0, 1, 0, 24]);
    assert_eq!(edited.as_song_structure().unwrap().phrases.iter().map(|p| p.beat).collect::<Vec<_>>(), [1, 33, 65]);
}

#[test]
fn an_unknown_phrase_version_is_preserved_without_editing() {
    let bytes = versioned_pssi(2, &[entry(1, 1, 1, 1, 0, 0, None)], false);
    let anlz = rbl_anlz::parse(&bytes).unwrap();
    let section = &anlz.sections[0];
    assert!(section.as_song_structure().is_none());
    assert!(section.with_phrase_edit(rbl_anlz::PhraseEdit::Cut { beat: 33 }).is_none());
    assert_eq!(section.with_export_phrase_mask(), *section);
}

// ---- PSSI ----

#[test]
fn reads_an_unmasked_song_structure() {
    let entries = vec![
        entry(1, 1, 1, 0, 0, 0, None),
        entry(2, 65, 5, 1, 0, 0, None),
        entry(3, 129, 3, 0, 0, 0, None),
    ];
    let anlz = rbl_anlz::parse(&pssi_local(1, 721, 0, &entries, false)).unwrap();
    let structure = anlz.song_structure().unwrap();
    assert_eq!(structure.mood, Mood::High);
    assert_eq!(structure.end_beat, 721);
    assert_eq!(structure.bank, 0);
    assert_eq!(structure.phrases.len(), 3);
    assert_eq!(structure.phrases[1].index, 2);
    assert_eq!(structure.phrases[1].beat, 65);
    assert_eq!(structure.phrases[1].kind, 5);
    assert_eq!(structure.phrases[2].label, "DOWN");
}

#[test]
fn unmasks_a_masked_song_structure_to_the_same_phrases() {
    let entries = vec![
        entry(1, 1, 1, 0, 0, 0, None),
        entry(2, 65, 5, 1, 0, 0, None),
        entry(3, 129, 2, 0, 1, 0, None),
    ];
    let plain = rbl_anlz::parse(&pssi_local(1, 400, 3, &entries, false)).unwrap();
    let masked = rbl_anlz::parse(&pssi_local(1, 400, 3, &entries, true)).unwrap();
    // The mask must leave nothing behind: the two decode identically.
    assert_eq!(masked.song_structure().unwrap(), plain.song_structure().unwrap());
    assert_eq!(labels(&pssi_local(1, 400, 3, &entries, true)), ["INTRO 2", "CHORUS 1", "UP 3"]);
}

#[test]
fn a_masked_tag_is_not_mistaken_for_a_plain_one() {
    // 53 entries is the one count whose pad puts a zero in the mood's high
    // byte; if the plain/masked test were naive this is where it would fail.
    let entries: Vec<Vec<u8>> = (0..53).map(|i| entry(i + 1, i * 16, 9, 0, 0, 0, None)).collect();
    let anlz = rbl_anlz::parse(&pssi_local(2, 900, 0, &entries, true)).unwrap();
    let structure = anlz.song_structure().unwrap();
    assert_eq!(structure.mood, Mood::Mid);
    assert_eq!(structure.phrases.len(), 53);
    assert!(structure.phrases.iter().all(|p| p.label == "CHORUS"));
}

#[test]
fn reads_the_exported_header_split_as_well_as_the_local_one() {
    let entries = vec![entry(1, 1, 1, 1, 0, 0, None), entry(2, 33, 6, 0, 0, 0, None)];
    assert_eq!(labels(&pssi_exported(1, &entries, true)), ["INTRO 1", "OUTRO 2"]);
    assert_eq!(labels(&pssi_exported(1, &entries, false)), ["INTRO 1", "OUTRO 2"]);
}

#[test]
fn high_mood_labels_come_from_the_flag_bytes_not_the_kind() {
    // Every combination in Deep Symmetry's table, and the only ones the
    // reference library uses.
    let entries = vec![
        entry(1, 0, 1, 1, 0, 0, None),
        entry(2, 0, 1, 0, 0, 0, None),
        entry(3, 0, 2, 0, 0, 0, None),
        entry(4, 0, 2, 0, 0, 1, None),
        entry(5, 0, 2, 0, 1, 0, None),
        entry(6, 0, 3, 0, 0, 0, None),
        entry(7, 0, 5, 1, 0, 0, None),
        entry(8, 0, 5, 0, 0, 0, None),
        entry(9, 0, 6, 1, 0, 0, None),
        entry(10, 0, 6, 0, 0, 0, None),
    ];
    assert_eq!(
        labels(&pssi_local(1, 0, 0, &entries, false)),
        [
            "INTRO 1", "INTRO 2", "UP 1", "UP 2", "UP 3", "DOWN", "CHORUS 1", "CHORUS 2",
            "OUTRO 1", "OUTRO 2",
        ]
    );
}

#[test]
fn the_same_kind_means_different_things_in_each_mood() {
    // Kind 5 is a chorus in the high mood, "Verse 4" in the mid one, and
    // "Verse 2" in the low one — the flag bytes are ignored outside high.
    let entries = vec![entry(1, 0, 5, 1, 0, 0, None)];
    assert_eq!(labels(&pssi_local(1, 0, 0, &entries, false)), ["CHORUS 1"]);
    assert_eq!(labels(&pssi_local(2, 0, 0, &entries, false)), ["VERSE 4"]);
    assert_eq!(labels(&pssi_local(3, 0, 0, &entries, false)), ["VERSE 2"]);
}

#[test]
fn the_low_moods_three_ids_for_a_verse_collapse_to_one_label() {
    let entries: Vec<Vec<u8>> = (2..=7).map(|k| entry(k, 0, k, 0, 0, 0, None)).collect();
    assert_eq!(
        labels(&pssi_local(3, 0, 0, &entries, false)),
        ["VERSE 1", "VERSE 1", "VERSE 1", "VERSE 2", "VERSE 2", "VERSE 2"]
    );
}

#[test]
fn an_unknown_kind_gets_no_label_rather_than_an_invented_one() {
    let entries = vec![entry(1, 8, 4, 0, 0, 0, None), entry(2, 16, 99, 0, 0, 0, None)];
    // Kind 4 is not part of the high mood's vocabulary, and 99 is nobody's.
    assert_eq!(labels(&pssi_local(1, 0, 0, &entries, false)), ["", ""]);
}

#[test]
fn reads_a_fill_in() {
    let entries = vec![entry(1, 545, 2, 0, 0, 0, Some(575)), entry(2, 577, 5, 1, 0, 0, None)];
    let anlz = rbl_anlz::parse(&pssi_local(1, 0, 0, &entries, false)).unwrap();
    let phrases = anlz.phrases().unwrap();
    assert!(phrases[0].fill_in);
    assert_eq!(phrases[0].fill_in_beat, 575);
    assert!(!phrases[1].fill_in);
    assert_eq!(phrases[1].fill_in_beat, 0);
}

#[test]
fn a_count_the_payload_cannot_back_yields_the_phrases_that_are_there() {
    let entries = vec![entry(1, 1, 1, 1, 0, 0, None), entry(2, 33, 3, 0, 0, 0, None)];
    let mut bytes = pssi_local(1, 0, 0, &entries, false);
    // Claim ten entries where two were written. The section framing stays
    // honest, so this is a damaged tag rather than a damaged file.
    let at = bytes.windows(4).position(|w| w == b"PSSI").unwrap();
    bytes[at + 16..at + 18].copy_from_slice(&10_u16.to_be_bytes());
    assert_eq!(labels(&bytes), ["INTRO 1", "DOWN"]);
}

#[test]
fn a_truncated_or_nonsense_song_structure_returns_none_rather_than_panicking() {
    let entries = vec![entry(1, 1, 1, 0, 0, 0, None)];
    let full = pssi_local(1, 0, 0, &entries, false);

    // Every prefix of a real tag, cut inside the section, must be survivable.
    for cut in 0..full.len() {
        let mut builder = AnlzBuilder::new();
        let at = full.windows(4).position(|w| w == b"PSSI").unwrap();
        let tail = &full[at + 12..];
        let take = cut.min(tail.len());
        let header_len = take.min(20);
        builder.raw(
            FourCc::new(b"PSSI"),
            tail[..header_len].to_vec(),
            tail[header_len..take].to_vec(),
        );
        let bytes = builder.finish();
        // Must not panic; may or may not decode.
        let _ = rbl_anlz::parse(&bytes).unwrap().song_structure();
    }

    // A mood that is neither plain nor masked is not a song structure.
    let mut builder = AnlzBuilder::new();
    let mut header = Vec::new();
    header.extend_from_slice(&24_u32.to_be_bytes());
    header.extend_from_slice(&1_u16.to_be_bytes());
    builder.raw(FourCc::new(b"PSSI"), header, vec![0xAB; 40]);
    assert!(rbl_anlz::parse(&builder.finish()).unwrap().song_structure().is_none());

    // An entry size smaller than the fields it must hold.
    let mut builder = AnlzBuilder::new();
    let mut header = Vec::new();
    header.extend_from_slice(&8_u32.to_be_bytes());
    header.extend_from_slice(&2_u16.to_be_bytes());
    header.extend_from_slice(&1_u16.to_be_bytes());
    builder.raw(FourCc::new(b"PSSI"), header, vec![0; 32]);
    assert!(rbl_anlz::parse(&builder.finish()).unwrap().song_structure().is_none());
}

#[test]
fn a_file_without_a_song_structure_has_no_phrases() {
    let mut builder = AnlzBuilder::new();
    builder.path("/Music/one.mp3");
    builder.waveform_preview(b"PWAV", &[1, 2, 3]);
    assert!(rbl_anlz::parse(&builder.finish()).unwrap().phrases().is_none());
}

// ---- PVDI ----

fn pvdi(declared: u32, payload: &[u8]) -> Vec<u8> {
    let mut header = Vec::new();
    header.extend_from_slice(&1024_u32.to_be_bytes());
    header.extend_from_slice(&0x5622_0001_u32.to_be_bytes());
    header.extend_from_slice(&declared.to_be_bytes());
    let mut builder = AnlzBuilder::new();
    builder.raw(FourCc::new(b"PVDI"), header, payload.to_vec());
    builder.finish()
}

#[test]
fn reads_the_vocal_strip() {
    let strip = [0, 0, 1, 2, 3, 4, 4, 4, 3, 1, 0];
    let anlz = rbl_anlz::parse(&pvdi(strip.len() as u32, &strip)).unwrap();
    assert_eq!(anlz.vocals().unwrap(), strip);
    assert!(strip.iter().all(|v| *v <= rbl_anlz::VOCAL_MAX));
}

#[test]
fn a_vocal_strip_whose_count_disagrees_with_its_payload_is_refused() {
    // The count and the byte length are the same number in every real file,
    // so a mismatch means this is not the layout we measured.
    assert!(rbl_anlz::parse(&pvdi(50, &[0, 1, 2])).unwrap().vocals().is_none());
    assert!(rbl_anlz::parse(&pvdi(0, &[0, 1, 2])).unwrap().vocals().is_none());
}

#[test]
fn a_truncated_vocal_tag_returns_none_rather_than_panicking() {
    for header_len in 0..12 {
        let mut builder = AnlzBuilder::new();
        builder.raw(FourCc::new(b"PVDI"), vec![0; header_len], vec![1, 2, 3]);
        assert!(rbl_anlz::parse(&builder.finish()).unwrap().vocals().is_none());
    }
    let mut builder = AnlzBuilder::new();
    builder.path("/Music/one.mp3");
    assert!(rbl_anlz::parse(&builder.finish()).unwrap().vocals().is_none());
}

#[test]
fn one_vocal_entry_covers_the_measured_frame() {
    // 44100/2048, which is what 600 tracks measure at against both the PWV7
    // column count and the PQTZ grid.
    assert!((rbl_anlz::VOCAL_FRAME_MS - 46.44).abs() < 0.01);
}
