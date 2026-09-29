//! A folder on disk as a track list: the Explorer's view.
//!
//! The list is what one directory read found, in two kinds. A file the
//! library already holds is that library row — its title, its key, its
//! analysis — matched by path. A file it does not is a loose file: shown by
//! its name at first, with what its tags say once the window it sits in is
//! fetched. Nothing here reads a directory; the caller lists the folder and
//! hands the names in, so this crate stays about the library and the listing
//! stays about the disk.
//!
//! Loose files are never written to the library from here. Rekordbox adds a
//! file to the collection when it is loaded from the Explorer; this does not,
//! and the reason is recorded in `TODO.md`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::{strings::fold, Library, Row, SortColumn, ViewSpec};

/// What a loose file's row id starts with, so it cannot be mistaken for a
/// track id, which is decimal.
pub const LOOSE_PREFIX: &str = "file:";

/// The path behind a loose file's row id, or `None` for any other id.
#[must_use]
pub fn loose_path(id: &str) -> Option<&Path> {
    id.strip_prefix(LOOSE_PREFIX).map(Path::new)
}

/// A loose file's row id.
#[must_use]
pub fn loose_id(path: &Path) -> String {
    format!("{LOOSE_PREFIX}{}", path.display())
}

/// What a loose file's tags say, read when its window is first fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LooseTags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub duration_sec: u32,
}

/// A file in the folder that the library does not hold.
#[derive(Debug)]
pub struct LooseFile {
    pub name: String,
    pub path: PathBuf,
    tags: OnceLock<LooseTags>,
}

/// Readers a window's tags are spread over.
///
/// A tag read is a few small reads and seeks, and on a card in a USB reader
/// each one waits on the reader rather than on the data — 14 ms a file
/// there against under a millisecond from the local disk [OBS]. Eight at a
/// time turns a page of a slow card from seconds into a fraction of one.
const TAG_READERS: usize = 8;

impl LooseFile {
    /// The file's tags, if they have been read. The name is what there is
    /// to show until then.
    pub fn tags_if_read(&self) -> Option<&LooseTags> {
        self.tags.get()
    }

    /// The file's tags, read on the first call and kept.
    ///
    /// One `lofty` probe per file, header only and without the cover art —
    /// a few hundred microseconds from a local disk. It is here rather than
    /// at open so a folder of five thousand tracks opens on the directory
    /// read alone, and a window of rows pays for its own files when it is
    /// fetched.
    pub fn tags(&self) -> &LooseTags {
        self.tags.get_or_init(|| match rbl_db::import::read_tags(&self.path) {
            Ok(read) => LooseTags {
                title: read.title,
                artist: read.artist,
                album: read.album,
                genre: read.genre,
                duration_sec: read.duration_sec,
            },
            // Unreadable, or not really audio: the name is what there is.
            Err(_) => LooseTags { title: stem_of(&self.path), ..LooseTags::default() },
        })
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// One row of a folder view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderEntry {
    /// A library row.
    Track(Row),
    /// An index into [`FolderView::file`].
    File(usize),
}

/// A folder, ordered and filtered the way a [`crate::View`] is.
#[derive(Debug, Default)]
pub struct FolderView {
    pub entries: Vec<FolderEntry>,
    files: Vec<LooseFile>,
    /// True when the listing was cut at its cap, so the caller can say so.
    pub truncated: bool,
}

impl FolderView {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// A window of entries, clamped to what exists.
    pub fn window(&self, offset: usize, len: usize) -> &[FolderEntry] {
        let start = offset.min(self.entries.len());
        let end = start.saturating_add(len).min(self.entries.len());
        self.entries.get(start..end).unwrap_or(&[])
    }

    /// The loose file an entry points at.
    pub fn file(&self, index: usize) -> Option<&LooseFile> {
        self.files.get(index)
    }

    /// Reads the tags of the window's loose files that have none yet,
    /// several at a time, and stops taking new ones once `budget` is spent.
    ///
    /// Bounded so a page always lands: a file whose tags were not reached
    /// shows its name, and is read on the next fetch of its page. Files
    /// already read cost nothing here, so scrolling back is free.
    pub fn read_tags_in(&self, offset: usize, len: usize, budget: Duration) {
        let pending: Vec<&LooseFile> = self
            .window(offset, len)
            .iter()
            .filter_map(|entry| match *entry {
                FolderEntry::File(index) => self.files.get(index).filter(|f| f.tags.get().is_none()),
                FolderEntry::Track(_) => None,
            })
            .collect();
        if pending.is_empty() {
            return;
        }
        let deadline = Instant::now() + budget;
        let next = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..pending.len().min(TAG_READERS) {
                scope.spawn(|| {
                    while Instant::now() < deadline {
                        let Some(file) = pending.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        let _ = file.tags();
                    }
                });
            }
        });
    }
}

/// The key a path is matched by, so the library's spelling and the disk's
/// agree.
///
/// Case folds because both file systems this runs on ignore it; accents
/// fold because rekordbox stores `café` composed and an HFS+ card hands it
/// back decomposed [OBS] — 20 of the 2,911 tracks in the reference
/// library's largest folder differ from the directory read by that alone.
/// Combining marks are dropped for the same reason: `fold` maps a composed
/// `é` to `e`, and this makes `e` + U+0301 land in the same place.
fn path_key(path: &str) -> String {
    let mut key = fold(path);
    key.retain(|ch| !('\u{300}'..='\u{36f}').contains(&ch));
    key
}

fn hash_of(key: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

impl Library {
    /// The row whose file is at `path`, if the library holds it.
    ///
    /// Through a map from a hash of the folded path to rows, built on first
    /// use: hashes rather than the paths themselves, so the map is a few
    /// hundred kilobytes over 38k tracks rather than several megabytes of
    /// copied strings. A hash can collide, so the hit is confirmed against
    /// the stored path before it is believed.
    #[must_use]
    pub fn row_for_path(&self, path: &Path) -> Option<Row> {
        let wanted = path_key(&path.to_string_lossy());
        let rows = self.rows_by_path().get(&hash_of(&wanted))?;
        rows.iter()
            .copied()
            .find(|&row| path_key(self.folder_path.get(row as usize)) == wanted)
    }

    fn rows_by_path(&self) -> &HashMap<u64, Vec<Row>> {
        self.by_path.get_or_init(|| {
            let mut map: HashMap<u64, Vec<Row>> = HashMap::with_capacity(self.count);
            for row in 0..self.count {
                let stored = self.folder_path.get(row);
                if stored.is_empty() {
                    continue;
                }
                map.entry(hash_of(&path_key(stored)))
                    .or_default()
                    .push(u32::try_from(row).unwrap_or(u32::MAX));
            }
            map
        })
    }

    /// Builds the Explorer's view of one folder.
    ///
    /// `files` is the folder's audio files as listed, name and path each,
    /// already in name order — that order is what `TrackNo` keeps. The sort
    /// and the search treat both kinds alike where they can: a loose file
    /// sorts by its name where a track sorts by its title, and by nothing
    /// under every other column, since its tags are not read until its
    /// window is fetched [ASSUME: what rekordbox sorts unimported files by
    /// is not captured].
    #[must_use]
    pub fn open_folder(&self, files: Vec<(String, PathBuf)>, spec: &ViewSpec) -> FolderView {
        self.open_folder_scoped(files, spec, crate::SearchField::All)
    }

    pub fn open_folder_scoped(&self, files: Vec<(String, PathBuf)>, spec: &ViewSpec, field: crate::SearchField) -> FolderView {
        let mut view = FolderView { entries: Vec::with_capacity(files.len()), ..FolderView::default() };
        for (name, path) in files {
            if let Some(row) = self.row_for_path(&path) {
                view.entries.push(FolderEntry::Track(row));
            } else {
                view.entries.push(FolderEntry::File(view.files.len()));
                view.files.push(LooseFile { name, path, tags: OnceLock::new() });
            }
        }

        let query = fold(spec.query.trim());
        if !query.is_empty() {
            let files = &view.files;
            view.entries.retain(|entry| match *entry {
                FolderEntry::Track(row) => self.row_matches_in(row, &query, field),
                FolderEntry::File(index) => {
                    files.get(index).is_some_and(|file| {
                        use crate::SearchField;
                        let value = match field {
                            SearchField::All | SearchField::Title => &file.name,
                            SearchField::Artist => &file.tags().artist,
                            SearchField::Album => &file.tags().album,
                            SearchField::Genre => &file.tags().genre,
                            _ => "",
                        };
                        matches_name(&fold(value), &query)
                    })
                }
            });
        }

        // The folder is at most a few thousand rows, so a key per row is
        // cheap; the library's rank arrays cannot place a loose file anyway.
        match spec.sort {
            SortColumn::TrackNo => {}
            SortColumn::Bpm | SortColumn::Duration | SortColumn::Rating | SortColumn::PlayCount | SortColumn::KeyCamelot => {
                let key = |entry: &FolderEntry| match *entry {
                    FolderEntry::Track(row) => match spec.sort {
                        SortColumn::Bpm => self.bpm_x100.get(row as usize).copied().unwrap_or(0),
                        SortColumn::Duration => self.length_sec.get(row as usize).copied().unwrap_or(0),
                        SortColumn::PlayCount => u32::from(self.play_count.get(row as usize).copied().unwrap_or(0)),
                        SortColumn::KeyCamelot => crate::key::camelot_rank(self.key_name(row)),
                        _ => u32::from(self.rating.get(row as usize).copied().unwrap_or(0)),
                    },
                    // A loose file has no key or number; it goes with the
                    // blanks, which is last for a key and first for a number.
                    FolderEntry::File(_) => if spec.sort == SortColumn::KeyCamelot { u32::MAX } else { 0 },
                };
                view.entries.sort_by_key(key);
            }
            SortColumn::Key => {
                let name = |entry: &FolderEntry| match *entry {
                    FolderEntry::Track(row) => self.key_name(row),
                    FolderEntry::File(_) => "",
                };
                view.entries.sort_by(|a, b| crate::key::cmp_names(name(a), name(b)));
            }
            column => {
                let files = &view.files;
                let key = |entry: &FolderEntry| -> &str {
                    match *entry {
                        FolderEntry::Track(row) => self.folded_text(row, column),
                        FolderEntry::File(index) => match column {
                            SortColumn::Title => files.get(index).map_or("", |f| f.name.as_str()),
                            _ => "",
                        },
                    }
                };
                let mut keyed: Vec<(String, FolderEntry)> = view
                    .entries
                    .iter()
                    .map(|entry| (fold(key(entry)), *entry))
                    .collect();
                keyed.sort_by(|a, b| a.0.cmp(&b.0));
                view.entries = keyed.into_iter().map(|(_, entry)| entry).collect();
            }
        }
        if spec.descending {
            view.entries.reverse();
        }
        view
    }

    /// A track's text under a sort column, already folded — the interners'
    /// folded forms are what the rank arrays were built from, so this orders
    /// the way the collection does.
    fn folded_text(&self, row: Row, column: SortColumn) -> &str {
        let id = |ids: &[u32]| ids.get(row as usize).copied().unwrap_or(crate::NO_ID);
        match column {
            SortColumn::Title => self.title_folded.get(row as usize),
            SortColumn::Artist => self.artists.folded(id(&self.artist)),
            SortColumn::Album => self.albums.folded(id(&self.album)),
            SortColumn::Genre => self.genres.folded(id(&self.genre)),
            SortColumn::Label => self.labels.folded(id(&self.label)),
            SortColumn::Comment => self.comment.get(row as usize),
            SortColumn::DateAdded => self.date_added.get(row as usize),
            SortColumn::ReleaseDate => self.release_date.get(row as usize),
            SortColumn::TrackNo | SortColumn::Bpm | SortColumn::Duration | SortColumn::Rating | SortColumn::PlayCount
            | SortColumn::Key | SortColumn::KeyCamelot => "",
        }
    }
}

/// Every token of the query somewhere in the name, as the library search does.
fn matches_name(folded_name: &str, folded_query: &str) -> bool {
    folded_query.split_whitespace().all(|token| folded_name.contains(token))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::testing::{library_from, TestTrack};

    fn library() -> Library {
        library_from(&[
            TestTrack { id: 1, title: "Zebra", artist: "ARTBAT", bpm_x100: 12800, path: "/music/zebra.mp3", ..TestTrack::default() },
            TestTrack { id: 2, title: "Apple", artist: "Meduza", bpm_x100: 13000, path: "/music/Café Del Mar.mp3", ..TestTrack::default() },
            TestTrack { id: 3, title: "Mango", artist: "Tujamo", bpm_x100: 12400, path: "/elsewhere/mango.mp3", ..TestTrack::default() },
        ])
    }

    fn spec(sort: SortColumn, descending: bool, query: &str) -> ViewSpec {
        ViewSpec {
            source: crate::TrackSource::Collection,
            sort,
            descending,
            query: query.to_owned(),
            filter: crate::TrackFilter::default(),
        }
    }

    fn listed(names: &[&str]) -> Vec<(String, PathBuf)> {
        names.iter().map(|n| ((*n).to_owned(), PathBuf::from(format!("/music/{n}")))).collect()
    }

    #[test]
    fn a_path_the_library_holds_finds_its_row() {
        let lib = library();
        assert_eq!(lib.row_for_path(Path::new("/music/zebra.mp3")), Some(0));
        assert_eq!(lib.row_for_path(Path::new("/elsewhere/mango.mp3")), Some(2));
        assert_eq!(lib.row_for_path(Path::new("/music/nothing.mp3")), None);
        assert_eq!(lib.row_for_path(Path::new("")), None);
    }

    #[test]
    fn a_path_matches_however_the_disk_spells_it() {
        let lib = library();
        // Case: both file systems ignore it.
        assert_eq!(lib.row_for_path(Path::new("/music/ZEBRA.MP3")), Some(0));
        // A decomposed `é` from an HFS+ card against the composed one stored.
        assert_eq!(lib.row_for_path(Path::new("/music/Cafe\u{301} Del Mar.mp3")), Some(1));
    }

    #[test]
    fn a_folder_lists_library_rows_and_loose_files_in_listing_order() {
        let lib = library();
        let view = lib.open_folder(listed(&["a.mp3", "Café Del Mar.mp3", "zebra.mp3"]), &spec(SortColumn::TrackNo, false, ""));
        assert_eq!(view.entries, [FolderEntry::File(0), FolderEntry::Track(1), FolderEntry::Track(0)]);
        assert_eq!(view.file(0).unwrap().name, "a.mp3");
        assert_eq!(view.window(1, 10), &view.entries[1..]);
        assert_eq!(view.window(5, 10), &[]);
    }

    #[test]
    fn sorting_by_title_uses_a_loose_files_name() {
        let lib = library();
        let view = lib.open_folder(listed(&["zzz.mp3", "Café Del Mar.mp3", "bbb.mp3", "zebra.mp3"]), &spec(SortColumn::Title, false, ""));
        // Apple (track 2), bbb, Zebra (track 1), zzz.
        assert_eq!(view.entries, [FolderEntry::Track(1), FolderEntry::File(1), FolderEntry::Track(0), FolderEntry::File(0)]);
        let down = lib.open_folder(listed(&["zzz.mp3", "Café Del Mar.mp3", "bbb.mp3", "zebra.mp3"]), &spec(SortColumn::Title, true, ""));
        assert_eq!(down.entries, [FolderEntry::File(0), FolderEntry::Track(0), FolderEntry::File(1), FolderEntry::Track(1)]);
    }

    #[test]
    fn sorting_by_bpm_puts_loose_files_first_because_they_have_none() {
        let lib = library();
        let view = lib.open_folder(listed(&["zebra.mp3", "loose.mp3", "Café Del Mar.mp3"]), &spec(SortColumn::Bpm, false, ""));
        assert_eq!(view.entries, [FolderEntry::File(0), FolderEntry::Track(0), FolderEntry::Track(1)]);
    }

    #[test]
    fn the_query_matches_a_tracks_fields_and_a_loose_files_name() {
        let lib = library();
        let files = listed(&["zebra.mp3", "summer mix.mp3", "Café Del Mar.mp3"]);
        let view = lib.open_folder(files, &spec(SortColumn::TrackNo, false, "artbat"));
        assert_eq!(view.entries, [FolderEntry::Track(0)]);
        let files = listed(&["zebra.mp3", "summer mix.mp3", "Café Del Mar.mp3"]);
        let view = lib.open_folder(files, &spec(SortColumn::TrackNo, false, "SUMMER"));
        assert_eq!(view.entries, [FolderEntry::File(0)]);
        assert_eq!(view.file(0).unwrap().name, "summer mix.mp3");
    }

    #[test]
    fn a_windows_tags_are_read_together_and_a_spent_budget_leaves_the_rest_unread() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        for i in 0..20 {
            let path = dir.path().join(format!("{i:02}.mp3"));
            std::fs::write(&path, b"not audio").unwrap();
            files.push((format!("{i:02}.mp3"), path));
        }
        let lib = library();
        let view = lib.open_folder(files, &spec(SortColumn::TrackNo, false, ""));
        assert!(view.file(0).unwrap().tags_if_read().is_none());

        view.read_tags_in(0, 10, Duration::from_secs(5));
        for i in 0..10 {
            assert!(view.file(i).unwrap().tags_if_read().is_some(), "{i}");
        }
        for i in 10..20 {
            assert!(view.file(i).unwrap().tags_if_read().is_none(), "{i}");
        }

        // No budget at all: nothing new is read, and nothing already read is lost.
        view.read_tags_in(0, 20, Duration::ZERO);
        assert!(view.file(5).unwrap().tags_if_read().is_some());
        assert!(view.file(15).unwrap().tags_if_read().is_none());
    }

    #[test]
    fn a_loose_files_id_answers_with_its_path_wherever_a_track_id_would() {
        let lib = library();
        assert_eq!(lib.audio_path_of("1"), Some("/music/zebra.mp3"));
        assert_eq!(lib.audio_path_of("file:/loose/one.mp3"), Some("/loose/one.mp3"));
        assert_eq!(lib.audio_path_of("nope"), None);
    }

    #[test]
    fn a_loose_file_that_cannot_be_read_still_has_a_title() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not really.mp3");
        std::fs::write(&path, b"nope").unwrap();
        let lib = library();
        let view = lib.open_folder(vec![("not really.mp3".to_owned(), path.clone())], &spec(SortColumn::TrackNo, false, ""));
        let file = view.file(0).unwrap();
        assert_eq!(file.tags().title, "not really");
        assert_eq!(file.tags().duration_sec, 0);
        // Read once: the second call is the same object.
        assert!(std::ptr::eq(file.tags(), file.tags()));
        assert_eq!(loose_path(&loose_id(&path)), Some(path.as_path()));
        assert_eq!(loose_path("12345"), None);
    }
}
