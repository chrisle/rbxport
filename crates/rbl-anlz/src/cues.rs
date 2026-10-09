//! Device cue lists populated from the master database, whose share files
//! contain empty cue lists. Layout checked against rekordbox 7 USB exports.
use crate::Section;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportCue {
    /// Master database kind: memory=0, A-C=1-3, D-P=5-17.
    pub kind: u8,
    pub time_ms: u32,
    pub loop_time_ms: Option<u32>,
    pub color_id: u8,
    pub color_code: u8,
    pub comment: String,
    pub active_loop: bool,
    pub loop_numerator: u16,
    pub loop_denominator: u16,
}

impl ExportCue {
    pub fn slot(&self) -> u32 {
        u32::from(if self.kind >= 5 {
            self.kind - 1
        } else {
            self.kind
        })
    }
}

fn put32(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_be_bytes());
}
fn put16(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_be_bytes());
}

/// DAT carries memory and A-C; EXT adds D-P and the complete extended lists.
pub fn sections(cues: &[ExportCue], extended: bool) -> Vec<Section> {
    let ordered: Vec<_> = cues.iter().collect();
    let mut out = Vec::new();
    for modern in [false, true] {
        if modern && !extended {
            continue;
        }
        for hot in [true, false] {
            let list: Vec<_> = ordered
                .iter()
                .copied()
                .filter(|c| {
                    (c.kind != 0) == hot
                        && (modern
                            || if extended {
                                hot && c.slot() > 3
                            } else {
                                c.slot() <= 3
                            })
                })
                .collect();
            let count = u16::try_from(list.len()).unwrap_or(u16::MAX);
            let mut header = vec![0; if modern { 8 } else { 12 }];
            put32(&mut header, 0, u32::from(hot));
            put16(&mut header, if modern { 4 } else { 6 }, count);
            if !modern {
                put32(
                    &mut header,
                    8,
                    if hot || count == 0 {
                        u32::MAX
                    } else {
                        u32::from(count - 1)
                    },
                );
            }
            let mut payload = Vec::new();
            for (index, cue) in list.into_iter().take(usize::from(count)).enumerate() {
                let comment: Vec<u8> = if cue.comment.is_empty() {
                    Vec::new()
                } else {
                    cue.comment
                        .encode_utf16()
                        .chain([0])
                        .flat_map(u16::to_be_bytes)
                        .collect()
                };
                let length = if modern { 88 + comment.len() } else { 56 };
                let mut entry = vec![0; length];
                entry[..4].copy_from_slice(if modern { b"PCP2" } else { b"PCPT" });
                put32(&mut entry, 4, if modern { 16 } else { 28 });
                put32(&mut entry, 8, u32::try_from(length).unwrap_or(u32::MAX));
                put32(&mut entry, 12, cue.slot());
                let at = if modern { 16 } else { 28 };
                entry[at] = if cue.loop_time_ms.is_some() { 2 } else { 1 };
                entry[at + 1..at + 4].copy_from_slice(&[0, 3, 232]);
                put32(&mut entry, at + 4, cue.time_ms);
                put32(&mut entry, at + 8, cue.loop_time_ms.unwrap_or(u32::MAX));
                if modern {
                    entry[28] = cue.color_id;
                    entry[29] = 1;
                    put16(&mut entry, 36, cue.loop_numerator);
                    put16(&mut entry, 38, cue.loop_denominator);
                    put32(
                        &mut entry,
                        40,
                        u32::try_from(comment.len()).unwrap_or(u32::MAX),
                    );
                    entry[44..44 + comment.len()].copy_from_slice(&comment);
                    entry[44 + comment.len()] = cue.color_code;
                    entry[45 + comment.len()..48 + comment.len()]
                        .copy_from_slice(&export_colour(cue.color_code, cue.slot()));
                } else {
                    put32(&mut entry, 16, if cue.active_loop { 4 } else { 0 });
                    put32(&mut entry, 20, 0x10000);
                    put16(
                        &mut entry,
                        24,
                        if hot || index == 0 {
                            u16::MAX
                        } else {
                            u16::try_from(index - 1).unwrap_or(u16::MAX)
                        },
                    );
                    put16(
                        &mut entry,
                        26,
                        if hot || index + 1 == usize::from(count) {
                            u16::MAX
                        } else {
                            u16::try_from(index + 1).unwrap_or(u16::MAX)
                        },
                    );
                }
                payload.extend(entry);
            }
            out.push(Section::new(
                if modern { b"PCO2" } else { b"PCOB" },
                header,
                payload,
            ));
        }
    }
    out
}

// CDJ-3000 3.20 EP122 palette, file offset 0x1b55a58 (also at three
// independent copies). Every previously measured export color matches it.
const DEVICE_PALETTE: [[u8; 3]; 65] = [
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0xff],
    [0x00, 0x1c, 0xff],
    [0x00, 0x38, 0xff],
    [0x00, 0x54, 0xff],
    [0x00, 0x70, 0xff],
    [0x00, 0x8c, 0xff],
    [0x00, 0xa8, 0xff],
    [0x00, 0xc4, 0xff],
    [0x00, 0xe0, 0xff],
    [0x00, 0xff, 0xff],
    [0x00, 0xff, 0xe8],
    [0x00, 0xff, 0xd1],
    [0x00, 0xff, 0xba],
    [0x00, 0xff, 0xa3],
    [0x00, 0xff, 0x8c],
    [0x00, 0xff, 0x75],
    [0x00, 0xff, 0x5e],
    [0x00, 0xff, 0x47],
    [0x00, 0xff, 0x30],
    [0x00, 0xff, 0x1a],
    [0x00, 0xff, 0x00],
    [0x1a, 0xff, 0x00],
    [0x33, 0xff, 0x00],
    [0x4d, 0xff, 0x00],
    [0x66, 0xff, 0x00],
    [0x80, 0xff, 0x00],
    [0x99, 0xff, 0x00],
    [0xb3, 0xff, 0x00],
    [0xcc, 0xff, 0x00],
    [0xe6, 0xff, 0x00],
    [0xff, 0xff, 0x00],
    [0xff, 0xe8, 0x00],
    [0xff, 0xd1, 0x00],
    [0xff, 0xba, 0x00],
    [0xff, 0xa3, 0x00],
    [0xff, 0x8c, 0x00],
    [0xff, 0x75, 0x00],
    [0xff, 0x5e, 0x00],
    [0xff, 0x47, 0x00],
    [0xff, 0x30, 0x00],
    [0xff, 0x1a, 0x00],
    [0xff, 0x00, 0x00],
    [0xff, 0x00, 0x17],
    [0xff, 0x00, 0x2e],
    [0xff, 0x00, 0x45],
    [0xff, 0x00, 0x5c],
    [0xff, 0x00, 0x73],
    [0xff, 0x00, 0x8a],
    [0xff, 0x00, 0xa1],
    [0xff, 0x00, 0xb8],
    [0xff, 0x00, 0xcf],
    [0xff, 0x00, 0xe6],
    [0xff, 0x00, 0xff],
    [0xe6, 0x00, 0xff],
    [0xcc, 0x00, 0xff],
    [0xb3, 0x00, 0xff],
    [0x99, 0x00, 0xff],
    [0x80, 0x00, 0xff],
    [0x66, 0x00, 0xff],
    [0x4d, 0x00, 0xff],
    [0x33, 0x00, 0xff],
    [0x1a, 0x00, 0xff],
    [0, 0, 0],
    [255, 255, 255],
];

/// The device RGB a player is sent for a hot cue's `ColorTableIndex`, as
/// `PCO2` carries it after the colour code. `slot` is 1 for A; a code of 0
/// gets the slot's default colour.
pub fn export_colour(code: u8, slot: u32) -> [u8; 3] {
    // Default colors read from rekordbox exports for A-D and I-L.
    let default = match slot {
        1 | 9 => 43,
        2 | 10 => 8,
        3 | 11 => 23,
        4 | 12 => 60,
        _ => 0,
    };
    DEVICE_PALETTE
        .get(usize::from(if code == 0 { default } else { code }))
        .copied()
        .unwrap_or([0; 3])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    #[test]
    fn matches_a_rekordbox_hot_cue_record_and_complete_palette() {
        let cue = ExportCue {
            kind: 1,
            time_ms: 83_032,
            color_code: 46,
            ..Default::default()
        };
        let sections = sections(&[cue], true);
        let entry = &sections[2].payload;
        let expected = "50435032000000100000005800000001010003e800014458ffffffff000100000000000000000000000000002eff005c00000000000000000000000000000000000000000000000000000000000000000000000000000000";
        let actual = entry.iter().fold(String::new(), |mut s, v| {
            use std::fmt::Write;
            write!(s, "{v:02x}").unwrap();
            s
        });
        assert_eq!(actual, expected);
        for (code, rgb) in crate::MEASURED_CUE_COLOURS.iter().skip(1) {
            assert_eq!(export_colour(*code, 1), *rgb);
        }
        assert_eq!(export_colour(0, 1), [255, 0, 23]);
        assert_eq!(export_colour(62, 1), [26, 0, 255]);
    }
    #[test]
    fn exports_memory_loop_comments_colors_and_the_d_slot() {
        let cues = vec![
            ExportCue {
                kind: 5,
                time_ms: 1000,
                color_code: 8,
                comment: "Drop 🎵".into(),
                ..Default::default()
            },
            ExportCue {
                time_ms: 2000,
                loop_time_ms: Some(4000),
                active_loop: true,
                loop_numerator: 4,
                loop_denominator: 1,
                comment: "Loop".into(),
                ..Default::default()
            },
        ];
        let dat = sections(&cues, false);
        assert_eq!(dat[0].payload, [] as [u8; 0]);
        assert_eq!(&dat[1].payload[16..20], &4_u32.to_be_bytes());
        let ext = sections(&cues, true);
        let decoded = ext[2].as_cue_entries().unwrap();
        assert_eq!(decoded[0].hot_cue, 4);
        assert_eq!(decoded[0].comment.as_deref(), Some("Drop 🎵"));
        assert_eq!(decoded[0].rgb, Some([0, 196, 255]));
        let memory = ext[3].as_cue_entries().unwrap();
        assert_eq!(
            (memory[0].kind, memory[0].time_ms, memory[0].loop_time_ms),
            (2, 2000, 4000)
        );
        assert_eq!(&ext[3].payload[36..40], &[0, 4, 0, 1]);
    }
}
