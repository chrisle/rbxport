//! Writing ANLZ files.
//!
//! Sections carry their own framing, so re-emitting a parsed file reproduces it
//! byte for byte. Tags we cannot author honestly — `PSSI` (phrases) and `PVDI`
//! (vocals) — are copied through rather than invented or dropped.

use rbl_core::FourCc;

use crate::{Beat, Section, SECTION_FRAME};

/// The `PMAI` header bytes rekordbox writes after the 12-byte frame.
///
/// Taken from real files; the meaning of the fields is not documented, so they
/// are reproduced rather than derived.
const DEFAULT_HEADER_EXTRA: [u8; 16] =
    [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}
fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// Renders a file from its header extras and sections.
pub fn render(header_extra: &[u8], sections: &[Section]) -> Vec<u8> {
    let len_header = SECTION_FRAME + header_extra.len();
    let body_len: usize = sections
        .iter()
        .map(|s| SECTION_FRAME + s.header.len() + s.payload.len())
        .sum();

    let mut out = Vec::with_capacity(len_header + body_len);
    out.extend_from_slice(b"PMAI");
    out.extend_from_slice(&be32(u32::try_from(len_header).unwrap_or(0)));
    out.extend_from_slice(&be32(u32::try_from(len_header + body_len).unwrap_or(0)));
    out.extend_from_slice(header_extra);

    for section in sections {
        out.extend_from_slice(&section.tag.0);
        out.extend_from_slice(&be32(section.len_header()));
        out.extend_from_slice(&be32(section.len_tag()));
        out.extend_from_slice(&section.header);
        out.extend_from_slice(&section.payload);
    }
    out
}

/// One `PQTZ` section for a grid.
///
/// Separate from the builder so an existing file's grid can be replaced
/// without rebuilding the file around it.
#[must_use]
pub fn beat_grid_section(beats: &[Beat]) -> Section {
    let mut header = Vec::with_capacity(12);
    header.extend_from_slice(&be32(0));
    // The low 16 bits are the signed grid offset, initially zero.
    header.extend_from_slice(&be32(0x0008_0000));
    header.extend_from_slice(&be32(u32::try_from(beats.len()).unwrap_or(0)));

    let mut payload = Vec::with_capacity(beats.len() * 8);
    for beat in beats {
        payload.extend_from_slice(&be16(beat.beat_number));
        payload.extend_from_slice(&be16(beat.tempo_x100));
        payload.extend_from_slice(&be32(beat.time_ms));
    }
    Section::new(b"PQTZ", header, payload)
}

/// An empty `PQT2` section: see [`AnlzBuilder::extended_grid_empty`].
#[must_use]
pub fn extended_grid_empty_section() -> Section {
    let mut header = vec![0_u8; 44];
    header[4..8].copy_from_slice(&be32(0x0100_0002));
    Section::new(b"PQT2", header, Vec::new())
}

/// Builds an ANLZ file section by section.
#[derive(Debug)]
pub struct AnlzBuilder {
    header_extra: Vec<u8>,
    sections: Vec<Section>,
}

impl Default for AnlzBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// `PVBR` with no seek table: 401 zero words after a zero header word,
/// which is what rekordbox itself writes for 65 of 400 reference `.DAT`
/// files [OBS] — the ones with no VBR frames to index.
#[must_use]
pub fn vbr_table_zero_section() -> Section {
    Section::new(b"PVBR", be32(0).to_vec(), vec![0; 401 * 4])
}

impl AnlzBuilder {
    pub fn new() -> Self {
        Self { header_extra: DEFAULT_HEADER_EXTRA.to_vec(), sections: Vec::new() }
    }

    /// Reproduces an existing file's header bytes, for re-emission.
    pub fn header_extra(&mut self, bytes: &[u8]) -> &mut Self {
        self.header_extra = bytes.to_vec();
        self
    }

    /// `PPTH` — the path of the audio, UTF-16BE with a NUL terminator.
    pub fn path(&mut self, path: &str) -> &mut Self {
        let mut text: Vec<u8> = Vec::new();
        for unit in path.encode_utf16() {
            text.extend_from_slice(&unit.to_be_bytes());
        }
        text.extend_from_slice(&[0, 0]);
        let header = be32(u32::try_from(text.len()).unwrap_or(0)).to_vec();
        self.sections.push(Section::new(b"PPTH", header, text));
        self
    }

    /// `PQTZ` — the beat grid.
    pub fn beat_grid(&mut self, beats: &[Beat]) -> &mut Self {
        self.sections.push(beat_grid_section(beats));
        self
    }

    /// `PWAV` / `PWV2` — one byte per column.
    pub fn waveform_preview(&mut self, tag: &[u8; 4], data: &[u8]) -> &mut Self {
        let mut header = Vec::with_capacity(8);
        header.extend_from_slice(&be32(u32::try_from(data.len()).unwrap_or(0)));
        header.extend_from_slice(&be32(0x0001_0000));
        self.sections.push(Section::new(tag, header, data.to_vec()));
        self
    }

    /// `PWV3`..`PWV7` — a stride per column.
    ///
    /// The third header word is constant per tag in every one of 400
    /// reference `.EXT` files [OBS]: `0x0096_0000` on `PWV3`, `0x0096_0305`
    /// on `PWV5` and `0` on `PWV4`; its meaning is [UNKNOWN] and it is
    /// mirrored rather than derived.
    pub fn waveform_scroll(&mut self, tag: &[u8; 4], stride: u32, data: &[u8]) -> &mut Self {
        let entries = if stride == 0 { 0 } else { data.len() / stride as usize };
        let trailer: u32 = match tag {
            b"PWV5" => 0x0096_0305,
            b"PWV4" => 0,
            _ => 0x0096_0000,
        };
        let mut header = Vec::with_capacity(12);
        header.extend_from_slice(&be32(stride));
        header.extend_from_slice(&be32(u32::try_from(entries).unwrap_or(0)));
        // PWV6 has a 20-byte header, not the 24-byte scroll header.
        if tag != b"PWV6" {
            header.extend_from_slice(&be32(trailer));
        }
        self.sections.push(Section::new(tag, header, data.to_vec()));
        self
    }

    /// Adds the zero VBR seek table to this builder.
    pub fn vbr_table_zero(&mut self) -> &mut Self {
        self.sections.push(vbr_table_zero_section());
        self
    }

    /// The pair of empty cue lists rekordbox writes: hot cues (type 1) then
    /// memory cues (type 0), as `PCOB`; and, for an `.EXT`, the same pair
    /// again as `PCO2`. Headers as in all 400 reference files [OBS]: a `PCOB`
    /// carries the list type, two zero halves and `0xffff_ffff`; a `PCO2`
    /// the type and two zero halves.
    pub fn cue_lists(&mut self, extended: bool) -> &mut Self {
        for kind in [1_u32, 0] {
            let mut header = Vec::with_capacity(12);
            header.extend_from_slice(&be32(kind));
            header.extend_from_slice(&be16(0));
            header.extend_from_slice(&be16(0));
            header.extend_from_slice(&be32(0xffff_ffff));
            self.sections.push(Section::new(b"PCOB", header, Vec::new()));
        }
        if extended {
            for kind in [1_u32, 0] {
                let mut header = Vec::with_capacity(8);
                header.extend_from_slice(&be32(kind));
                header.extend_from_slice(&be16(0));
                header.extend_from_slice(&be16(0));
                self.sections.push(Section::new(b"PCO2", header, Vec::new()));
            }
        }
        self
    }

    /// An empty `PQT2`: the 44-byte header with its constant `0x0100_0002`
    /// and every count zero, and no payload — the shape 36 of 400 reference
    /// `.EXT` files carry [OBS]. The payload of a filled one is [UNKNOWN]
    /// (see `Anlz::has_extended_grid`), so an empty one is the honest form.
    pub fn extended_grid_empty(&mut self) -> &mut Self {
        self.sections.push(extended_grid_empty_section());
        self
    }

    /// An empty cue list, which is what the share tree holds.
    pub fn empty_cue_list(&mut self, extended: bool) -> &mut Self {
        self.empty_cue_list_of(extended, 0)
    }

    /// An empty cue list of one type: 0 the memory cues, 1 the hot cues. A
    /// real file carries one of each.
    pub fn empty_cue_list_of(&mut self, extended: bool, list_type: u32) -> &mut Self {
        let mut header = Vec::with_capacity(12);
        header.extend_from_slice(&be32(list_type));
        header.extend_from_slice(&be16(0));
        header.extend_from_slice(&be16(0)); // zero entries
        header.extend_from_slice(&be32(0));
        let tag: &[u8; 4] = if extended { b"PCO2" } else { b"PCOB" };
        self.sections.push(Section::new(tag, header, Vec::new()));
        self
    }

    /// Copies a section through unchanged. This is what preserves `PSSI` and
    /// `PVDI` on re-emission: rekordbox authored them and we cannot.
    pub fn copy_section(&mut self, section: &Section) -> &mut Self {
        self.sections.push(section.clone());
        self
    }

    /// Adds a section from raw parts.
    pub fn raw(&mut self, tag: FourCc, header: Vec<u8>, payload: Vec<u8>) -> &mut Self {
        self.sections.push(Section { tag, header, payload });
        self
    }

    pub fn finish(&self) -> Vec<u8> {
        render(&self.header_extra, &self.sections)
    }
}
