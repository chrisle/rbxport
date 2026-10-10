//! The per-track blobs a player asks a database server for, built from the
//! analysis files and the cue table in the layouts rekordbox 7.2.11 sends
//! (measured against a CDJ-3000 where captured fixtures are available).
//! The VBR compatibility placeholder is retained behavior, not a vendor oracle.

use rbl_anlz::{Anlz, Beat, Section};
use rbl_index::Cue;

/// An analysis tag as the `2c04` / `2d04` replies carry it: a little-endian
/// length, the tag framed as in the file, zero-padded to a multiple of four.
pub fn tag_blob(section: &Section) -> Vec<u8> {
    let framed_len = usize::try_from(section.len_tag()).unwrap_or(0);
    let padded = framed_len.div_ceil(4) * 4;
    let mut out = Vec::with_capacity(4 + padded);
    out.extend_from_slice(&u32::try_from(padded).unwrap_or(u32::MAX).to_le_bytes());
    out.extend_from_slice(&section.tag.0);
    out.extend_from_slice(&section.len_header().to_be_bytes());
    out.extend_from_slice(&section.len_tag().to_be_bytes());
    out.extend_from_slice(&section.header);
    out.extend_from_slice(&section.payload);
    out.resize(4 + padded, 0);
    out
}

/// The beat grid reply (`4602`): a 20-byte header — `00 00 08 00`, the beat
/// count, the byte length of the entries, 1, 1 — then 16 bytes a beat: the
/// beat number, a zero, the tempo ×100 and the time in milliseconds, all
/// little-endian, then eight `ff` bytes.
pub fn beat_grid_blob(beats: &[Beat]) -> Vec<u8> {
    let count = u32::try_from(beats.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(20 + beats.len() * 16);
    out.extend_from_slice(&[0x00, 0x00, 0x08, 0x00]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.saturating_mul(16).to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    for beat in beats {
        out.push(u8::try_from(beat.beat_number).unwrap_or(0));
        out.push(0);
        out.extend_from_slice(&beat.tempo_x100.to_le_bytes());
        out.extend_from_slice(&beat.time_ms.to_le_bytes());
        out.extend_from_slice(&[0xff; 8]);
    }
    out
}

/// The four bytes rekordbox appends to every waveform preview; constant
/// across tracks, meaning unknown.
const PREVIEW_TAIL: [u8; 4] = [0x9e, 0xeb, 0x78, 0x10];

/// The waveform preview reply (`4402`): each of the 400 `PWAV` columns as
/// two bytes — height (the low five bits) then whiteness (the high three) —
/// then the 100 `PWV2` columns, then a fixed tail.
///
/// The columns go out as rekordbox's are: `PWV2` the height alone (1..=15)
/// and no height 0. A Nexus player checks this reply and discards the whole
/// preview when a `PWV2` byte is above 15, and treats a zero column as
/// unfinished (issue #278); analysis written by earlier versions of this app
/// has both. A rekordbox-written column is unchanged.
pub fn waveform_preview_blob(pwav: &[u8], pwv2: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pwav.len() * 2 + pwv2.len() + 4);
    for &column in pwav {
        let column = rbl_anlz::encode::device_preview_byte(column);
        out.push(column & 0x1f);
        out.push(column >> 5);
    }
    out.extend(pwv2.iter().map(|&b| rbl_anlz::encode::device_tiny_preview_byte(b)));
    out.extend_from_slice(&PREVIEW_TAIL);
    out
}

/// The waveform detail reply (`4a02`): a 20-byte header — the column count,
/// 1, the count again, 150 (columns a second), 1, little-endian — then the
/// `PWV3` columns as they are.
pub fn waveform_detail_blob(pwv3: &[u8]) -> Vec<u8> {
    let count = u32::try_from(pwv3.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(20 + pwv3.len());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&150_u32.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
    out.extend_from_slice(pwv3);
    out
}

/// Byte at `0x34` of every extended cue entry, constant, meaning unknown.
const EXTENDED_CUE_UNKNOWN: u8 = 0x56;
/// The word after a comment, constant, meaning unknown.
const EXTENDED_CUE_AFTER_COMMENT: [u8; 4] = [0x2c, 0x00, 0x00, 0x00];
/// The colour rekordbox gave every hot cue in the capture: code `15`, then
/// the LED colour green. `[ASSUME]` the same for every hot cue until the
/// colour table is read.
const HOT_CUE_COLOUR: [u8; 4] = [0x15, 0x00, 0xff, 0x00];

/// One cue as the extended list carries it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtendedCue {
    pub position_ms: u32,
    /// Where a loop ends; 0 for a plain cue.
    pub out_ms: u32,
    /// 1–8 for hot cues A–H, 0 for a memory cue.
    pub hot_slot: u8,
    pub comment: String,
}

impl From<&Cue> for ExtendedCue {
    fn from(cue: &Cue) -> Self {
        let hot_slot = cue.hot_letter().map_or(0, |letter| letter as u8 - b'A' + 1);
        Self {
            position_ms: cue.position_ms,
            out_ms: cue.out_ms,
            hot_slot: if (1..=8).contains(&hot_slot) {
                hot_slot
            } else {
                0
            },
            comment: String::new(),
        }
    }
}

/// The extended cue-list reply (`4e02`): hot cues A–H first, then the memory
/// cues in the order given (rekordbox sends them in table order, not by
/// position, and chains each to its neighbours in that order). Returns the
/// blob and the entry count, which the reply repeats as its last argument.
pub fn extended_cues_blob(cues: &[ExtendedCue]) -> (Vec<u8>, u32) {
    let mut hot: Vec<&ExtendedCue> = cues.iter().filter(|c| c.hot_slot != 0).collect();
    hot.sort_by_key(|c| c.hot_slot);
    let memory: Vec<&ExtendedCue> = cues.iter().filter(|c| c.hot_slot == 0).collect();

    let mut out = Vec::with_capacity((hot.len() + memory.len()) * 0x90);
    for cue in &hot {
        out.extend_from_slice(&extended_cue_entry(cue, None, None));
    }
    let count = memory.len();
    for (index, cue) in memory.iter().enumerate() {
        let previous = index
            .checked_sub(1)
            .map(|i| u16::try_from(i).unwrap_or(u16::MAX));
        let next = (index + 1 < count).then(|| u16::try_from(index + 1).unwrap_or(u16::MAX));
        out.extend_from_slice(&extended_cue_entry(cue, previous, next));
    }
    (
        out,
        u32::try_from(hot.len() + memory.len()).unwrap_or(u32::MAX),
    )
}

/// [OBS] V6 `GetVbrInf` returns 400 32-bit words and a trailing scalar.
const VBR_BLOB_LEN: usize = 1604;

/// The existing zero-filled `2504` / `4502` compatibility placeholder.
///
/// [OBS] This request carries VBR analysis, separately from cue reads.
/// [ASSUME] Retain the existing bytes until the unavailable-data/client
/// contract is established. These zeros do not establish that a track is
/// CBR or lacks VBR data. [UNKNOWN] Track-specific contents, unavailable reply
/// delivery and load/seek effects remain issue 13's evidence tasks.
pub fn vbr_compatibility_blob() -> Vec<u8> {
    vec![0_u8; VBR_BLOB_LEN]
}

/// One entry: a fixed head to `0x48`, the comment's UTF-16LE byte length,
/// the comment with its NUL, a constant word, the colour (hot cues only),
/// and zero padding to the length rekordbox writes.
fn extended_cue_entry(cue: &ExtendedCue, previous: Option<u16>, next: Option<u16>) -> Vec<u8> {
    let comment: Vec<u8> = if cue.comment.is_empty() {
        Vec::new()
    } else {
        cue.comment
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    // rekordbox wrote 0x90 bytes for an entry with a nine-character comment:
    // 0x2a of zero padding after the colour. Kept relative to the comment.
    let len = 0x4a + comment.len() + 8 + 0x2a;
    let mut e = vec![0_u8; len];
    e[0..4].copy_from_slice(&u32::try_from(len).unwrap_or(0).to_le_bytes());
    e[4] = cue.hot_slot;
    // `01` a cue, `02` a loop.
    let is_loop = cue.out_ms > cue.position_ms;
    e[6] = if is_loop { 2 } else { 1 };
    e[10..12].copy_from_slice(&1000_u16.to_le_bytes());
    e[12..16].copy_from_slice(&cue.position_ms.to_le_bytes());
    e[16..20].copy_from_slice(&if is_loop { cue.out_ms } else { 0xffff_ffff }.to_le_bytes());
    // Memory cues chain to their neighbours; hot cues carry `ffff ffff`.
    let link = |value: Option<u16>| value.unwrap_or(0xffff).to_le_bytes();
    e[0x20..0x22].copy_from_slice(&link(previous));
    e[0x22..0x24].copy_from_slice(&link(next));
    e[0x34] = EXTENDED_CUE_UNKNOWN;
    e[0x48..0x4a].copy_from_slice(
        &u16::try_from(comment.len())
            .unwrap_or(u16::MAX)
            .to_le_bytes(),
    );
    let mut at = 0x4a;
    e[at..at + comment.len()].copy_from_slice(&comment);
    at += comment.len();
    e[at..at + 4].copy_from_slice(&EXTENDED_CUE_AFTER_COMMENT);
    at += 4;
    if cue.hot_slot != 0 {
        e[at..at + 4].copy_from_slice(&HOT_CUE_COLOUR);
    }
    e
}

/// The sections a track's blobs are built from, read once per file.
pub struct Analysis<'a> {
    pub dat: Option<&'a Anlz>,
    pub ext: Option<&'a Anlz>,
    pub two_ex: Option<&'a Anlz>,
}

impl Analysis<'_> {
    pub fn beat_grid(&self) -> Option<Vec<u8>> {
        let beats = self.dat?.beat_grid()?;
        Some(beat_grid_blob(&beats))
    }

    pub fn waveform_preview(&self) -> Option<Vec<u8>> {
        let dat = self.dat?;
        let (_, pwav) = dat.waveform(b"PWAV")?;
        let pwv2 = dat.waveform(b"PWV2").map_or(&[][..], |(_, bytes)| bytes);
        Some(waveform_preview_blob(pwav, pwv2))
    }

    pub fn waveform_detail(&self) -> Option<Vec<u8>> {
        let (_, pwv3) = self.ext?.waveform(b"PWV3")?;
        Some(waveform_detail_blob(pwv3))
    }

    /// A tag by fourcc from the file named by its extension.
    pub fn tag(&self, fourcc: &[u8; 4], extension: &[u8; 3]) -> Option<Vec<u8>> {
        let file = match extension {
            b"EXT" => self.ext?,
            b"2EX" => self.two_ex?,
            b"DAT" => self.dat?,
            _ => return None,
        };
        Some(tag_blob(file.section(fourcc)?))
    }
}
