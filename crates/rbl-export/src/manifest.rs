//! What a previous export left on the stick.
//!
//! Rekordbox re-copies everything on every export. We keep a small record of
//! what was written so a second export to the same stick copies only what
//! actually changed — on a 200-track playlist that is the difference between
//! minutes and seconds over USB 2.0.
//!
//! It lives outside `PIONEER/rekordbox/`, in a directory of our own, so no
//! player and no version of rekordbox has to know about it. Losing it is
//! harmless: the export falls back to writing everything.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where the record sits, relative to the root of the stick.
pub const MANIFEST_PATH: &str = "PIONEER/rbxport/manifest.json";

/// Bumped when a field changes meaning. An older or newer manifest is ignored
/// rather than guessed at, which costs one full export and never a wrong one.
pub const MANIFEST_VERSION: u32 = 1;

/// The state of one export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub db_id: u64,
    #[serde(default)]
    pub baseline: Option<crate::snapshot::Snapshot>,
    pub version: u32,
    /// When this export ran, in the library's timestamp format.
    pub written: String,
    #[serde(default)]
    pub tracks: Vec<ManifestTrack>,
    /// The playlists the export was asked for, so a sync window can offer
    /// the same selection next time. Absent in records written before it
    /// was kept, which read as "no selection".
    #[serde(default)]
    pub playlists: Vec<ManifestPlaylist>,
    /// `djmdContent.ID` of the tracks on the stick in no playlist: put
    /// there on their own by Export Track, and kept by later syncs unless
    /// the user enables music cleanup. Absent in older records.
    #[serde(default)]
    pub loose: Vec<u64>,
}

/// One playlist as it was asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestPlaylist {
    #[serde(default)]
    pub device_only: bool,
    #[serde(default)]
    pub export_id: u32,
    #[serde(default)]
    pub folder: bool,
    /// `djmdPlaylist.ID`, or 0 when the playlist did not come from the library.
    pub library_id: u64,
    pub name: String,
}

/// One track as it was left on the stick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTrack {
    #[serde(default)]
    pub analysis_hashes: std::collections::BTreeMap<String, u64>,
    /// Companion files required by the completed export.
    #[serde(default)]
    pub analysis_extensions: Vec<String>,
    #[serde(default)]
    pub audio_hash: u64,
    /// The id a player sees. Kept stable across syncs so a deck's own caches,
    /// and any playlist that names it, still point at the same track.
    pub export_id: u32,
    /// `djmdContent.ID`, or 0 when the track did not come from the library.
    pub library_id: u64,
    /// Where the audio was read from.
    pub source: String,
    /// Where it was written, relative to the stick root.
    pub audio: String,
    /// The analysis directory, relative to the stick root; empty when none.
    pub anlz_dir: String,
    /// Size and modification time of the source at copy time — together these
    /// decide whether the audio needs copying again.
    pub size: u64,
    pub modified: i64,
    /// Hash of the analysis bytes, so re-analysis is noticed.
    pub analysis: u64,
    /// The artwork's small file on the stick, relative to the root; empty
    /// when the track has none. Absent from older manifests.
    #[serde(default)]
    pub artwork: String,
    /// Conversion profile; empty for original bytes. Old manifests default to original.
    #[serde(default)]
    pub conversion: String,
    #[serde(default)]
    pub conversion_source_hash: u64,
    /// The audio was already on the stick, where the library keeps it, and
    /// the export only pointed the databases at it. It is the library's file,
    /// not a copy of ours, so a later sync never deletes or replaces it.
    /// Absent in older records, which only ever name copies.
    #[serde(default)]
    pub in_place: bool,
    /// Size and modification time of the copy on the stick, as this export
    /// left it. While the copy still reads the same, a sync trusts
    /// `audio_hash` rather than reading every byte back off the stick.
    /// Anything that rewrites the copy (rekordbox exporting over it, a
    /// file manager) moves its time, and the copy is hashed again. Absent
    /// in older records and for files the export did not write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_stamp: Option<(u64, i64)>,
}

impl ManifestTrack {
    /// How a track is recognised across exports.
    pub fn key(&self) -> String {
        track_key(self.library_id, &self.source)
    }
}

/// Identity of a track across exports: its library id when it has one, and its
/// source path otherwise. A file dragged in from outside the library has no id,
/// but its path is as stable as anything we have.
pub fn track_key(library_id: u64, source: &str) -> String {
    if library_id == 0 {
        source.to_owned()
    } else {
        format!("#{library_id}")
    }
}

impl Manifest {
    pub fn path(destination: &Path) -> PathBuf {
        crate::export_root(destination).join("rbxport/manifest.json")
    }

    /// Reads the record a previous export left, if there is a usable one.
    ///
    /// Every failure — absent, truncated, from another version — reads as "no
    /// record", because the only cost of that is copying more than we had to.
    pub fn load(destination: &Path) -> Option<Self> {
        let bytes = std::fs::read(Self::path(destination)).ok()?;
        let manifest: Self = serde_json::from_slice(&bytes).ok()?;
        (manifest.version == MANIFEST_VERSION).then_some(manifest)
    }

    /// Writes the record, replacing any previous one.
    ///
    /// Written to a temporary name and renamed, so a stick pulled mid-write
    /// leaves either the old record or the new one, never half of either.
    pub fn save(&self, destination: &Path) -> std::io::Result<()> {
        self.save_at(destination, crate::export_root_name(destination).map_err(std::io::Error::other)?)
    }
    pub fn save_at(&self, destination: &Path, root_name: &str) -> std::io::Result<()> {
        let path = destination.join(root_name).join("rbxport/manifest.json");
        if let Some(parent) = path.parent() {
            rbl_core::durable::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        rbl_core::durable::write(&path, &bytes)?;
        Ok(())
    }
}

/// FNV-1a over the analysis bytes.
///
/// Not a checksum anybody else reads — it only has to notice that a track was
/// re-analysed, and it runs over bytes already in memory.
#[must_use]
pub fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_library_track_is_recognised_by_id_not_by_path() {
        // Moving a file in the library must not read as a different track.
        assert_eq!(track_key(42, "/a/one.mp3"), track_key(42, "/b/one.mp3"));
        // A track from outside the library has only its path to go on.
        assert_ne!(track_key(0, "/a/one.mp3"), track_key(0, "/b/one.mp3"));
    }

    #[test]
    fn an_absent_or_damaged_record_reads_as_no_record() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Manifest::load(dir.path()).is_none());

        let path = Manifest::path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(Manifest::load(dir.path()).is_none());

        std::fs::write(&path, br#"{"version":99,"written":"","tracks":[]}"#).unwrap();
        assert!(Manifest::load(dir.path()).is_none(), "a version we do not know is not a record");
    }

    #[test]
    fn a_saved_record_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            db_id: 0, baseline: None,
            version: MANIFEST_VERSION,
            written: "2026-09-08 00:00:00.000 +00:00".to_owned(),
            tracks: vec![ManifestTrack {
                analysis_hashes: std::collections::BTreeMap::new(),
                analysis_extensions: vec!["DAT".into()],
                audio_hash: 0,
                export_id: 7,
                library_id: 42,
                source: "/music/one.mp3".to_owned(),
                audio: "/Contents/A/B/one.mp3".to_owned(),
                anlz_dir: "/PIONEER/USBANLZ/P000/00000007".to_owned(),
                size: 1234,
                modified: 99,
                analysis: 5,
                artwork: String::new(),
                conversion: String::new(),
                conversion_source_hash: 0,
                in_place: false,
                copy_stamp: Some((1234, 1_789_000_000_000_000_000)),
            }],
            playlists: vec![ManifestPlaylist { device_only: false, export_id: 1, folder: false, library_id: 9, name: "Set".to_owned() }],
            loose: vec![42],
        };
        manifest.save(dir.path()).unwrap();
        let read = Manifest::load(dir.path()).expect("saved manifest");
        assert_eq!(read.tracks.len(), 1);
        assert_eq!(read.tracks[0].export_id, 7);
        assert_eq!(read.tracks[0].key(), "#42");
        assert_eq!(read.playlists[0].library_id, 9);
        assert_eq!(read.loose, vec![42]);
        assert_eq!(read.tracks[0].copy_stamp, Some((1234, 1_789_000_000_000_000_000)));
        // The temporary must not survive the rename.
        assert!(!Manifest::path(dir.path()).with_extension("json.part").exists());
    }

    #[test]
    fn the_hash_notices_a_changed_byte() {
        assert_eq!(hash(b"PMAI"), hash(b"PMAI"));
        assert_ne!(hash(b"PMAI"), hash(b"PMAJ"));
        assert_ne!(hash(b""), hash(b"\0"));
    }
}
