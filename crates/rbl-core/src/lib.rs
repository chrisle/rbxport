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
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExportMetadata {
    pub track_number: u32,
    pub disc_number: u16,
    pub bit_depth: u16,
    pub play_count: u32,
    pub analysed: u32,
    pub hot_cue_auto_load: bool,
    pub date_created: String,
    pub isrc: String,
    /// The track's `contentLink` as the stick carries it: `exportLibrary.db`'s
    /// `content.contentLink` and the word at `export.pdb` track-row offset
    /// `0x04`. For a library track this is [`device_content_link`] of its
    /// `djmdContent.ContentLink`; a track read back from a stick keeps the
    /// stick's value. 0 when unknown (sticks written before #640).
    #[serde(default)]
    pub content_link: u32,
}

/// The `contentLink` rekordbox gives a track it exports, from the library
/// row's `djmdContent.ContentLink`: the bits `0x000c_1600` kept and `0x100`
/// set [OBS static: rekordbox 7.2.19 arm64,
/// `db::DatabaseMediator::export_track_data_to_db` at `0x1009b84dc`:
/// `and 0xc1600; orr 0x100` into the device row's `+0x400`, which
/// `db::DevSqliteDBController::insertTrack` writes as `contentLink`].
///
/// The kept bits are what rekordbox reads as "analysed by this version"
/// (`db::RowDataTrack::isAnalyzedOldVer` at `0x10008a5b0` needs `0x200`,
/// `0x400`, `0x40000` and `0x80000`; `clear3BandAnalyzed` at `0x10070b238`
/// clears `0xc0000`, the 3-band waveform). Without them rekordbox 7 reports
/// "Some tracks in the device were analyzed by an older version of
/// rekordbox" (#640). A library value of `0xc060e` or `0x2c060e` gives
/// `0xc0700`, the value a rekordbox 7 stick carries on all but one of its
/// 1563 tracks [OBS docs/reference/usb-export-db.md].
#[must_use]
pub const fn device_content_link(library: u32) -> u32 {
    (library & 0x000c_1600) | 0x100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourcc_roundtrips_as_text() {
        assert_eq!(FourCc::new(b"PQTZ").to_string(), "PQTZ");
    }

    #[test]
    fn device_content_link_matches_rekordbox_export() {
        // This machine's rekordbox 7.2.19 library rows, and the value its
        // own export writes [OBS: 0xc0700 on a rekordbox 7 stick].
        assert_eq!(device_content_link(0x000c_060e), 0x000c_0700);
        assert_eq!(device_content_link(0x002c_060e), 0x000c_0700);
        // What RBX writes on analysis, and a row rekordbox never opened.
        assert_eq!(device_content_link(0x002c_0600), 0x000c_0700);
        assert_eq!(device_content_link(0), 0x100);
        // Cloud sharing (0x400000) and 0x200000 (set on the library rows
        // that have a `.3EX`, which no stick gets) stay in the library;
        // 0x1000 goes along.
        assert_eq!(device_content_link(0x006c_1600), 0x000c_1700);
    }

    #[test]
    fn fourcc_does_not_panic_on_non_utf8() {
        assert_eq!(FourCc([0xff, 0xfe, 0xfd, 0xfc]).as_str(), "????");
    }
}
pub mod paths;
