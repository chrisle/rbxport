//! Reader and writer for rekordbox ANLZ analysis files (`.DAT`, `.EXT`, `.2EX`).
//!
//! A file is a `PMAI` header followed by type-tagged sections. Each section is
//! `fourcc`, `len_header`, `len_tag`, then **tag-specific header fields up to
//! `len_header`**, then the payload.
//!
//! That middle part is easy to get wrong: `len_header` differs per tag (16 for
//! `PPTH`, 20 for `PWAV`, 24 for `PQTZ`, 14 for `PWVC`), and the fields it holds
//! are the ones a naive reader expects to find at the start of the body. Reading
//! them from the body instead silently consumes the first bytes of real data.
//!
//! To make that impossible, every section keeps its raw framing — the header
//! bytes and the payload exactly as they appeared — and decoded values are
//! derived from those. Re-emitting a parsed file therefore reproduces it byte
//! for byte, including tags we cannot author ourselves.

pub mod cues;
pub mod encode;
pub mod grid;
pub mod phrase;
pub mod vocal;
pub mod write;

use std::path::{Path, PathBuf};

use rbl_core::FourCc;

#[derive(Debug, thiserror::Error)]
pub enum AnlzError {
    #[error("not an ANLZ file: expected a PMAI header")]
    NotAnlz,
    #[error("file is truncated: {0}")]
    Truncated(&'static str),
    #[error("file declares {declared} bytes but contains {actual}")]
    BadFileLength { declared: u64, actual: u64 },
    #[error("section {tag} declares {declared} bytes but only {available} remain")]
    BadSectionLength {
        tag: String,
        declared: u64,
        available: u64,
    },
    #[error("section {tag} has header length {declared}, outside its {section_len}-byte frame")]
    BadSectionHeaderLength {
        tag: String,
        declared: u64,
        section_len: u64,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AnlzError>;

pub use encode::{author, author_with_overview, share_tree_ppth, AnalysisFiles, BandColumn, Existing};
pub use phrase::{Mood, Phrase, PhraseEdit, SongStructure};
pub use vocal::{VOCAL_FRAME_MS, VOCAL_MAX};
pub use write::AnlzBuilder;

/// Bytes of section framing before the tag-specific header fields.
pub const SECTION_FRAME: usize = 12;

/// A beat in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// 1..=4, where 1 is the downbeat.
    pub beat_number: u16,
    /// BPM x100 at this beat.
    pub tempo_x100: u16,
    /// Milliseconds from the start at 100% pitch.
    pub time_ms: u32,
}

/// One section, with its framing preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub tag: FourCc,
    /// Tag-specific header fields: the bytes between the 12-byte frame and
    /// `len_header`. Their meaning depends on the tag.
    pub header: Vec<u8>,
    /// Everything from `len_header` to `len_tag`.
    pub payload: Vec<u8>,
}

impl Section {
    pub fn new(tag: &[u8; 4], header: Vec<u8>, payload: Vec<u8>) -> Self {
        Self {
            tag: FourCc::new(tag),
            header,
            payload,
        }
    }

    /// `len_header` as written: the frame plus the tag header.
    pub fn len_header(&self) -> u32 {
        u32::try_from(SECTION_FRAME + self.header.len()).unwrap_or(u32::MAX)
    }

    /// `len_tag` as written: everything, including the frame.
    pub fn len_tag(&self) -> u32 {
        u32::try_from(SECTION_FRAME + self.header.len() + self.payload.len()).unwrap_or(u32::MAX)
    }

    fn header_u4(&self, at: usize) -> u32 {
        let b = &self.header;
        u32::from_be_bytes([
            b.get(at).copied().unwrap_or(0),
            b.get(at + 1).copied().unwrap_or(0),
            b.get(at + 2).copied().unwrap_or(0),
            b.get(at + 3).copied().unwrap_or(0),
        ])
    }

    /// `PPTH` — the audio file this analysis describes.
    pub fn as_path(&self) -> Option<String> {
        (self.tag == FourCc::new(b"PPTH")).then(|| utf16be_to_string(&self.payload))
    }

    /// `PQTZ` — the beat grid.
    pub fn as_beat_grid(&self) -> Option<Vec<Beat>> {
        if self.tag != FourCc::new(b"PQTZ") {
            return None;
        }
        // The beat count is the last field of the tag header.
        let count = self.header_u4(8) as usize;
        let mut beats = Vec::with_capacity(count.min(self.payload.len() / 8));
        for chunk in self.payload.as_chunks::<8>().0.iter().take(count) {
            beats.push(Beat {
                beat_number: u16::from_be_bytes([chunk[0], chunk[1]]),
                tempo_x100: u16::from_be_bytes([chunk[2], chunk[3]]),
                time_ms: u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
            });
        }
        Some(beats)
    }

    /// Bytes per waveform column, where the tag carries one.
    pub fn waveform_stride(&self) -> Option<u32> {
        match &self.tag.0 {
            // A single byte per column; the header holds a length and flags.
            b"PWAV" | b"PWV2" => Some(1),
            b"PWV3" | b"PWV4" | b"PWV5" | b"PWV6" | b"PWV7" => Some(self.header_u4(0).max(1)),
            _ => None,
        }
    }

    /// Waveform payload, if this section holds one.
    pub fn waveform(&self) -> Option<&[u8]> {
        self.waveform_stride().map(|_| self.payload.as_slice())
    }

    /// True when this is a cue list. Share-tree cue lists are always empty:
    /// cues live in `djmdCue`. Verified across the whole library.
    pub fn is_cue_list(&self) -> bool {
        matches!(&self.tag.0, b"PCOB" | b"PCO2")
    }

    /// `PCO2` — the extended cue list, entry by entry.
    ///
    /// Empty in the share tree, where the cue list is a header and nothing
    /// else, and populated in an export, where rekordbox writes the cues it
    /// wants a player to draw. Only the extended list is read: the older
    /// `PCOB`/`PCPT` form carries no colour, which is the only reason to read
    /// one of these at all.
    pub fn as_cue_entries(&self) -> Option<Vec<CueEntry>> {
        if self.tag != FourCc::new(b"PCO2") {
            return None;
        }
        let b = &self.payload;
        let mut entries = Vec::new();
        let mut at = 0usize;
        while at + CUE_ENTRY_MIN <= b.len() {
            if b.get(at..at + 4) != Some(b"PCP2") {
                break;
            }
            let len_entry = be32(b, at + 8) as usize;
            if len_entry < CUE_ENTRY_MIN || at + len_entry > b.len() {
                break;
            }
            let e = b.get(at..at + len_entry).unwrap_or_default();
            entries.push(cue_entry(e, len_entry));
            at += len_entry;
        }
        Some(entries)
    }
}

/// One entry of an extended cue list.
///
/// The colour is the point: an entry rekordbox wrote carries both the index it
/// stores in `djmdCue.ColorTableIndex` and the RGB it paints for that index,
/// which is the only place the two have been seen side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueEntry {
    /// Zero for a memory cue, otherwise the hot cue's slot — 1 is A.
    pub hot_cue: u32,
    /// 1 is a cue, 2 a loop.
    pub kind: u8,
    pub time_ms: u32,
    /// Where a loop returns to. Only meaningful on a loop.
    pub loop_time_ms: u32,
    /// The colour a *memory* cue or loop was given, as a row of the colour
    /// table. Hot cues use `color_code` instead.
    pub color_id: u8,
    pub comment: Option<String>,
    /// `djmdCue.ColorTableIndex`.
    pub color_code: Option<u8>,
    /// What rekordbox paints for that index.
    pub rgb: Option<[u8; 3]>,
}

/// Magic, the two lengths, the hot cue, the kind, the two times, the colour
/// row and its eleven trailing bytes: an entry cannot be shorter than this.
const CUE_ENTRY_MIN: usize = 40;

/// The complete CDJ/device RGB palette for `djmdCue.ColorTableIndex`.
///
/// Extracted from rekordbox 7's `djplay::getDevicePadColor(int)` static table
/// and cross-checked against the RGB bytes rekordbox writes to USB `PCO2`
/// entries. Index 0 is the black sentinel; 63 and 64 are black and white.
pub const MEASURED_CUE_COLOURS: &[(u8, [u8; 3])] = &[
    (0, [0x00, 0x00, 0x00]),
    (1, [0x00, 0x00, 0xFF]),
    (2, [0x00, 0x1C, 0xFF]),
    (3, [0x00, 0x38, 0xFF]),
    (4, [0x00, 0x54, 0xFF]),
    (5, [0x00, 0x70, 0xFF]),
    (6, [0x00, 0x8C, 0xFF]),
    (7, [0x00, 0xA8, 0xFF]),
    (8, [0x00, 0xC4, 0xFF]),
    (9, [0x00, 0xE0, 0xFF]),
    (10, [0x00, 0xFF, 0xFF]),
    (11, [0x00, 0xFF, 0xE8]),
    (12, [0x00, 0xFF, 0xD1]),
    (13, [0x00, 0xFF, 0xBA]),
    (14, [0x00, 0xFF, 0xA3]),
    (15, [0x00, 0xFF, 0x8C]),
    (16, [0x00, 0xFF, 0x75]),
    (17, [0x00, 0xFF, 0x5E]),
    (18, [0x00, 0xFF, 0x47]),
    (19, [0x00, 0xFF, 0x30]),
    (20, [0x00, 0xFF, 0x1A]),
    (21, [0x00, 0xFF, 0x00]),
    (22, [0x1A, 0xFF, 0x00]),
    (23, [0x33, 0xFF, 0x00]),
    (24, [0x4D, 0xFF, 0x00]),
    (25, [0x66, 0xFF, 0x00]),
    (26, [0x80, 0xFF, 0x00]),
    (27, [0x99, 0xFF, 0x00]),
    (28, [0xB3, 0xFF, 0x00]),
    (29, [0xCC, 0xFF, 0x00]),
    (30, [0xE6, 0xFF, 0x00]),
    (31, [0xFF, 0xFF, 0x00]),
    (32, [0xFF, 0xE8, 0x00]),
    (33, [0xFF, 0xD1, 0x00]),
    (34, [0xFF, 0xBA, 0x00]),
    (35, [0xFF, 0xA3, 0x00]),
    (36, [0xFF, 0x8C, 0x00]),
    (37, [0xFF, 0x75, 0x00]),
    (38, [0xFF, 0x5E, 0x00]),
    (39, [0xFF, 0x47, 0x00]),
    (40, [0xFF, 0x30, 0x00]),
    (41, [0xFF, 0x1A, 0x00]),
    (42, [0xFF, 0x00, 0x00]),
    (43, [0xFF, 0x00, 0x17]),
    (44, [0xFF, 0x00, 0x2E]),
    (45, [0xFF, 0x00, 0x45]),
    (46, [0xFF, 0x00, 0x5C]),
    (47, [0xFF, 0x00, 0x73]),
    (48, [0xFF, 0x00, 0x8A]),
    (49, [0xFF, 0x00, 0xA1]),
    (50, [0xFF, 0x00, 0xB8]),
    (51, [0xFF, 0x00, 0xCF]),
    (52, [0xFF, 0x00, 0xE6]),
    (53, [0xFF, 0x00, 0xFF]),
    (54, [0xE6, 0x00, 0xFF]),
    (55, [0xCC, 0x00, 0xFF]),
    (56, [0xB3, 0x00, 0xFF]),
    (57, [0x99, 0x00, 0xFF]),
    (58, [0x80, 0x00, 0xFF]),
    (59, [0x66, 0x00, 0xFF]),
    (60, [0x4D, 0x00, 0xFF]),
    (61, [0x33, 0x00, 0xFF]),
    (62, [0x1A, 0x00, 0xFF]),
    (63, [0x00, 0x00, 0x00]),
    (64, [0xFF, 0xFF, 0xFF]),
];

/// The RGB for a `ColorTableIndex`, or `None` where it has not been read.
pub fn cue_colour(index: u8) -> Option<[u8; 3]> {
    MEASURED_CUE_COLOURS
        .iter()
        .find(|&&(i, _)| i == index)
        .map(|&(_, rgb)| rgb)
}

/// The complete RGB palette rekordbox paints on screen.
///
/// Extracted from rekordbox 7's `djplay::getPadColor(int)` static table. This
/// intentionally differs from the saturated device/export palette above.
pub const DRAWN_CUE_COLOURS: &[(u8, [u8; 3])] = &[
    (0, [0x00, 0x00, 0x00]),
    (1, [0x30, 0x5A, 0xFF]),
    (2, [0x50, 0x73, 0xFF]),
    (3, [0x50, 0x8C, 0xFF]),
    (4, [0x50, 0xA0, 0xFF]),
    (5, [0x50, 0xB4, 0xFF]),
    (6, [0x50, 0xB0, 0xF2]),
    (7, [0x50, 0xAE, 0xE8]),
    (8, [0x45, 0xAC, 0xDB]),
    (9, [0x00, 0xE0, 0xFF]),
    (10, [0x19, 0xDA, 0xF0]),
    (11, [0x32, 0xD2, 0xE6]),
    (12, [0x21, 0xB4, 0xB9]),
    (13, [0x20, 0xAA, 0xA0]),
    (14, [0x1F, 0xA3, 0x92]),
    (15, [0x19, 0xA0, 0x8C]),
    (16, [0x14, 0xA5, 0x84]),
    (17, [0x14, 0xAA, 0x7D]),
    (18, [0x10, 0xB1, 0x76]),
    (19, [0x30, 0xD2, 0x6E]),
    (20, [0x37, 0xDE, 0x5A]),
    (21, [0x3C, 0xEB, 0x50]),
    (22, [0x28, 0xE2, 0x14]),
    (23, [0x7D, 0xC1, 0x3D]),
    (24, [0x8C, 0xC8, 0x32]),
    (25, [0x9B, 0xD7, 0x23]),
    (26, [0xA5, 0xE1, 0x16]),
    (27, [0xA5, 0xDC, 0x0A]),
    (28, [0xAA, 0xD2, 0x08]),
    (29, [0xB4, 0xC8, 0x05]),
    (30, [0xB4, 0xBE, 0x04]),
    (31, [0xBA, 0xB4, 0x04]),
    (32, [0xC3, 0xAF, 0x04]),
    (33, [0xE1, 0xAA, 0x00]),
    (34, [0xFF, 0xA0, 0x00]),
    (35, [0xFF, 0x96, 0x00]),
    (36, [0xFF, 0x8C, 0x00]),
    (37, [0xFF, 0x75, 0x00]),
    (38, [0xE0, 0x64, 0x1B]),
    (39, [0xE0, 0x46, 0x1E]),
    (40, [0xE0, 0x30, 0x1E]),
    (41, [0xE0, 0x28, 0x23]),
    (42, [0xE6, 0x28, 0x28]),
    (43, [0xFF, 0x37, 0x6F]),
    (44, [0xFF, 0x2D, 0x6F]),
    (45, [0xFF, 0x12, 0x7B]),
    (46, [0xF5, 0x1E, 0x8C]),
    (47, [0xEB, 0x2D, 0xA0]),
    (48, [0xE6, 0x37, 0xB4]),
    (49, [0xDE, 0x44, 0xCF]),
    (50, [0xDE, 0x44, 0x8D]),
    (51, [0xE6, 0x30, 0xB4]),
    (52, [0xE6, 0x19, 0xDC]),
    (53, [0xE6, 0x00, 0xFF]),
    (54, [0xDC, 0x00, 0xFF]),
    (55, [0xCC, 0x00, 0xFF]),
    (56, [0xB4, 0x32, 0xFF]),
    (57, [0xB9, 0x3C, 0xFF]),
    (58, [0xC5, 0x42, 0xFF]),
    (59, [0xAA, 0x5A, 0xFF]),
    (60, [0xAA, 0x72, 0xFF]),
    (61, [0x82, 0x72, 0xFF]),
    (62, [0x64, 0x73, 0xFF]),
    (63, [0x00, 0x00, 0x00]),
    (64, [0xFF, 0xFF, 0xFF]),
];

/// The RGB rekordbox paints for a `ColorTableIndex`, or `None` if it is not a
/// valid palette index.
pub fn cue_colour_drawn(index: u8) -> Option<[u8; 3]> {
    DRAWN_CUE_COLOURS
        .iter()
        .find(|&&(i, _)| i == index)
        .map(|&(_, rgb)| rgb)
}

fn cue_entry(e: &[u8], len_entry: usize) -> CueEntry {
    let mut entry = CueEntry {
        hot_cue: be32(e, 12),
        kind: e.get(16).copied().unwrap_or(0),
        time_ms: be32(e, 20),
        loop_time_ms: be32(e, 24),
        color_id: e.get(28).copied().unwrap_or(0),
        comment: None,
        color_code: None,
        rgb: None,
    };
    // A comment is what pushes the entry past the fixed part, and the colour
    // sits after the comment rather than at a fixed offset — so an entry with
    // no room for a comment length has no colour either.
    if len_entry <= CUE_ENTRY_MIN + 3 {
        return entry;
    }
    let len_comment = be32(e, CUE_ENTRY_MIN) as usize;
    let start = CUE_ENTRY_MIN + 4;
    let Some(end) = start
        .checked_add(len_comment)
        .filter(|&end| end <= len_entry)
    else {
        return entry;
    };
    if len_comment > 0 {
        entry.comment = Some(utf16be_to_string(e.get(start..end).unwrap_or_default()));
    }
    if let Some(colour) = e.get(end..end + 4) {
        entry.color_code = colour.first().copied();
        entry.rgb = Some([
            colour.get(1).copied().unwrap_or(0),
            colour.get(2).copied().unwrap_or(0),
            colour.get(3).copied().unwrap_or(0),
        ]);
    }
    entry
}

#[derive(Debug, Clone, Default)]
pub struct Anlz {
    /// Header bytes after the 12-byte `PMAI` frame, preserved for re-emission.
    pub header_extra: Vec<u8>,
    pub sections: Vec<Section>,
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([
        b.get(at).copied().unwrap_or(0),
        b.get(at + 1).copied().unwrap_or(0),
        b.get(at + 2).copied().unwrap_or(0),
        b.get(at + 3).copied().unwrap_or(0),
    ])
}

/// Parses an ANLZ file.
pub fn parse(bytes: &[u8]) -> Result<Anlz> {
    if bytes.len() < SECTION_FRAME {
        return Err(AnlzError::Truncated("header"));
    }
    if bytes.get(0..4) != Some(b"PMAI") {
        return Err(AnlzError::NotAnlz);
    }
    let len_header = be32(bytes, 4) as usize;
    if len_header < SECTION_FRAME || len_header > bytes.len() {
        return Err(AnlzError::Truncated("header length"));
    }
    let file_len = be32(bytes, 8) as usize;
    if file_len != bytes.len() {
        return Err(AnlzError::BadFileLength {
            declared: file_len as u64,
            actual: bytes.len() as u64,
        });
    }
    let header_extra = bytes.get(SECTION_FRAME..len_header).unwrap_or(&[]).to_vec();

    let mut sections = Vec::new();
    let mut at = len_header;
    while at + SECTION_FRAME <= bytes.len() {
        let tag = FourCc([
            bytes.get(at).copied().unwrap_or(0),
            bytes.get(at + 1).copied().unwrap_or(0),
            bytes.get(at + 2).copied().unwrap_or(0),
            bytes.get(at + 3).copied().unwrap_or(0),
        ]);
        let section_header = be32(bytes, at + 4) as usize;
        let section_len = be32(bytes, at + 8) as usize;

        // A zero or overlong length would loop forever or read past the end.
        if section_len < SECTION_FRAME || at + section_len > bytes.len() {
            return Err(AnlzError::BadSectionLength {
                tag: tag.to_string(),
                declared: section_len as u64,
                available: (bytes.len() - at) as u64,
            });
        }
        if section_header < SECTION_FRAME || section_header > section_len {
            return Err(AnlzError::BadSectionHeaderLength {
                tag: tag.to_string(),
                declared: section_header as u64,
                section_len: section_len as u64,
            });
        }
        let header_end = at + section_header;
        sections.push(Section {
            tag,
            header: bytes
                .get(at + SECTION_FRAME..header_end)
                .unwrap_or(&[])
                .to_vec(),
            payload: bytes
                .get(header_end..at + section_len)
                .unwrap_or(&[])
                .to_vec(),
        });
        at += section_len;
    }
    if at != bytes.len() {
        return Err(AnlzError::Truncated("section frame"));
    }

    Ok(Anlz {
        header_extra,
        sections,
    })
}

fn utf16be_to_string(raw: &[u8]) -> String {
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

impl Anlz {
    pub fn read(path: &Path) -> Result<Self> {
        parse(&std::fs::read(path)?)
    }

    pub fn section(&self, tag: &[u8; 4]) -> Option<&Section> {
        let want = FourCc::new(tag);
        self.sections.iter().find(|s| s.tag == want)
    }

    pub fn beat_grid(&self) -> Option<Vec<Beat>> {
        self.sections.iter().find_map(Section::as_beat_grid)
    }

    /// Signed millisecond correction at PQTZ +0x12. rekordbox's
    /// `MstSaveQtzOffset` rewrites this field without moving the beat records.
    pub fn grid_offset(&self) -> Option<i16> {
        let bytes = self.section(b"PQTZ")?.header.get(6..8)?;
        Some(i16::from_be_bytes([bytes[0], bytes[1]]))
    }

    pub fn with_grid_offset(&self, offset_ms: i16) -> Option<Vec<u8>> {
        let mut next = self.clone();
        let section = next
            .sections
            .iter_mut()
            .find(|s| s.tag == FourCc::new(b"PQTZ"))?;
        section
            .header
            .get_mut(6..8)?
            .copy_from_slice(&offset_ms.to_be_bytes());
        Some(next.to_bytes())
    }

    /// Every extended cue entry in the file, across every `PCO2` section.
    pub fn cue_entries(&self) -> Vec<CueEntry> {
        self.sections
            .iter()
            .filter_map(Section::as_cue_entries)
            .flatten()
            .collect()
    }

    pub fn path(&self) -> Option<String> {
        self.sections.iter().find_map(Section::as_path)
    }

    /// Waveform payload and its stride for a given tag.
    pub fn waveform(&self, tag: &[u8; 4]) -> Option<(u32, &[u8])> {
        let section = self.section(tag)?;
        Some((section.waveform_stride()?, section.payload.as_slice()))
    }

    /// Re-emits the file exactly as parsed.
    pub fn to_bytes(&self) -> Vec<u8> {
        write::render(&self.header_extra, &self.sections)
    }

    /// Whether this file carries extended beat data. Its payload contains an
    /// opaque 16-bit value per beat. Grid saves preserve it only when the
    /// old-grid checksum, endpoints, times and tempos remain compatible;
    /// see [`Anlz::with_extended_grid_edit`].
    #[must_use]
    pub fn has_extended_grid(&self) -> bool {
        self.section(b"PQT2").is_some()
    }

    /// The file with its `PQT2` emptied and every other section byte-for-byte
    /// as it was, or `None` when there is no filled `PQT2` to empty.
    ///
    /// What a grid edit does to the `.EXT`: the extended grid describes the
    /// beats the `.DAT` used to have, and its payload cannot be re-derived
    /// (see [`Anlz::has_extended_grid`]), so it is replaced with the empty
    /// form 36 of 400 reference `.EXT` files carry [OBS] — the shape
    /// rekordbox 7.2.11 accepted on a registered analysis and left in place
    /// when it loaded the track (recorded 2026-09-17, see
    /// `rbl-db/src/write.rs`).
    #[must_use]
    pub fn with_extended_grid_cleared(&self) -> Option<Vec<u8>> {
        let at = self
            .sections
            .iter()
            .position(|s| s.tag == FourCc::new(b"PQT2"))?;
        if self.sections.get(at).is_some_and(|s| s.payload.is_empty()) {
            return None;
        }
        let mut sections = self.sections.clone();
        if let Some(slot) = sections.get_mut(at) {
            *slot = write::extended_grid_empty_section();
        }
        Some(write::render(&self.header_extra, &sections))
    }

    /// Preserve extended payload only when its old-grid checksum, endpoints,
    /// offset and every new time/tempo still agree (`CAnalyzerIF`'s save path).
    /// Beat-number-only edits refresh the header without inventing payload.
    #[must_use]
    pub fn with_extended_grid_edit(
        &self,
        old: &[Beat],
        new: &[Beat],
        offset: i16,
    ) -> Option<Vec<u8>> {
        let at = self
            .sections
            .iter()
            .position(|s| s.tag == FourCc::new(b"PQT2"))?;
        let section = self.sections.get(at)?;
        let word = |at: usize| {
            section
                .header
                .get(at..at + 4)
                .and_then(|b| <[u8; 4]>::try_from(b).ok())
                .map(u32::from_be_bytes)
        };
        let packed = |b: &Beat| (u32::from(b.beat_number) << 16) | u32::from(b.tempo_x100);
        let checksum = |beats: &[Beat]| {
            beats.iter().fold(0_u32, |sum, b| {
                sum.wrapping_add(b.time_ms)
                    .wrapping_add(u32::from(b.beat_number))
                    .wrapping_add(u32::from(b.tempo_x100))
            })
        };
        let preserve = !old.is_empty()
            && old.len() == new.len()
            && section.header.len() >= 36
            && section.payload.len() == old.len() * 2
            && word(28) == u32::try_from(old.len()).ok()
            && word(12) == old.first().map(packed)
            && word(16) == old.first().map(|b| b.time_ms)
            && word(20) == old.last().map(packed)
            && word(24) == old.last().map(|b| b.time_ms)
            && word(32) == Some(checksum(old))
            && section.header.get(8..10) == Some(offset.to_be_bytes().as_slice())
            && old.iter().zip(new).all(|(a, b)| {
                i64::from(a.time_ms) + i64::from(offset) == i64::from(b.time_ms)
                    && a.tempo_x100 == b.tempo_x100
            });
        let mut next = self.clone();
        let slot = next.sections.get_mut(at)?;
        if slot.header.len() < 36 {
            *slot = write::extended_grid_empty_section();
        } else {
            slot.header.get_mut(8..36)?.fill(0);
            if preserve {
                let first = new.first()?;
                let last = new.last()?;
                for (at, value) in [
                    (12, packed(first)),
                    (16, first.time_ms),
                    (20, packed(last)),
                    (24, last.time_ms),
                    (28, u32::try_from(new.len()).ok()?),
                    (32, checksum(new)),
                ] {
                    slot.header
                        .get_mut(at..at + 4)?
                        .copy_from_slice(&value.to_be_bytes());
                }
            } else {
                slot.payload.clear();
            }
        }
        let bytes = next.to_bytes();
        (bytes != self.to_bytes()).then_some(bytes)
    }

    /// The file with its beat grid replaced and every other section
    /// byte-for-byte as it was.
    ///
    /// A file with no `PQTZ` gains one after `PPTH` and `PVBR`, matching the
    /// order in a freshly initialized DAT [OBS].
    #[must_use]
    pub fn with_beat_grid(&self, beats: &[Beat]) -> Vec<u8> {
        let replacement = write::beat_grid_section(beats);
        let mut sections = self.sections.clone();
        if let Some(at) = sections.iter().position(|s| s.tag == FourCc::new(b"PQTZ")) {
            sections[at] = replacement;
        } else {
            let after_required_prefix = sections
                .iter()
                .take_while(|section| section.tag == FourCc::new(b"PPTH") || section.tag == FourCc::new(b"PVBR"))
                .count();
            sections.insert(after_required_prefix, replacement);
        }
        write::render(&self.header_extra, &sections)
    }
}

/// Resolves `djmdContent.AnalysisDataPath` against the share root.
pub fn resolve(share_root: &Path, analysis_data_path: &str) -> PathBuf {
    share_root.join(analysis_data_path.trim_start_matches('/'))
}

/// The `.EXT` / `.2EX` sibling of a `.DAT` path.
pub fn sibling(dat: &Path, extension: &str) -> PathBuf {
    dat.with_extension(extension)
}

#[cfg(test)]
mod grid_offset_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn offset_edits_preserve_every_other_byte_and_the_original_beats() {
        let beats = vec![Beat {
            beat_number: 1,
            tempo_x100: 12800,
            time_ms: 1000,
        }];
        let original = Anlz {
            header_extra: vec![0; 16],
            sections: vec![write::beat_grid_section(&beats)],
        };
        let before = original.to_bytes();
        for offset in [234, -467, i16::MIN, i16::MAX, 0] {
            let changed = original.with_grid_offset(offset).unwrap();
            let parsed = parse(&changed).unwrap();
            assert_eq!(parsed.grid_offset(), Some(offset));
            assert_eq!(parsed.beat_grid(), Some(beats.clone()));
            assert_eq!(parsed.with_grid_offset(0).unwrap(), before);
        }
        assert!(Anlz::default().with_grid_offset(1).is_none());
    }
}
