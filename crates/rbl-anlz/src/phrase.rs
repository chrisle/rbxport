//! `PSSI` — the song structure, which rekordbox draws as the phrase strip
//! above the waveform.
//!
//! The tag lives in the `.EXT` file. Its shape is documented by Deep Symmetry
//! [REF] and confirmed here against the reference library [OBS]:
//!
//! ```text
//! 0c  version          u16   supported versions 0 and 1
//! 0e  len_entry_bytes  u16   normally 24
//! 10  len_entries      u16   phrase count
//! 12  ── body, sometimes XOR-masked ──
//!     +00  mood       u16   1 high, 2 mid, 3 low
//!     +02  reserved   u32
//!     +06  reserved   u16   zero after decoding
//!     +08  end_beat   u16   where the last phrase ends
//!     +0a  unknown    2 bytes
//!     +0c  bank       u8    Lighting-mode stylistic bank
//!     +0d  unknown    1 byte
//!     +0e  entries, `len_entry_bytes` each
//! ```
//!
//! and each entry:
//!
//! ```text
//! +00 index u16   1-based
//! +02 beat  u16   first beat of the phrase
//! +04 kind  u16   meaning depends on the mood
//! +07 k1    u8  ┐
//! +09 k2    u8  ├ high-mood label variants
//! +13 k3    u8  ┘
//! +0b b     u8    0: one extra beat, 1: three (only seen on "UP 3")
//! +0c beat2 u16 ┐
//! +0e beat3 u16 ├ lighting hints inside the phrase, purpose [UNKNOWN]
//! +10 beat4 u16 ┘
//! +14 fill      u16   non-zero when the phrase ends with a fill-in
//! +16 fill_beat u16   where that fill-in starts
//! ```
//!
//! **The masking.** The body from byte `0x12` may be XOR-masked with a
//! repeating 19-byte pad. Its starting offset is the tag version modulo 19,
//! and each pad byte adds the low byte of `len_entries`. The reserved word
//! at bytes `0x18..0x1a` must be zero after decoding; check it before deciding
//! whether to unmask, then validate it and the mood again afterward.

use crate::Section;
use rbl_core::FourCc;

/// The fixed XOR pad, before `len_entries` is added
/// to every byte.
const MASK_BASE: [u8; 19] = [
    0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3, 0xE8,
    0xE9, 0xF4, 0xE1,
];

/// Bytes of the body before the first entry.
const BODY_PREAMBLE: usize = 14;

/// The smallest entry this can read every documented field out of.
const MIN_ENTRY_BYTES: usize = 24;

/// How rekordbox classified the track as a whole, which decides what the
/// per-phrase `kind` means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    High,
    Mid,
    Low,
}

impl Mood {
    fn from_u16(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::High),
            2 => Some(Self::Mid),
            3 => Some(Self::Low),
            _ => None,
        }
    }
}

/// One phrase of the song structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    /// 1-based position in the tag.
    pub index: u16,
    /// The beat the phrase starts on, 1-based, matching the `PQTZ` grid.
    pub beat: u16,
    /// The raw `kind`. Only meaningful together with the mood.
    pub kind: u16,
    /// What rekordbox draws, e.g. `INTRO 2`, `UP 3`, `VERSE 1`. Empty for a
    /// `kind` no mood defines, rather than a made-up name.
    pub label: &'static str,
    /// A fill-in at the end of the phrase, which rekordbox dots on the
    /// waveform.
    pub fill_in: bool,
    /// The beat that fill-in starts on. Zero when there is none.
    pub fill_in_beat: u16,
}

/// A parsed `PSSI`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongStructure {
    pub mood: Mood,
    /// The beat the last phrase ends on. Audio may continue past it.
    pub end_beat: u16,
    /// The Lighting-mode bank the user assigned. 0 in every file checked,
    /// meaning no assignment [OBS]; values above 8 have been seen elsewhere,
    /// so it is kept raw rather than turned into an enum.
    pub bank: u8,
    pub phrases: Vec<Phrase>,
}

/// The label rekordbox shows for a `kind` in a given mood.
///
/// The high mood's numbered variants are not in `kind` at all: they come from
/// three flag bytes, in the combinations tabulated by Deep Symmetry [REF].
/// Every one of the 8,488 high-mood phrases in the reference library uses one
/// of those combinations and no other [OBS].
fn label_for(mood: Mood, kind: u16, k1: u8, k2: u8, k3: u8) -> &'static str {
    match mood {
        Mood::High => match (kind, k1, k2, k3) {
            (1, 1, _, _) => "INTRO 1",
            (1, _, _, _) => "INTRO 2",
            (2, _, 0, 0) => "UP 1",
            (2, _, 0, _) => "UP 2",
            (2, _, _, _) => "UP 3",
            (3, ..) => "DOWN",
            (5, 1, _, _) => "CHORUS 1",
            (5, ..) => "CHORUS 2",
            (6, 1, _, _) => "OUTRO 1",
            (6, ..) => "OUTRO 2",
            _ => "",
        },
        // rekordbox draws no distinction between the three ids it has for
        // each of "Verse 1" and "Verse 2" in the low mood.
        Mood::Low => match kind {
            1 => "INTRO",
            2..=4 => "VERSE 1",
            5..=7 => "VERSE 2",
            8 => "BRIDGE",
            9 => "CHORUS",
            10 => "OUTRO",
            _ => "",
        },
        Mood::Mid => match kind {
            1 => "INTRO",
            2 => "VERSE 1",
            3 => "VERSE 2",
            4 => "VERSE 3",
            5 => "VERSE 4",
            6 => "VERSE 5",
            7 => "VERSE 6",
            8 => "BRIDGE",
            9 => "CHORUS",
            10 => "OUTRO",
            _ => "",
        },
    }
}

fn be16(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([
        bytes.get(at).copied().unwrap_or(0),
        bytes.get(at + 1).copied().unwrap_or(0),
    ])
}

fn phrase_layout(raw: &[u8]) -> Option<(u16, usize, u16)> {
    raw.get(..6)?;
    let version = be16(raw, 0);
    let entry_bytes = usize::from(be16(raw, 2));
    if version > 1 || entry_bytes < MIN_ENTRY_BYTES {
        return None;
    }
    Some((version, entry_bytes, be16(raw, 4)))
}

/// Undoes the XOR pad in place.
fn unmask(body: &mut [u8], len_entries: u16, version: u16) {
    // The pad's period does not divide a `u16` boundary, so the offset within
    // it is the offset within the body — not within any chunk.
    #[allow(clippy::cast_possible_truncation)]
    let add = len_entries as u8;
    let offset = usize::from(version) % MASK_BASE.len();
    for (at, byte) in body.iter_mut().enumerate() {
        let pad = MASK_BASE
            .get((at % MASK_BASE.len() + offset) % MASK_BASE.len())
            .copied()
            .unwrap_or(0)
            .wrapping_add(add);
        *byte ^= pad;
    }
}

fn decoded_body(raw: &[u8], count: u16, version: u16) -> Option<(Vec<u8>, Mood)> {
    let mut body = raw.get(6..)?.to_vec();
    body.get(..BODY_PREAMBLE)?;
    if be16(&body, 6) != 0 {
        unmask(&mut body, count, version);
    }
    if be16(&body, 6) != 0 {
        return None;
    }
    let mood = Mood::from_u16(be16(&body, 0))?;
    Some((body, mood))
}

impl Section {
    /// `PSSI` — the song structure, unmasked if it needed it.
    ///
    /// `None` for any other tag, and for a `PSSI` too short or too damaged to
    /// read: a garbled tag must cost the phrase strip, not the track.
    #[must_use]
    pub fn as_song_structure(&self) -> Option<SongStructure> {
        if self.tag != FourCc::new(b"PSSI") {
            return None;
        }
        // Field offsets are relative to the joined tag bytes, independent of
        // where its declared header/payload boundary falls.
        let mut raw = Vec::with_capacity(self.header.len() + self.payload.len());
        raw.extend_from_slice(&self.header);
        raw.extend_from_slice(&self.payload);

        let (version, len_entry_bytes, len_entries) = phrase_layout(&raw)?;
        let (body, mood) = decoded_body(&raw, len_entries, version)?;

        let entries = body.get(BODY_PREAMBLE..)?;
        // A count the payload cannot back is a damaged tag; read what is
        // there rather than trusting the header or refusing the whole tag.
        let available = entries.len() / len_entry_bytes;
        let count = (len_entries as usize).min(available);

        let mut phrases = Vec::with_capacity(count);
        for entry in entries.chunks_exact(len_entry_bytes).take(count) {
            let kind = be16(entry, 4);
            let k1 = entry.get(0x07).copied().unwrap_or(0);
            let k2 = entry.get(0x09).copied().unwrap_or(0);
            let k3 = entry.get(0x13).copied().unwrap_or(0);
            phrases.push(Phrase {
                index: be16(entry, 0),
                beat: be16(entry, 2),
                kind,
                label: label_for(mood, kind, k1, k2, k3),
                fill_in: be16(entry, 0x14) != 0,
                fill_in_beat: be16(entry, 0x16),
            });
        }

        Some(SongStructure {
            mood,
            end_beat: be16(&body, 8),
            bank: body.get(0x0c).copied().unwrap_or(0),
            phrases,
        })
    }
}

/// An edit to the phrases, from the GRID panel's PHRASE EDIT buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhraseEdit {
    /// CUT: the phrase under `beat` is split there. The new phrase carries
    /// the old one's kind. A fill-in remains only on the latter half, and
    /// only if its start does not precede the new phrase start.
    Cut { beat: u16 },
    /// CLEAR: the phrase under `beat` is taken out; the one before it runs
    /// on to where it ended.
    Clear { beat: u16 },
}

impl Section {
    /// The `PSSI` with an edit applied, written unmasked as rekordbox 7
    /// writes its own; `None` for any other tag, a tag that does not read,
    /// or an edit that changes nothing (a cut on a phrase's first beat, a
    /// clear with no phrase under the beat).
    ///
    /// Each entry is carried byte for byte apart from its index, beat and
    /// fill-in bounds, so the fields nobody has named keep whatever they held;
    /// a cut phrase's lighting hints are copied into both halves [ASSUME].
    #[must_use]
    pub fn with_phrase_edit(&self, edit: PhraseEdit) -> Option<Section> {
        if self.tag != FourCc::new(b"PSSI") {
            return None;
        }
        let mut raw = Vec::with_capacity(self.header.len() + self.payload.len());
        raw.extend_from_slice(&self.header);
        raw.extend_from_slice(&self.payload);
        let (version, entry_bytes, len_entries) = phrase_layout(&raw)?;
        let (body, _) = decoded_body(&raw, len_entries, version)?;
        let preamble = body.get(..BODY_PREAMBLE)?.to_vec();
        let count = (len_entries as usize).min(body.get(BODY_PREAMBLE..)?.len() / entry_bytes);
        let mut entries: Vec<Vec<u8>> = body
            .get(BODY_PREAMBLE..)?
            .chunks_exact(entry_bytes)
            .take(count)
            .map(<[u8]>::to_vec)
            .collect();

        let at = match edit {
            PhraseEdit::Cut { beat } | PhraseEdit::Clear { beat } => {
                entries.iter().rposition(|entry| be16(entry, 2) <= beat)?
            }
        };
        match edit {
            PhraseEdit::Cut { beat } => {
                let first = entries.get(at)?;
                if be16(first, 2) == beat {
                    return None;
                }
                let mut second = first.clone();
                second.get_mut(2..4)?.copy_from_slice(&beat.to_be_bytes());
                if be16(&second, 0x16) < beat {
                    second.get_mut(0x14..0x18)?.fill(0);
                }
                entries.get_mut(at)?.get_mut(0x14..0x18)?.fill(0);
                entries.insert(at + 1, second);
            }
            PhraseEdit::Clear { .. } => {
                entries.remove(at);
            }
        }
        for (i, entry) in entries.iter_mut().enumerate() {
            let index = u16::try_from(i + 1).unwrap_or(u16::MAX);
            if let Some(slot) = entry.get_mut(0..2) {
                slot.copy_from_slice(&index.to_be_bytes());
            }
        }

        let mut out = Vec::with_capacity(raw.len() + entry_bytes);
        out.extend_from_slice(raw.get(..4)?);
        out.extend_from_slice(&u16::try_from(entries.len()).unwrap_or(u16::MAX).to_be_bytes());
        out.extend_from_slice(&preamble);
        for entry in &entries {
            out.extend_from_slice(entry);
        }
        // Preserve the file's declared header/payload boundary.
        let split = self.header.len().min(out.len());
        Some(Section::new(b"PSSI", out.get(..split)?.to_vec(), out.get(split..)?.to_vec()))
    }
}

impl crate::Anlz {
    /// The file with its phrases edited, every other section as it was;
    /// `None` when there is no `PSSI` or the edit changes nothing.
    #[must_use]
    pub fn with_phrase_edit(&self, edit: PhraseEdit) -> Option<Vec<u8>> {
        let at = self.sections.iter().position(|s| s.tag == FourCc::new(b"PSSI"))?;
        let edited = self.sections.get(at)?.with_phrase_edit(edit)?;
        let mut sections = self.sections.clone();
        *sections.get_mut(at)? = edited;
        Some(crate::write::render(&self.header_extra, &sections))
    }

    /// The song structure, if the file carries one.
    #[must_use]
    pub fn song_structure(&self) -> Option<SongStructure> {
        self.sections.iter().find_map(Section::as_song_structure)
    }

    /// The phrases alone, which is what the waveform strip draws.
    #[must_use]
    pub fn phrases(&self) -> Option<Vec<Phrase>> {
        self.song_structure().map(|s| s.phrases)
    }
}


impl Section {
    /// Mask a plaintext phrase section as rekordbox does on USB export.
    /// Existing masked bytes and all unknown fields are preserved.
    #[must_use]
    pub fn with_export_phrase_mask(&self) -> Self {
        if self.tag != FourCc::new(b"PSSI") { return self.clone(); }
        let mut raw = self.header.clone();
        raw.extend_from_slice(&self.payload);
        let Some((version, _, count)) = phrase_layout(&raw) else { return self.clone(); };
        if raw.len() < 6 + BODY_PREAMBLE || be16(&raw, 12) != 0 || Mood::from_u16(be16(&raw, 6)).is_none() {
            return self.clone();
        }
        unmask(&mut raw[6..], count, version);
        Self::new(b"PSSI", raw[..self.header.len()].to_vec(), raw[self.header.len()..].to_vec())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod edit_tests {
    use super::*;

    /// A rekordbox 7 style PSSI: 20 header bytes, three mid-mood phrases.
    fn pssi(masked: bool) -> Section {
        let mut raw = Vec::new();
        raw.extend_from_slice(&24_u32.to_be_bytes());
        raw.extend_from_slice(&3_u16.to_be_bytes());
        let mut body = Vec::new();
        body.extend_from_slice(&2_u16.to_be_bytes()); // mood mid
        body.extend_from_slice(&[0; 6]);
        body.extend_from_slice(&64_u16.to_be_bytes()); // end beat
        body.extend_from_slice(&[0, 0, 0, 0]); // unknown, bank, unknown
        for (index, beat, kind) in [(1_u16, 1_u16, 1_u16), (2, 17, 2), (3, 33, 3)] {
            let mut entry = vec![0_u8; 24];
            entry[0..2].copy_from_slice(&index.to_be_bytes());
            entry[2..4].copy_from_slice(&beat.to_be_bytes());
            entry[4..6].copy_from_slice(&kind.to_be_bytes());
            entry[0x0c..0x0e].copy_from_slice(&(beat + 4).to_be_bytes()); // a hint
            body.extend_from_slice(&entry);
        }
        if masked {
            unmask(&mut body, 3, 0); // XOR is its own inverse
        }
        raw.extend_from_slice(&body);
        Section::new(b"PSSI", raw[..20].to_vec(), raw[20..].to_vec())
    }

    fn beats(section: &Section) -> Vec<(u16, u16, u16)> {
        section.as_song_structure().unwrap().phrases.iter().map(|p| (p.index, p.beat, p.kind)).collect()
    }

    #[test]
    fn a_cut_splits_the_phrase_under_the_beat_and_a_clear_takes_it_out() {
        let original = pssi(false);
        assert_eq!(beats(&original), vec![(1, 1, 1), (2, 17, 2), (3, 33, 3)]);

        let cut = original.with_phrase_edit(PhraseEdit::Cut { beat: 25 }).unwrap();
        assert_eq!(beats(&cut), vec![(1, 1, 1), (2, 17, 2), (3, 25, 2), (4, 33, 3)]);
        assert_eq!(cut.header.len(), 20, "the split is kept");
        // The copied half keeps the kind and the hint bytes of the phrase it came from.
        let raw: Vec<u8> = [cut.header.clone(), cut.payload.clone()].concat();
        assert_eq!(&raw[20 + 2 * 24 + 0x0c..20 + 2 * 24 + 0x0e], &21_u16.to_be_bytes());
        assert!(original.with_phrase_edit(PhraseEdit::Cut { beat: 17 }).is_none(), "a cut on a first beat is nothing");
        assert!(original.with_phrase_edit(PhraseEdit::Cut { beat: 0 }).is_none(), "before the first phrase");

        let cleared = original.with_phrase_edit(PhraseEdit::Clear { beat: 20 }).unwrap();
        assert_eq!(beats(&cleared), vec![(1, 1, 1), (2, 33, 3)]);
        let structure = cleared.as_song_structure().unwrap();
        assert_eq!((structure.mood, structure.end_beat), (Mood::Mid, 64));

        // A masked file edits the same and comes back unmasked.
        let edited = pssi(true).with_phrase_edit(PhraseEdit::Cut { beat: 25 }).unwrap();
        assert_eq!(beats(&edited), beats(&cut));
        assert_eq!(be16(&[edited.header.clone(), edited.payload.clone()].concat(), 6), 2, "the mood reads plainly");
        assert!(Section::new(b"PQTZ", vec![], vec![]).with_phrase_edit(PhraseEdit::Clear { beat: 1 }).is_none());
    }
}
