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
//!
//! What has changed is worked out here, not in the interface: the notes are
//! the full published release history, and the part that matters is the sections newer
//! than the version running and no newer than the one on offer — a user
//! two releases behind should read both.
//!
//! The download reports through one event, `update:progress`, at most about
//! ten times a second: a chunk arrives every few kilobytes, and an event per
//! chunk would be IPC for nothing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use semver::Version;
use serde::Serialize;
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
    pub fn clear_stale(app: &AppHandle) {
        elevated_update::clear_shared_staging();
        if let Some(dir) = Self::staging_dir(app) {
            if elevated_update::shared_staging_dir().as_deref() != Some(dir.as_path()) {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }
}

/// Whether the bundle this process runs from can be swapped on disk while
/// it runs, so an update is in place the moment it is downloaded.
fn swaps_in_place() -> bool {
    use tauri::utils::config::BundleType;
    use tauri::utils::platform::bundle_type;
    matches!(bundle_type(), Some(BundleType::App | BundleType::AppImage))
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

fn updater_error(message: &str, err: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorKind::Internal, message).with_detail(err.to_string())
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
    let found = app
        .updater()
        .map_err(|e| updater_error("The updater is not configured.", e))?
        .check()
        .await
        .map_err(|e| updater_error("The update check could not reach the download server.", e))?;

    let Some(update) = found else {
        *updates.pending.lock() = None;
        return Ok(UpdateCheckDto { current_version, version: None, date: None, changes: Vec::new(), ready: None });
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
    let dto = UpdateCheckDto { current_version, version: Some(update.version.clone()), date, changes, ready };
    *pending = Some(Pending { update, placement });
    Ok(dto)
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
    if cfg!(debug_assertions) {
        return Err(AppError::new(
            ErrorKind::Internal,
            "A development build cannot be updated in place.",
        ));
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
    match placement {
        Placement::Installed => app.restart(),
        Placement::Staged(path) => {
            // Taken out first: `install` ends the process on Windows, and the
            // quit that follows must not run the same installer again.
            if let Some(pending) = updates.pending.lock().as_mut() {
                pending.placement = None;
            }
            match elevated_update::launch(&path, true) {
                Ok(true) => std::process::exit(0),
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "the protected update task could not start"),
            }
            let bytes = std::fs::read(&path).map_err(|e| updater_error("The update could not be read back.", e))?;
            // On Windows this spawns the installer and ends the process; a
            // Linux package is installed in place and the process is still
            // here, so a restart runs it.
            tauri::async_runtime::spawn_blocking(move || update.install(bytes))
                .await
                .map_err(|e| updater_error("The update could not be installed.", e))?
                .map_err(|e| updater_error("The update could not be installed.", e))?;
            app.restart()
        }
    }
}

/// The quit: a staged installer runs now, silently, and does not bring the
/// app back — the person closed it. Only Windows stages an installer the
/// quit can run; a Linux package would prompt for a password, and stays for
/// [`restart_to_update`].
pub fn on_exit(app: &AppHandle) {
    if !cfg!(windows) {
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
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => tracing::warn!(%error, "the protected update task could not start at quit"),
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
}
