//! `PVDI` — vocal intensity in the `.2EX` analysis file.
//!
//! The 24-byte section header has four big-endian u16 fields after its
//! framing: version, frame numerator, frame denominator and point size;
//! then a u32 point count. Supported points are one byte per 1024/22050 s.
//! Bit 0x8000 of the version selects the USB XOR mask. Remaining version
//! bits do not change decoding. Unsupported rates and point sizes are
//! retained as raw sections but not decoded.

use std::borrow::Cow;

use crate::Section;
use rbl_core::FourCc;

/// Milliseconds covered by a supported vocal-intensity point.
pub const VOCAL_FRAME_MS: f64 = 1000.0 * 1024.0 / 22_050.0;

/// Full vocal intensity; the low three bits carry the value.
pub const VOCAL_MAX: u8 = 4;

// USB mask in payload order; the transform is symmetric.
const USB_MASK: [u8; 19] = [
    0x1e, 0xdd, 0x1e, 0x19, 0x02, 0x19, 0x1b, 0x11, 0x19, 0x23,
    0x18, 0x19, 0x24, 0x11, 0xfb, 0x11, 0x1e, 0x2a, 0x15,
];

impl Section {
    /// Supported vocal points, borrowing plaintext and owning decoded USB data.
    /// Raw section bytes are unchanged. Invalid or unsupported framing is not
    /// returned as intensity data.
    #[must_use]
    pub fn as_vocals(&self) -> Option<Cow<'_, [u8]>> {
        if self.tag != FourCc::new(b"PVDI") || self.header.len() != 12 {
            return None;
        }
        let field = |at| self.header.get(at..at + 2)
            .and_then(|bytes| <[u8; 2]>::try_from(bytes).ok()).map(u16::from_be_bytes);
        let version = field(0)?;
        if field(2)? != 1024 || field(4)? != 22_050 || field(6)? != 1 {
            return None;
        }
        let declared = u32::from_be_bytes(self.header.get(8..12)?.try_into().ok()?);
        if usize::try_from(declared).ok()? != self.payload.len() {
            return None;
        }
        if version & 0x8000 == 0 {
            Some(Cow::Borrowed(&self.payload))
        } else {
            let mut points = self.payload.clone();
            for (at, point) in points.iter_mut().enumerate() {
                *point ^= USB_MASK[at % USB_MASK.len()];
            }
            Some(Cow::Owned(points))
        }
    }
}

impl crate::Anlz {
    /// Vocal intensity, with a masked USB section decoded when necessary.
    #[must_use]
    pub fn vocals(&self) -> Option<Cow<'_, [u8]>> {
        self.sections.iter().find_map(Section::as_vocals)
    }
}
