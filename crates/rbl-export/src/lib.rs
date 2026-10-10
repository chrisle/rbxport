//! Writes a rekordbox USB export.
//!
//! An export is a directory tree a CDJ can browse:
//!
//! ```text
//!   Contents/<Artist>/<Album>/<file>       the audio
//!   PIONEER/USBANLZ/<P###>/<8-hex>/…       the analysis
//!   PIONEER/rekordbox/export.pdb           the database
//! ```
//!
//! Nothing here touches the user's library: it reads from an already-loaded
//! index and writes only under the destination directory.

pub mod device_library;
pub mod estimate;
pub mod ext_pdb;
pub mod manifest;
pub mod sync_record;
pub mod snapshot;
mod reconcile;
mod verification;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub use manifest::{track_key, Manifest, ManifestPlaylist, ManifestTrack};
pub use sync_record::{SyncNode, SyncRecord, SyncSource};

use rbl_pdb::build::FileBuilder;
use rbl_pdb::rows::{
    artwork_row,
    album_row, artist_row, color_row, key_row, playlist_entry_row, playlist_row,
    simple_named_row, track_row, TrackInput,
};

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("Export stopped.")]
    Cancelled,
    #[error("the destination is not a directory: {0}")]
    NotADirectory(PathBuf),
    #[error("the device went away during the export")]
    DeviceGone,
    #[error("USB sync conflict: {0}")]
    Conflict(String),
    #[error("nothing to export")]
    Empty,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("could not write exportLibrary.db: {0}")]
    OneLibrary(String),
}

pub type Result<T> = std::result::Result<T, ExportError>;

/// Which rekordbox library root a fresh volume should use.
///
/// Existing exports always keep their spelling; this preference only decides
/// the root when neither library exists yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportRoot {
    Standard,
    Hidden,
}

impl ExportRoot {
    const fn name(self) -> &'static str {
        match self {
            Self::Standard => "PIONEER",
            Self::Hidden => ".PIONEER",
        }
    }

    /// Rekordbox uses the hidden root on HFS+ volumes [OBS]. Filesystem names
    /// differ by OS and API, so accept the forms returned by sysinfo, Disk
    /// Utility and common mount tools.
    #[must_use]
    pub fn for_file_system(file_system: &str) -> Option<Self> {
        let normalized: String = file_system
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '+')
            .flat_map(char::to_uppercase)
            .collect();
        matches!(normalized.as_str(), "HFS" | "HFS+" | "HFSPLUS")
            .then_some(Self::Hidden)
            .or_else(|| normalized.starts_with("MACOSEXTENDED").then_some(Self::Hidden))
    }
}

/// `DeviceSQL` page size rekordbox uses.
const PAGE_SIZE: usize = 4096;

/// One track to export.
#[derive(Debug, Clone, Default)]
pub struct SourceTrack {
    pub metadata: rbl_core::ExportMetadata,
    /// None preserves analysis cue lists; Some replaces them with library cues.
    pub cues: Option<Vec<rbl_anlz::cues::ExportCue>>,
    /// Existing device identity, supplied by reconciliation rather than callers.
    pub device: Option<DeviceTrack>,
    /// `djmdContent.ID`, or 0 for a file that is not in the library. This is
    /// how a sync recognises a track it has already written.
    pub id: u64,
    /// Where the audio currently lives.
    pub source_path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub key: String,
    pub comment: String,
    pub date_added: String,
    pub release_date: String,
    pub bpm_x100: u32,
    pub duration_sec: u16,
    pub rating: u8,
    pub color_id: u8,
    pub year: u16,
    pub bitrate: u32,
    pub sample_rate: u32,
    /// `djmdContent.FileSize`, what rekordbox writes into the stick's
    /// database whether or not the file on disk still measures that; 0
    /// means unknown, and the copied file's size is written instead.
    pub file_size: u64,
    /// Analysis to copy alongside, as (extension, bytes).
    pub analysis: Vec<(String, Vec<u8>)>,
    /// The track's artwork, where the library keeps it; `None` for none.
    pub artwork: Option<PathBuf>,
    /// The library's ids of the My Tags on the track.
    pub my_tags: Vec<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceTrack {
    pub id: u32,
    pub master_db_id: u64,
    pub master_content_id: u64,
    pub audio: String,
    pub analysis_dir: String,
    pub preserve: bool,
}

/// One My Tag of the library, to be listed on the stick: a category
/// (`attribute` 1, `parent` 0) or a tag under one (`attribute` 0).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceMyTag {
    pub id: u64,
    pub seq: u32,
    pub name: String,
    pub attribute: u8,
    pub parent: u64,
}

/// A playlist to include.
#[derive(Debug, Clone, Default)]
pub struct SourcePlaylist {
    pub device_id: u32,
    pub device_only: bool,
    /// `djmdPlaylist.ID`, or 0 for a playlist that is not in the library.
    /// Recorded on the stick so the next sync can start from the same
    /// selection.
    pub id: u64,
    pub name: String,
    /// Master-library parent folder id; 0 for the root.
    pub parent_id: u64,
    pub folder: bool,
    /// Indices into the track slice.
    pub track_indices: Vec<usize>,
}

/// Where an export has got to, reported after each track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportProgress {
    /// `checking`, `copying`, `database`, `verifying`, or `publishing`.
    pub stage: &'static str,
    /// Tracks dealt with so far, copied, reused or skipped.
    pub done: usize,
    pub total: usize,
    /// The track just dealt with.
    pub title: String,
}

#[derive(Debug, Clone, Default)]
pub struct ExportReport {
    pub tracks: usize,
    pub playlists: usize,
    pub bytes_copied: u64,
    pub analysis_files: usize,
    /// Artwork files written this run; four per image, as rekordbox writes.
    pub artwork_files: usize,
    pub pdb_bytes: usize,
    /// Tracks skipped because their audio was missing or unreadable.
    pub skipped: Vec<String>,
    /// Tracks left out because their audio could not be read, converted or
    /// copied, as "title: reason". The rest of the export carries on without
    /// them.
    pub failed: Vec<String>,
    /// Tracks whose audio was already on the stick, unchanged, and left alone.
    pub reused: usize,
    /// What not copying them saved.
    pub bytes_reused: u64,
    /// Of `reused`, tracks whose library file already lives on the stick
    /// itself: the databases point at it where it is and nothing is copied,
    /// as rekordbox does.
    pub in_place: usize,
    /// Tracks taken off the stick because the selection no longer holds them.
    pub removed: usize,
    /// Tracks the stick did not hold before this export.
    pub tracks_added: usize,
    /// Tracks the stick already held whose audio was written again because
    /// it changed.
    pub tracks_updated: usize,
    /// Playlists newly present in this generation (folders excluded).
    pub playlists_added: usize,
    /// Playlists removed from this generation (folders excluded).
    pub playlists_removed: usize,
    /// Whether `exportLibrary.db` was written.
    pub one_library: bool,
}

/// The eight colour labels rekordbox writes to every export.
const COLORS: [&str; 8] =
    ["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"];

/// The longest directory name rekordbox writes under `Contents/`: an artist
/// or album longer than this is cut, not the file name [OBS 7.2.11, four
/// albums cut at exactly 48 on the reference export].
const DIR_NAME_MAX: usize = 48;
const FILE_NAME_MAX: usize = 120;

/// A directory component under `Contents/`: FAT-safe and cut to rekordbox's
/// length, trailing spaces and dots dropped after the cut too.
fn dir_name(name: &str) -> String {
    let safe = fat_safe(name);
    if safe.chars().count() <= DIR_NAME_MAX {
        return safe;
    }
    let mut cut: String = safe.chars().take(DIR_NAME_MAX).collect();
    while cut.ends_with('.') || cut.ends_with(' ') {
        cut.pop();
    }
    if cut.is_empty() { "Unknown".to_owned() } else { cut }
}

/// Makes a name safe for FAT32, which is what a DJ stick is formatted as.
fn fat_safe(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let mut out: String = name
        .nfc()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    // Trailing dots and spaces are not addressable on FAT.
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("Unknown");
    }
    out
}

/// A rekordbox-style audio filename: FAT-safe and short enough for the player,
/// while retaining the extension that identifies the audio format [OBS: the
/// RBX-21 reporter's rekordbox export shortened the stem and kept `.aiff`].
/// macOS can expose decomposed Unicode names, so `fat_safe` normalizes before
/// the byte limit is applied and staged and published paths stay identical.
fn fat_file_name(name: &str) -> String {
    let safe = fat_safe(name);
    if safe.len() <= FILE_NAME_MAX {
        return safe;
    }
    let (stem, suffix) = safe
        .rsplit_once('.')
        .filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty() && extension.len() < FILE_NAME_MAX)
        .map_or((safe.as_str(), String::new()), |(stem, extension)| (stem, format!(".{extension}")));
    let mut end = stem.len().min(FILE_NAME_MAX.saturating_sub(suffix.len()));
    while !stem.is_char_boundary(end) { end -= 1; }
    let mut shortened = stem[..end].trim_end_matches(['.', ' ']).to_owned();
    if shortened.is_empty() { shortened.push_str("Unknown"); }
    shortened.push_str(&suffix);
    shortened
}

/// The lookup tables an export builds as it walks the tracks.
struct Interns<'a> {
    artists: &'a mut Intern,
    albums: &'a mut Intern,
    genres: &'a mut Intern,
    labels: &'a mut Intern,
    keys: &'a mut Intern,
}

/// Interns names into ids starting at 1, preserving first-seen order.
#[derive(Default)]
struct Intern {
    ids: BTreeMap<String, u32>,
    order: Vec<String>,
}

impl Intern {
    fn id(&mut self, name: &str) -> u32 {
        if name.is_empty() {
            return 0;
        }
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = u32::try_from(self.order.len()).unwrap_or(0) + 1;
        self.ids.insert(name.to_owned(), id);
        self.order.push(name.to_owned());
        id
    }

    fn entries(&self) -> impl Iterator<Item = (u32, &str)> {
        self.order
            .iter()
            .enumerate()
            .map(|(i, name)| (u32::try_from(i).unwrap_or(0) + 1, name.as_str()))
    }
}

/// Where one track's files land on the stick.
///
/// Derived from the track and its export id alone, so the same track lands in
/// the same place on every sync and a second export can tell "already there"
/// from "moved".
#[derive(Clone)]
struct Layout {
    /// Relative to the stick root, with the leading slash a pdb row carries.
    audio: String,
    /// The analysis directory, relative to the stick root.
    anlz_dir: String,
    file_name: String,
}

/// CDJ-3000 firmware 3.20, `sub_12f3f28`: UTF-16 path hash and shard.
/// Matches all 61 independently exported rekordbox reference tracks.
fn analysis_directory(audio: &str, root: &str) -> String {
    let hash = audio.encode_utf16().take_while(|c| *c != 0)
        .fold(0_u32, |h, c| h.wrapping_mul(0x34f5_501d).wrapping_add(u32::from(c) * 0x93b6)) % 0x0003_0d43;
    let shard = [0, 2, 6, 7, 9, 13, 16].iter().enumerate()
        .fold(0_u32, |shard, (i, bit)| shard | ((hash >> bit) & 1) << i);
    format!("/{root}/USBANLZ/P{shard:03X}/{hash:08X}")
}

fn layout(track: &SourceTrack, export_id: u32) -> Layout {
    let on_disk = track
        .source_path
        .file_name()
        .map_or_else(|| format!("track-{export_id}.mp3"), |n| n.to_string_lossy().into_owned());
    let file_name = fat_file_name(&on_disk);
    let artist_dir = dir_name(if track.artist.is_empty() { "UnknownArtist" } else { &track.artist });
    let album_dir = dir_name(if track.album.is_empty() { "UnknownAlbum" } else { &track.album });
    Layout {
        audio: format!("/Contents/{artist_dir}/{album_dir}/{file_name}"),
        // The two levels are how rekordbox spreads analysis across the tree
        // rather than putting a quarter of a million files in one directory.
        anlz_dir: format!("/PIONEER/USBANLZ/P{:03}/{export_id:08X}", export_id / 1000),
        file_name,
    }
}

/// Resolve both layouts without creating a second, competing library.
pub fn export_root_name(root: &Path) -> Result<&'static str> {
    export_root_name_with(root, None)
}

/// Resolve an existing library first, then use `preferred` for a blank volume.
pub fn export_root_name_with(root: &Path, preferred: Option<ExportRoot>) -> Result<&'static str> {
    let present = |name: &str| {
        let p = root.join(name);
        p.join("rekordbox/export.pdb").exists() || p.join("rekordbox/exportLibrary.db").exists() || p.join("DEVSETTING.DAT").exists()
    };
    match (present("PIONEER"), present(".PIONEER")) {
        (true, true) => Err(ExportError::Conflict("Both PIONEER and .PIONEER contain libraries. Reconcile them before syncing.".into())),
        (false, true) => Ok(".PIONEER"),
        (true, false) => Ok("PIONEER"),
        (false, false) => Ok(preferred.unwrap_or(ExportRoot::Standard).name()),
    }
}
pub fn export_root(root: &Path) -> PathBuf {
    root.join(export_root_name(root).unwrap_or("PIONEER"))
}

pub(crate) fn checked_under(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative.trim_start_matches('/'));
    if relative.as_os_str().is_empty() || relative.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(ExportError::Conflict("Invalid device-relative path".into()));
    }
    let path = root.join(relative);
    if path.exists() && !path.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(ExportError::Conflict("A device file points outside the USB".into()));
    }
    Ok(path)
}

pub(crate) fn path_key(path: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    path.nfc().collect::<String>().to_lowercase()
}

/// Where a track's audio already sits on the stick, when the library keeps the
/// file there: the stick-relative path the databases can name as it is.
///
/// rekordbox does not copy such a file. `DatabaseMediator::
/// get_device_file_path_candidate` (7.2.19 arm64 @0x1009ba39c) keeps the
/// on-device path when the track's path starts with the device root and
/// `hasSpecialCharInFilePath` (@0x1009baeb8) finds no component a stick cannot
/// carry; `export_track_data` (@0x1009b8ca4) then copies the file onto itself,
/// which `juce::File::copyFileTo` treats as done [OBS static]. A path with
/// such a component is copied under `Contents/` like any other, as there.
fn on_stick(destination: &Path, source: &Path) -> Option<String> {
    use unicode_normalization::UnicodeNormalization;
    let root = destination.canonicalize().ok()?;
    let file = source.canonicalize().ok().filter(|f| f.is_file())?;
    let relative = file.strip_prefix(&root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else { return None };
        let name = name.to_str()?;
        // `fat_safe` changes nothing but the characters a stick cannot carry
        // (and the normal form, which a lookup on the stick ignores).
        if fat_safe(name) != name.nfc().collect::<String>() { return None; }
        parts.push(name);
    }
    // Never adopt our own staging area or the rekordbox folders as music.
    let first = parts.first()?;
    if ["PIONEER", ".PIONEER", PUBLICATION].iter().any(|r| first.eq_ignore_ascii_case(r)) { return None; }
    Some(format!("/{}", parts.join("/")))
}

/// Whether a previous export's entry names the library's own file, where the
/// library keeps it on the stick, rather than a copy the export made. Such a
/// file is never the export's to delete, replace, or rename.
///
/// The `in_place` flag says so for entries this version wrote. An entry from
/// an older version has no flag, yet its library file may sit exactly where
/// the export would have copied it (`Contents/<Artist>/<Album>/<file>`), and
/// that export then copied it onto itself: a library track whose source is the
/// very file its audio path names is the library's too.
fn owns_library_file(destination: &Path, entry: &ManifestTrack) -> bool {
    entry.in_place
        || (entry.library_id != 0 && same_file(Path::new(&entry.source), &under(destination, &entry.audio)))
}

/// The audio paths, by [`path_key`], of every library file a previous export
/// left in place (see [`owns_library_file`]).
fn previous_in_place(destination: &Path, previous: Option<&Manifest>) -> BTreeSet<String> {
    previous.into_iter().flat_map(|m| &m.tracks)
        .filter(|t| owns_library_file(destination, t))
        .map(|t| path_key(&t.audio))
        .collect()
}

/// [`on_stick`] for each track the selection names; `None` for a track that
/// is copied. Only one track can own a file: a second library entry for the
/// same file is copied beside it rather than sharing its analysis.
///
/// A device track kept only because the stick still lists it (its history or
/// a playlist made on the player) stays in place too when it is a library
/// file an earlier export left there: it keeps its path, nothing copies over
/// it, and the record keeps saying the file is not the export's.
fn in_place_paths(destination: &Path, tracks: &[SourceTrack], previous: &BTreeSet<String>) -> Vec<Option<String>> {
    let mut claimed = BTreeSet::new();
    tracks.iter().map(|track| {
        if let Some(device) = track.device.as_ref().filter(|d| d.preserve) {
            return (previous.contains(&path_key(&device.audio))
                && same_file(&track.source_path, &under(destination, &device.audio))
                && claimed.insert(path_key(&device.audio)))
                .then(|| device.audio.clone());
        }
        on_stick(destination, &track.source_path).filter(|path| claimed.insert(path_key(path)))
    }).collect()
}

fn layouts(tracks: &[SourceTrack], ids: &[u32], root: &str, previous: Option<&Manifest>, in_place: &[Option<String>], previous_in_place: &BTreeSet<String>) -> Vec<Layout> {
    // Library files on the stick keep their names, and nothing copied may land
    // on one: not this run's, nor one an earlier run pointed at and left.
    let mut used: BTreeSet<String> = in_place.iter().flatten().map(|path| path_key(path))
        .chain(previous_in_place.iter().cloned())
        .collect();
    tracks.iter().zip(ids).zip(in_place).map(|((track, id), in_place)| {
        let mut place = layout(track, *id);
        if let Some(device) = track.device.as_ref().filter(|d| d.preserve) {
            place.audio.clone_from(&device.audio);
            place.file_name = Path::new(&place.audio).file_name().unwrap_or_default().to_string_lossy().into_owned();
            place.anlz_dir.clone_from(&device.analysis_dir);
        } else if let Some(audio) = in_place {
            place.audio.clone_from(audio);
            audio.rsplit('/').next().unwrap_or_default().clone_into(&mut place.file_name);
        }
        place.anlz_dir = place.anlz_dir.replacen("/PIONEER/", &format!("/{root}/"), 1);
        // A file left in place is where the library keeps it: never renamed.
        if in_place.is_some() { return place; }
        // Keep a previous collision suffix when another colliding track goes away.
        if let Some(old) = previous.and_then(|m| m.tracks.iter().find(|t| t.key() == track_key(track.id, &track.source_path.to_string_lossy()))) {
            if old.conversion.is_empty() && Path::new(&old.audio).parent() == Path::new(&place.audio).parent() && old.source == track.source_path.to_string_lossy() {
                place.audio.clone_from(&old.audio);
                place.file_name = Path::new(&old.audio).file_name().map_or_else(|| place.file_name.clone(), |f| f.to_string_lossy().into_owned());
            }
        }
        if !used.insert(path_key(&place.audio)) {
            let original = Path::new(&place.audio);
            let stem = original.file_stem().unwrap_or_default().to_string_lossy();
            let ext = original.extension().map_or(String::new(), |s| format!(".{}", s.to_string_lossy()));
            let parent = original.parent().unwrap_or(Path::new("/Contents"));
            let mut serial = 0_u32;
            loop {
                let suffix = if serial == 0 { format!("-{id}") } else { format!("-{id}-{serial}") };
                let name = format!("{stem}{suffix}{ext}");
                let audio = parent.join(&name).to_string_lossy().into_owned();
                if used.insert(path_key(&audio)) { place.audio = audio; place.file_name = name; break; }
                serial += 1;
            }
        }
        place
    }).collect()
}

/// Compare bytes, not merely size/mtime: removable media may have been edited.
fn files_equal(a: &Path, b: &Path) -> std::io::Result<bool> {
    use std::io::Read;
    let mut a = std::fs::File::open(a)?;
    let mut b = match std::fs::File::open(b) { Ok(f) => f, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false), Err(e) => return Err(e) };
    if a.metadata()?.len() != b.metadata()?.len() { return Ok(false); }
    let mut left = vec![0_u8; 65536]; let mut right = vec![0_u8; 65536];
    loop {
        let n = a.read(&mut left)?;
        if n == 0 { return Ok(true); }
        b.read_exact(&mut right[..n])?;
        if left[..n] != right[..n] { return Ok(false); }
    }
}

fn file_hash(path: &Path) -> std::io::Result<u64> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    let mut buffer = vec![0_u8; 65536];
    loop { let n = f.read(&mut buffer)?; if n == 0 { break; } for b in &buffer[..n] { h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3); } }
    Ok(h)
}

fn playlist_ids(playlists: &[SourcePlaylist], previous: Option<&Manifest>) -> Result<Vec<u32>> {
    let mut used: BTreeSet<u32> = previous.into_iter().flat_map(|m| &m.playlists).map(|p| p.export_id).filter(|id| *id != 0).collect();
    used.extend(playlists.iter().map(|p| p.device_id).filter(|id| *id != 0));
    let mut result = Vec::new();
    let mut keys = BTreeSet::new();
    for (position, p) in playlists.iter().enumerate() {
        if p.id != 0 && !keys.insert(p.id) { return Err(ExportError::Conflict("Duplicate source playlist ID".into())); }
        let known = previous.and_then(|m| m.playlists.iter().enumerate().find(|(_, old)| if p.id != 0 { p.id == old.library_id } else { p.name == old.name && p.folder == old.folder })).map(|(i, old)| if old.export_id == 0 { u32::try_from(i + 1).unwrap_or(0) } else { old.export_id });
        let id = (p.device_id != 0).then_some(p.device_id).or(known).unwrap_or_else(|| {
            let mut id = u32::try_from(position + 1).unwrap_or(1);
            while used.contains(&id) { id = id.saturating_add(1); }
            id
        });
        if result.contains(&id) { return Err(ExportError::Conflict("Ambiguous playlist identity".into())); }
        used.insert(id); result.push(id);
    }
    for p in playlists {
        if p.parent_id != 0 && !playlists.iter().any(|n| n.id == p.parent_id && n.folder) { return Err(ExportError::Conflict(format!("Missing parent folder for '{}'", p.name))); }
    }
    Ok(result)
}

/// Gives every track the id it had on this stick last time, and a fresh one
/// otherwise.
///
/// Ids must not shift between syncs: a deck caches artwork and waveforms
/// against them, and every playlist entry names one. Reassigning by position
/// would silently repoint half the stick after a single track was removed.
fn assign_ids(tracks: &[SourceTrack], previous: Option<&Manifest>) -> Vec<u32> {
    let mut known: BTreeMap<String, u32> = BTreeMap::new();
    let mut used: BTreeSet<u32> = BTreeSet::new();
    if let Some(manifest) = previous {
        for entry in &manifest.tracks {
            known.insert(entry.key(), entry.export_id);
            used.insert(entry.export_id);
        }
    }

    used.extend(tracks.iter().filter_map(|t| t.device.as_ref().map(|d| d.id)));
    let mut ids = Vec::with_capacity(tracks.len());
    let mut next: u32 = 1;
    for track in tracks {
        let key = track_key(track.id, &track.source_path.to_string_lossy());
        let id = track.device.as_ref().map(|d| d.id).or_else(|| known.get(&key).copied()).unwrap_or_else(|| {
            while used.contains(&next) {
                next = next.saturating_add(1);
            }
            next
        });
        used.insert(id);
        // The same track listed twice keeps one id rather than taking two.
        known.insert(key, id);
        ids.push(id);
    }
    ids
}

/// Resolves a stick-relative path against the destination.
fn under(destination: &Path, relative: &str) -> PathBuf {
    destination.join(relative.trim_start_matches('/'))
}

/// The source's size and modification time, which together decide whether a
/// copy can be skipped.
fn source_stamp(meta: &std::fs::Metadata) -> (u64, i64) {
    // Nanoseconds, not seconds: a re-encode that happens to land on the same
    // byte count within the same second would otherwise read as unchanged.
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0);
    (meta.len(), modified)
}

/// The size and modification time of a file on the stick, or `None` when
/// there is no file there.
fn copy_stamp(path: &Path) -> Result<Option<(u64, i64)>> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some(source_stamp(&meta))),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) if is_device_gone(&e) => Err(ExportError::DeviceGone),
        Err(e) => Err(e.into()),
    }
}

/// Whether the copy on the stick is the one the manifest recorded: the same
/// size and the same modification time, to the resolution the file system
/// reports. A copy rewritten in place, by rekordbox exporting over it or by
/// anything else, gets the time of that write. A file system that reports a
/// finer time straight after a write than after a remount, or a stick moved
/// to a computer in another time zone (FAT keeps local time), reads as
/// changed: that costs one hashing pass, after which the new time is
/// recorded, and never a wrong reuse.
fn same_copy(recorded: Option<(u64, i64)>, now: Option<(u64, i64)>) -> bool {
    matches!((recorded, now), (Some(recorded), Some(now)) if recorded == now && recorded.1 != 0)
}

/// Whether two paths name the same file on disk.
///
/// A stick is FAT32 and a Mac's disk is case-insensitive by default, so
/// renaming an artist from TRIODE to Triode changes the path we write without
/// changing the file. Deleting "the old path" afterwards would delete the copy
/// just made, and the stick would name audio that is no longer there.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Removes something the export no longer references, and any directory it
/// leaves empty behind it.
///
/// Failures are ignored on purpose: a file that would not delete leaves the
/// stick untidy, and failing the whole export over it would be worse.
fn remove_under(destination: &Path, relative: &str, directory: bool) {
    if relative.is_empty() {
        return;
    }
    let path = under(destination, relative);
    if directory {
        let _ = std::fs::remove_dir_all(&path);
    } else {
        let _ = std::fs::remove_file(&path);
    }
    let mut current = path.parent().map(Path::to_path_buf);
    while let Some(dir) = current {
        // Stop at the stick root, and stop as soon as a directory still holds
        // something — `remove_dir` refuses a non-empty one, which is the test.
        if dir == destination || !dir.starts_with(destination) || std::fs::remove_dir(&dir).is_err() {
            break;
        }
        current = dir.parent().map(Path::to_path_buf);
    }
}

/// Writes an export into `destination`.
///
/// Exporting to a stick that already holds one of ours is a sync, not a
/// rewrite: the manifest left by the previous run says what is already there,
/// and only what changed is copied. Tracks that have left the selection are
/// removed. Without a manifest — a fresh stick, or one rekordbox wrote —
/// everything is written.
pub fn export(
    destination: &Path,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
) -> Result<ExportReport> {
    export_with(destination, tracks, playlists, None)
}

/// [`export`], with what a stick that holds no `exportLibrary.db` yet
/// starts from in place of the reference rows: the Preferences window's
/// DJ System choices. A stick that already has a library keeps its own
/// settings, and `defaults` is not looked at.
pub fn export_with(
    destination: &Path,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
    defaults: Option<&rbl_onelibrary::settings::StickSettings>,
) -> Result<ExportReport> {
    export_full(destination, tracks, playlists, &[], defaults, None, &mut |_| {})
}

/// [`export_with`], with the library's My Tags listed on the stick as well.
///
/// The categories and tags go to `exportLibrary.db` whole, as rekordbox
/// writes them (every live row of `djmdMyTag`, 99 on the reference library),
/// and each track's memberships with them; `export.pdb` has no My Tag
/// table, so a player filters by them only through the library file.
#[allow(clippy::too_many_lines, reason = "one linear pipeline; splitting it would hide the order writes happen in")]
pub fn export_full(
    destination: &Path,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
    my_tags: &[SourceMyTag],
    defaults: Option<&rbl_onelibrary::settings::StickSettings>,
    sync: Option<&SyncSource>,
    progress: &mut dyn FnMut(&ExportProgress),
) -> Result<ExportReport> {
    export_with_options(destination, tracks, playlists, my_tags, &ExportOptions { defaults, sync, ..Default::default() }, progress)
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompatibilityFormat { Wav, Aiff, Mp3 }
impl CompatibilityFormat {
    fn audio(self) -> rbl_audio::compatibility::Format {
        match self {
            Self::Wav => rbl_audio::compatibility::Format::Wav,
            Self::Aiff => rbl_audio::compatibility::Format::Aiff,
            Self::Mp3 => rbl_audio::compatibility::Format::Mp3,
        }
    }
}

#[derive(Default)]
pub struct ExportOptions<'a> {
    pub defaults: Option<&'a rbl_onelibrary::settings::StickSettings>,
    pub sync: Option<&'a SyncSource>,
    pub compatibility: Option<CompatibilityFormat>,
    pub root: Option<ExportRoot>,
    /// Add the selection to what the stick holds rather than make the stick
    /// hold exactly it: playlists and folders an earlier export put there
    /// that the selection does not name stay as the stick has them, and in
    /// their place, instead of being taken off. Export Playlist and Export
    /// Track add to a device as rekordbox's do; a SYNC leaves this off.
    pub keep_unlisted: bool,
}

/// Export with optional conversion of audio outside the common CDJ formats.
/// Conversion shares the export's staging/publication and never edits sources.
#[allow(clippy::too_many_lines, reason = "ordered export publication pipeline")]
pub fn export_with_options(
    destination: &Path,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
    my_tags: &[SourceMyTag],
    options: &ExportOptions<'_>,
    progress: &mut dyn FnMut(&ExportProgress),
) -> Result<ExportReport> {
    export_cancellable(destination, tracks, playlists, my_tags, options, progress, &|| false)
}

/// Cancellation is checked between tracks and before publishing staged files.
/// Once publication starts it finishes atomically rather than leaving a partial library.
#[allow(clippy::too_many_lines, reason = "ordered export publication pipeline")]
pub fn export_cancellable(
    destination: &Path,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
    my_tags: &[SourceMyTag],
    options: &ExportOptions<'_>,
    progress: &mut dyn FnMut(&ExportProgress),
    cancelled: &dyn Fn() -> bool,
) -> Result<ExportReport> {
    if cancelled() { return Err(ExportError::Cancelled); }
    let &ExportOptions { defaults, sync, compatibility, root, keep_unlisted } = options;
    if destination.exists() && !destination.is_dir() {
        return Err(ExportError::NotADirectory(destination.to_owned()));
    }

    let root_name = export_root_name_with(destination, root)?;
    let contents = destination.join("Contents");
    let anlz_root = destination.join(root_name).join("USBANLZ");
    let db_dir = destination.join(root_name).join("rekordbox");
    rbl_core::durable::create_dir_all(&contents)?;
    rbl_core::durable::create_dir_all(&anlz_root)?;
    rbl_core::durable::create_dir_all(&db_dir)?;

    recover(destination)?;
    let publication = rbl_core::durable::Publication::new(destination, PUBLICATION)?;
    let mut previous = Manifest::load(destination);
    let before = snapshot::Snapshot::read(destination)?;
    let analysis_before = snapshot::analysis_stamp(destination, &before)?;
    let db_id = sync.map_or(0, |s| s.db_id);
    if let Some(old) = previous.as_mut().filter(|m| m.db_id == 0 && db_id != 0) {
        if sync_record::read(destination).is_some_and(|r| r.db_id == db_id) {
            // Old manifests lacked DBID. Upgrade only when rekordbox's own
            // record identifies the same source database.
            old.db_id = db_id;
        }
    }
    before.check_baseline(previous.as_ref(), db_id)?;
    let (tracks, playlists) = reconcile::prepare(destination, &before, previous.as_ref(), tracks, playlists, db_id, keep_unlisted)?;
    let tracks = tracks.as_slice();
    let mut merged_tags = my_tags.to_vec();
    for tag in &before.my_tags {
        if !merged_tags.iter().any(|t|t.id==tag.id) { merged_tags.push(tag.clone()); }
    }
    let my_tags = merged_tags.as_slice();
    let playlists = playlists.as_slice();
    let playlist_ids = playlist_ids(playlists, previous.as_ref())?;
    let ids = assign_ids(tracks, previous.as_ref());
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() || playlists.iter().any(|p| p.track_indices.iter().any(|i| *i >= tracks.len()) || (p.folder && !p.track_indices.is_empty())) {
        return Err(ExportError::Conflict("Duplicate tracks or invalid playlist membership".into()));
    }
    // Entries are taken out as they are matched; whatever is left at the end
    // is what the selection no longer holds.
    let mut stale: BTreeMap<String, &ManifestTrack> = previous
        .as_ref()
        .map(|m| m.tracks.iter().map(|entry| (entry.key(), entry)).collect())
        .unwrap_or_default();

    let mut report = ExportReport::default();
    let mut obsolete = Vec::new();
    let mut artists = Intern::default();
    let mut albums = Intern::default();
    let mut genres = Intern::default();
    let mut labels = Intern::default();
    let mut keys = Intern::default();
    // Artwork by where it comes from: two tracks of one album share one
    // image, and the stick carries it once.
    let mut artwork = Intern::default();

    let mut track_rows: Vec<Vec<u8>> = Vec::with_capacity(tracks.len());
    let mut one_library_tracks: Vec<OneLibraryTrack> = Vec::with_capacity(tracks.len());
    // Indexed by position in `tracks`, so a skipped track does not shift the
    // ones after it out from under the playlists.
    let mut export_ids: Vec<Option<u32>> = vec![None; tracks.len()];
    let mut recorded: Vec<ManifestTrack> = Vec::with_capacity(tracks.len());
    let interns = Interns {
        artists: &mut artists,
        albums: &mut albums,
        genres: &mut genres,
        labels: &mut labels,
        keys: &mut keys,
    };

    let previous_in_place = previous_in_place(destination, previous.as_ref());
    let in_place_paths = in_place_paths(destination, tracks, &previous_in_place);
    let layouts = layouts(tracks, &ids, root_name, previous.as_ref(), &in_place_paths, &previous_in_place);
    // Losing access to a selected source must never delete its good USB copy.
    for track in tracks {
        if !track.source_path.is_file() && previous.as_ref().is_some_and(|m| m.tracks.iter().any(|t| t.key() == track_key(track.id, &track.source_path.to_string_lossy()))) {
            return Err(ExportError::Conflict(format!("Source unavailable for '{}'. Reconnect or relocate it before syncing; the USB has not been changed.", track.title)));
        }
    }
    let mut deletions = Vec::new();
    let mut written_audio_paths = BTreeSet::new();
    let mut written_analysis_paths: BTreeSet<String> = tracks.iter()
        .filter_map(|t| t.device.as_ref().filter(|d| d.preserve).map(|d| path_key(&d.analysis_dir)))
        .collect();
    for (index, track) in tracks.iter().enumerate() {
        // Reported before the track is dealt with, so a skip reports too:
        // whatever happens below, the count moves on by one.
        publication.check_root()?;
        progress(&ExportProgress { stage: "checking", done: index, total: tracks.len(), title: track.title.clone() });
        if cancelled() { return Err(ExportError::Cancelled); }
        let export_id = ids.get(index).copied().unwrap_or(0);
        let mut place = layouts[index].clone();
        let source = track.source_path.to_string_lossy().into_owned();
        let key = track_key(track.id, &source);

        // Read the source before claiming the previous entry: a track whose
        // audio has gone leaves its entry in `stale`, so the copy on the stick
        // is removed rather than orphaned by databases that no longer name it.
        let (size, modified) = match std::fs::metadata(&track.source_path) {
            Ok(meta) => source_stamp(&meta),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                report.skipped.push(track.title.clone());
                continue;
            }
            Err(e) if is_device_gone(&e) => return Err(ExportError::DeviceGone),
            Err(e) => return Err(e.into()),
        };

        // Reconciliation may carry unrelated device tracks alongside this
        // selection. Their files remain untouched by compatibility conversion.
        let conversion = compatibility.filter(|_| !track.device.as_ref().is_some_and(|d| d.preserve))
            .map(CompatibilityFormat::audio);
        let conversion = match conversion {
            Some(target) => match rbl_audio::compatibility::needs_conversion(&track.source_path) {
                Ok(true) => Some(target),
                Ok(false) => None,
                Err(e) => {
                    fail_track(&mut report, track, &format!("Could not read the audio: {e}"));
                    continue;
                }
            },
            None => None,
        };
        if let Some(target) = conversion {
            let stem = Path::new(&place.file_name).file_stem().unwrap_or_default().to_string_lossy();
            let name = fat_file_name(&format!("{stem}-rbx-cdj-{export_id}.{}", target.extension()));
            let parent = place.audio.rsplit_once('/').map_or("/Contents", |(parent, _)| parent);
            place.audio = format!("{parent}/{name}");
            place.file_name = name;
        }
        if !track.device.as_ref().is_some_and(|d| d.preserve) {
            place.anlz_dir = analysis_directory(&place.audio, root_name);
            let original = PathBuf::from(&place.audio);
            let stem = original.file_stem().unwrap_or_default().to_string_lossy();
            let ext = original.extension().map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
            let parent = original.parent().unwrap_or(Path::new("/Contents"));
            let mut suffix = 0_u32;
            while !written_analysis_paths.insert(path_key(&place.anlz_dir)) {
                suffix += 1;
                if suffix > 200_003 { return Err(ExportError::Conflict("Analysis directory space exhausted".into())); }
                place.file_name = format!("{stem}-{export_id}-{suffix}{ext}");
                place.audio = format!("{}/{}", parent.display(), place.file_name);
                place.anlz_dir = analysis_directory(&place.audio, root_name);
            }
        }
        if !written_audio_paths.insert(path_key(&place.audio)) {
            return Err(ExportError::Conflict(format!("Conversion would create duplicate audio path: {}", place.audio)));
        }
        // Still the library's own file on the stick, not renamed for a
        // conversion or an analysis collision: point at it, copy nothing.
        let in_place = in_place_paths.get(index).and_then(Option::as_deref) == Some(place.audio.as_str());
        let profile = conversion.map_or("", rbl_audio::compatibility::Format::profile);
        let audio_dest = under(destination, &place.audio);
        // Looked at, not yet claimed: a track that fails below must leave
        // its entry in `stale`.
        let previous_entry = stale.get(&key).copied();
        // Unchanged means: same source bytes by size and time, same place on
        // the stick, and still actually there.
        let unchanged_metadata = !in_place && previous_entry.is_some_and(|c| {
            c.audio == place.audio && c.size == size && c.modified == modified && c.conversion == profile
        });
        // The stick's copy as it is now, by size and time only.
        let copy_now = if unchanged_metadata { copy_stamp(&audio_dest)? } else { None };
        // The copy is still the one this export recorded: nothing has
        // written to it since, so the bytes `audio_hash` names are the bytes
        // there. Trusting that, as rekordbox trusts its device database for
        // a track already exported, saves reading the whole stick back.
        let copy_unchanged = previous_entry.is_some_and(|c| c.audio_hash != 0 && same_copy(c.copy_stamp, copy_now));
        let source_hash = if let Some(c) = previous_entry.filter(|_| conversion.is_some() && copy_unchanged) {
            c.conversion_source_hash
        } else if conversion.is_some() {
            match file_hash(&track.source_path) {
                Ok(hash) => hash,
                Err(e) if is_device_gone(&e) => return Err(ExportError::DeviceGone),
                Err(e) => {
                    fail_track(&mut report, track, &format!("Could not read the audio: {e}"));
                    continue;
                }
            }
        } else { 0 };
        let carried = stale.remove(&key);
        if carried.is_none() { report.tracks_added += 1; }
        // Otherwise the copy was written since this export recorded it, or
        // the record predates `copy_stamp`. Hash the USB copy once against
        // the recorded hash and the source: a same-size rewrite on the stick
        // is caught here, and the copy's new stamp is recorded so the next
        // sync can trust it again.
        let existing_audio_hash = if copy_unchanged {
            carried.map(|c| c.audio_hash)
        } else if unchanged_metadata {
            match carried.filter(|c| c.audio_hash != 0) {
                Some(_) => match file_hash(&audio_dest) {
                    Ok(hash) => Some(hash),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e.into()),
                },
                None => None,
            }
        } else { None };
        let current_source_hash = if unchanged_metadata && !copy_unchanged && conversion.is_none() {
            Some(file_hash(&track.source_path)?)
        } else { None };
        let unchanged = unchanged_metadata && if let Some(c) = carried {
            if copy_unchanged { true }
            else if conversion.is_some() && c.conversion_source_hash != source_hash { false }
            else if c.audio_hash != 0 {
                existing_audio_hash == Some(c.audio_hash)
                    && (conversion.is_some() || current_source_hash == existing_audio_hash)
            }
            else { files_equal(&track.source_path, &audio_dest)? }
        } else { false };

        let output_size;
        if in_place {
            output_size = size;
            report.reused += 1;
            report.in_place += 1;
            report.bytes_reused += size;
        } else if unchanged {
            output_size = std::fs::metadata(&audio_dest)?.len();
            report.reused += 1;
            report.bytes_reused += output_size;
        } else {
            if carried.is_some() { report.tracks_updated += 1; }
            progress(&ExportProgress { stage: "copying", done: index, total: tracks.len(), title: track.title.clone() });
            let audio_dest = under(publication.stage(), &place.audio);
            if let Some(parent) = audio_dest.parent() {
                std::fs::create_dir_all(parent).map_err(context(format!("Could not create the folder for '{}'", track.title)))?;
            }
            let written = match conversion {
                Some(target) => rbl_audio::compatibility::convert(&track.source_path, &audio_dest, target)
                    .map_err(|e| std::io::Error::other(format!("{}: {e}", track.title))),
                None => copy_staged(&track.source_path, &audio_dest),
            };
            match written {
                Ok(bytes) => { output_size = bytes; report.bytes_copied += bytes; },
                Err(e) if is_device_gone(&e) => return Err(ExportError::DeviceGone),
                Err(e) => {
                    // One bad file must not stop the sync. Take back what this
                    // track had counted, leave any earlier copy to be handled
                    // as for a track that is no longer selected, and go on.
                    let _ = std::fs::remove_file(&audio_dest);
                    if carried.is_some() { report.tracks_updated -= 1; } else { report.tracks_added -= 1; }
                    if let Some(c) = carried { stale.insert(key, c); }
                    fail_track(&mut report, track, &format!("Could not copy it to the USB: {e}"));
                    continue;
                }
            }
        }

        // Renaming an artist moves the file; the copy under the old name would
        // otherwise sit on the stick forever, unreferenced.
        if let Some(c) = carried {
            // A file that was the library's own is never ours to delete.
            if c.audio != place.audio && !owns_library_file(destination, c) && !same_file(&under(destination, &c.audio), &audio_dest) {
                obsolete.push((c.audio.clone(), false));
            }
            if c.anlz_dir != place.anlz_dir {
                obsolete.push((c.anlz_dir.clone(), true));
            }
        }

        let exported_analysis = if track.device.as_ref().is_some_and(|d| d.preserve) { None } else {
            Some(track.analysis.iter().map(|(extension, bytes)| {
                let mut parsed = rbl_anlz::parse(bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
                let path_bytes = rbl_anlz::AnlzBuilder::new().path(&place.audio).finish();
                let path = rbl_anlz::parse(&path_bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
                // A DAT/EXT/2EX has one PPTH. Drop it by tag rather than by
                // decoded value, so a malformed legacy path cannot survive
                // alongside the replacement path.
                parsed.sections.retain(|s| s.tag != rbl_core::FourCc::new(b"PPTH"));
                parsed.sections.splice(0..0, path.sections);
                // Older RBXport analysis files predate the PVBR section. A
                // freshly initialized Pioneer DAT always writes it between
                // PPTH and PQTZ, even when there are no VBR frames. Repair
                // those stale files while exporting rather than asking the
                // user to re-analyse every affected track.
                if extension.eq_ignore_ascii_case("DAT") && parsed.section(b"PVBR").is_none() {
                    parsed.sections.insert(1, rbl_anlz::write::vbr_table_zero_section());
                }
                parsed.sections = parsed.sections.iter().map(rbl_anlz::Section::with_export_phrase_mask).collect();
                // Analysis written before issue #278 put whiteness bits in
                // `PWV2` and zero-height preview columns, neither of which
                // rekordbox writes; a Nexus player (XDJ-1000MK2) then rejects
                // the track's whole preview, and with it the detail waveform
                // and hot-cue markers on load. rekordbox-written `PWAV` and
                // `PWV2` pass through unchanged.
                parsed.sections = parsed.sections.iter().map(rbl_anlz::encode::with_device_preview).collect();
                if let Some(cues) = &track.cues {
                    if extension == "DAT" || extension == "EXT" {
                        parsed.sections.retain(|s| !s.is_cue_list());
                        parsed.sections.extend(rbl_anlz::cues::sections(cues, extension == "EXT"));
                    }
                }
                Ok::<_, std::io::Error>((extension.clone(), parsed.to_bytes()))
            }).collect::<std::io::Result<Vec<_>>>()?)
        };
        let analysis = exported_analysis.as_deref().unwrap_or(&track.analysis);
        if let Some(c) = carried {
            snapshot::check_analysis(destination, c, analysis)?;
        }
        let mut analysis_hash: u64 = 0;
        for (extension, bytes) in analysis {
            analysis_hash = analysis_hash.rotate_left(7)
                ^ manifest::hash(extension.as_bytes())
                ^ manifest::hash(bytes);
        }
        let anlz_dir = under(destination, &place.anlz_dir);
        let analysis_current = carried
            .is_some_and(|c| c.anlz_dir == place.anlz_dir && c.analysis == analysis_hash)
            && analysis
                .iter()
                .all(|(extension, bytes)| std::fs::read(anlz_dir.join(format!("ANLZ0000.{extension}"))).is_ok_and(|b| b == *bytes));
        for extension in ["DAT", "EXT", "2EX"] {
            if !track.analysis.iter().any(|(e, _)| e == extension) && anlz_dir.join(format!("ANLZ0000.{extension}")).exists() {
                deletions.push(PathBuf::from(format!("{}/ANLZ0000.{extension}", place.anlz_dir.trim_start_matches('/'))));
            }
        }
        if !track.analysis.is_empty() && !analysis_current {
            let anlz_dir = under(publication.stage(), &place.anlz_dir);
            let failed = context(format!("Could not write the analysis of '{}'", track.title));
            std::fs::create_dir_all(&anlz_dir).map_err(&failed)?;
            for (extension, bytes) in analysis {
                std::fs::write(anlz_dir.join(format!("ANLZ0000.{extension}")), bytes).map_err(&failed)?;
                report.analysis_files += 1;
            }
        }
        // The path the databases carry, whether or not the file was written
        // this run.
        let analyze_path = if track.analysis.iter().any(|(e, _)| e.eq_ignore_ascii_case("DAT")) {
            format!("{}/ANLZ0000.DAT", place.anlz_dir)
        } else {
            String::new()
        };

        // The artwork, written once per image: a second track of the same
        // album finds its id already taken and its files already there.
        let artwork_id = match track.artwork.as_deref().filter(|p| p.is_file()) {
            Some(image) => {
                let id = artwork.id(&image.to_string_lossy());
                report.artwork_files += write_artwork(publication.stage(), destination, root_name, id, image)?;
                id
            }
            None => 0,
        };

        // An adopted file is not read: on a stick of music that would be
        // every byte of it, every sync, to protect nothing we wrote.
        let audio_hash = if in_place {
            0
        } else if unchanged {
            existing_audio_hash.map_or_else(|| file_hash(&audio_dest), Ok)?
        } else {
            file_hash(&under(publication.stage(), &place.audio))?
        };
        // Publication renames the staged copy into place, which keeps its
        // size and time, so the staged file's stamp is the published one's.
        let recorded_stamp = if in_place {
            None
        } else if unchanged {
            copy_now
        } else {
            copy_stamp(&under(publication.stage(), &place.audio))?
        };
        recorded.push(ManifestTrack {
            analysis_hashes: analysis.iter().map(|(e, b)| (e.clone(), manifest::hash(b))).collect(),
            analysis_extensions: analysis.iter().map(|(e, _)| e.clone()).collect(),
            export_id,
            library_id: track.id,
            source,
            audio: place.audio.clone(),
            anlz_dir: if track.analysis.is_empty() { String::new() } else { place.anlz_dir.clone() },
            size,
            modified,
            analysis: analysis_hash,
            artwork: if artwork_id == 0 {
                String::new()
            } else {
                artwork_path(artwork_id, "a", false).replacen("/PIONEER/", &format!("/{root_name}/"), 1)
            },
            conversion: profile.to_owned(),
            conversion_source_hash: source_hash,
            audio_hash,
            in_place,
            copy_stamp: recorded_stamp,
        });

        // The same facts the pdb row carries, kept for exportLibrary.db.
        // Gathered here rather than re-derived later, so the two databases
        // cannot disagree about a path or a size.
        let mut metadata = track.metadata.clone();
        if conversion.is_some() { metadata.bit_depth = 16; }
        one_library_tracks.push(OneLibraryTrack {
            metadata,
            cues: track.cues.clone(),
            year: track.year, release_date: track.release_date.clone(),
            bitrate: if conversion.is_some() { 0 } else { track.bitrate },
            sample_rate: if conversion.is_some() { 44_100 } else { track.sample_rate },
            export_id,
            library_id: track.device.as_ref().map_or(track.id, |d| d.master_content_id),
            master_db_id: track.device.as_ref().map_or(db_id, |d| d.master_db_id),
            file_size: output_size,
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            genre: track.genre.clone(),
            label: track.label.clone(),
            key: track.key.clone(),
            color_id: track.color_id,
            bpm_x100: track.bpm_x100,
            duration_sec: track.duration_sec,
            rating: track.rating,
            comment: track.comment.clone(),
            date_added: track.date_added.clone(),
            audio_path: place.audio.clone(),
            file_name: place.file_name.clone(),
            analysis_path: analyze_path.clone(),
            image_id: artwork_id,
            my_tags: track.my_tags.clone(),
        });

        track_rows.push(track_row(&TrackInput {
            hot_cue_auto_load: track.metadata.hot_cue_auto_load,
            id: export_id,
            artwork_id,
            artist_id: interns.artists.id(&track.artist),
            album_id: interns.albums.id(&track.album),
            genre_id: interns.genres.id(&track.genre),
            label_id: interns.labels.id(&track.label),
            key_id: interns.keys.id(&track.key),
            color_id: track.color_id,
            rating: track.rating,
            tempo_x100: track.bpm_x100,
            duration_sec: track.duration_sec,
            year: track.year,
            bitrate: conversion.map_or(track.bitrate, rbl_audio::compatibility::Format::bitrate),
            sample_rate: if conversion.is_some() { rbl_audio::compatibility::RATE } else { track.sample_rate },
            // The library's figure when it has one, as rekordbox writes it,
            // even where the file has since changed by a few bytes of tags.
            file_size: u32::try_from(
                (if conversion.is_some() { output_size } else if track.file_size > 0 { track.file_size } else { size }).min(u64::from(u32::MAX)),
            )
            .unwrap_or(0),
            track_number: track.metadata.track_number,
            disc_number: track.metadata.disc_number,
            sample_depth: conversion.map_or(track.metadata.bit_depth, |_| 16),
            play_count: u16::try_from(track.metadata.play_count).unwrap_or(u16::MAX),
            isrc: track.metadata.isrc.clone(),
            title: track.title.clone(),
            filename: place.file_name,
            file_path: place.audio,
            analyze_path,
            comment: track.comment.clone(),
            date_added: track.date_added.clone(),
            release_date: track.release_date.clone(),
            ..TrackInput::default()
        }));
        if let Some(slot) = export_ids.get_mut(index) {
            *slot = Some(export_id);
        }
        report.tracks += 1;
    }

    // Whatever the previous export left that this one does not name.
    for entry in stale.values() {
        // A file that was the library's own is never ours to delete.
        if !owns_library_file(destination, entry) { obsolete.push((entry.audio.clone(), false)); }
        obsolete.push((entry.anlz_dir.clone(), true));
        report.removed += 1;
    }

    // Playlists reference export ids, so they are built after the tracks.
    let mut playlist_rows = Vec::with_capacity(playlists.len());
    let mut entry_rows = Vec::new();
    for (i, playlist) in playlists.iter().enumerate() {
        let playlist_id = playlist_ids[i];
        playlist_rows.push(playlist_row(
            playlist_id,
            playlists.iter().position(|p| p.id != 0 && p.id == playlist.parent_id).map_or(0, |p| playlist_ids[p]),
            u32::try_from(i).unwrap_or(0) + 1,
            playlist.folder,
            &playlist.name,
        ));
        let mut position: u32 = 0;
        for &track_index in &playlist.track_indices {
            let Some(Some(export_id)) = export_ids.get(track_index).copied() else { continue };
            position += 1;
            entry_rows.push(playlist_entry_row(position, export_id, playlist_id));
        }
        report.playlists += 1;
    }
    let previous_playlists: BTreeSet<u32> = before.legacy.as_ref().into_iter()
        .flat_map(|library| &library.playlists).filter(|playlist| !playlist.folder).map(|playlist| playlist.id).collect();
    let current_playlists: BTreeSet<u32> = playlists.iter().zip(&playlist_ids)
        .filter(|(playlist, _)| !playlist.folder).map(|(_, id)| *id).collect();
    report.playlists_added = current_playlists.difference(&previous_playlists).count();
    report.playlists_removed = previous_playlists.difference(&current_playlists).count();

    // Carry device colour labels into both database formats.
    let existing_database = db_dir.join("exportLibrary.db");
    let settings = if existing_database.exists() {
        Some(rbl_onelibrary::settings::StickSettings::read(&existing_database).map_err(|e| one_library_error(&e))?)
    } else { None };
    // The artwork table names the small image of each; a player derives the
    // others from the same name [ASSUME: what the `artwork` row of a
    // rekordbox export names, of the four files it writes per image].
    let artwork_paths: Vec<(u32, String)> =
        artwork.entries().map(|(id, _)| (id, artwork_path(id, "a", false).replacen("/PIONEER/", &format!("/{root_name}/"), 1))).collect();
    let artwork_rows: Vec<Vec<u8>> = artwork_paths.iter().map(|(id, path)| artwork_row(*id, path)).collect();

    // A DeviceSQL row has to fit on one page. Without this check the page
    // builder cuts the row off and the stick's export.pdb is corrupt, which
    // verification reports only as the two databases disagreeing.
    let limit = rbl_pdb::build::max_row_len(PAGE_SIZE);
    if let Some((row, track)) = track_rows.iter().zip(&one_library_tracks).find(|(row, _)| row.len() > limit) {
        return Err(ExportError::Conflict(format!(
            "'{}' has more text in its title, comment and other tags than export.pdb can hold for one track ({} bytes; at most {limit}). Shorten them and sync again; the USB was left as it was.",
            track.title,
            row.len()
        )));
    }
    let genre_rows: Vec<_> = genres.entries().map(|(id, n)| simple_named_row(id, n)).collect();
    let artist_rows: Vec<_> = artists.entries().map(|(id, n)| artist_row(id, n)).collect();
    let album_rows: Vec<_> = albums.entries().map(|(id, n)| album_row(id, 0, n)).collect();
    let label_rows: Vec<_> = labels.entries().map(|(id, n)| simple_named_row(id, n)).collect();
    let key_rows: Vec<_> = keys.entries().map(|(id, n)| key_row(id, n)).collect();
    for (kind, names, rows) in [
        ("genre", &genres, &genre_rows), ("artist", &artists, &artist_rows), ("album", &albums, &album_rows),
        ("label", &labels, &label_rows), ("key", &keys, &key_rows),
    ] {
        if let Some(((_, name), row)) = names.entries().zip(rows).find(|(_, row)| row.len() > limit) {
            let start: String = name.chars().take(40).collect();
            return Err(ExportError::Conflict(format!(
                "The {kind} '{start}…' is longer than export.pdb can hold ({} bytes; at most {limit}). Shorten it and sync again; the USB was left as it was.",
                row.len()
            )));
        }
    }
    if let Some((row, playlist)) = playlist_rows.iter().zip(playlists).find(|(row, _)| row.len() > limit) {
        let start: String = playlist.name.chars().take(40).collect();
        return Err(ExportError::Conflict(format!(
            "The playlist name '{start}…' is longer than export.pdb can hold ({} bytes; at most {limit}). Shorten it and sync again; the USB was left as it was.",
            row.len()
        )));
    }

    progress(&ExportProgress { stage: "database", done: tracks.len(), total: tracks.len(), title: String::new() });
    let pdb = build_pdb(&PdbTables {
        tracks: &track_rows,
        genres: &genre_rows,
        artists: &artist_rows,
        albums: &album_rows,
        labels: &label_rows,
        keys: &key_rows,
        playlists: &playlist_rows,
        entries: &entry_rows,
        artwork: &artwork_rows,
        history: &before.history,
        colors: settings.as_ref().or(defaults).map_or(&[], |s| &s.colors),
        device_name: settings.as_ref().or(defaults).map_or("", |s| s.device_name.as_str()),
        background: before.legacy_background,
    });
    report.pdb_bytes = pdb.len();
    let staged_db = publication.stage().join(root_name).join("rekordbox");
    std::fs::create_dir_all(&staged_db).map_err(context("Could not write export.pdb".to_owned()))?;
    std::fs::write(staged_db.join("export.pdb"), &pdb).map_err(context("Could not write export.pdb".to_owned()))?;
    // The tags and the tracks carrying them, for the player's Track Filter.
    let master_db_id = my_tag_master_db_id(sync);
    let tagged: Vec<(u32, &[u64])> = one_library_tracks.iter().map(|t| (t.export_id, t.my_tags.as_slice())).collect();
    std::fs::write(staged_db.join("exportExt.pdb"), ext_pdb::build(my_tags, &tagged, master_db_id)).map_err(context("Could not write exportExt.pdb".to_owned()))?;

    write_one_library(&staged_db, &one_library_tracks, playlists, &playlist_ids, &export_ids, &artwork_paths, my_tags, settings.as_ref().or(defaults), master_db_id, &before, Some(&existing_database))?;
    report.one_library = true;

    // The DJ's My Settings, as rekordbox puts them on every stick it writes.
    if let Some(source) = rbl_core::paths::rekordbox_settings_dir() {
        for name in MY_SETTINGS_FILES {
            let from = source.join(name);
            if from.is_file() && !destination.join(root_name).join(name).exists() {
                copy_staged(&from, &publication.stage().join(root_name).join(name))?;
            }
        }
    }

    // The sync record, so rekordbox's Sync Manager opens on this selection
    // too. A playlist that is not the library's has no id to record.
    if let Some(sync) = sync {
        let ticked: Vec<u64> = playlists.iter().filter(|p| !p.folder).map(|p| p.id).filter(|&id| id != 0 && id < reconcile::DEVICE_PLAYLIST_ID_BASE).collect();
        let kept = sync_record::read(destination).map(|r| r.timestamps).unwrap_or_default();
        let device_ids = playlists.iter().zip(&playlist_ids).map(|(p, id)| (p.id, *id)).collect();
        let bytes = sync_record::render_with_ids(sync, &ticked, rbl_core::time::unix_millis(), &kept, &device_ids);
        for file in sync_record::FILES { std::fs::write(publication.stage().join(file.replacen("PIONEER/", &format!("{root_name}/"), 1)), &bytes).map_err(context(format!("Could not write {file}")))?; }
    }

    // Last, so a run that fails part way leaves the older record standing and
    // the next attempt re-copies rather than trusting a half-written stick.
    // A track in no playlist was put on the stick on its own; the record
    // says so, so the next sync keeps it.
    let in_a_playlist: BTreeSet<usize> = playlists.iter().flat_map(|p| p.track_indices.iter().copied()).collect();
    let loose: Vec<u64> = tracks
        .iter()
        .enumerate()
        .filter(|(index, track)| !in_a_playlist.contains(index) && track.id != 0 && !track.device.as_ref().is_some_and(|d| d.preserve) && export_ids[*index].is_some())
        .map(|(_, track)| track.id)
        .collect();
    let retained: BTreeSet<String> = recorded.iter().flat_map(|track| [track.audio.to_lowercase(), track.anlz_dir.to_lowercase()]).collect();
    // The audio paths as both databases name them, which is what the staged
    // copies are published under.
    let named: Vec<String> = recorded.iter().map(|track| track.audio.clone()).collect();
    let after = snapshot::Snapshot::read_at(publication.stage(), root_name)?;
    before.check_retained_history(&after)?;
    before.check_changes(previous.as_ref(), &after)?;
    progress(&ExportProgress { stage: "verifying", done: tracks.len(), total: tracks.len(), title: String::new() });
    let verified = verification::verify_staged(publication.stage(), destination, &after)?;
    if !verified.is_ok() {
        return Err(ExportError::Conflict(format!("The export did not verify, so the USB was left as it was: {}", verification_failure(&verified))));
    }
    Manifest {
        db_id,
        baseline: Some(after),
        version: manifest::MANIFEST_VERSION,
        written: rbl_core::time::now(),
        tracks: recorded,
        playlists: playlists
            .iter()
            .enumerate()
            .map(|(i, p)| manifest::ManifestPlaylist { library_id: p.id, name: p.name.clone(), export_id: playlist_ids[i], folder: p.folder, device_only: p.device_only })
            .collect(),
        loose,
    }
    .save_at(publication.stage(), root_name)
    .map_err(context("Could not write the rbxport manifest".to_owned()))?;
    let named: Vec<&str> = named.iter().map(String::as_str).collect();
    let mut files = staged_files(publication.stage(), &named)?;
    // A rebuilt database never inherits WAL pages from its previous image.
    files.splice(0..0, [format!("{root_name}/rekordbox/exportLibrary.db-wal").into(), format!("{root_name}/rekordbox/exportLibrary.db-shm").into()]);
    files.extend(deletions);
    // Catch another writer changing a database while this export was staging.
    if snapshot::Snapshot::read(destination)? != before || snapshot::analysis_stamp(destination, &before)? != analysis_before {
        return Err(ExportError::Conflict("The device changed during sync. Close other writers and retry.".into()));
    }
    if cancelled() { return Err(ExportError::Cancelled); }
    progress(&ExportProgress { stage: "publishing", done: tracks.len(), total: tracks.len(), title: String::new() });
    publication.commit(&files)?;
    for (path, directory) in obsolete {
        if !retained.contains(&path.to_lowercase()) { remove_under(destination, &path, directory); }
    }

    Ok(report)
}

/// The rows of the tables an export fills from the library; every other
/// table rekordbox writes is the same on every stick.
struct PdbTables<'a> {
    tracks: &'a [Vec<u8>],
    genres: &'a [Vec<u8>],
    artists: &'a [Vec<u8>],
    albums: &'a [Vec<u8>],
    labels: &'a [Vec<u8>],
    keys: &'a [Vec<u8>],
    playlists: &'a [Vec<u8>],
    entries: &'a [Vec<u8>],
    /// Empty on a stick with no artwork, and on a blank one.
    artwork: &'a [Vec<u8>],
    history: &'a [snapshot::History],
    colors: &'a [rbl_onelibrary::settings::ColorName],
    /// `property.deviceName` in `exportLibrary.db`; the PDB carries a copy.
    device_name: &'a str,
    /// "Background Color : Device Library", carried from the stick.
    background: u8,
}

/// Builds `export.pdb` with the twenty tables rekordbox writes, in its
/// order: the eight the library fills, the eight colours, the artwork
/// (type 13) among six that are always empty (types 9 to 15), the browse column
/// names and the History menu's two tables (`rbl_pdb::reference`), and the
/// one `property` row: device name, track count, the export's date and the
/// Device Library background colour [OBS 7.2.14]. A player looks the table
/// list up by type, so the empty ones have to be there.
fn build_pdb(tables: &PdbTables<'_>) -> Vec<u8> {
    use rbl_pdb::reference;
    let mut file = FileBuilder::new(PAGE_SIZE);
    file.add_table(0, tables.tracks);
    file.add_table(1, tables.genres);
    file.add_table(2, tables.artists);
    file.add_table(3, tables.albums);
    file.add_table(4, tables.labels);
    file.add_table(5, tables.keys);
    file.add_table(
        6,
        &COLORS
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let id=u16::try_from(i).unwrap_or(0)+1;
                let name=tables.colors.iter().find(|c|c.id==i64::from(id)).map_or(*name,|c|c.name.as_str());
                color_row(id,name)
            })
            .collect::<Vec<_>>(),
    );
    file.add_table(7, tables.playlists);
    file.add_table(8, tables.entries);
    for page_type in 9..=15 {
        // The artwork table names the small image of each; a player derives
        // the others from the same name [ASSUME: what the `artwork` row of a
        // rekordbox export names, of the four files it writes per image].
        let rows;
        let table = match page_type {
            11 => { rows = tables.history.iter().filter(|h| !h.folder).map(|h| simple_named_row(u32::try_from(h.id).unwrap_or(0), &h.name)).collect::<Vec<_>>(); &rows },
            12 => { rows = tables.history.iter().filter(|h| !h.folder).flat_map(|h| h.tracks.iter().enumerate().map(move |(i,id)| {
                let mut row = Vec::with_capacity(12);
                row.extend_from_slice(&id.to_le_bytes());
                row.extend_from_slice(&u32::try_from(h.id).unwrap_or(0).to_le_bytes());
                row.extend_from_slice(&u32::try_from(i+1).unwrap_or(0).to_le_bytes()); row
            })).collect::<Vec<_>>(); &rows },
            13 => tables.artwork,
            _ => &[],
        };
        file.add_table(page_type, table);
    }
    let constant = |rows: &[&[u8]]| rows.iter().map(|row| row.to_vec()).collect::<Vec<_>>();
    file.add_table(16, &constant(reference::COLUMNS));
    file.add_table(17, &constant(reference::HISTORY_PLAYLISTS));
    file.add_table(18, &constant(reference::HISTORY_ENTRIES));
    // The local day, as rekordbox dates the export where the machine is.
    let property = rbl_pdb::rows::PdbProperty {
        device_name: tables.device_name.to_owned(),
        contents: u32::try_from(tables.tracks.len()).unwrap_or(u32::MAX),
        created_date: rbl_core::time::local_date(),
        background_color: tables.background,
        ..rbl_pdb::rows::PdbProperty::default()
    };
    file.add_table(19, &rbl_pdb::rows::property_row(&property).map_or_else(Vec::new, |row| vec![row]));
    file.finish()
}

/// Writes the database folders an empty stick gets, as rekordbox does the
/// moment a drive is connected: the selected root's `rekordbox/export.pdb` with the
/// twenty tables and no tracks, `exportLibrary.db` holding `defaults` (or
/// the reference rows), and the `USBANLZ` and `Contents` directories. With
/// these in place the device's settings can be edited before anything is
/// exported. A stick that already has a database is left alone.
pub fn create_library(
    destination: &Path,
    defaults: Option<&rbl_onelibrary::settings::StickSettings>,
    my_tags: &[SourceMyTag],
    sync: Option<&SyncSource>,
) -> Result<bool> {
    create_library_with_root(destination, defaults, my_tags, sync, None)
}

/// [`create_library`], choosing the rekordbox root for a blank volume.
pub fn create_library_with_root(
    destination: &Path,
    defaults: Option<&rbl_onelibrary::settings::StickSettings>,
    my_tags: &[SourceMyTag],
    sync: Option<&SyncSource>,
    preferred_root: Option<ExportRoot>,
) -> Result<bool> {
    if destination.exists() && !destination.is_dir() {
        return Err(ExportError::NotADirectory(destination.to_owned()));
    }
    recover(destination)?;
    let publication = rbl_core::durable::Publication::new(destination, PUBLICATION)?;
    let root_name = export_root_name_with(destination, preferred_root)?;
    let db_dir = destination.join(root_name).join("rekordbox");
    let legacy = db_dir.join("export.pdb").exists();
    let one = db_dir.join("exportLibrary.db").exists();
    if legacy && one { return Ok(false); }
    if legacy || one {
        // Convert by reading and retaining the existing content. Never publish
        // an empty sibling over a populated library.
        let existing = snapshot::Snapshot::read(destination)?;
        let (tracks, playlists) = reconcile::all(destination, &existing, Manifest::load(destination).as_ref())?;
        drop(publication);
        export_full(destination, &tracks, &playlists, my_tags, defaults, sync, &mut |_| {})?;
        return Ok(true);
    }
    rbl_core::durable::create_dir_all(destination.join("Contents"))?;
    rbl_core::durable::create_dir_all(destination.join(root_name).join("USBANLZ"))?;
    rbl_core::durable::create_dir_all(&db_dir)?;
    let db_dir = publication.stage().join(root_name).join("rekordbox");
    rbl_core::durable::create_dir_all(&db_dir)?;
    let pdb = build_pdb(&PdbTables {
        tracks: &[],
        genres: &[],
        artists: &[],
        albums: &[],
        labels: &[],
        keys: &[],
        playlists: &[],
        entries: &[],
        artwork: &[],
        history: &[],
        colors: defaults.map_or(&[], |s| &s.colors),
        device_name: defaults.map_or("", |s| s.device_name.as_str()),
        background: 0,
    });
    rbl_core::durable::write(&db_dir.join("export.pdb"), &pdb)?;
    // The library's tags go on even a stick with no tracks [OBS 7.2.11:
    // the blank stick's `exportExt.pdb` held all 99].
    let master_db_id = my_tag_master_db_id(sync);
    rbl_core::durable::write(&db_dir.join("exportExt.pdb"), &ext_pdb::build(my_tags, &[], master_db_id))?;
    write_one_library(&db_dir, &[], &[], &[], &[], &[], my_tags, defaults, master_db_id, &snapshot::Snapshot::default(), None)?;
    let files = staged_files(publication.stage(), &[])?;
    publication.commit(&files)?;
    Ok(true)
}

/// The number `exportExt.pdb` and `exportLibrary.db`'s `property` row both
/// carry as the tags' master database id. rekordbox wrote 1744129535 for
/// this library, whose `DBID` is 1912725212, on every export; how it gets
/// from one to the other is [UNKNOWN] (not the DBID, nor CRC32/FNV/Adler/
/// MD5/SHA of the DBID or the device id). The two files only have to
/// agree with each other, so the library's own id stands in.
fn my_tag_master_db_id(sync: Option<&SyncSource>) -> u32 {
    sync.map_or(0, |s| u32::try_from(s.db_id & 0xffff_ffff).unwrap_or(0))
}

/// The subset of a track `exportLibrary.db` needs.
struct OneLibraryTrack {
    metadata: rbl_core::ExportMetadata,
    cues: Option<Vec<rbl_anlz::cues::ExportCue>>,
    year: u16, release_date: String, bitrate: u32, sample_rate: u32,
    export_id: u32,
    library_id: u64,
    master_db_id: u64,
    file_size: u64,
    title: String,
    artist: String,
    album: String,
    genre: String,
    label: String,
    key: String,
    color_id: u8,
    bpm_x100: u32,
    duration_sec: u16,
    rating: u8,
    comment: String,
    date_added: String,
    audio_path: String,
    file_name: String,
    analysis_path: String,
    /// The artwork's id in the `artwork` table and the `image` table; 0 none.
    image_id: u32,
    my_tags: Vec<u64>,
}

/// Where an image's files go on the stick, and what the databases name.
///
/// rekordbox writes four files per image — `a<id>.jpg`, `a<id>_m.jpg`,
/// `b<id>.jpg` and `b<id>_m.jpg` — twenty images to a five-digit folder,
/// ids 1–19 in `00001`, 20–39 in `00002` and so on [OBS, 2026-09-17 parity
/// test: 61 images over `00001`–`00004`]. `a<id>.jpg` is the library's own
/// 80×80 `artwork_s.jpg` and `a<id>_m.jpg` its 240×240 `artwork_m.jpg`,
/// byte for byte, and each `b` file is the same bytes as its `a` [OBS
/// 2026-09-18, md5 of the stick's files against `share/PIONEER/Artwork`].
fn artwork_path(id: u32, prefix: &str, medium: bool) -> String {
    let folder = id / ARTWORK_PER_FOLDER + 1;
    let suffix = if medium { "_m" } else { "" };
    format!("/PIONEER/Artwork/{folder:05}/{prefix}{id}{suffix}.jpg")
}

/// Images to a stick artwork folder.
const ARTWORK_PER_FOLDER: u32 = 20;

/// The four names an image is written under, each with whether it is the
/// medium size.
fn artwork_names(id: u32) -> [(String, bool); 4] {
    [
        (artwork_path(id, "a", false), false),
        (artwork_path(id, "a", true), true),
        (artwork_path(id, "b", false), false),
        (artwork_path(id, "b", true), true),
    ]
}

/// The library's file for one size of an image: `artwork_s.jpg` or
/// `artwork_m.jpg` beside the `artwork.jpg` the track names, when the
/// library has it; otherwise the named file itself, so a track whose
/// artwork was added by hand still gets a picture rather than none.
fn artwork_source(image: &Path, medium: bool) -> PathBuf {
    let sibling = image.with_file_name(if medium { "artwork_m.jpg" } else { "artwork_s.jpg" });
    if sibling.is_file() { sibling } else { image.to_path_buf() }
}

/// Copies an image to its four places, skipping any already there at the
/// same size. Returns how many files were written.
fn write_artwork(destination: &Path, existing: &Path, root_name: &str, id: u32, image: &Path) -> Result<usize> {
    let mut written = 0;
    for (name, medium) in artwork_names(id) {
        let source = artwork_source(image, medium);
        let name = name.replacen("/PIONEER/", &format!("/{root_name}/"), 1);
        let target = under(destination, &name);
        if files_equal(&source, &target)? || files_equal(&source, &under(existing, &name))? {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match copy_staged(&source, &target) {
            Ok(_) => written += 1,
            Err(e) if is_device_gone(&e) => return Err(ExportError::DeviceGone),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(written)
}

/// Writes `exportLibrary.db` beside `export.pdb`.
#[allow(clippy::too_many_arguments, reason = "the stick's tables, each from its own source")]
fn write_one_library(
    db_dir: &Path,
    tracks: &[OneLibraryTrack],
    playlists: &[SourcePlaylist],
    playlist_ids: &[u32],
    export_ids: &[Option<u32>],
    artwork_paths: &[(u32, String)],
    my_tags: &[SourceMyTag],
    defaults: Option<&rbl_onelibrary::settings::StickSettings>,
    master_db_id: u32,
    before: &snapshot::Snapshot,
    existing_database: Option<&Path>,
) -> Result<()> {
    use rbl_onelibrary::build::Builder;
    use rbl_onelibrary::settings::StickSettings;

    let path = db_dir.join("exportLibrary.db");
    // The database is rebuilt from scratch, but the settings the stick already
    // carries — its name, which browse categories and sorts are on, the colour
    // comments — are the user's and survive the rebuild. A stick that holds
    // none starts from the defaults it was given or the reference rows.
    // An unreadable existing database is refused, preserving its settings.
    let fresh = || defaults.cloned().unwrap_or_default();
    let settings = if path.exists() {
        StickSettings::read(&path).map_err(|e| one_library_error(&e))?
    } else {
        fresh()
    };
    // An export is written into a fresh directory, but a resumed one may find
    // the previous attempt's file; replacing it is correct, keeping it is not.
    let staging = tempfile::tempdir_in(db_dir)?;
    let staged = staging.path().join("exportLibrary.db");
    let mut builder = Builder::create_with(&staged, &settings).map_err(|e| one_library_error(&e))?;

    for (id, image) in artwork_paths {
        builder.add_image(i64::from(*id), image).map_err(|e| one_library_error(&e))?;
    }
    // Every tag the library has, whether or not a track here carries it,
    // which is what rekordbox lists; then only the memberships of tags that
    // exist, so a stale membership cannot point at nothing.
    let mut known_tags: BTreeSet<u64> = BTreeSet::new();
    for tag in my_tags {
        let id = i64::try_from(tag.id).unwrap_or(0);
        if id == 0 {
            continue;
        }
        builder
            .add_my_tag(
                id,
                i64::from(tag.seq),
                &tag.name,
                i64::from(tag.attribute),
                i64::try_from(tag.parent).unwrap_or(0),
            )
            .map_err(|e| one_library_error(&e))?;
        known_tags.insert(tag.id);
    }

    add_tracks(&mut builder, tracks, &known_tags)?;

    for (i, playlist) in playlists.iter().enumerate() {
        let playlist_id = i64::from(playlist_ids[i]);
        let parent = playlists.iter().position(|p| p.id != 0 && p.id == playlist.parent_id).map_or(0, |p| i64::from(playlist_ids[p]));
        builder
            .add_playlist_node(playlist_id, &playlist.name, parent, i64::try_from(i).unwrap_or(0), playlist.folder)
            .map_err(|e| one_library_error(&e))?;
        let mut position: i64 = 0;
        for &track_index in &playlist.track_indices {
            let Some(Some(export_id)) = export_ids.get(track_index).copied() else { continue };
            position += 1;
            builder
                .add_to_playlist(playlist_id, i64::from(export_id), position)
                .map_err(|e| one_library_error(&e))?;
        }
    }

    // The date only, which is what rekordbox's own export carries. The
    // device name is the one the stick has been given, and empty until
    // then: rekordbox writes it empty on a fresh export [OBS 7.2.11].
    if let Some(path) = existing_database.filter(|p| p.exists()) {
        builder.preserve_cues(path).map_err(|e| one_library_error(&e))?;
    }
    for track in tracks {
        if let Some(cues) = &track.cues {
            builder.replace_cues(i64::from(track.export_id), cues).map_err(|e| one_library_error(&e))?;
        }
    }
    for history in &before.history {
        builder.add_history(history.id, &history.name, history.parent, history.sequence, history.folder).map_err(|e| one_library_error(&e))?;
        for (position, id) in history.tracks.iter().enumerate() {
            builder.add_history_track(history.id, i64::from(*id), i64::try_from(position + 1).unwrap_or(0)).map_err(|e| one_library_error(&e))?;
        }
    }
    let created = rbl_core::time::local_date();
    builder.finish(&settings.device_name, &created, master_db_id).map_err(|e| one_library_error(&e))?;
    rbl_core::durable::replace(&staged, &path).map_err(|e| ExportError::OneLibrary(e.to_string()))?;
    Ok(())
}

/// Interns each track's lookups, adds it, and tags it — the part of
/// [`write_one_library`] with a body per track rather than per collection.
fn add_tracks(
    builder: &mut rbl_onelibrary::build::Builder,
    tracks: &[OneLibraryTrack],
    known_tags: &BTreeSet<u64>,
) -> Result<()> {
    use rbl_onelibrary::build::{LookupTable, Track};
    for track in tracks {
        let artist = builder.intern(LookupTable::Artist, &track.artist).map_err(|e| one_library_error(&e))?;
        let album = builder.intern(LookupTable::Album, &track.album).map_err(|e| one_library_error(&e))?;
        let genre = builder.intern(LookupTable::Genre, &track.genre).map_err(|e| one_library_error(&e))?;
        let label = builder.intern(LookupTable::Label, &track.label).map_err(|e| one_library_error(&e))?;
        let key = builder.intern(LookupTable::Key, &track.key).map_err(|e| one_library_error(&e))?;
        builder
            .add_track(&Track {
                metadata: track.metadata.clone(),
                file_type: i64::from(rbl_pdb::rows::audio_file_type(&track.file_name)),
                year: i64::from(track.year), release_date: track.release_date.clone(),
                bitrate: i64::from(track.bitrate), sample_rate: i64::from(track.sample_rate),
                content_id: i64::from(track.export_id),
                title: track.title.clone(),
                artist_id: Some(artist),
                album_id: Some(album),
                genre_id: Some(genre),
                label_id: Some(label),
                key_id: Some(key),
                color_id: Some(i64::from(track.color_id)),
                bpm_x100: i64::from(track.bpm_x100),
                length: i64::from(track.duration_sec),
                track_no: i64::from(track.metadata.track_number),
                path: track.audio_path.clone(),
                file_name: track.file_name.clone(),
                file_size: i64::try_from(track.file_size).unwrap_or(i64::MAX),
                master_db_id: i64::try_from(track.master_db_id).unwrap_or(0),
                master_content_id: i64::try_from(track.library_id).unwrap_or(0),
                analysis_path: track.analysis_path.clone(),
                // Stars are multiples of 51 here as everywhere else.
                rating: i64::from(track.rating) * 51,
                comment: track.comment.clone(),
                date_added: track.date_added.clone(),
                image_id: (track.image_id != 0).then_some(i64::from(track.image_id)),
            })
            .map_err(|e| one_library_error(&e))?;
        for tag in &track.my_tags {
            if !known_tags.contains(tag) {
                continue;
            }
            builder
                .tag_track(i64::try_from(*tag).unwrap_or(0), i64::from(track.export_id))
                .map_err(|e| one_library_error(&e))?;
        }
    }
    Ok(())
}

fn one_library_error(error: &rbl_onelibrary::Error) -> ExportError {
    ExportError::OneLibrary(error.to_string())
}

/// Copies a file's bytes and nothing else, reading the next chunk while the
/// last one is being written.
///
/// `std::fs::copy` on macOS carries extended attributes along, and on a
/// FAT stick each of those becomes an `AppleDouble` `._` file, so only the
/// data goes. The read and the write are on different devices — the export
/// measured 11.6 MB/s from an SD card into a stick that takes 48 MB/s [OBS
/// 2026-09-17] — so they overlap: a reader fills a short queue of chunks
/// and the writer drains it, and a track costs the slower of the two
/// rather than their sum.
fn copy_data(from: &Path, to: &Path) -> std::io::Result<u64> {
    copy_data_with_durability(from, to, true)
}

/// A staged image is flushed as a batch by `Publication::commit`; syncing it
/// here as well only makes removable media wait twice for the same bytes.
fn copy_staged(from: &Path, to: &Path) -> std::io::Result<u64> {
    copy_data_with_durability(from, to, false)
}

fn copy_data_with_durability(from: &Path, to: &Path, durable: bool) -> std::io::Result<u64> {
    use std::io::{Read, Write};
    let mut source = std::fs::File::open(from)?;
    let parent = to.parent().unwrap_or(Path::new("."));
    let staging = tempfile::NamedTempFile::new_in(parent)?;
    let mut target = staging.reopen()?;
    let (send, receive) = std::sync::mpsc::sync_channel::<Vec<u8>>(COPY_QUEUE);
    let writer = std::thread::spawn(move || -> std::io::Result<u64> {
        let mut total: u64 = 0;
        for chunk in receive {
            target.write_all(&chunk)?;
            total += chunk.len() as u64;
        }
        if durable { target.sync_all()?; }
        Ok(total)
    });
    let read_result = (|| -> std::io::Result<()> {
        loop {
            let mut buffer = vec![0_u8; COPY_BUFFER];
            let read = source.read(&mut buffer)?;
            if read == 0 {
                return Ok(());
            }
            buffer.truncate(read);
            if send.send(buffer).is_err() {
                // The writer stopped early; its error is the one to report.
                return Ok(());
            }
        }
    })();
    drop(send);
    let written = writer
        .join()
        .map_err(|_| std::io::Error::other("the copy's writer thread panicked"))??;
    read_result?;
    staging.persist(to).map_err(|e| e.error)?;
    if durable { rbl_core::durable::sync_dir(parent)?; }
    Ok(written)
}

/// Chunks the reader may run ahead of the writer by.
const COPY_QUEUE: usize = 4;

/// One megabyte: large enough that a 13 MB track is a dozen writes, small
/// enough not to matter on a laptop.
const COPY_BUFFER: usize = 1 << 20;

/// The files rekordbox copies from its own settings directory to every
/// stick it exports to: the player and mixer "My Settings" and the DJ
/// profile [OBS 7.2.11, byte-identical to the files in
/// `rbl_core::paths::rekordbox_settings_dir`].
pub const MY_SETTINGS_FILES: [&str; 4] = ["MYSETTING.DAT", "MYSETTING2.DAT", "DJMMYSETTING.DAT", "djprofile.nxs"];

/// Puts the My Settings files from `source` on the stick, as rekordbox
/// does on export. A file the stick already has is kept: it is the DJ's
/// own, set on a player. Returns how many were written; `source` absent or
/// empty writes none, which is not an error — a machine without rekordbox
/// has none to give.
pub fn copy_my_settings(destination: &Path, source: &Path) -> Result<usize> {
    let pioneer = export_root(destination);
    let mut written = 0;
    for name in MY_SETTINGS_FILES {
        let from = source.join(name);
        let to = pioneer.join(name);
        if !from.is_file() || to.exists() {
            continue;
        }
        rbl_core::durable::create_dir_all(&pioneer)?;
        copy_data(&from, &to)?;
        written += 1;
    }
    Ok(written)
}

/// An I/O error that says what was being done when it happened, keeping its
/// kind; the bare OS text ("No such file or directory") names nothing a
/// user can act on.
/// Records a track the export is going on without.
fn fail_track(report: &mut ExportReport, track: &SourceTrack, reason: &str) {
    tracing::warn!(title = %track.title, path = %track.source_path.display(), %reason, "track left out of the export");
    report.failed.push(format!("{}: {reason}", track.title));
}

fn context(what: String) -> impl Fn(std::io::Error) -> std::io::Error {
    move |error| std::io::Error::new(error.kind(), format!("{what}: {error}"))
}

/// Errors that mean the media was unplugged mid-write.
fn is_device_gone(e: &std::io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        // ENXIO, ENODEV, EIO
        Some(6 | 19 | 5)
    )
}

/// Re-reads an export with the independent parser and checks it is coherent.
///
/// A writer that verifies itself proves little; this reads the file back the
/// same way a player would and confirms the tracks and playlists survived.
pub use verification::VerifyReport;
pub fn verify(destination: &Path) -> Result<VerifyReport> {
    recover(destination)?;
    verification::verify(destination)
}

pub fn verify_databases(destination: &Path) -> Result<VerifyReport> {
    verification::verify_databases(destination)
}

/// What a failed verification found, as one clause: the problems, then
/// the audio it could not find, without an empty list when there is none.
#[must_use]
pub fn verification_failure(report: &VerifyReport) -> String {
    let mut problems = report.errors.clone();
    if !report.parsed && problems.is_empty() {
        problems.push("the databases could not be read back".to_owned());
    }
    if !report.missing_audio.is_empty() {
        const SHOWN: usize = 3;
        let missing = report.missing_audio.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
        let more = report.missing_audio.len().saturating_sub(SHOWN);
        problems.push(if more > 0 { format!("audio missing: {missing} and {more} more") } else { format!("audio missing: {missing}") });
    }
    problems.join("; ")
}

const PUBLICATION: &str = ".rbxport-publication";

/// Complete an interrupted export before reading or changing the device.
///
/// An export that stopped part way leaves its journal on the device, and
/// this finishes it. When the device was written to since, which rekordbox
/// does by itself the moment it sees the stick, the journal cannot be
/// finished without overwriting those writes, and refusing left the stick
/// unusable until it was reformatted (#229). Instead the interrupted export
/// is settled around those writes, finished or rolled back depending on
/// which generation the other writer saw (see
/// `rbl_core::durable::Publication::set_aside`), and its journal is set aside
/// under `rbxport/recovered-<ms>/` in the library root, so the next export
/// starts from the device as it is now.
pub fn recover(destination: &Path) -> std::io::Result<()> {
    match rbl_core::durable::Publication::recover(destination, PUBLICATION) {
        Err(error) if rbl_core::durable::is_device_changed(&error) => {
            let root_name = export_root_name(destination).unwrap_or("PIONEER");
            let keep = destination.join(root_name).join(format!("rbxport/recovered-{}", rbl_core::time::unix_millis()));
            if let Some(theirs) = rbl_core::durable::Publication::set_aside(destination, PUBLICATION, &keep)? {
                tracing::warn!(
                    device = %destination.display(),
                    kept = %keep.display(),
                    changed_by_another_writer = ?theirs,
                    %error,
                    "settled an interrupted export the device had changed since, keeping the other writer's files; the next export starts from the device as it is"
                );
            }
            Ok(())
        }
        other => other,
    }
}

/// The files staged under `root`, relative to it, under the names they are
/// to be published as.
///
/// The directory listing is not that name on a stick mounted by macOS:
/// its FAT32 and exFAT drivers list every name in NFD while the export
/// writes NFC (`fat_safe`), and a FAT32 stick then refuses to rename the
/// file by its listed name [OBS 2026-10-08; see `rbl_core::durable`]. So a
/// listed path is published as the one of `named` it matches in NFC, which
/// is how the databases name it, and in NFC when it matches none.
fn staged_files(root: &Path, named: &[&str]) -> std::io::Result<Vec<PathBuf>> {
    fn walk(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                // An AppleDouble companion can go with its file meanwhile.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            if file_type.is_dir() { walk(root, &entry.path(), files)?; }
            else { files.push(entry.path().strip_prefix(root).map_err(std::io::Error::other)?.to_owned()); }
        }
        Ok(())
    }
    let named: BTreeMap<PathBuf, PathBuf> = named
        .iter()
        .map(|path| {
            let path = PathBuf::from(path.trim_start_matches('/'));
            (rbl_core::durable::nfc_path(&path), path)
        })
        .collect();
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    let mut files: Vec<PathBuf> = files
        .into_iter()
        .map(|listed| {
            let nfc = rbl_core::durable::nfc_path(&listed);
            named.get(&nfc).cloned().unwrap_or(nfc)
        })
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn analysis_paths_match_independent_rekordbox_exports() {
        assert_eq!(analysis_directory("/Contents/Hosanna, Westend/Drum Death - Extended Mix/20130688_drum_death_(extended_mix).mp3", "PIONEER"), "/PIONEER/USBANLZ/P002/0002583E");
        assert_eq!(analysis_directory("/Contents/Meduza, Aya Anne, GENESI (ITA)/Freak EP/20063130_freak_(feat._aya_anne)_(feat._aya_a.mp3", "PIONEER"), "/PIONEER/USBANLZ/P018/00008F82");
    }

    #[test]
    fn hfs_names_select_the_hidden_rekordbox_root() {
        for name in ["hfs", "HFS+", "hfsplus", "Mac OS Extended (Journaled)"] {
            assert_eq!(ExportRoot::for_file_system(name), Some(ExportRoot::Hidden), "{name}");
        }
        for name in ["FAT32", "exFAT", "APFS", "ext4", ""] {
            assert_eq!(ExportRoot::for_file_system(name), None, "{name}");
        }
    }

    #[test]
    fn fat_safe_replaces_characters_a_stick_cannot_hold() {
        assert_eq!(fat_safe("A/B:C*D?E"), "A_B_C_D_E");
        assert_eq!(fat_safe("trailing dots..."), "trailing dots");
        assert_eq!(fat_safe(""), "Unknown");
        assert_eq!(fat_safe("   "), "Unknown");
        // Unicode is fine on FAT32 long names.
        assert_eq!(fat_safe("Ébano — Tiësto"), "Ébano — Tiësto");
        assert_eq!(fat_safe("Kesa\u{308} (On Kaunis).aiff"), "Kesä (On Kaunis).aiff");
    }

    #[test]
    fn long_fat_filenames_keep_their_audio_extension() {
        let original = format!("Relative Progress , Steve Nash , 120 Dance Moves {}.aiff", "extended ".repeat(20));
        let shortened = fat_file_name(&original);
        assert!(shortened.len() <= FILE_NAME_MAX);
        assert!(Path::new(&shortened)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("aiff")));
        assert_eq!(fat_file_name("Omega - The Hidden Beauty Of Dutch House '94-'98 - 04 Le Rève.aiff"),
            "Omega - The Hidden Beauty Of Dutch House '94-'98 - 04 Le Rève.aiff");
    }

    /// macOS lists a FAT32 or exFAT stick's names in NFD whatever form they
    /// were written in, and a FAT32 stick will not rename a file by that
    /// name: publishing from the listing failed at 99% with "No such file or
    /// directory" (#122, #161). The listing has to come back as the names
    /// the databases use. A temporary directory keeps the form a name is
    /// given in, so an NFD file here lists the way the stick does.
    #[test]
    fn staged_files_are_published_under_the_names_the_databases_use() {
        let stage = tempfile::tempdir().unwrap();
        let listed_nfd = stage.path().join("Contents/Bjo\u{308}rk/Album/Kesa\u{308}.mp3");
        std::fs::create_dir_all(listed_nfd.parent().unwrap()).unwrap();
        std::fs::write(&listed_nfd, b"audio").unwrap();
        let other = stage.path().join("Contents/Bjo\u{308}rk/Album/Ebano\u{301}.mp3");
        std::fs::write(&other, b"audio").unwrap();
        std::fs::create_dir_all(stage.path().join("PIONEER/rekordbox")).unwrap();
        std::fs::write(stage.path().join("PIONEER/rekordbox/export.pdb"), b"pdb").unwrap();

        // One named by the databases (in NFC, as `fat_safe` writes names),
        // one not named at all.
        let files = staged_files(stage.path(), &["/Contents/Bj\u{f6}rk/Album/Kes\u{e4}.mp3"]).unwrap();
        assert_eq!(files, vec![
            PathBuf::from("Contents/Bj\u{f6}rk/Album/Eban\u{f3}.mp3"),
            PathBuf::from("Contents/Bj\u{f6}rk/Album/Kes\u{e4}.mp3"),
            PathBuf::from("PIONEER/rekordbox/export.pdb"),
        ]);
    }

    #[test]
    fn interning_assigns_stable_ids_from_one() {
        let mut intern = Intern::default();
        assert_eq!(intern.id("ARTBAT"), 1);
        assert_eq!(intern.id("Meduza"), 2);
        assert_eq!(intern.id("ARTBAT"), 1, "repeat lookups must be stable");
        // An empty name has no row; zero means "none".
        assert_eq!(intern.id(""), 0);
        let names: Vec<&str> = intern.entries().map(|(_, n)| n).collect();
        assert_eq!(names, vec!["ARTBAT", "Meduza"]);
    }
}
