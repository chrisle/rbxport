//! Missing files: the `[!]` a track whose file is gone shows in the
//! Collection, the Missing File Manager's list, Auto Relocate and Delete.
//!
//! [OBS rekordbox 7.2.14, Windows, chris-win11 2026-10-08, issue #201]
//! File › Display All Missing Files opens the Missing File Manager: every
//! track whose file is not there (36,444 of the rig's 38,733), with a
//! "N Track" count and Auto Relocate, Relocate, Delete and OK. The browser
//! checks the working path of each row it draws
//! (`BrowseBasicView::updateMissingStatus` @0x100321010 in 7.2.19 macOS,
//! see `rbl_db::track_path`); this reads the same path, the index's
//! resolved `folder_path`.
//!
//! Auto Relocate's search is by file name alone, as rekordbox's own is
//! described: a track whose `FileNameL` turns up under a search folder is
//! pointed at the first one found, folders searched in the order given.
//! Nothing else about the file is checked — a same-named file that is a
//! different recording is the user's to notice, and Relocate is there to fix
//! it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use rbl_index::Library;

use crate::commands::{blocking, permanent_edit, reload, write_error, MAX_ROWS};
use crate::dto::{MissingTrackDto, MissingTracksDto};
use crate::error::AppResult;
use crate::commands::Touched;
use crate::state::AppState;

/// Whether a track's file is gone: it has a path and nothing is there.
///
/// An empty path is a track that never had a file, not one that lost it.
pub fn is_missing(path: &str) -> bool {
    // perf-ok: one `stat` per row drawn, which is what rekordbox's own
    // browser does; whole-library scans run under `blocking`.
    !path.is_empty() && !Path::new(path).exists()
}

/// The rows of every missing track, in the index's order.
fn scan(library: &Library) -> Vec<u32> {
    (0..library.len())
        .filter(|&index| is_missing(library.folder_path.get(index)))
        .filter_map(|index| u32::try_from(index).ok())
        .collect()
}

/// The Missing File Manager's last scan of a library.
///
/// The list can run to tens of thousands of tracks, more than one response
/// may carry, so it is paged out from here; it belongs to the library it was
/// taken of and is thrown away once that is reloaded.
pub struct MissingScan {
    library: Arc<Library>,
    rows: Arc<Vec<u32>>,
}

/// The missing rows of `library`: a fresh scan when asked for or when the
/// last one was of another library, the last one otherwise.
fn missing_rows(state: &AppState, library: &Arc<Library>, rescan: bool) -> Arc<Vec<u32>> {
    let mut held = state.missing_scan.lock();
    if !rescan {
        if let Some(scan) = held.as_ref().filter(|scan| Arc::ptr_eq(&scan.library, library)) {
            return Arc::clone(&scan.rows);
        }
    }
    let rows = Arc::new(scan(library));
    *held = Some(MissingScan { library: Arc::clone(library), rows: Arc::clone(&rows) });
    rows
}

fn missing_dto(library: &Library, row: u32) -> MissingTrackDto {
    let index = row as usize;
    MissingTrackDto {
        id: library.ids.get(index).copied().unwrap_or(0).to_string(),
        title: library.title.get(index).to_owned(),
        artist: library.artist_name(row).to_owned(),
        album: library.album_name(row).to_owned(),
        path: library.folder_path.get(index).to_owned(),
    }
}

/// A page of the Missing File Manager's list.
///
/// `rescan` checks every track's file again, as opening the manager does;
/// otherwise the page comes from the last scan.
#[tauri::command]
pub async fn missing_tracks(
    state: State<'_, Arc<AppState>>,
    offset: u32,
    limit: u32,
    rescan: bool,
) -> AppResult<MissingTracksDto> {
    let library = state.library()?;
    let state = Arc::clone(&state);
    let wanted = limit.min(MAX_ROWS) as usize;
    blocking("missing_tracks", move || {
        let rows = missing_rows(&state, &library, rescan);
        let tracks = rows
            .iter()
            .skip(offset as usize)
            .take(wanted)
            .map(|&row| missing_dto(&library, row))
            .collect();
        Ok(MissingTracksDto { total: u32::try_from(rows.len()).unwrap_or(u32::MAX), tracks })
    })
    .await
}

/// The tracks an action over missing files applies to, as `(id, file name)`:
/// the ones named that are still missing, or with none named every missing
/// track, checked afresh.
fn chosen(state: &AppState, library: &Arc<Library>, tracks: Option<&[String]>) -> Vec<(String, String)> {
    let rows: Vec<u32> = match tracks {
        None => missing_rows(state, library, true).as_ref().clone(),
        Some(ids) => ids
            .iter()
            .filter_map(|id| library.row_of(id))
            .filter(|&row| is_missing(library.folder_path.get(row as usize)))
            .collect(),
    };
    rows.into_iter()
        .filter_map(|row| {
            let index = row as usize;
            let name = file_name(library.folder_path.get(index))?;
            Some((library.ids.get(index).copied().unwrap_or(0).to_string(), name))
        })
        .collect()
}

/// What an automatic relocate did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateReportDto {
    pub relocated: u32,
    /// Missing tracks whose file name was found in none of the folders.
    pub unresolved: u32,
}

/// How deep under a search folder the walk goes. A music library is a
/// handful of levels; a folder that is a whole drive is not walked to the
/// bottom.
const MAX_DEPTH: usize = 16;
/// How many entries are looked at in all, so a search folder pointed at the
/// root of a disk ends rather than runs for minutes.
const MAX_ENTRIES: usize = 500_000;

/// Every file under `folders`, by name, the first found winning.
///
/// Walked breadth-first per folder in the order given, so a name that
/// appears twice resolves to the shallower one in the earlier folder — the
/// one somebody would point at by hand.
pub fn index_folders(folders: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut found: HashMap<String, PathBuf> = HashMap::new();
    let mut seen = 0_usize;
    for folder in folders {
        let mut level: Vec<PathBuf> = vec![folder.clone()];
        for _ in 0..MAX_DEPTH {
            let mut next = Vec::new();
            for dir in &level {
                // perf-ok: a plain function, run under `blocking` by the command below.
                let Ok(entries) = std::fs::read_dir(dir) else { continue };
                for entry in entries.flatten() {
                    seen += 1;
                    if seen > MAX_ENTRIES {
                        return found;
                    }
                    let path = entry.path();
                    let Ok(kind) = entry.file_type() else { continue };
                    if kind.is_dir() {
                        // Hidden directories are skipped: `.Trashes`, `.Spotlight-V100`
                        // and the like hold copies nobody wants pointed at.
                        if entry.file_name().to_string_lossy().starts_with('.') {
                            continue;
                        }
                        next.push(path);
                    } else if kind.is_file() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        found.entry(name).or_insert(path);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            level = next;
        }
    }
    found
}

/// The file name a library row's path ends in.
fn file_name(path: &str) -> Option<String> {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned())
}

/// Points missing tracks at same-named files under the folders: the tracks
/// named, or every missing track when none are.
#[tauri::command]
pub async fn auto_relocate<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    folders: Vec<String>,
    tracks: Option<Vec<String>>,
) -> AppResult<RelocateReportDto> {
    let library = state.library()?;
    let state_for_reload = Arc::clone(&state);
    let writing = Arc::clone(&state);
    // `blocking` is `spawn_blocking` with a name: the walk and the writes
    // happen off the async thread.
    let report = blocking("auto_relocate", move || {
        // The missing tracks first, from the index: the walk is the slow
        // part, and a library with nothing missing need not walk at all.
        let missing = chosen(&writing, &library, tracks.as_deref());
        if missing.is_empty() {
            return Ok(RelocateReportDto { relocated: 0, unresolved: 0 });
        }

        let roots: Vec<PathBuf> = folders.iter().map(PathBuf::from).collect();
        let found = index_folders(&roots);

        writing
            .write(|writer| {
                let mut relocated = 0_u32;
                let mut unresolved = 0_u32;
                for (id, name) in &missing {
                    match found.get(name) {
                        Some(path) => {
                            writer.relocate(id, path)?;
                            relocated += 1;
                        }
                        None => unresolved += 1,
                    }
                }
                Ok(RelocateReportDto { relocated, unresolved })
            })
            .map_err(write_error)
    })
    .await?;

    // Only reload if anything moved; a search that found nothing has not
    // changed the library.
    if report.relocated > 0 {
        reload(app, state_for_reload).await?;
    }
    Ok(report)
}

/// The Missing File Manager's Delete: the missing tracks named, or every
/// missing track when none are, leave the collection and every playlist, as
/// Remove from Collection does. Returns how many went.
///
/// A track whose file has come back since the list was drawn is left alone.
#[tauri::command]
pub async fn remove_missing_tracks<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    tracks: Option<Vec<String>>,
) -> AppResult<u32> {
    let library = state.library()?;
    let checking = Arc::clone(&state);
    let ids: Vec<String> = blocking("remove_missing_tracks_scan", move || {
        Ok(chosen(&checking, &library, tracks.as_deref()).into_iter().map(|(id, _)| id).collect())
    })
    .await?;
    if ids.is_empty() {
        return Ok(0);
    }
    let count = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    permanent_edit(app, state, "remove_missing_tracks", Touched::Tracks, move |w| {
        for id in &ids {
            w.delete_track(id)?;
        }
        Ok(())
    })
    .await?;
    Ok(count)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_first_folder_and_the_shallower_file_win() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        // perf-ok: a test's fixture, not a command.
        std::fs::create_dir_all(a.join("deep")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("deep/song.mp3"), b"x").unwrap();
        std::fs::write(a.join("other.mp3"), b"x").unwrap();
        std::fs::write(b.join("song.mp3"), b"y").unwrap();

        let found = index_folders(&[a.clone(), b.clone()]);
        assert_eq!(found.get("song.mp3"), Some(&a.join("deep/song.mp3")));
        assert_eq!(found.get("other.mp3"), Some(&a.join("other.mp3")));

        let found = index_folders(&[b.clone(), a.clone()]);
        assert_eq!(found.get("song.mp3"), Some(&b.join("song.mp3")));
    }

    #[test]
    fn hidden_directories_and_missing_folders_are_passed_over() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".Trashes")).unwrap();
        std::fs::write(dir.path().join(".Trashes/song.mp3"), b"x").unwrap();
        let found = index_folders(&[dir.path().to_path_buf(), dir.path().join("nowhere")]);
        assert!(found.is_empty());
    }

    #[test]
    fn a_track_is_missing_only_when_it_has_a_path_and_nothing_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("here.mp3");
        std::fs::write(&here, b"x").unwrap();
        assert!(!is_missing(here.to_str().unwrap()));
        assert!(is_missing(dir.path().join("gone.mp3").to_str().unwrap()));
        // No path is a track that never had a file, which is not this.
        assert!(!is_missing(""));
    }

    #[test]
    fn the_scan_lists_the_missing_tracks_in_index_order() {
        use rbl_index::testing::{library_from, TestTrack};
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path().join("here.mp3");
        std::fs::write(&here, b"x").unwrap();
        let here: &'static str = Box::leak(here.to_string_lossy().into_owned().into_boxed_str());
        let gone: &'static str = Box::leak(dir.path().join("gone.mp3").to_string_lossy().into_owned().into_boxed_str());
        let also: &'static str = Box::leak(dir.path().join("also.mp3").to_string_lossy().into_owned().into_boxed_str());
        let library = library_from(&[
            TestTrack { id: 1, title: "Gone", path: gone, ..TestTrack::default() },
            TestTrack { id: 2, title: "Here", path: here, ..TestTrack::default() },
            TestTrack { id: 3, title: "Never had one", path: "", ..TestTrack::default() },
            TestTrack { id: 4, title: "Also gone", album: "Lost", path: also, ..TestTrack::default() },
        ]);
        let rows = scan(&library);
        let listed: Vec<MissingTrackDto> = rows.iter().map(|&row| missing_dto(&library, row)).collect();
        let titles: Vec<&str> = listed.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["Gone", "Also gone"]);
        assert_eq!(listed[1].album, "Lost");
        assert_eq!(listed[1].path, also);
    }

    #[test]
    fn a_file_name_is_the_last_segment_of_a_row_path() {
        assert_eq!(file_name("/Users/x/Music/Track.aiff").as_deref(), Some("Track.aiff"));
        assert_eq!(file_name(""), None);
    }
}
