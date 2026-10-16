//! Software updates: what is new, fetching it, and putting it in place
//! without asking.
//!
//! The release workflow publishes `latest.json` to the download bucket in
//! the updater's own format — version, date, a signed URL per platform, and
//! the release workflow's generated release-note Markdown as the notes. The plugin fetches that, compares
//! the version with this build's and verifies the signature against the
//! public key in `tauri.conf.json`; this module turns the result into what
//! the interface shows and does what it asks.
//!
//! An update is taken in the background: the app checks on its own, and
//! what it finds is downloaded and put in place with nothing shown, so the
//! next launch is the new version. How "in place" happens depends on what
//! the plugin can do while the app runs:
//!
//! - macOS and a Linux `AppImage`: the bundle on disk is swapped straight
//!   after the download. The running process is not touched; the next
//!   launch runs the new one.
//! - Windows: the NSIS installer has to close the app to write over it, so
//!   the download is staged on disk and the installer runs silently when
//!   the app quits ([`on_exit`]). Restart Now runs it at once instead.
//! - A Linux package (`.deb`, `.rpm`): installing needs a password prompt,
//!   which is not quiet, so the download is staged and installed only from
//!   the Update Manager's Restart Now.
//! - The Microsoft Store (an MSIX package): the Store installs its updates,
//!   so nothing here downloads or installs one. The NSIS installer would put
//!   a second, separate copy next to the Store's, and Windows would go on
//!   launching the Store's older one (#189). A check says the Store keeps
//!   this copy up to date instead, without asking the download server.
//! - Nix: the package lives in the read-only Nix store and Nix installs its
//!   updates, so the built-in updater is unavailable there too (#177). The
//!   Nix launcher says so through [`NIX_ENV`]; a check names the Nix channel
//!   as what updates this copy, without asking the download server, and
//!   nothing is downloaded or installed.
//!
//! What has changed is worked out here, not in the interface: the notes are
//! the full published release history, and the part that matters is the sections newer
//! than the version running and no newer than the one on offer — a user
//! two releases behind should read both.
//!
//! The download reports through one event, `update:progress`, at most about
//! ten times a second: a chunk arrives every few kilobytes, and an event per
//! chunk would be IPC for nothing.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use semver::Version;
use serde::Serialize;
use tauri::utils::config::BundleType;
use tauri::utils::platform::bundle_type;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::elevated_update;
use crate::error::{AppError, AppResult, ErrorKind};

/// The event a download's progress goes out on.
pub const PROGRESS_EVENT: &str = "update:progress";

/// The least time between two progress events.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Where a downloaded update stands.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Placement {
    /// On disk in the app's place; a restart runs it.
    Installed,
    /// An installer at this path, to run when the app quits (Windows) or on
    /// request (a Linux package).
    Staged(PathBuf),
}

/// One update as the check found it and as far as it has got.
struct Pending {
    /// The plugin's `Update` carries the URL, the signature and the headers
    /// the check was made with; the download must use that same one rather
    /// than check again, or the version taken could be a different one from
    /// the version shown.
    update: Update,
    placement: Option<Placement>,
}

/// The update the last check found, held across the download and whatever
/// follows it.
#[derive(Default)]
pub struct Updates {
    pending: Mutex<Option<Pending>>,
    /// One download at a time: a second request while the first runs would
    /// fetch the installer twice and put it in place twice.
    busy: AtomicBool,
    /// Why the last update, run after the app quit, did not install: what
    /// the protected Windows helper left behind. The next check shows it once.
    previous_failure: Mutex<Option<String>>,
}

impl Updates {
    /// Where a staged installer is kept between the download and the quit.
    fn staging_dir(app: &AppHandle) -> Option<PathBuf> {
        if let Some(dir) = elevated_update::shared_staging_dir().filter(|dir| dir.is_dir()) {
            return Some(dir);
        }
        app.path().app_cache_dir().ok().map(|dir| dir.join("update"))
    }

    /// Clears what an earlier run staged and did not use — a crash, or an
    /// installer the quit already ran. The next check downloads afresh, which
    /// is the safe side: the signature was checked on the bytes that came
    /// down, not on a file that has sat in a cache since.
    ///
    /// What the protected Windows helper said about the last update is read
    /// first: it goes to the log and to the next check (#457).
    pub fn clear_stale(app: &AppHandle) {
        let outcome = elevated_update::take_outcome();
        if let Some(failure) = previous_failure(&outcome) {
            if let Some(updates) = app.try_state::<std::sync::Arc<Updates>>() {
                *updates.previous_failure.lock() = Some(failure);
            }
        }
        if let Some(dir) = Self::staging_dir(app) {
            if elevated_update::shared_staging_dir().as_deref() != Some(dir.as_path()) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }
}

/// Logs how the last protected update ended and returns the failure the next
/// check should show, if it failed.
fn previous_failure(outcome: &elevated_update::Outcome) -> Option<String> {
    if let Some(version) = &outcome.installed {
        tracing::info!(%version, "the protected update helper installed the update");
    }
    let reason = outcome.failure.as_ref()?;
    tracing::warn!(error = %reason, "the last update could not be installed");
    Some(format!("The last update could not be installed: {reason}"))
}

/// Whether the bundle this process runs from can be swapped on disk while
/// it runs, so an update is in place the moment it is downloaded.
fn swaps_in_place() -> bool {
    matches!(bundle_type(), Some(BundleType::App | BundleType::AppImage))
}

/// `GetCurrentPackageFullName` results, as `winerror.h` numbers them. Kept
/// here rather than taken from `windows-sys` so the decision below is the
/// same code, and tested, on every platform.
#[cfg(any(windows, test))]
const ERROR_SUCCESS: u32 = 0;
#[cfg(any(windows, test))]
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
#[cfg(test)]
const APPMODEL_ERROR_NO_PACKAGE: u32 = 15_700;

/// Whether a `GetCurrentPackageFullName` result means the process runs with
/// a package identity: installed from an MSIX package, which on Windows
/// rbxport only ships through the Microsoft Store.
///
/// Asked with an empty buffer, a packaged process gets
/// `ERROR_INSUFFICIENT_BUFFER` (the name does not fit) and an unpackaged one
/// `APPMODEL_ERROR_NO_PACKAGE`. Any other answer is not a package identity
/// Windows vouched for, and the app keeps its own updater.
#[cfg(any(windows, test))]
fn has_package_identity(status: u32) -> bool {
    matches!(status, ERROR_SUCCESS | ERROR_INSUFFICIENT_BUFFER)
}

/// Whether this process was installed by the Microsoft Store, which then
/// owns its updates.
#[cfg(windows)]
#[allow(unsafe_code)]
fn store_install() -> bool {
    use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
    let mut length: u32 = 0;
    // SAFETY: a zero length with a null buffer is the documented way to ask
    // only whether there is a package name and how long it is; nothing is
    // written through the null pointer.
    let status = unsafe { GetCurrentPackageFullName(&raw mut length, std::ptr::null_mut()) };
    has_package_identity(status)
}

/// Only Windows has Store packages.
#[cfg(not(windows))]
fn store_install() -> bool {
    false
}

fn store_refusal() -> AppError {
    AppError::new(ErrorKind::Internal, "The Microsoft Store installs this copy of rbxport's updates.")
}

/// The variable the Nix launcher (`packaging/nix/bin.nix`) sets, to `nix`,
/// on the app it starts: the package manager that owns this copy's updates.
pub const NIX_ENV: &str = "RBXPORT_PACKAGE_MANAGER";

/// The file the first Nix launcher (`bin.nix` before #177) wrote beside the
/// writable `AppImage` copy it seeded and let the built-in updater replace.
const NIX_SEED_MARKER: &str = ".nix-seed-version";

/// Whether this copy was installed through Nix, which then owns its updates.
///
/// Any one of three facts says so, taken as arguments to keep the decision
/// testable:
/// - the launcher's marker, [`NIX_ENV`] set to `nix`;
/// - an `AppImage` path inside the Nix store, which nothing but Nix writes;
/// - an `AppImage` with the first Nix launcher's seed marker beside it. That
///   launcher ran a writable copy in the data directory and set no marker, so
///   without this a copy it had already updated would go on updating itself
///   outside Nix.
fn installed_by_nix(marker: Option<&OsStr>, appimage: Option<&Path>, seeded: impl Fn(&Path) -> bool) -> bool {
    if marker.is_some_and(|value| value == "nix") {
        return true;
    }
    let Some(appimage) = appimage else { return false };
    appimage.starts_with("/nix/store") || appimage.parent().is_some_and(|dir| seeded(&dir.join(NIX_SEED_MARKER)))
}

/// Whether this process was installed through Nix. Nix packages rbxport for
/// Linux only.
fn nix_install() -> bool {
    cfg!(target_os = "linux")
        && installed_by_nix(
            std::env::var_os(NIX_ENV).as_deref(),
            std::env::var_os("APPIMAGE").as_deref().map(Path::new),
            Path::exists,
        )
}

fn nix_refusal() -> AppError {
    AppError::new(ErrorKind::Internal, "Nix installs this copy of rbxport's updates.")
}

/// The refusal for a copy whose updates a package manager or store installs,
/// if this is one.
fn managed_refusal() -> Option<AppError> {
    if store_install() {
        Some(store_refusal())
    } else if nix_install() {
        Some(nix_refusal())
    } else {
        None
    }
}

/// Why this build cannot replace itself, if it cannot, mirroring the refusal
/// the interface already shows for a development build.
///
/// A release build whose bundle marker is absent is one a distribution or
/// package manager installed (a Linux source build, `.deb`/`.rpm`, or a Nix
/// store build): it cannot write its own executable, so it must say so rather
/// than download and fail on every launch. Taking the two facts as arguments
/// keeps the decision testable.
fn self_update_refusal(debug: bool, bundle: Option<&BundleType>) -> Option<AppError> {
    if debug {
        return Some(AppError::new(
            ErrorKind::Internal,
            "A development build cannot be updated in place.",
        ));
    }
    // `cfg!` rather than an attribute keeps `bundle` used on every platform,
    // where the workspace denies warnings.
    if cfg!(target_os = "linux") && bundle.is_none() {
        return Some(AppError::new(
            ErrorKind::Internal,
            "This build does not manage its own updates; use the package manager it was installed with.",
        ));
    }
    None
}

/// One release's entry in the published release notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeDto {
    pub version: String,
    /// As the release notes write it, `2026-09-10`, when the heading has one.
    pub date: Option<String>,
    /// The section's markdown, headings included.
    pub body: String,
}

/// What a check found.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckDto {
    pub current_version: String,
    /// The version on offer, or `None` when this build is the newest.
    pub version: Option<String>,
    /// RFC 3339, from the feed's `pub_date`.
    pub date: Option<String>,
    /// The release-note sections between the two versions, newest first.
    pub changes: Vec<ChangeDto>,
    /// The version on offer is already downloaded this run: in place, or
    /// staged for the quit. Nothing to fetch again.
    pub ready: Option<UpdateReadyDto>,
    /// This copy came from the Microsoft Store, which installs its updates;
    /// the check did not ask the download server, and `version` is `None`.
    pub store_install: bool,
    /// This copy was installed through Nix, whose channel brings its
    /// updates; the check did not ask the download server, and `version` is
    /// `None`.
    pub nix_install: bool,
    /// Why the last update, installed after the app quit, failed, when it
    /// did. Given once, by the first check after the launch that found it.
    pub previous_failure: Option<String>,
}

/// A downloaded update, and whether it is already in the app's place.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReadyDto {
    pub version: String,
    /// `true` when the next launch runs it as things stand; `false` when
    /// an installer still has to run, at the quit or on request.
    pub installed: bool,
}

/// How far a download has got.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub downloaded: u64,
    /// `None` when the server did not say how big the file is.
    pub total: Option<u64>,
}

/// An updater failure, as the interface shows it and as the log keeps it.
///
/// The log is what a bug report carries, and the plugin's own messages are
/// below the level it keeps for dependencies, so without this line a report
/// of an update that would not install says nothing about why (#457): not
/// whether a password prompt was refused, a package manager failed, or a
/// file could not be moved.
fn updater_error(message: &str, err: impl std::fmt::Display) -> AppError {
    let detail = err.to_string();
    tracing::warn!(error = %detail, bundle = bundle_name(bundle_type().as_ref()), "{message}");
    AppError::new(ErrorKind::Internal, message).with_detail(detail)
}

/// The bundle this process runs from, as a log field: the updater takes a
/// different route for each one.
fn bundle_name(bundle: Option<&BundleType>) -> &'static str {
    match bundle {
        Some(BundleType::App) => "app",
        Some(BundleType::Dmg) => "dmg",
        Some(BundleType::AppImage) => "appimage",
        Some(BundleType::Deb) => "deb",
        Some(BundleType::Rpm) => "rpm",
        Some(BundleType::Nsis) => "nsis",
        Some(BundleType::Msi) => "msi",
        None => "none",
    }
}

/// Asks the bucket what the newest version is.
///
/// Network I/O, so it is awaited on the plugin's own client rather than run
/// on the command's thread. A build that cannot reach the bucket reports
/// that plainly rather than as "up to date".
#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    updates: tauri::State<'_, std::sync::Arc<Updates>>,
) -> AppResult<UpdateCheckDto> {
    let current_version = app.package_info().version.to_string();
    if store_install() {
        *updates.pending.lock() = None;
        return Ok(store_check(current_version));
    }
    if nix_install() {
        *updates.pending.lock() = None;
        return Ok(nix_check(current_version));
    }
    let found = app
        .updater()
        .map_err(|e| updater_error("The updater is not configured.", e))?
        .check()
        .await
        .map_err(|e| updater_error("The update check could not reach the download server.", e))?;

    let Some(update) = found else {
        *updates.pending.lock() = None;
        return Ok(UpdateCheckDto {
            current_version,
            version: None,
            date: None,
            changes: Vec::new(),
            ready: None,
            store_install: false,
            nix_install: false,
            previous_failure: updates.previous_failure.lock().take(),
        });
    };

    let changes = match (Version::parse(&update.current_version), Version::parse(&update.version)) {
        (Ok(current), Ok(target)) => {
            changes_between(update.body.as_deref().unwrap_or(""), &current, &target)
        }
        // A version that does not parse is not something to refuse an update
        // over; the notes just cannot be narrowed.
        _ => Vec::new(),
    };
    let date = update.date.and_then(|d| {
        d.format(&time::format_description::well_known::Rfc3339).ok()
    });
    // A check that finds the version already taken this run — a manual check
    // after the automatic one did its work — keeps that rather than fetching
    // it again.
    let mut pending = updates.pending.lock();
    let placement = pending
        .as_ref()
        .filter(|p| p.update.version == update.version)
        .and_then(|p| p.placement.clone());
    let ready = placement.as_ref().map(|p| UpdateReadyDto {
        version: update.version.clone(),
        installed: *p == Placement::Installed,
    });
    let dto = UpdateCheckDto {
        current_version,
        version: Some(update.version.clone()),
        date,
        changes,
        ready,
        store_install: false,
        nix_install: false,
        previous_failure: updates.previous_failure.lock().take(),
    };
    *pending = Some(Pending { update, placement });
    Ok(dto)
}

/// What a check answers in a Microsoft Store install: nothing on offer from
/// the download server, and the Store named as what updates this copy.
fn store_check(current_version: String) -> UpdateCheckDto {
    UpdateCheckDto {
        current_version,
        version: None,
        date: None,
        changes: Vec::new(),
        ready: None,
        store_install: true,
        nix_install: false,
        previous_failure: None,
    }
}

/// What a check answers in a Nix install: nothing on offer from the download
/// server, and the Nix channel named as what updates this copy.
fn nix_check(current_version: String) -> UpdateCheckDto {
    UpdateCheckDto {
        current_version,
        version: None,
        date: None,
        changes: Vec::new(),
        ready: None,
        store_install: false,
        nix_install: true,
        previous_failure: None,
    }
}

/// The update already downloaded in this run, without a network check.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's State extractor is injected by value")]
pub fn ready_update(updates: tauri::State<'_, std::sync::Arc<Updates>>) -> Option<UpdateReadyDto> {
    updates.pending.lock().as_ref().and_then(|pending| {
        pending.placement.as_ref().map(|placement| UpdateReadyDto {
            version: pending.update.version.clone(),
            installed: *placement == Placement::Installed,
        })
    })
}

/// Downloads the update the last check found and puts it in place.
///
/// Progress goes out as `update:progress`. Where the bundle can be swapped
/// while the app runs (macOS, an `AppImage`) the download is installed at
/// once and the next launch runs it; elsewhere the installer is staged on
/// disk for [`on_exit`] or [`restart_to_update`]. Returns what happened, or
/// what already had when the same version was downloaded earlier this run.
#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    updates: tauri::State<'_, std::sync::Arc<Updates>>,
) -> AppResult<UpdateReadyDto> {
    if let Some(refusal) = managed_refusal() {
        return Err(refusal);
    }
    if let Some(refusal) = self_update_refusal(cfg!(debug_assertions), bundle_type().as_ref()) {
        return Err(refusal);
    }
    let (update, placement) = {
        let pending = updates.pending.lock();
        let Some(pending) = pending.as_ref() else {
            return Err(AppError::new(
                ErrorKind::NotFound,
                "There is no update to download — check for updates first.",
            ));
        };
        (pending.update.clone(), pending.placement.clone())
    };
    if let Some(placement) = placement {
        return Ok(UpdateReadyDto { version: update.version, installed: placement == Placement::Installed });
    }
    if updates.busy.swap(true, Ordering::SeqCst) {
        return Err(AppError::new(ErrorKind::Internal, "The update is already downloading."));
    }

    let handle = app.clone();
    // Shared by the two callbacks below, which is why it is an atomic and
    // not a `u64` they would both need to borrow.
    let downloaded = AtomicU64::new(0);
    // `None` until the first chunk, so the first event goes out at once.
    let mut last_sent: Option<Instant> = None;
    let bytes = update
        .download(
            |chunk, total| {
                let so_far = downloaded.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                let now = Instant::now();
                if last_sent.is_some_and(|at| now.duration_since(at) < PROGRESS_EVERY) {
                    return;
                }
                last_sent = Some(now);
                let _ = handle.emit(PROGRESS_EVENT, ProgressDto { downloaded: so_far, total });
            },
            || {
                // The last event carries the full size, so a bar that missed
                // the final chunk still reaches its end.
                let all = downloaded.load(Ordering::Relaxed);
                let _ = app.emit(PROGRESS_EVENT, ProgressDto { downloaded: all, total: Some(all) });
            },
        )
        .await;
    let placed = match bytes {
        Ok(bytes) => place(&app, &update, bytes).await,
        Err(e) => Err(updater_error("The update could not be downloaded.", e)),
    };
    updates.busy.store(false, Ordering::SeqCst);
    let placement = placed?;

    let installed = placement == Placement::Installed;
    if let Some(pending) = updates.pending.lock().as_mut() {
        if pending.update.version == update.version {
            pending.placement = Some(placement);
        }
    }
    Ok(UpdateReadyDto { version: update.version, installed })
}

/// Puts downloaded bytes where the next launch, the quit or a request finds
/// them. The install itself is file I/O the plugin does synchronously, so
/// it runs on a blocking worker rather than the async runtime.
async fn place(app: &AppHandle, update: &Update, bytes: Vec<u8>) -> AppResult<Placement> {
    if swaps_in_place() {
        let update = update.clone();
        tauri::async_runtime::spawn_blocking(move || update.install(bytes))
            .await
            .map_err(|e| updater_error("The update could not be installed.", e))?
            .map_err(|e| updater_error("The update could not be installed.", e))?;
        return Ok(Placement::Installed);
    }
    let dir = Updates::staging_dir(app)
        .ok_or_else(|| AppError::new(ErrorKind::Internal, "There is nowhere to keep the update."))?;
    let shared = elevated_update::shared_staging_dir().as_deref() == Some(dir.as_path());
    // Preserve the installer-created ACL on the shared directory. Only its
    // known candidate files are disposable; the fallback cache directory can
    // still be replaced wholesale.
    if shared {
        elevated_update::clear_shared_staging();
    } else {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| updater_error("The update could not be kept.", e))?;
    }
    let path = if shared {
        dir.join("pending.update")
    } else {
        dir.join(format!("{}.update", update.version))
    };
    std::fs::write(&path, &bytes).map_err(|e| updater_error("The update could not be kept.", e))?;
    elevated_update::stage(&path, &update.version, &update.signature)
        .map_err(|e| updater_error("The update could not be kept.", e))?;
    Ok(Placement::Staged(path))
}

/// Runs the update that is ready: restarts into an installed one, or runs a
/// staged installer, which relaunches the app itself when it is done.
///
/// A success never returns — the process ends either way. Returning is
/// failure, and the interface treats it as one.
#[tauri::command]
pub async fn restart_to_update(
    app: AppHandle,
    updates: tauri::State<'_, std::sync::Arc<Updates>>,
) -> AppResult<()> {
    if let Some(refusal) = managed_refusal() {
        return Err(refusal);
    }
    let (update, placement) = {
        let pending = updates.pending.lock();
        let Some(pending) = pending.as_ref() else {
            return Err(AppError::new(ErrorKind::NotFound, "There is no update to restart into."));
        };
        match pending.placement.clone() {
            Some(placement) => (pending.update.clone(), placement),
            None => {
                return Err(AppError::new(ErrorKind::NotFound, "The update has not been downloaded yet."));
            }
        }
    };
    // Which route the restart takes, so a report of one that did not come
    // back says whether an installer still had to run (and ask for a
    // password) or the new version was already on disk.
    tracing::info!(
        version = %update.version,
        bundle = bundle_name(bundle_type().as_ref()),
        staged = matches!(placement, Placement::Staged(_)),
        "restarting into the update"
    );
    match placement {
        Placement::Installed => app.restart(),
        Placement::Staged(path) => {
            // Taken out first: `install` ends the process on Windows, and the
            // quit that follows must not run the same installer again.
            if let Some(pending) = updates.pending.lock().as_mut() {
                pending.placement = None;
            }
            // The protected task (Windows): quit only once its helper has
            // accepted the update. Starting the task proves nothing more, and
            // quitting on that alone left the app gone and nothing installed
            // whenever the helper refused (#457).
            let launch_path = path.clone();
            let handoff = tauri::async_runtime::spawn_blocking(move || elevated_update::launch(&launch_path, true))
                .await
                .map_err(|e| updater_error("The update could not be installed.", e))?;
            match handoff {
                elevated_update::Handoff::TakenOver => std::process::exit(0),
                elevated_update::Handoff::NotShared => {}
                elevated_update::Handoff::Unavailable(error) => {
                    tracing::warn!(%error, "the protected update task could not start");
                }
                elevated_update::Handoff::Refused(reason) => {
                    // These bytes failed the helper's checks; the ordinary
                    // installer must not run them either.
                    return Err(updater_error("The update could not be installed.", reason));
                }
            }
            let bytes = std::fs::read(&path).map_err(|e| updater_error("The update could not be read back.", e))?;
            // On Windows this spawns the installer and ends the process; a
            // Linux package is installed in place and the process is still
            // here, so a restart runs it.
            let to_install = update.clone();
            let outcome = tauri::async_runtime::spawn_blocking(move || to_install.install(bytes))
                .await
                .map_err(|e| updater_error("The update could not be installed.", e))?
                .map_err(|e| updater_error("The update could not be installed.", e));
            if let Err(error) = outcome {
                // Still downloaded: a refused password prompt (Linux) can be
                // answered by pressing Restart Now again, without a second
                // download. Windows keeps it out, so the quit does not ask for
                // the same administrator consent that was just refused.
                if !cfg!(windows) {
                    keep_staged(&updates, &update.version, path);
                }
                return Err(error);
            }
            app.restart()
        }
    }
}

/// Puts a staged installer back as the pending update's placement after an
/// install that did not happen, unless a newer check replaced the update.
fn keep_staged(updates: &Updates, version: &str, path: PathBuf) {
    if let Some(pending) = updates.pending.lock().as_mut() {
        if pending.update.version == version && pending.placement.is_none() {
            pending.placement = Some(Placement::Staged(path));
        }
    }
}

/// The quit: a staged installer runs now, silently, and does not bring the
/// app back — the person closed it. Only Windows stages an installer the
/// quit can run; a Linux package would prompt for a password, and stays for
/// [`restart_to_update`].
pub fn on_exit(app: &AppHandle) {
    if !cfg!(windows) || store_install() {
        return;
    }
    let Some(updates) = app.try_state::<std::sync::Arc<Updates>>() else { return };
    let staged = {
        let mut pending = updates.pending.lock();
        match pending.as_mut() {
            Some(p) => match p.placement.take() {
                Some(Placement::Staged(path)) => Some((p.update.clone(), path)),
                _ => None,
            },
            None => None,
        }
    };
    let Some((update, path)) = staged else { return };
    match elevated_update::launch(&path, false) {
        elevated_update::Handoff::TakenOver => return,
        elevated_update::Handoff::NotShared => {}
        elevated_update::Handoff::Unavailable(error) => {
            tracing::warn!(%error, "the protected update task could not start at quit");
        }
        elevated_update::Handoff::Refused(reason) => {
            tracing::warn!(error = %reason, "the protected update helper refused the update at quit");
            return;
        }
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            // `install` spawns the installer and ends this process itself.
            if let Err(e) = update.restart_after_install(false).install(bytes) {
                tracing::warn!(error = %e, "the staged update could not be installed at quit");
            }
        }
        Err(e) => tracing::warn!(error = %e, "the staged update could not be read back at quit"),
    }
}

/// Returns the process exit code when Windows started this binary as the
/// protected update helper, or `None` for an ordinary application launch.
pub fn run_elevated_helper_if_requested() -> Option<i32> {
    elevated_update::run_if_requested()
}

/// The release-note sections newer than `current` and no newer than `target`,
/// newest first — what somebody on `current` gets by taking `target`.
///
/// A section starts at a `## [x.y.z]` heading and runs to the next one; the
/// link definitions at the foot of the file (`[x.y.z]: https://…`) are not
/// part of any release and are dropped.
pub fn changes_between(changelog: &str, current: &Version, target: &Version) -> Vec<ChangeDto> {
    let mut sections: Vec<ChangeDto> = Vec::new();
    let mut open: Option<(Version, ChangeDto, Vec<String>)> = None;

    let close = |open: Option<(Version, ChangeDto, Vec<String>)>, out: &mut Vec<ChangeDto>| {
        if let Some((version, mut change, lines)) = open {
            if version > *current && version <= *target {
                lines.join("\n").trim().clone_into(&mut change.body);
                out.push(change);
            }
        }
    };

    for line in changelog.lines() {
        if let Some((version, date)) = release_heading(line) {
            close(open.take(), &mut sections);
            open = Some((
                version.clone(),
                ChangeDto { version: version.to_string(), date, body: String::new() },
                vec![line.to_owned()],
            ));
            continue;
        }
        if is_link_definition(line) {
            continue;
        }
        if let Some((_, _, lines)) = open.as_mut() {
            lines.push(line.to_owned());
        }
    }
    close(open.take(), &mut sections);
    sections
}

/// `## [0.4.0] — 2026-09-10` → the version and the date, if the line is one.
fn release_heading(line: &str) -> Option<(Version, Option<String>)> {
    let rest = line.strip_prefix("## [")?;
    let (version, after) = rest.split_once(']')?;
    let version = Version::parse(version.trim()).ok()?;
    Some((version, first_date(after)))
}

/// The first `YYYY-MM-DD` in the text, whatever is around it: "— 2026-09-10
/// — not published" has one, and the remark is left to the body's heading.
fn first_date(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(9))
        .map(|i| &bytes[i..i + 10])
        .find(|w| {
            w.iter().enumerate().all(|(j, c)| {
                if j == 4 || j == 7 { *c == b'-' } else { c.is_ascii_digit() }
            })
        })
        .and_then(|w| std::str::from_utf8(w).ok())
        .map(str::to_owned)
}

fn is_link_definition(line: &str) -> bool {
    line.starts_with('[') && line.contains("]: ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\nWhat changed.\n\n\
## [0.4.0] — 2026-09-10\n\n### Added\n- A limiter.\n\n\
## [0.3.0] — 2026-09-10 — not published\n\nSee 0.4.0.\n\n\
## [0.2.0] — 2026-09-09\n\n### Added\n- Hot cues.\n\n\
## [0.1.0] — 2026-09-09\n\nThe first build.\n\n\
[0.4.0]: https://example.com/compare/v0.3.0...v0.4.0\n\
[0.1.0]: https://example.com/releases/tag/v0.1.0\n";

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// What the log keeps while `f` runs, as plain text.
    fn logged(f: impl FnOnce()) -> String {
        #[derive(Clone, Default)]
        struct Buffer(std::sync::Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        let bytes = buffer.0.lock().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn an_update_that_will_not_install_says_why_in_the_log() {
        // #457: a report of an update that asked for a password and then did
        // not install carried nothing about the failure, because the error
        // went only to the window and the plugin logs below the kept level.
        let mut error = None;
        let log = logged(|| {
            error = Some(updater_error("The update could not be installed.", "Failed to move the new app into place"));
        });
        let error = error.unwrap();
        assert_eq!(error.message, "The update could not be installed.");
        assert_eq!(error.detail.as_deref(), Some("Failed to move the new app into place"));
        assert!(log.contains("WARN"), "not a warning: {log}");
        assert!(log.contains("The update could not be installed."), "the message is missing: {log}");
        assert!(log.contains("Failed to move the new app into place"), "the plugin's reason is missing: {log}");
        assert!(log.contains("bundle="), "the bundle the updater took a route for is missing: {log}");
    }

    #[test]
    fn the_helpers_reason_reaches_the_log_and_the_next_check() {
        // #457: on Windows the protected helper's reason was written to
        // last-error.txt and deleted unread at the next launch.
        let failed = elevated_update::Outcome {
            failure: Some("rbxport did not exit before the update timeout".to_owned()),
            installed: None,
        };
        let mut shown = None;
        let log = logged(|| shown = previous_failure(&failed));
        assert_eq!(
            shown.as_deref(),
            Some("The last update could not be installed: rbxport did not exit before the update timeout")
        );
        assert!(log.contains("WARN"), "not a warning: {log}");
        assert!(log.contains("rbxport did not exit before the update timeout"), "the reason is missing: {log}");

        let installed = elevated_update::Outcome { failure: None, installed: Some("2.0.0".to_owned()) };
        let mut shown = Some(String::new());
        let log = logged(|| shown = previous_failure(&installed));
        assert_eq!(shown, None);
        assert!(log.contains("installed the update") && log.contains("2.0.0"), "{log}");
        assert_eq!(previous_failure(&elevated_update::Outcome::default()), None);
    }

    #[test]
    fn every_bundle_has_its_own_log_name() {
        let all = [
            BundleType::App,
            BundleType::Dmg,
            BundleType::AppImage,
            BundleType::Deb,
            BundleType::Rpm,
            BundleType::Nsis,
            BundleType::Msi,
        ];
        let mut names: Vec<&str> = all.iter().map(|b| bundle_name(Some(b))).collect();
        names.push(bundle_name(None));
        assert_eq!(names, ["app", "dmg", "appimage", "deb", "rpm", "nsis", "msi", "none"]);
    }

    #[test]
    fn a_packaged_process_is_a_store_install_and_an_unpackaged_one_is_not() {
        // GetCurrentPackageFullName with an empty buffer: a package name
        // that does not fit means there is one.
        assert!(has_package_identity(ERROR_INSUFFICIENT_BUFFER));
        assert!(has_package_identity(ERROR_SUCCESS));
        // The NSIS install, a portable copy, a development run.
        assert!(!has_package_identity(APPMODEL_ERROR_NO_PACKAGE));
        // Anything else is not an identity Windows vouched for.
        assert!(!has_package_identity(87), "ERROR_INVALID_PARAMETER");
    }

    #[test]
    fn a_store_check_offers_nothing_and_names_the_store() {
        let dto = store_check("1.2.0".to_owned());
        assert!(dto.store_install);
        assert_eq!(dto.version, None);
        assert!(dto.ready.is_none() && dto.changes.is_empty());
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["storeInstall"], serde_json::Value::Bool(true));
        assert_eq!(json["currentVersion"], "1.2.0");
    }

    #[test]
    fn a_nix_check_offers_nothing_and_names_nix() {
        let dto = nix_check("1.3.0".to_owned());
        assert!(dto.nix_install && !dto.store_install);
        assert_eq!(dto.version, None);
        assert!(dto.ready.is_none() && dto.changes.is_empty());
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["nixInstall"], serde_json::Value::Bool(true));
        assert_eq!(json["storeInstall"], serde_json::Value::Bool(false));
    }

    #[test]
    fn the_nix_launcher_marker_makes_a_nix_install() {
        let never = |_: &Path| false;
        assert!(installed_by_nix(Some(OsStr::new("nix")), None, never));
        // The launcher runs the AppImage extracted from the store, with
        // APPIMAGE naming the store copy.
        assert!(installed_by_nix(
            Some(OsStr::new("nix")),
            Some(Path::new("/home/dj/.cache/rbxport/x/rbxport.AppImage")),
            never
        ));
        assert!(!installed_by_nix(Some(OsStr::new("")), None, never));
        assert!(!installed_by_nix(Some(OsStr::new("flatpak")), None, never));
    }

    #[test]
    fn an_appimage_in_the_nix_store_is_a_nix_install() {
        let never = |_: &Path| false;
        let store = Path::new("/nix/store/abc-rbxport-bin-1.3.0/share/rbxport/rbxport.AppImage");
        assert!(installed_by_nix(None, Some(store), never));
        // A path that only starts with the same letters is not the store.
        assert!(!installed_by_nix(None, Some(Path::new("/nix/storefront/rbxport.AppImage")), never));
    }

    #[test]
    fn a_copy_seeded_by_the_first_nix_launcher_is_a_nix_install() {
        // The first Nix launcher copied the AppImage into the data directory, wrote
        // this marker beside it and set no variable. A copy it seeded must not
        // go on replacing itself once it is running this build.
        let seeded = Path::new("/home/dj/.local/share/rbxport/rbxport.AppImage");
        let marker = Path::new("/home/dj/.local/share/rbxport/.nix-seed-version");
        assert!(installed_by_nix(None, Some(seeded), |path: &Path| path == marker));
    }

    #[test]
    fn a_downloaded_appimage_is_not_a_nix_install_and_keeps_its_updater() {
        // The AppImage a person downloads and runs: no marker, outside the
        // store, nothing seeded beside it.
        let downloaded = Path::new("/home/dj/Applications/rbxport-1.3.0-linux-x86_64.AppImage");
        assert!(!installed_by_nix(None, Some(downloaded), |_: &Path| false));
        assert!(!installed_by_nix(None, None, |_: &Path| false));
        // Without the Nix launcher's variable, this test process is not one.
        if std::env::var_os(NIX_ENV).is_none() && std::env::var_os("APPIMAGE").is_none() {
            assert!(!nix_install());
            assert!(managed_refusal().is_none() || cfg!(windows));
        }
        // And an AppImage build may still replace itself.
        assert!(self_update_refusal(false, Some(&BundleType::AppImage)).is_none());
    }

    #[test]
    fn the_nix_launcher_sets_the_marker_the_app_reads_and_seeds_no_writable_copy() {
        let launcher = include_str!("../../packaging/nix/bin.nix");
        assert!(
            launcher.contains(&format!("export {NIX_ENV}=nix")),
            "bin.nix must tell the app that Nix owns its updates"
        );
        // The writable copy is what let the built-in updater update a Nix
        // install outside Nix (#177).
        assert!(!launcher.contains("chmod u+w"), "bin.nix must not seed a writable AppImage");
    }

    #[test]
    fn this_platform_is_not_a_store_install_off_windows() {
        if !cfg!(windows) {
            assert!(!store_install());
        }
    }

    #[test]
    fn only_the_sections_between_the_two_versions_are_kept_newest_first() {
        let changes = changes_between(LOG, &v("0.2.0"), &v("0.4.0"));
        let versions: Vec<&str> = changes.iter().map(|c| c.version.as_str()).collect();
        assert_eq!(versions, ["0.4.0", "0.3.0"]);
    }

    #[test]
    fn a_section_carries_its_heading_date_and_body_without_the_link_foot() {
        let changes = changes_between(LOG, &v("0.0.1"), &v("0.1.0"));
        assert_eq!(changes.len(), 1);
        let first = &changes[0];
        assert_eq!(first.date.as_deref(), Some("2026-09-09"));
        assert_eq!(first.body, "## [0.1.0] — 2026-09-09\n\nThe first build.");
    }

    #[test]
    fn the_date_survives_a_remark_after_it() {
        let changes = changes_between(LOG, &v("0.2.0"), &v("0.3.0"));
        assert_eq!(changes[0].date.as_deref(), Some("2026-09-10"));
        assert!(changes[0].body.starts_with("## [0.3.0] — 2026-09-10 — not published"));
    }

    #[test]
    fn up_to_date_or_ahead_gives_nothing() {
        assert_eq!(changes_between(LOG, &v("0.4.0"), &v("0.4.0")), [] as [ChangeDto; 0]);
        assert_eq!(changes_between(LOG, &v("0.5.0"), &v("0.4.0")), [] as [ChangeDto; 0]);
        assert_eq!(changes_between("", &v("0.1.0"), &v("0.4.0")), [] as [ChangeDto; 0]);
    }

    #[test]
    fn a_heading_without_a_date_is_a_release_with_none() {
        let (version, date) = release_heading("## [0.1.0]").unwrap();
        assert_eq!(version, v("0.1.0"));
        assert_eq!(date, None);
        assert_eq!(release_heading("## [0.1.0] — first build"), Some((v("0.1.0"), None)));
    }

    #[test]
    fn a_pre_release_version_is_a_release_heading_and_sorts_before_the_release() {
        let (version, date) = release_heading("## [0.5.0-rc1] — 2026-09-10").unwrap();
        assert_eq!(version, v("0.5.0-rc1"));
        assert_eq!(date.as_deref(), Some("2026-09-10"));
        let log = "## [0.5.0] — 2026-09-11\n\nfinal\n\n## [0.5.0-rc1] — 2026-09-10\n\ncandidate\n";
        let versions: Vec<String> =
            changes_between(log, &v("0.4.0"), &v("0.5.0")).into_iter().map(|c| c.version).collect();
        assert_eq!(versions, ["0.5.0", "0.5.0-rc1"]);
        // Somebody on the candidate gets the release alone.
        let versions: Vec<String> = changes_between(log, &v("0.5.0-rc1"), &v("0.5.0"))
            .into_iter()
            .map(|c| c.version)
            .collect();
        assert_eq!(versions, ["0.5.0"]);
    }

    #[test]
    fn the_first_date_is_found_wherever_it_sits_and_a_near_miss_is_not_one() {
        assert_eq!(first_date(" — 2026-09-10").as_deref(), Some("2026-09-10"));
        assert_eq!(first_date("pulled; see 2026-09-11 for the fix").as_deref(), Some("2026-09-11"));
        assert_eq!(first_date(" — 2026-09-10 and 2026-09-12").as_deref(), Some("2026-09-10"));
        assert_eq!(first_date(""), None);
        assert_eq!(first_date(" — 2026-09"), None, "a month is not a day");
        assert_eq!(first_date(" — 20260910"), None, "no dashes, no date");
        assert_eq!(first_date(" — 2026/09/10"), None, "the wrong separators");
    }

    #[test]
    fn a_heading_that_is_not_a_version_is_body_not_a_section() {
        let log = "## [0.2.0] — 2026-09-09\n\n## [Unreleased]\n- not yet\n\n## [0.1.0]\n\nfirst\n";
        let changes = changes_between(log, &v("0.0.0"), &v("9.0.0"));
        assert_eq!(changes.len(), 2);
        assert!(changes[0].body.contains("[Unreleased]"));
        assert_eq!(changes[1].date, None);
    }

    #[test]
    fn the_very_next_version_is_one_section_and_a_version_not_in_the_log_is_none() {
        let changes = changes_between(LOG, &v("0.1.0"), &v("0.2.0"));
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].version, "0.2.0");
        // A target the changelog has no section for: nothing to show, and
        // nothing older shown in its place.
        assert_eq!(changes_between(LOG, &v("0.4.0"), &v("0.4.1")), [] as [ChangeDto; 0]);
    }

    #[test]
    fn a_link_definition_inside_a_section_is_dropped_but_a_link_in_prose_is_kept() {
        let log = "## [0.2.0] — 2026-09-09\n\n- One.\n[0.2.0]: https://example.com/v0.2.0\n- Two.\n\n\
See [the notes](https://example.com) — and `[x]: y` inline is prose.\n";
        let changes = changes_between(log, &v("0.1.0"), &v("0.2.0"));
        assert_eq!(changes.len(), 1);
        let body = &changes[0].body;
        assert!(!body.contains("]: https://"), "the definition stayed: {body}");
        assert!(body.contains("- One.\n- Two."), "the bullets around it moved: {body}");
        assert!(body.contains("See [the notes](https://example.com)"));
        assert!(body.contains("`[x]: y` inline"));
    }

    #[test]
    fn windows_line_endings_read_the_same_as_unix_ones() {
        let crlf = LOG.replace('\n', "\r\n");
        let from_crlf = changes_between(&crlf, &v("0.1.0"), &v("0.4.0"));
        let from_lf = changes_between(LOG, &v("0.1.0"), &v("0.4.0"));
        assert_eq!(from_crlf, from_lf);
        assert!(from_crlf.iter().all(|c| !c.body.contains('\r')), "a carriage return leaked");
    }

    #[test]
    fn a_release_linux_build_without_a_bundle_refuses_to_update_in_place() {
        // A from-source or package-managed Linux build has no bundle for the
        // updater to swap, so it must refuse rather than download and fail on
        // every launch.
        #[cfg(target_os = "linux")]
        {
            assert!(
                self_update_refusal(false, None).is_some(),
                "a bundleless Linux release must refuse"
            );
            assert!(
                self_update_refusal(false, Some(&BundleType::AppImage)).is_none(),
                "an AppImage build may replace itself"
            );
        }
        // A development build refuses whatever the bundle is.
        assert!(self_update_refusal(true, None).is_some());
        assert!(self_update_refusal(true, Some(&BundleType::AppImage)).is_some());
    }
}
