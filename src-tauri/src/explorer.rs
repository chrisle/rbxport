//! The Explorer: the disk as a section of the tree, a folder as a track list.
//!
//! Every read here is one directory, on a blocking thread, when asked. The
//! tree asks for a folder's subfolders when it is opened; the browser asks
//! for a folder's audio files when it is selected; and neither ever asks for
//! more than that, so a volume with a million files costs one `readdir` of
//! its top level until somebody opens something.

use std::path::PathBuf;
use std::sync::Arc;

use rbl_index::folder::{loose_id, FolderEntry, FolderView};
use rbl_index::Library;
use tauri::State;

use crate::commands::{blocking, MAX_ROWS};
use crate::dto::{ExplorerChildrenDto, ExplorerRootDto, RowDto, ViewHandleDto, ViewSpecDto};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::state::{rows_to_dto, sort_from_wire, AppState};

/// Subfolders sent for one folder: the first two thousand by name, which is
/// about 42 KB of names for the reference card's 14,503-folder `RB` [OBS]
/// and inside the response cap. A folder with more shows these and says how
/// many were left out.
const MAX_CHILDREN: usize = 2000;

/// Audio files listed for one folder. The list itself stays in Rust and is
/// paged out through `fetch_rows`, so the cap bounds the directory read and
/// the sort, not the response: the largest folder in the reference library
/// holds 2,911 tracks [OBS] and opens well inside this.
const MAX_FILES: usize = 5000;

/// How long one page may spend reading loose files' tags.
///
/// Past `interaction.fetchRowsMs` in `perf-budgets.json`, knowingly: that
/// budget is for the index, which is in memory, and a loose file's tags are
/// on a disk that may be a card in a USB reader. A page of such files costs
/// its reads once; what the budget does not reach shows its name and is read
/// on the page's next fetch.
const TAG_BUDGET: std::time::Duration = std::time::Duration::from_millis(200);

/// Where the Explorer starts: the music folder, the home folder, the system
/// volume, and every other mounted volume.
#[tauri::command]
pub async fn explorer_roots() -> AppResult<Vec<ExplorerRootDto>> {
    blocking("explorer_roots", || {
        Ok(rbl_devices::explorer::roots()
            .into_iter()
            .map(|root| ExplorerRootDto { name: root.name, path: root.path.display().to_string() })
            .collect())
    })
    .await
}

/// The folders directly under `path`, by name.
///
/// A folder that cannot be read answers with nothing rather than an error:
/// the tree shows an empty branch, which is what a folder you may not look
/// inside is.
#[tauri::command]
pub async fn explorer_children(path: String) -> AppResult<ExplorerChildrenDto> {
    blocking("explorer_children", move || {
        let listing = rbl_devices::explorer::subfolders(&PathBuf::from(path), MAX_CHILDREN);
        Ok(ExplorerChildrenDto {
            total: u32::try_from(listing.total).unwrap_or(u32::MAX),
            names: listing.entries.into_iter().map(|entry| entry.name).collect(),
        })
    })
    .await
}

/// Opens a folder as a view: its audio files, matched against the library,
/// ordered and filtered the way the spec says.
///
/// An empty path is the section heading, which lists nothing.
pub async fn open_folder(
    state: &State<'_, Arc<AppState>>,
    path: String,
    spec: &ViewSpecDto,
) -> AppResult<ViewHandleDto> {
    let library = state.library()?;
    let handle = Arc::clone(state);
    let parsed = rbl_index::ViewSpec {
        source: rbl_index::TrackSource::Collection,
        sort: sort_from_wire(&spec.sort),
        descending: spec.descending,
        query: spec.query.clone(),
        filter: rbl_index::TrackFilter::default(),
    };
    let field = spec.search_field;
    blocking("open_view", move || {
        let listing = if path.is_empty() {
            rbl_devices::explorer::Listing::default()
        } else {
            rbl_devices::explorer::audio_files(&PathBuf::from(path), MAX_FILES)
        };
        let truncated = listing.truncated();
        let files = listing.entries.into_iter().map(|entry| (entry.name, entry.path)).collect();
        let mut view = library.open_folder_scoped(files, &parsed, field);
        view.truncated = truncated;
        let (view_id, len, generation) = handle.open_folder_view(view);
        Ok(ViewHandleDto { view_id, len, gen: generation })
    })
    .await
}

/// A window of a folder view's rows.
///
/// A library row is the library's row. A loose file is a row of its own:
/// its `file:` id, its tags read here on the blocking thread for the window
/// alone — so a folder of five thousand files opens on the directory read
/// and each page pays for its own hundred probes.
pub async fn fetch_rows(
    library: Arc<Library>,
    folder: Arc<FolderView>,
    offset: u32,
    len: u32,
) -> AppResult<Vec<RowDto>> {
    if len > MAX_ROWS {
        return Err(
            AppError::new(ErrorKind::Malformed, "Too many rows requested at once.")
                .with_detail(format!("len {len} exceeds the {MAX_ROWS}-row cap")),
        );
    }
    blocking("fetch_rows", move || {
        let offset = offset as usize;
        folder.read_tags_in(offset, len as usize, TAG_BUDGET);
        Ok(folder
            .window(offset, len as usize)
            .iter()
            .enumerate()
            .map(|(at, entry)| {
                let position = offset + at;
                match *entry {
                    FolderEntry::Track(row) => rows_to_dto(&library, &[row], position)
                        .pop()
                        .unwrap_or_else(|| loose_row(position, "", "", None)),
                    FolderEntry::File(index) => match folder.file(index) {
                        Some(file) => {
                            loose_row(position, &file.name, &loose_id(&file.path), file.tags_if_read())
                        }
                        None => loose_row(position, "", "", None),
                    },
                }
            })
            .collect())
    })
    .await
}

/// The ids between two positions of a folder view, for a shift-click.
pub async fn ids_in_range(
    library: Arc<Library>,
    folder: Arc<FolderView>,
    from: u32,
    to: u32,
) -> AppResult<Vec<String>> {
    blocking("view_ids_in_range", move || {
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        let (lo, hi) = (lo as usize, (hi as usize).min(folder.len().saturating_sub(1)));
        Ok(folder
            .entries
            .get(lo..=hi)
            .unwrap_or(&[])
            .iter()
            .filter_map(|entry| match *entry {
                FolderEntry::Track(row) => library.ids.get(row as usize).map(ToString::to_string),
                FolderEntry::File(index) => folder.file(index).map(|file| loose_id(&file.path)),
            })
            .collect())
    })
    .await
}

/// A row for a file the library does not hold. Unanalysed, unrated, no
/// artwork: what there is to say about it is its name and its tags — and
/// its name alone, as the title, until the tags have been read.
fn loose_row(position: usize, name: &str, id: &str, tags: Option<&rbl_index::folder::LooseTags>) -> RowDto {
    RowDto {
        id: id.to_owned(),
        track_no: u32::try_from(position + 1).unwrap_or(u32::MAX),
        title: tags.map_or_else(
            || name.rsplit_once('.').map_or(name, |(stem, _)| stem).to_owned(),
            |t| t.title.clone(),
        ),
        artist: tags.map_or_else(String::new, |t| t.artist.clone()),
        album: tags.map_or_else(String::new, |t| t.album.clone()),
        genre: tags.map_or_else(String::new, |t| t.genre.clone()),
        label: String::new(),
        comment: String::new(),
        bpm_x100: 0,
        key: String::new(),
        duration_sec: tags.map_or(0, |t| t.duration_sec),
        rating: 0,
        analysed: 0,
        date_added: String::new(),
        release_date: String::new(),
        hot_cues: Vec::new(),
        memory_cues: Vec::new(),
        artwork_hue: 0,
        has_artwork: false,
        file_name: name.to_owned(),
        // Listed because it was just read off the disk.
        missing: false,
        extra: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    #[test]
    fn the_listing_and_the_importer_agree_on_what_counts_as_audio() {
        // `rbl-devices` lists files by extension and `rbl-db` imports them by
        // the same rule; this crate is the one that depends on both.
        assert_eq!(rbl_devices::explorer::AUDIO_EXTENSIONS, rbl_db::import::AUDIO_EXTENSIONS);
    }
}
