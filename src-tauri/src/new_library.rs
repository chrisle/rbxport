//! Starting with no library to open: what the window asks at startup, and
//! acting on the answer — a library found on a drive, one picked by hand,
//! the default folder's (made when there is none), or trying again once a
//! drive is connected. See `rbl_db::locate` for how the library is chosen.

use std::{path::PathBuf, sync::Arc};

use tauri::{Manager, State};

use crate::commands::blocking;
use crate::dto::{DriveLibraryDto, LibraryProblemDto};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::state::AppState;

/// Why the library did not load, or nothing while it is loading or loaded.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's State extractor is injected by value")]
pub fn library_problem(state: State<'_, Arc<AppState>>) -> Option<LibraryProblemDto> {
    state.library_problem()
}

/// The problem to report when no library opened and `locate` says why:
/// `None` when there is a library, which then failed for another reason.
pub(crate) fn problem_from(located: &rbl_db::locate::Located) -> Option<LibraryProblemDto> {
    use rbl_db::locate::Located;
    match located {
        Located::Found { .. } => None,
        Located::Absent { master_db } => Some(LibraryProblemDto::Missing { master_db: master_db.display().to_string() }),
        Located::Unavailable { master_db, origin, default_master_db } => Some(LibraryProblemDto::Unavailable {
            master_db: master_db.display().to_string(),
            configured_by: *origin,
            default_master_db: default_master_db.display().to_string(),
            default_exists: default_master_db.is_file(),
        }),
    }
}

/// Uses the library in rekordbox's default folder, making a new, empty one
/// when there is none, then loads it as startup would. `library:ready`
/// follows when it is up.
///
/// Decided again here rather than trusted from startup: a library that has
/// appeared since — rekordbox installed while the question was open — is
/// loaded, never replaced, and nothing is ever made on a configured drive
/// that is missing.
#[tauri::command]
pub async fn create_library(app: tauri::AppHandle) -> AppResult<()> {
    blocking("create_library", move || {
        let used = rbl_db::new_library::use_default().map_err(|e| {
            AppError::new(ErrorKind::Internal, format!("Could not make the library: {e}")).with_detail(e.to_string())
        })?;
        tracing::info!(path = %used.master_db.display(), "using the library in the default folder");
        reload(app);
        Ok(())
    })
    .await
}

/// Opens an existing `master.db` — one found on a drive or picked by hand —
/// after proving it opens with rekordbox's key, and saves it as this
/// application's library. rekordbox's own files are not touched.
#[tauri::command]
pub async fn use_existing_library(app: tauri::AppHandle, path: String) -> AppResult<()> {
    blocking("use_existing_library", move || {
        let selected = rbl_db::new_library::use_existing(&PathBuf::from(path)).map_err(|e| {
            AppError::new(ErrorKind::NotFound, format!("Could not use that rekordbox library: {e}"))
                .with_detail(e.to_string())
        })?;
        tracing::info!(path = %selected.master_db.display(), "selected an existing library");
        reload(app);
        Ok(())
    })
    .await
}

/// The rekordbox libraries on connected drives, where rekordbox's Database
/// management looks for them. Only looks; nothing is opened.
#[tauri::command]
pub async fn discover_libraries() -> AppResult<Vec<DriveLibraryDto>> {
    blocking("discover_libraries", || {
        Ok(rbl_devices::libraries::discover()
            .into_iter()
            .map(|found| DriveLibraryDto {
                name: found.name,
                volume: found.volume.display().to_string(),
                master_db: found.master_db.display().to_string(),
            })
            .collect())
    })
    .await
}

/// Looks for the library again, for after its drive has been connected.
#[tauri::command]
pub fn retry_library(app: tauri::AppHandle) {
    reload(app);
}

fn reload(app: tauri::AppHandle) {
    app.state::<Arc<AppState>>().set_library_problem(None);
    crate::spawn_library_load(app);
}
