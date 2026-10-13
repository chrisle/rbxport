//! Copying a stick's tracks to the computer for Import Playlist, as
//! rekordbox does with the tracks of a device playlist that its collection
//! does not have.
//!
//! What rekordbox does [static, rekordbox 7.2.19.0342 arm64, with symbols]:
//!
//! - The copies go under `PioneerDJ/Imported from Device` in the user's
//!   music folder: `importFromDeviceCopySettingWindow` (@0x1001d6af0) takes
//!   `juce::File::getSpecialLocation(3)`, `userMusicDirectory`, and its
//!   child `"PioneerDJ/Imported from Device"` (@0x1001d6ee4..0x1001d6f04).
//! - Under that folder each file goes where an export puts it on a stick,
//!   `Contents/<Artist>/<Album>/<file>`, with `UnknownArtist` and
//!   `UnknownAlbum` for a blank name: `import_from_d_file_copy`
//!   (@0x1001daa68) asks `DatabaseMediator::calc_device_file_path`
//!   (@0x1009b6e0c), which builds the path with
//!   `get_device_file_path_candidate` (@0x1009ba39c), the function the USB
//!   export uses.
//! - A file already at that path is never replaced: `calc_device_file_path`
//!   tries `<name>-1.<ext>`, `<name>-2.<ext>`, … until one does not exist,
//!   cutting the name so the whole stays within 48 characters
//!   (@0x1009b6eb0..0x1009b70cc). See [`numbered`].
//!
//! rbx copies each file to a temporary name in the destination folder,
//! flushes it, compares it byte for byte with the stick's file, and only
//! then gives it its name, with a rename that refuses to replace anything
//! ([`rbl_core::durable::persist_new`]). [`Copies::roll_back`] removes what
//! a failed import copied, and only that.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The folder under the user's music folder that rekordbox copies a device
/// playlist's missing tracks to.
pub const IMPORTED_FROM_DEVICE: [&str; 2] = ["PioneerDJ", "Imported from Device"];

/// Where rekordbox puts copies, under the user's music folder `music`.
#[must_use]
pub fn destination(music: &Path) -> PathBuf {
    IMPORTED_FROM_DEVICE.iter().fold(music.to_path_buf(), |path, part| path.join(part))
}

/// Where a stick track's copy goes under `root`, before any clash:
/// `Contents/<Artist>/<Album>/<file>`, the names made safe the way an
/// export makes them.
#[must_use]
pub fn candidate(root: &Path, artist: &str, album: &str, file_name: &str) -> PathBuf {
    let artist = crate::dir_name(if artist.trim().is_empty() { "UnknownArtist" } else { artist.trim() });
    let album = crate::dir_name(if album.trim().is_empty() { "UnknownAlbum" } else { album.trim() });
    root.join("Contents").join(artist).join(album).join(crate::fat_safe(file_name))
}

/// The name rekordbox tries for the `n`th clash of `file_name`:
/// `<stem>-<n><.ext>`, the stem cut so the whole is 48 characters when it
/// would be longer [static, `calc_device_file_path` @0x1009b6f5c..0x1009b6fd8:
/// the cut is taken when the name and `-<n>` together reach 49 characters].
#[must_use]
pub fn numbered(file_name: &str, n: u32) -> String {
    const LIMIT: usize = 48;
    let (stem, extension) = match file_name.rfind('.') {
        Some(dot) => (&file_name[..dot], &file_name[dot..]),
        None => (file_name, ""),
    };
    let suffix = format!("-{n}");
    let length = file_name.chars().count() + suffix.chars().count();
    let stem: String = if length > LIMIT {
        let keep = LIMIT.saturating_sub(suffix.chars().count() + extension.chars().count());
        stem.chars().take(keep).collect()
    } else {
        stem.to_owned()
    };
    format!("{stem}{suffix}{extension}")
}

/// Free space where `path` is, or on its nearest existing parent.
pub fn free_space(path: &Path) -> std::io::Result<u64> {
    let mut at = path;
    loop {
        if at.exists() {
            return fs2::available_space(at);
        }
        match at.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => at = parent,
            _ => return fs2::available_space(Path::new(".")),
        }
    }
}

/// The copies one import made, so a failed import can take them back.
#[derive(Debug, Default)]
pub struct Copies {
    files: Vec<PathBuf>,
    /// Folders the copies made, outermost first.
    dirs: Vec<PathBuf>,
}

impl Copies {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The files copied so far.
    #[must_use]
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Copies `source` to `wanted`, or, when something is already there,
    /// to the first free name [`numbered`] gives. Never replaces a file.
    /// Returns where the copy went, once it has been checked against the
    /// source byte for byte.
    pub fn copy(&mut self, source: &Path, wanted: &Path) -> std::io::Result<PathBuf> {
        // Another writer taking the name between the check and the rename
        // costs a fresh copy; this many in a row is not worth chasing.
        for _ in 0..8 {
            match self.copy_once(source, wanted) {
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                other => return other,
            }
        }
        Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("no free name for {}", wanted.display())))
    }

    fn copy_once(&mut self, source: &Path, wanted: &Path) -> std::io::Result<PathBuf> {
        let parent = wanted
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "a copy needs a folder"))?;
        let name = wanted
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "a copy needs a file name"))?
            .to_owned();
        self.make_dirs(parent)?;
        let mut temp = tempfile::Builder::new().prefix(".rbx-import-").suffix(".part").tempfile_in(parent)?;
        let mut from = std::fs::File::open(source)?;
        std::io::copy(&mut from, temp.as_file_mut())?;
        // rekordbox's copy keeps the file's modification time [OBS rekordbox
        // 7.2.14, chris-win11, #198]; so does this one, where it can.
        if let Ok(modified) = from.metadata().and_then(|m| m.modified()) {
            let _ = temp.as_file().set_modified(modified);
        }
        temp.as_file().sync_all()?;
        if !same_bytes(source, temp.path())? {
            return Err(std::io::Error::other(format!("the copy of {} did not match the device's file", source.display())));
        }
        let target = (0..10_000_u32)
            .map(|n| if n == 0 { parent.join(&name) } else { parent.join(numbered(&name, n)) })
            .find(|target| target.symlink_metadata().is_err())
            .ok_or_else(|| std::io::Error::other(format!("no free name for {name}")))?;
        rbl_core::durable::persist_new(temp.into_temp_path(), &target)?;
        rbl_core::durable::sync_dir(parent)?;
        self.files.push(target.clone());
        Ok(target)
    }

    /// An earlier copy of `source` under `wanted`'s name or one of its
    /// numbered names ([`numbered`]), byte for byte the same, if there is
    /// one: what a second import of the same stick track can use instead of
    /// copying it again. Only reads.
    ///
    /// rekordbox avoids that second copy another way: after an import it
    /// writes the new collection track's `masterDbId` and `masterContentId`
    /// into the stick's `exportLibrary.db` row, so the next import matches
    /// it [OBS rekordbox 7.2.14, chris-win11, #198: both rows changed, the
    /// second import asked nothing and made no copy]. rbx leaves the stick
    /// unwritten and finds its own copy instead.
    pub fn existing_copy(source: &Path, wanted: &Path) -> std::io::Result<Option<PathBuf>> {
        let (Some(parent), Some(name)) = (wanted.parent(), wanted.file_name().and_then(|n| n.to_str())) else {
            return Ok(None);
        };
        for n in 0..10_000_u32 {
            let at = if n == 0 { parent.join(name) } else { parent.join(numbered(name, n)) };
            match std::fs::metadata(&at) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
                Ok(meta) if meta.is_file() && same_bytes(source, &at)? => return Ok(Some(at)),
                Ok(_) => {}
            }
        }
        Ok(None)
    }

    /// Removes every file this import copied and the folders it made for
    /// them, when they are empty. Nothing else is touched.
    pub fn roll_back(self) -> std::io::Result<()> {
        let mut first_error = None;
        for file in &self.files {
            if let Err(error) = std::fs::remove_file(file) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    first_error.get_or_insert(error);
                }
            }
        }
        for dir in self.dirs.iter().rev() {
            // Fails, and is kept, when something else is in it.
            let _ = std::fs::remove_dir(dir);
        }
        first_error.map_or(Ok(()), Err)
    }

    fn make_dirs(&mut self, dir: &Path) -> std::io::Result<()> {
        let mut missing = Vec::new();
        let mut at = dir;
        while at.symlink_metadata().is_err() {
            missing.push(at.to_path_buf());
            match at.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => at = parent,
                _ => break,
            }
        }
        rbl_core::durable::create_dir_all(dir)?;
        self.dirs.extend(missing.into_iter().rev());
        Ok(())
    }
}

/// Whether two files hold the same bytes.
fn same_bytes(a: &Path, b: &Path) -> std::io::Result<bool> {
    let (mut a, mut b) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let (mut left, mut right) = (vec![0_u8; 1 << 16], vec![0_u8; 1 << 16]);
    loop {
        let n = read_full(&mut a, &mut left)?;
        let m = read_full(&mut b, &mut right)?;
        if n != m || left[..n] != right[..m] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

fn read_full(file: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}
