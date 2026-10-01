//! Container framing, tag decoding, and the malformed inputs that must degrade
//! rather than panic.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use rbl_anlz::{parse, Anlz, AnlzBuilder, AnlzError, Beat, Section};
use rbl_core::FourCc;

/// Builds a PMAI file from (tag, tag-header, payload) triples.
///
/// `len_header` differs per tag in real files, which is exactly the detail that
/// makes a naive reader consume the first bytes of the payload as header fields.
fn build(sections: &[(&[u8; 4], Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let header_extra = [0_u8; 16];
    let mut body = Vec::new();
    for (tag, header, payload) in sections {
        let len_header = 12 + header.len();
        let len_tag = len_header + payload.len();
        body.extend_from_slice(*tag);
        body.extend_from_slice(&(len_header as u32).to_be_bytes());
        body.extend_from_slice(&(len_tag as u32).to_be_bytes());
        body.extend_from_slice(header);
        body.extend_from_slice(payload);
    }
    let len_header = 12 + header_extra.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"PMAI");
    out.extend_from_slice(&(len_header as u32).to_be_bytes());
    out.extend_from_slice(&((len_header + body.len()) as u32).to_be_bytes());
    out.extend_from_slice(&header_extra);
    out.extend_from_slice(&body);
    out
}

fn beat_grid_parts(beats: &[(u16, u16, u32)]) -> (Vec<u8>, Vec<u8>) {
    let mut header = Vec::new();
    header.extend_from_slice(&0_u32.to_be_bytes());
    header.extend_from_slice(&0x0008_0000_u32.to_be_bytes());
    header.extend_from_slice(&(beats.len() as u32).to_be_bytes());
    let mut payload = Vec::new();
    for (number, tempo, time) in beats {
        payload.extend_from_slice(&number.to_be_bytes());
        payload.extend_from_slice(&tempo.to_be_bytes());
        payload.extend_from_slice(&time.to_be_bytes());
    }
    (header, payload)
}

fn path_parts(text: &str) -> (Vec<u8>, Vec<u8>) {
    let mut utf16: Vec<u8> = Vec::new();
    for unit in text.encode_utf16() {
        utf16.extend_from_slice(&unit.to_be_bytes());
    }
    utf16.extend_from_slice(&[0, 0]);
    ((utf16.len() as u32).to_be_bytes().to_vec(), utf16)
}

// ---- reading ----

#[test]
fn rejects_a_file_without_the_pmai_magic() {
    assert!(matches!(
        parse(b"NOPE\0\0\0\0\0\0\0\0"),
        Err(AnlzError::NotAnlz)
    ));
}

#[test]
fn rejects_a_truncated_header_without_panicking() {
    assert!(parse(b"PMAI").is_err());
    assert!(parse(b"PMAI\0\0").is_err());
    assert!(parse(&[]).is_err());
}

#[test]
fn reads_a_beat_grid() {
    let (h, p) = beat_grid_parts(&[(1, 12800, 0), (2, 12800, 469), (3, 12800, 938)]);
    let anlz = parse(&build(&[(b"PQTZ", h, p)])).unwrap();
    let beats = anlz.beat_grid().unwrap();
    assert_eq!(beats.len(), 3);
    assert_eq!(beats[0].beat_number, 1);
    assert_eq!(beats[1].tempo_x100, 12800);
    assert_eq!(beats[2].time_ms, 938);
}

#[test]
fn a_truncated_beat_grid_keeps_the_beats_it_could_read() {
    let (mut header, payload) = beat_grid_parts(&[(1, 12800, 0), (2, 12800, 469)]);
    header[8..12].copy_from_slice(&4_u32.to_be_bytes()); // claims four
    let anlz = parse(&build(&[(b"PQTZ", header, payload)])).unwrap();
    assert_eq!(anlz.beat_grid().unwrap().len(), 2);
}

#[test]
fn reads_a_utf16_path() {
    let (h, p) = path_parts("/Users/dj/Music/Track Ébano.mp3");
    let anlz = parse(&build(&[(b"PPTH", h, p)])).unwrap();
    assert_eq!(anlz.path().unwrap(), "/Users/dj/Music/Track Ébano.mp3");
}

#[test]
fn keeps_unknown_tags_verbatim() {
    let anlz = parse(&build(&[(b"PZZZ", vec![7, 7], vec![1, 2, 3, 4])])).unwrap();
    let section = anlz.section(b"PZZZ").unwrap();
    assert_eq!(section.tag, FourCc::new(b"PZZZ"));
    assert_eq!(section.header, vec![7, 7]);
    assert_eq!(section.payload, vec![1, 2, 3, 4]);
}

#[test]
fn tag_header_fields_are_not_read_from_the_payload() {
    // The bug this guards: PWAV keeps its length and flags in the tag header.
    // Reading them from the payload swallows the first eight bytes of waveform
    // data and silently returns the wrong signal — which is what happened until
    // a byte-for-byte rebuild of the real library exposed it.
    let mut header = Vec::new();
    header.extend_from_slice(&6_u32.to_be_bytes());
    header.extend_from_slice(&0x0001_0000_u32.to_be_bytes());
    let payload = vec![0x51, 0x4b, 0x52, 0x54, 0x50, 0x55];

    let anlz = parse(&build(&[(b"PWAV", header, payload.clone())])).unwrap();
    let (stride, data) = anlz.waveform(b"PWAV").unwrap();
    assert_eq!(stride, 1);
    assert_eq!(
        data,
        payload.as_slice(),
        "no payload bytes may be consumed as header"
    );
}

#[test]
fn reads_waveform_stride_from_the_tag_header() {
    let mut header = Vec::new();
    header.extend_from_slice(&2_u32.to_be_bytes()); // stride
    header.extend_from_slice(&3_u32.to_be_bytes()); // entries
    header.extend_from_slice(&0_u32.to_be_bytes());
    let payload = vec![1, 2, 3, 4, 5, 6];
    let anlz = parse(&build(&[(b"PWV5", header, payload.clone())])).unwrap();
    let (stride, data) = anlz.waveform(b"PWV5").unwrap();
    assert_eq!(stride, 2);
    assert_eq!(data, payload.as_slice());
}

#[test]
fn reads_pwv6s_three_byte_stride_from_its_short_header() {
    let anlz = parse(&build(&[(b"PWV6", vec![0, 0, 0, 3, 0, 0, 0, 2], vec![1; 6])])).unwrap();
    assert_eq!(anlz.waveform(b"PWV6").map(|(stride, _)| stride), Some(3));
}

#[test]
fn a_section_longer_than_the_file_is_reported() {
    let (h, p) = beat_grid_parts(&[(1, 12800, 0)]);
    let mut file = build(&[(b"PQTZ", h, p)]);
    let len = file.len();
    let at = 28 + 8;
    file[at..at + 4].copy_from_slice(&(len as u32 + 500).to_be_bytes());
    assert!(matches!(
        parse(&file),
        Err(AnlzError::BadSectionLength { .. })
    ));
}

#[test]
fn rejects_a_file_whose_declared_length_does_not_match() {
    let mut file = build(&[]);
    file[8..12].copy_from_slice(&27_u32.to_be_bytes());
    assert!(matches!(parse(&file), Err(AnlzError::BadFileLength { declared: 27, actual: 28 })));
}

#[test]
fn rejects_a_section_header_that_runs_past_its_frame() {
    let mut file = build(&[(b"PWAV", vec![0; 8], vec![1, 2, 3])]);
    file[32..36].copy_from_slice(&24_u32.to_be_bytes());
    assert!(matches!(parse(&file), Err(AnlzError::BadSectionHeaderLength { .. })));
}

#[test]
fn a_zero_length_section_does_not_loop_forever() {
    let (h, p) = beat_grid_parts(&[(1, 12800, 0)]);
    let mut file = build(&[(b"PQTZ", h, p)]);
    let at = 28 + 8;
    file[at..at + 4].copy_from_slice(&0_u32.to_be_bytes());
    assert!(parse(&file).is_err());
}

#[test]
fn several_sections_are_all_read() {
    let (ph, pp) = path_parts("/a.mp3");
    let (bh, bp) = beat_grid_parts(&[(1, 12000, 0)]);
    let anlz = parse(&build(&[
        (b"PPTH", ph, pp),
        (b"PQTZ", bh, bp),
        (b"PZZZ", vec![], vec![9]),
    ]))
    .unwrap();
    assert_eq!(anlz.sections.len(), 3);
    assert!(anlz.path().is_some());
    assert!(anlz.beat_grid().is_some());
}

#[test]
fn reading_a_missing_file_is_an_io_error() {
    let err = Anlz::read(std::path::Path::new("/definitely/not/here.DAT")).unwrap_err();
    assert!(matches!(err, AnlzError::Io(_)));
}

#[test]
fn resolve_joins_a_share_relative_path() {
    let share = std::path::Path::new("/Users/dj/Library/Pioneer/rekordbox/share");
    let p = rbl_anlz::resolve(share, "/PIONEER/USBANLZ/abc/def/ANLZ0000.DAT");
    assert!(p.ends_with("PIONEER/USBANLZ/abc/def/ANLZ0000.DAT"));
    assert!(p.starts_with(share));
}

#[test]
fn sibling_swaps_the_extension() {
    let dat = std::path::Path::new("/x/ANLZ0000.DAT");
    assert!(rbl_anlz::sibling(dat, "EXT").ends_with("ANLZ0000.EXT"));
    assert!(rbl_anlz::sibling(dat, "2EX").ends_with("ANLZ0000.2EX"));
}

// ---- writing ----

#[test]
fn parsing_then_re_emitting_reproduces_the_file_exactly() {
    // The property an export depends on. Verified separately across all 99,032
    // analysis files in the real library.
    let (ph, pp) = path_parts("/a.mp3");
    let (bh, bp) = beat_grid_parts(&[(1, 12800, 0), (2, 12800, 469)]);
    let original = build(&[
        (b"PPTH", ph, pp),
        (b"PQTZ", bh, bp),
        (b"PSSI", vec![1, 2], vec![3, 4, 5]),
        (b"PVDI", vec![], vec![9, 8, 7]),
    ]);
    let parsed = parse(&original).unwrap();
    assert_eq!(
        parsed.to_bytes(),
        original,
        "re-emission must be byte-identical"
    );
}

#[test]
fn a_written_beat_grid_reads_back_identically() {
    let beats: Vec<Beat> = (0..64)
        .map(|i| Beat {
            beat_number: (i % 4 + 1) as u16,
            tempo_x100: 12_800,
            time_ms: i as u32 * 469,
        })
        .collect();
    let mut builder = AnlzBuilder::new();
    builder.path("/Users/dj/Music/Track.mp3").beat_grid(&beats);
    let anlz = parse(&builder.finish()).unwrap();
    assert_eq!(anlz.path().unwrap(), "/Users/dj/Music/Track.mp3");
    assert_eq!(anlz.beat_grid().unwrap(), beats);
}

#[test]
fn written_waveforms_read_back_identically() {
    let data: Vec<u8> = (0..600).map(|i| (i % 256) as u8).collect();
    let mut builder = AnlzBuilder::new();
    builder
        .waveform_preview(b"PWAV", &data)
        .waveform_scroll(b"PWV5", 2, &data);
    let anlz = parse(&builder.finish()).unwrap();

    let (stride, read) = anlz.waveform(b"PWAV").unwrap();
    assert_eq!((stride, read), (1, data.as_slice()));
    let (stride, read) = anlz.waveform(b"PWV5").unwrap();
    assert_eq!((stride, read), (2, data.as_slice()));
}

#[test]
fn non_ascii_paths_survive_the_round_trip() {
    let path = "/Users/dj/Música/Ébano — Tiësto.mp3";
    let mut builder = AnlzBuilder::new();
    builder.path(path);
    assert_eq!(parse(&builder.finish()).unwrap().path().unwrap(), path);
}

#[test]
fn tags_we_cannot_author_are_carried_through() {
    let (ph, pp) = path_parts("/a.mp3");
    let original = build(&[
        (b"PPTH", ph, pp),
        (b"PSSI", vec![1], vec![2, 3, 4, 5]),
        (b"PVDI", vec![], vec![9, 8, 7]),
    ]);
    let parsed = parse(&original).unwrap();

    let mut builder = AnlzBuilder::new();
    builder.header_extra(&parsed.header_extra);
    for section in &parsed.sections {
        builder.copy_section(section);
    }
    let reparsed = parse(&builder.finish()).unwrap();

    let tags: Vec<String> = reparsed
        .sections
        .iter()
        .map(|s| s.tag.to_string())
        .collect();
    assert!(
        tags.contains(&"PSSI".to_owned()),
        "phrases must survive: {tags:?}"
    );
    assert!(
        tags.contains(&"PVDI".to_owned()),
        "vocal data must survive: {tags:?}"
    );
    assert_eq!(reparsed.section(b"PSSI").unwrap().payload, vec![2, 3, 4, 5]);
}

#[test]
fn an_empty_cue_list_is_what_the_share_tree_holds() {
    let mut builder = AnlzBuilder::new();
    builder.empty_cue_list(false).empty_cue_list(true);
    let anlz = parse(&builder.finish()).unwrap();
    let lists: Vec<&Section> = anlz.sections.iter().filter(|s| s.is_cue_list()).collect();
    assert_eq!(lists.len(), 2);
    assert!(lists.iter().all(|s| s.payload.is_empty()));
}

#[test]
fn a_file_with_no_sections_is_still_valid() {
    let anlz = parse(&AnlzBuilder::new().finish()).unwrap();
    assert!(anlz.sections.is_empty());
    assert!(anlz.beat_grid().is_none());
}

#[test]
fn replacing_the_grid_leaves_every_other_section_alone() {
    use rbl_anlz::grid::{apply, Edit};

    let beats: Vec<rbl_anlz::Beat> = (0..8)
        .map(|i| rbl_anlz::Beat {
            beat_number: (i % 4) + 1,
            tempo_x100: 12_000,
            time_ms: 500 + u32::from(i) * 500,
        })
        .collect();

    let mut builder = rbl_anlz::AnlzBuilder::new();
    builder.path("/Music/one.mp3");
    builder.beat_grid(&beats);
    builder.waveform_preview(b"PWAV", &[1, 2, 3, 4]);
    let original = builder.finish();
    let parsed = rbl_anlz::parse(&original).unwrap();

    // Replacing the grid with itself must be a no-op down to the byte, or the
    // rewrite is not safe to run over a file rekordbox authored.
    assert_eq!(parsed.with_beat_grid(&beats), original);

    let nudged = apply(&beats, Edit::Nudge(-40));
    let rewritten = rbl_anlz::parse(&parsed.with_beat_grid(&nudged)).unwrap();
    assert_eq!(rewritten.beat_grid().unwrap(), nudged);
    assert_eq!(rewritten.path().as_deref(), Some("/Music/one.mp3"));
    assert_eq!(rewritten.waveform(b"PWAV"), parsed.waveform(b"PWAV"));
    assert_eq!(rewritten.sections.len(), parsed.sections.len());
}

#[test]
fn a_file_with_no_grid_gains_one_after_its_required_prefix() {
    let mut builder = rbl_anlz::AnlzBuilder::new();
    builder.path("/Music/one.mp3").vbr_table_zero();
    builder.waveform_preview(b"PWAV", &[1, 2, 3]);
    let parsed = rbl_anlz::parse(&builder.finish()).unwrap();
    assert!(parsed.beat_grid().is_none());

    let beat = rbl_anlz::Beat {
        beat_number: 1,
        tempo_x100: 12_000,
        time_ms: 500,
    };
    let grown = rbl_anlz::parse(&parsed.with_beat_grid(&[beat])).unwrap();
    assert_eq!(grown.beat_grid().unwrap(), vec![beat]);
    // PPTH and PVBR precede the grid in a freshly initialized DAT.
    assert_eq!(grown.sections[0].tag.as_str(), "PPTH");
    assert_eq!(grown.sections[1].tag.as_str(), "PVBR");
    assert_eq!(grown.sections[2].tag.as_str(), "PQTZ");
    assert!(!grown.has_extended_grid());
}

/// One `PCP2` entry, built the way rekordbox writes it: the fixed part, a
/// UTF-16BE comment with its length, then the colour index and its RGB.
fn cue_entry(
    hot_cue: u32,
    kind: u8,
    time_ms: u32,
    comment: &str,
    colour: Option<(u8, [u8; 3])>,
) -> Vec<u8> {
    let mut comment_bytes = Vec::new();
    for unit in comment.encode_utf16() {
        comment_bytes.extend_from_slice(&unit.to_be_bytes());
    }
    if !comment.is_empty() {
        comment_bytes.extend_from_slice(&[0, 0]); // the trailing NUL rekordbox writes
    }

    let mut body = Vec::new();
    body.extend_from_slice(&hot_cue.to_be_bytes());
    body.push(kind);
    body.extend_from_slice(&[0, 0, 0]);
    body.extend_from_slice(&time_ms.to_be_bytes());
    body.extend_from_slice(&0_u32.to_be_bytes()); // loop time
    body.push(0); // colour row, for memory cues
    body.extend_from_slice(&[0_u8; 11]);
    body.extend_from_slice(&(comment_bytes.len() as u32).to_be_bytes());
    body.extend_from_slice(&comment_bytes);
    if let Some((code, rgb)) = colour {
        body.push(code);
        body.extend_from_slice(&rgb);
    }

    let len_entry = 12 + body.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"PCP2");
    out.extend_from_slice(&12_u32.to_be_bytes());
    out.extend_from_slice(&(len_entry as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

#[test]
fn extended_cues_carry_their_colour() {
    let mut payload = cue_entry(1, 1, 4321, "intro", Some((21, [0x00, 0xFF, 0x00])));
    payload.extend(cue_entry(2, 2, 90_000, "", Some((36, [0xFF, 0x8C, 0x00]))));
    let file = build(&[(b"PCO2", vec![0, 0, 0, 1, 0, 2, 0, 0], payload)]);

    let entries = parse(&file).unwrap().cue_entries();
    assert_eq!(entries.len(), 2);

    assert_eq!(entries[0].hot_cue, 1);
    assert_eq!(entries[0].kind, 1);
    assert_eq!(entries[0].time_ms, 4321);
    assert_eq!(entries[0].comment.as_deref(), Some("intro"));
    assert_eq!(entries[0].color_code, Some(21));
    assert_eq!(entries[0].rgb, Some([0x00, 0xFF, 0x00]));

    // A cue with no comment still has a colour: it sits after the comment,
    // wherever that leaves it.
    assert_eq!(entries[1].hot_cue, 2);
    assert_eq!(entries[1].comment, None);
    assert_eq!(entries[1].color_code, Some(36));
}

/// The share tree's cue lists are headers with nothing in them, and that is
/// not a parse failure.
#[test]
fn an_empty_cue_list_reads_as_no_cues() {
    let file = build(&[(b"PCO2", vec![0, 0, 0, 1, 0, 0, 0, 0], Vec::new())]);
    assert!(parse(&file).unwrap().cue_entries().is_empty());
}

/// A truncated entry stops the list rather than reading past it.
#[test]
fn a_truncated_cue_entry_does_not_run_off_the_end() {
    let mut payload = cue_entry(1, 1, 100, "a", Some((21, [1, 2, 3])));
    payload.truncate(payload.len() - 6);
    let file = build(&[(b"PCO2", vec![0, 0, 0, 1, 0, 1, 0, 0], payload)]);
    assert!(parse(&file).unwrap().cue_entries().is_empty());
}

/// The device palette is complete for every index rekordbox accepts.
#[test]
fn the_cue_palette_answers_for_every_valid_index() {
    assert_eq!(rbl_anlz::cue_colour(21), Some([0x00, 0xFF, 0x00]));
    assert_eq!(rbl_anlz::cue_colour(36), Some([0xFF, 0x8C, 0x00]));
    assert_eq!(rbl_anlz::cue_colour(41), Some([0xFF, 0x1A, 0x00]));
    assert_eq!(rbl_anlz::cue_colour(0), Some([0, 0, 0]));
    assert_eq!(rbl_anlz::cue_colour(64), Some([0xFF, 0xFF, 0xFF]));
    assert_eq!(rbl_anlz::cue_colour(65), None);
}

/// The desktop and device palettes cover the same complete index range while
/// retaining rekordbox's intentionally different RGB values.
#[test]
fn the_drawn_palette_covers_the_same_indices_as_the_stored_one() {
    let stored: Vec<u8> = rbl_anlz::MEASURED_CUE_COLOURS
        .iter()
        .map(|&(i, _)| i)
        .collect();
    let drawn: Vec<u8> = rbl_anlz::DRAWN_CUE_COLOURS
        .iter()
        .map(|&(i, _)| i)
        .collect();
    assert_eq!(stored, drawn);
    assert_eq!(stored, (0_u8..=64).collect::<Vec<_>>());
    assert_eq!(rbl_anlz::cue_colour_drawn(1), Some([0x30, 0x5A, 0xFF]));
    assert_eq!(rbl_anlz::cue_colour_drawn(21), Some([0x3C, 0xEB, 0x50]));
    assert_eq!(rbl_anlz::cue_colour_drawn(41), Some([0xE0, 0x28, 0x23]));
    assert_eq!(rbl_anlz::cue_colour_drawn(64), Some([0xFF, 0xFF, 0xFF]));
    assert_eq!(rbl_anlz::cue_colour_drawn(65), None);
}

/// A grid edit empties the `.EXT`'s extended grid rather than leaving one
/// that describes the beats the `.DAT` no longer has, and leaves every other
/// section as it was. A file with no `PQT2`, or an already empty one, has
/// nothing to do and says so.
#[test]
fn a_grid_edit_empties_the_extended_grid_and_nothing_else() {
    let mut filled = vec![0_u8; 44];
    filled[4..8].copy_from_slice(&0x0100_0002_u32.to_be_bytes());
    let file = build(&[
        (b"PPTH", vec![0, 0, 0, 4], vec![0, b'a', 0, 0]),
        (b"PQT2", filled.clone(), vec![1, 2, 3, 4, 5, 6]),
        (
            b"PWV3",
            vec![0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 0],
            vec![7, 8, 9],
        ),
    ]);
    let parsed = parse(&file).unwrap();
    let cleared = parse(
        &parsed
            .with_extended_grid_cleared()
            .expect("a filled PQT2 is emptied"),
    )
    .unwrap();
    assert_eq!(cleared.sections.len(), 3);
    assert_eq!(cleared.sections[0], parsed.sections[0]);
    assert_eq!(cleared.sections[2], parsed.sections[2]);
    let grid = &cleared.sections[1];
    assert_eq!(grid.tag, FourCc::new(b"PQT2"));
    assert!(grid.payload.is_empty());
    assert_eq!(grid.header.len(), 44);
    assert_eq!(&grid.header[4..8], &0x0100_0002_u32.to_be_bytes());
    assert!(
        grid.header[8..].iter().all(|&b| b == 0),
        "no beat is described"
    );

    assert!(
        cleared.with_extended_grid_cleared().is_none(),
        "already empty"
    );
    let without = parse(&build(&[(b"PPTH", vec![0, 0, 0, 4], vec![0, b'a', 0, 0])])).unwrap();
    assert!(
        without.with_extended_grid_cleared().is_none(),
        "nothing to empty"
    );
}
