//! Primitives shared across every rbxport crate.
//!
//! Shared types and durable filesystem publication; no network or Tauri.

pub mod durable;
pub mod ids;
pub mod mpeg;
pub mod musickey;
pub mod time;
pub mod xml;

use serde::{Deserialize, Serialize};

/// A `djmdContent.ID`. Rekordbox stores these as decimal strings; we parse them
/// to `u64` once at load and use a `u32` row index everywhere hot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContentId(pub u64);

/// A `djmdPlaylist.ID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(pub u64);

/// Index into the in-memory columnar library. Stable only within one snapshot.
pub type RowIndex = u32;

/// Rekordbox timestamp format, e.g. `2026-09-05 03:09:51.109 +00:00`.
///
/// Verified against the live library; the trailing offset is part of the string
/// rekordbox writes, not a formatting artifact.
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f +00:00";

/// Four-character ANLZ section tag, e.g. `PQTZ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    #[inline]
    pub const fn new(tag: &[u8; 4]) -> Self {
        Self(*tag)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        // ANLZ tags are ASCII by construction; fall back rather than panic.
        core::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl core::fmt::Display for FourCc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}


/// Library metadata needed by both USB database formats beyond the browse index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportMetadata {
    pub track_number: u32,
    pub disc_number: u16,
    pub bit_depth: u16,
    pub play_count: u32,
    pub analysed: u32,
    pub hot_cue_auto_load: bool,
    pub date_created: String,
    pub isrc: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourcc_roundtrips_as_text() {
        assert_eq!(FourCc::new(b"PQTZ").to_string(), "PQTZ");
    }

    #[test]
    fn fourcc_does_not_panic_on_non_utf8() {
        assert_eq!(FourCc([0xff, 0xfe, 0xfd, 0xfc]).as_str(), "????");
    }
}
pub mod paths;
