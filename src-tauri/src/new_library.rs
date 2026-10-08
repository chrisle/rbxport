//! Which library the window works on, the way rekordbox decides it: the
//! questions asked at startup when there is none to open, and the drive
//! list of Preferences › Advanced › Database › Database management. See
//! `rbl_db::locate` for the order and the rekordbox evidence.

use std::{path::PathBuf, sync::Arc};

use tauri::{Manager, State};

use crate::commands::blocking;
use crate::dto::{DatabaseDriveDto, LibraryProblemDto};
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
        Located::Unavailable { master_db, default_master_db } => Some(LibraryProblemDto::Unavailable {
            master_db: master_db.display().to_string(),
            default_master_db: default_master_db.display().to_string(),
        }),
    }
}

/// Makes a new, empty library in rekordbox's default folder, then loads it
/// as startup would. `library:ready` follows when it is up.
///
/// Planned again here rather than trusted from startup: a library that has
/// appeared since is loaded, never replaced, and nothing is made while the
/// configured library is on a drive that is missing.
#[tauri::command]
pub async fn create_library(app: tauri::AppHandle) -> AppResult<()> {
    blocking("create_library", move || {
        let plan = rbl_db::new_library::plan()
            .map_err(|e| AppError::new(ErrorKind::NotFound, "Could not find where the library should go.").with_detail(e.to_string()))?;
        if let Some(plan) = plan {
            let made = rbl_db::new_library::create(&plan).map_err(|e| {
                AppError::new(ErrorKind::Internal, format!("Could not make the library: {e}")).with_detail(e.to_string())
            })?;
            tracing::info!(path = %made.master_db.display(), "made a new library");
        }
        reload(app);
        Ok(())
    })
    .await
}

/// The Yes of rekordbox's "Cannot find Master Database" window, once its
/// "Location of Master Database will be changed to the default drive" is
/// confirmed: `masterDbDirectory` becomes the default folder, and that
/// folder's library is loaded, or offered to be made when there is none.
#[tauri::command]
pub async fn use_default_library(app: tauri::AppHandle) -> AppResult<()> {
    blocking("use_default_library", move || {
        rbl_db::locate::use_default().map_err(|e| {
            AppError::new(ErrorKind::Internal, "Failed to switch Master Database.").with_detail(e.to_string())
        })?;
        tracing::info!("master database set to the default drive");
        reload(app);
        Ok(())
    })
    .await
}

/// Database management's drive list: the default drive when it holds a
/// library, then every connected drive holding `PIONEER/Master/master.db`
/// or `.PIONEER/Master/master.db`, as `DetailDatabaseManagement::setup`
/// lists them. The library open now is marked. Only looks; nothing is
/// opened.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's State extractor is injected by value")]
pub async fn database_drives(state: State<'_, Arc<AppState>>) -> AppResult<Vec<DatabaseDriveDto>> {
    let current = state.location().ok().map(|location| location.master_db);
    blocking("database_drives", move || {
        let sources = rbl_db::locate::Sources::installed()
            .map_err(|e| AppError::new(ErrorKind::NotFound, "Could not read the library's location.").with_detail(e.to_string()))?;
        let mut drives = Vec::new();
        let default = sources.default_master_db();
        if default.is_file() {
            drives.push((rbl_devices::libraries::drive_name_of(&sources.default_dir), default));
        }
        drives.extend(rbl_devices::libraries::discover().into_iter().map(|found| (found.name, found.master_db)));
        // The library open now is listed even when it is neither: an
        // `options.json` from an earlier version naming another folder.
        if let Some(open) = current.as_ref().filter(|open| !drives.iter().any(|(_, master_db)| same(master_db, open))) {
            let folder = open.parent().map(PathBuf::from).unwrap_or_default();
            drives.insert(0, (rbl_devices::libraries::drive_name_of(&folder), open.clone()));
        }
        Ok(drives
            .into_iter()
            .map(|(name, master_db)| DatabaseDriveDto {
                current: current.as_ref().is_some_and(|open| same(open, &master_db)),
                name,
                master_db: master_db.display().to_string(),
            })
            .collect())
    })
    .await
}

/// Switches to the library in `master_db`, a drive from
/// [`database_drives`]: what choosing a drive in rekordbox's Database
/// management does once "Are you sure you want to switch Master Database?"
/// is confirmed. `masterDbDirectory` is set, and the application starts
/// again on the new library, so nothing from the old one is left open.
#[tauri::command]
pub async fn switch_library(app: tauri::AppHandle, master_db: String) -> AppResult<()> {
    blocking("switch_library", move || {
        let switched = rbl_db::locate::switch_to(&PathBuf::from(master_db)).map_err(|e| {
            AppError::new(ErrorKind::Internal, "Failed to switch Master Database.").with_detail(e.to_string())
        })?;
        tracing::info!(path = %switched.master_db.display(), "switched master database; restarting");
        app.restart();
    })
    .await
}

fn same(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn reload(app: tauri::AppHandle) {
    app.state::<Arc<AppState>>().set_library_problem(None);
    crate::spawn_library_load(app);
}
