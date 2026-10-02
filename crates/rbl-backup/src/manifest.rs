//! `manifest.json`: the library a backup came from and the parts it holds.
use crate::{refused, Result, LIBRARY_FILES};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

pub const NAME: &str = "manifest.json";

/// The version RBXport writes. Version 1 predates artwork and library files.
pub const VERSION: u32 = 2;

/// Larger than any real manifest; a bigger entry is not one.
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// The `master.db` the backup was taken from.
    pub library: PathBuf,
    /// Unix milliseconds.
    pub created_at: u64,
    /// Logical bytes copied into the backup, before compression.
    pub bytes: u64,
    #[serde(default)]
    pub includes_artwork: bool,
    /// Which of [`LIBRARY_FILES`] the backup holds.
    #[serde(default)]
    pub library_files: Vec<String>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let manifest: Self = serde_json::from_slice(bytes)
            .map_err(|_| refused("This file is not an RBXport backup."))?;
        if !matches!(manifest.version, 1 | 2) {
            return Err(refused("This backup was made by a newer version of RBXport. Update RBXport Restore to restore it."));
        }
        if manifest.library_files.iter().any(|name| !LIBRARY_FILES.contains(&name.as_str())) {
            return Err(refused("Unsupported library file in backup."));
        }
        Ok(manifest)
    }

    /// Reads the manifest of a backup ZIP, or of an older unpacked backup folder.
    pub fn read(path: &Path) -> Result<Self> {
        if path.is_dir() {
            return Self::parse(&fs::read(path.join(NAME))?);
        }
        Self::read_from(&mut crate::archive::open(path)?)
    }

    /// As [`Manifest::read`], from an archive already open.
    pub fn read_from(zip: &mut crate::archive::Archive) -> Result<Self> {
        let bytes = crate::archive::read_small(zip, NAME, MAX_BYTES)?
            .ok_or_else(|| refused("This file is not an RBXport backup."))?;
        Self::parse(&bytes)
    }

    /// Whether the backup was taken from the library at `location`.
    pub fn belongs_to(&self, location: &rbl_db::LibraryLocation) -> bool {
        self.library == location.master_db
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn older_manifests_default_the_newer_fields_and_unknown_files_are_refused() {
        let old = Manifest::parse(br#"{"version":1,"library":"/lib/master.db","created_at":5,"bytes":9}"#).unwrap();
        assert!(!old.includes_artwork);
        assert_eq!(old.library_files, [] as [std::path::PathBuf; 0]);
        assert!(Manifest::parse(br#"{"version":3,"library":"/l","created_at":5,"bytes":9}"#).is_err());
        assert!(Manifest::parse(br#"{"version":2,"library":"/l","created_at":5,"bytes":9,"library_files":["../x"]}"#).is_err());
        assert!(Manifest::parse(b"not json").is_err());
    }

    #[test]
    fn reads_from_an_archive_and_rejects_other_zips() {
        let dir = tempfile::tempdir().unwrap();
        let location = crate::testing::library(dir.path());
        let path = dir.path().join("backup.zip");
        crate::testing::archive(&location, &path, None);
        let manifest = Manifest::read(&path).unwrap();
        assert!(manifest.belongs_to(&location));
        assert!(manifest.includes_artwork);
        let other = dir.path().join("other.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&other).unwrap());
        zip.start_file("notes.txt", zip::write::SimpleFileOptions::default()).unwrap();
        zip.finish().unwrap();
        assert!(Manifest::read(&other).is_err());
        fs::write(dir.path().join("broken.zip"), b"PK").unwrap();
        assert!(Manifest::read(&dir.path().join("broken.zip")).is_err());
    }
}
