//! Windows' quiet, elevated half of an update.
//!
//! The application runs without administrator rights. Its installer does not:
//! a machine-wide installation lives under Program Files. The first update
//! after this helper ships still follows the ordinary Tauri path and receives
//! Windows' one UAC consent. That installer copies this executable beside the
//! app and registers a protected, on-demand SYSTEM task. Later downloads are
//! staged under `ProgramData`; the task launches this same signed application
//! with [`HELPER_ARGUMENT`], and this module verifies the update's minisign
//! signature again before copying it into Program Files and running NSIS with
//! `/S`.
//!
//! The double verification is deliberate. A standard user may write the
//! staging directory so the non-elevated app can download there. Trust begins
//! only after the elevated helper has read and verified the complete bytes;
//! the verified copy it executes is in Program Files, outside that user's
//! control, which closes the verify/execute race.
//!
//! The helper answers through files in the staging directory, because the app
//! cannot see a SYSTEM task's exit code (#457):
//!
//! - [`STAGED_ACCEPTED`] once the staged update's version and signature are
//!   good. Starting the task proves nothing more than that Task Scheduler took
//!   the request, so Restart Now quits only after this answer.
//! - [`STAGED_ERROR`] with the reason, whenever the helper gives up. The next
//!   launch logs it and shows it in the Update Manager.
//! - [`STAGED_INSTALLED`] once the installer has finished. The SYSTEM task
//!   cannot start the app on the person's desktop (NSIS `/R` uses the
//!   shell's window, and session 0 has none), so Restart Now leaves a small
//!   watcher in the person's own session that starts the app on this answer.

use std::path::{Path, PathBuf};

#[cfg(any(windows, test))]
use base64::Engine as _;
#[cfg(any(windows, test))]
use minisign_verify::{PublicKey, Signature};
#[cfg(windows)]
use semver::Version;
use serde::{Deserialize, Serialize};

pub const HELPER_ARGUMENT: &str = "--apply-staged-update";
#[cfg(windows)]
const TASK_NAME: &str = "rbxport Silent Update";
const STAGED_INSTALLER: &str = "pending.update";
const STAGED_METADATA: &str = "pending.json";
const STAGED_ERROR: &str = "last-error.txt";
const STAGED_ACCEPTED: &str = "accepted";
const STAGED_INSTALLED: &str = "installed.txt";
#[cfg(windows)]
const PROTECTED_INSTALLER: &str = "rbxport-update-installer.exe";
/// The name the helper moves its own image to while the installer runs, so
/// the installer can write a fresh `rbxport-updater.exe` (a running image can
/// be renamed but not overwritten).
#[cfg(windows)]
const RETIRED_HELPER: &str = "rbxport-updater.old.exe";
#[cfg(windows)]
const HELPER_IMAGE: &str = "rbxport-updater.exe";
/// How long the app waits for the helper to accept a staged update.
#[cfg(windows)]
const ANSWER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// How long the relaunch watcher waits for the installer to finish.
#[cfg(windows)]
const RELAUNCH_TIMEOUT_SECS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StagedMetadata {
    version: String,
    signature: String,
    restart_after_install: bool,
}

/// The shared, installer-created staging directory. Its ACL lets an ordinary
/// user write candidates; the elevated helper treats every byte as untrusted.
pub fn shared_staging_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("ProgramData")
            .map(|root| PathBuf::from(root).join("rbxport").join("updates"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn is_shared_staging(path: &Path) -> bool {
    shared_staging_dir().is_some_and(|dir| path.parent() == Some(dir.as_path()))
}

/// Records the signed metadata beside a downloaded installer. This is not a
/// trust boundary: the elevated process verifies both fields again.
pub fn stage(path: &Path, version: &str, signature: &str) -> Result<(), String> {
    if !is_shared_staging(path) {
        return Ok(());
    }
    let metadata = StagedMetadata {
        version: version.to_owned(),
        signature: signature.to_owned(),
        restart_after_install: false,
    };
    write_metadata(path, &metadata)
}

pub fn clear_shared_staging() {
    let Some(dir) = shared_staging_dir() else {
        return;
    };
    clear_dir(&dir);
}

fn clear_dir(dir: &Path) {
    for name in [STAGED_INSTALLER, STAGED_METADATA, STAGED_ERROR, STAGED_ACCEPTED, STAGED_INSTALLED] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// How the last protected update ended, as the helper left it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Why the helper gave up, from [`STAGED_ERROR`].
    pub failure: Option<String>,
    /// The version the helper installed, from [`STAGED_INSTALLED`].
    pub installed: Option<String>,
}

/// Reads how the last protected update ended, then clears the staging
/// directory. Before #457 the reason was deleted here unread, so an update
/// that failed after the app quit left no trace anywhere.
pub fn take_outcome() -> Outcome {
    let Some(dir) = shared_staging_dir() else {
        return Outcome::default();
    };
    let outcome = read_outcome(&dir);
    clear_dir(&dir);
    outcome
}

fn read_outcome(dir: &Path) -> Outcome {
    let text = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .ok()
            .map(|text| text.trim().to_owned())
    };
    Outcome {
        failure: text(STAGED_ERROR).map(|reason| {
            if reason.is_empty() {
                "the protected update helper failed without a reason".to_owned()
            } else {
                reason
            }
        }),
        installed: text(STAGED_INSTALLED).filter(|version| !version.is_empty()),
    }
}

/// What the helper has answered so far. A failure wins over an acceptance:
/// the helper may accept and then fail while waiting for the app to quit.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
enum Answer {
    Accepted,
    Refused(String),
}

#[cfg(any(windows, test))]
fn answer(dir: &Path) -> Option<Answer> {
    if let Some(reason) = read_outcome(dir).failure {
        return Some(Answer::Refused(reason));
    }
    dir.join(STAGED_ACCEPTED).is_file().then_some(Answer::Accepted)
}

/// What became of the request to the protected task.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code, reason = "only Windows has the protected task"))]
pub enum Handoff {
    /// The update is not in the shared staging directory, so there is no
    /// task to ask: the caller uses Tauri's ordinary installer.
    NotShared,
    /// The helper took the update over; the app should quit now.
    TakenOver,
    /// The helper looked at the update and refused it, with its reason. The
    /// ordinary installer must not run these bytes either.
    Refused(String),
    /// The task could not be started or never answered: the caller may use
    /// the ordinary installer instead.
    Unavailable(String),
}

/// Waits for the helper's answer. `helper_running` covers a helper from a
/// release before the answer files existed: it never writes one, but its
/// process is there, and it will install once the app quits.
#[cfg(any(windows, test))]
fn await_answer(
    dir: &Path,
    timeout: std::time::Duration,
    poll: std::time::Duration,
    helper_running: impl Fn() -> bool,
) -> Handoff {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match answer(dir) {
            Some(Answer::Accepted) => return Handoff::TakenOver,
            Some(Answer::Refused(reason)) => return Handoff::Refused(reason),
            None => {}
        }
        if std::time::Instant::now() >= deadline {
            if helper_running() {
                return Handoff::TakenOver;
            }
            return Handoff::Unavailable(format!(
                "the protected update task did not answer within {} s",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(poll);
    }
}

/// `text` as a PowerShell single-quoted string.
#[cfg(any(windows, test))]
fn powershell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The watcher Restart Now leaves behind in the person's own session: it
/// waits for the helper to finish, either way, then starts the app. On a
/// failure the old version opens and shows why.
#[cfg(any(windows, test))]
fn relaunch_script(dir: &Path, app: &Path, timeout_secs: u64) -> String {
    let dir = powershell_quote(&dir.to_string_lossy());
    let app = powershell_quote(&app.to_string_lossy());
    format!(
        "$d = {dir}; $deadline = (Get-Date).AddSeconds({timeout_secs}); \
         while ((Get-Date) -lt $deadline) {{ \
         if ((Test-Path -LiteralPath (Join-Path $d '{STAGED_INSTALLED}')) -or \
         (Test-Path -LiteralPath (Join-Path $d '{STAGED_ERROR}'))) {{ \
         Start-Process -FilePath {app}; exit 0 }}; \
         Start-Sleep -Milliseconds 500 }}; exit 1"
    )
}

/// Writes `text` whole or not at all, so a reader polling for it never sees
/// half of it.
#[cfg(any(windows, test))]
fn publish_text(path: &Path, text: &str) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, text)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    let _ = std::fs::remove_file(path);
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("could not publish {}: {error}", path.display()))
}

/// Starts the protected task when this is one of its shared staged files,
/// and waits for the helper to take the update over. Starting the task only
/// says Task Scheduler took the request; quitting on that alone left the app
/// gone with nothing installed whenever the helper then refused (#457).
///
/// With `restart_after_install`, a watcher is left in this session to start
/// the app again once the helper is done: the installer the SYSTEM task runs
/// cannot do that itself.
pub fn launch(path: &Path, restart_after_install: bool) -> Handoff {
    #[cfg(not(windows))]
    {
        let _ = (path, restart_after_install);
        Handoff::NotShared
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        if !is_shared_staging(path) {
            return Handoff::NotShared;
        }
        let Some(dir) = path.parent() else {
            return Handoff::NotShared;
        };
        let mut metadata = match read_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => return Handoff::Unavailable(error),
        };
        // A helper from before #457 still reads this and passes NSIS `/R`.
        metadata.restart_after_install = restart_after_install;
        if let Err(error) = write_metadata(path, &metadata) {
            return Handoff::Unavailable(error);
        }
        for stale in [STAGED_ERROR, STAGED_ACCEPTED, STAGED_INSTALLED] {
            let _ = std::fs::remove_file(dir.join(stale));
        }

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = match std::process::Command::new("schtasks.exe")
            .args(["/Run", "/TN", TASK_NAME])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
        {
            Ok(status) => status,
            Err(error) => {
                return Handoff::Unavailable(format!("could not start the protected update task: {error}"))
            }
        };
        if !status.success() {
            return Handoff::Unavailable(format!("the protected update task did not start: {status}"));
        }
        let handoff = await_answer(dir, ANSWER_TIMEOUT, std::time::Duration::from_millis(200), helper_running);
        if handoff == Handoff::TakenOver && restart_after_install {
            if let Err(error) = leave_relauncher(dir) {
                tracing::warn!(%error, "rbxport will not reopen by itself after the update");
            }
        }
        handoff
    }
}

/// Whether a protected helper process is running, for a helper that predates
/// the answer files.
#[cfg(windows)]
fn helper_running() -> bool {
    use sysinfo::{ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system.processes().values().any(|process| {
        process
            .name()
            .to_string_lossy()
            .eq_ignore_ascii_case(HELPER_IMAGE)
    })
}

#[cfg(windows)]
fn leave_relauncher(dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;

    let app = std::env::current_exe().map_err(|error| format!("could not locate rbxport: {error}"))?;
    let powershell = std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe"))
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    std::process::Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command"])
        .arg(relaunch_script(dir, &app, RELAUNCH_TIMEOUT_SECS))
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not start the relaunch watcher: {error}"))
}

/// Runs before Tauri starts when the protected scheduled task invokes the
/// helper copy. `None` means this is an ordinary app launch.
pub fn run_if_requested() -> Option<i32> {
    if !std::env::args_os().any(|argument| argument == HELPER_ARGUMENT) {
        return None;
    }

    #[cfg(not(windows))]
    {
        Some(1)
    }
    #[cfg(windows)]
    {
        match apply_staged_update() {
            Ok(()) => Some(0),
            Err(error) => {
                if let Some(dir) = shared_staging_dir() {
                    let _ = publish_text(&dir.join(STAGED_ERROR), &error);
                }
                Some(1)
            }
        }
    }
}

fn metadata_path(installer: &Path) -> Result<PathBuf, String> {
    let parent = installer
        .parent()
        .ok_or_else(|| "the staged update has no directory".to_owned())?;
    Ok(parent.join(STAGED_METADATA))
}

#[cfg(any(windows, test))]
fn read_metadata(installer: &Path) -> Result<StagedMetadata, String> {
    let path = metadata_path(installer)?;
    let bytes = std::fs::read(&path).map_err(|error| {
        format!(
            "could not read staged update metadata at {}: {error}",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("staged update metadata is invalid: {error}"))
}

fn write_metadata(installer: &Path, metadata: &StagedMetadata) -> Result<(), String> {
    let path = metadata_path(installer)?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(metadata)
        .map_err(|error| format!("could not encode staged update metadata: {error}"))?;
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("could not write staged update metadata: {error}"))?;
    let _ = std::fs::remove_file(&path);
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("could not publish staged update metadata: {error}"))
}

#[cfg(any(windows, test))]
fn updater_public_key() -> Result<String, String> {
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .map_err(|error| format!("the embedded Tauri configuration is invalid: {error}"))?;
    config
        .pointer("/plugins/updater/pubkey")
        .and_then(serde_json::Value::as_str)
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "the embedded updater public key is missing".to_owned())
}

#[cfg(any(windows, test))]
fn decode_base64_text(encoded: &str, what: &str) -> Result<String, String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("the {what} is not valid base64: {error}"))?;
    String::from_utf8(decoded).map_err(|error| format!("the decoded {what} is not UTF-8: {error}"))
}

#[cfg(any(windows, test))]
fn verify(bytes: &[u8], encoded_signature: &str) -> Result<(), String> {
    let public_key = decode_base64_text(&updater_public_key()?, "updater public key")?;
    let public_key = PublicKey::decode(&public_key)
        .map_err(|error| format!("the updater public key is invalid: {error}"))?;
    let signature = decode_base64_text(encoded_signature, "update signature")?;
    let signature = Signature::decode(&signature)
        .map_err(|error| format!("the update signature is invalid: {error}"))?;
    public_key
        .verify(bytes, &signature, true)
        .map_err(|error| format!("the staged update signature did not verify: {error}"))
}

#[cfg(windows)]
fn apply_staged_update() -> Result<(), String> {
    use std::time::{Duration, Instant};
    use sysinfo::{ProcessesToUpdate, System};

    let directory = shared_staging_dir().ok_or_else(|| "ProgramData is unavailable".to_owned())?;
    let helper = std::env::current_exe()
        .map_err(|error| format!("could not locate the protected update helper: {error}"))?;
    let install_directory = helper
        .parent()
        .ok_or_else(|| "the protected update helper has no directory".to_owned())?
        .to_path_buf();
    let retired = install_directory.join(RETIRED_HELPER);
    // What the last update moved aside; this copy is the one it installed.
    if helper != retired {
        let _ = std::fs::remove_file(&retired);
    }

    let staged = directory.join(STAGED_INSTALLER);
    let metadata = read_metadata(&staged)?;
    let target = Version::parse(&metadata.version)
        .map_err(|error| format!("the staged update version is invalid: {error}"))?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| format!("the installed version is invalid: {error}"))?;
    if target <= current {
        return Err(format!("refusing to install {target} over {current}"));
    }

    let bytes = std::fs::read(&staged).map_err(|error| {
        format!(
            "could not read the staged update at {}: {error}",
            staged.display()
        )
    })?;
    verify(&bytes, &metadata.signature)?;
    // The app is waiting for this before it quits.
    publish_text(&directory.join(STAGED_ACCEPTED), &metadata.version)?;

    // The task can begin before Tauri's event loop has fully gone away. Wait
    // for the main image, but not this `rbxport-updater.exe` helper, to leave.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut system = System::new_all();
    loop {
        system.refresh_processes(ProcessesToUpdate::All, true);
        let main_running = system.processes().values().any(|process| {
            process
                .name()
                .to_string_lossy()
                .eq_ignore_ascii_case("rbxport.exe")
        });
        if !main_running {
            break;
        }
        if Instant::now() >= deadline {
            return Err("rbxport did not exit before the update timeout".to_owned());
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    let protected = install_directory.join(PROTECTED_INSTALLER);
    std::fs::write(&protected, bytes)
        .map_err(|error| format!("could not write the protected installer: {error}"))?;

    // This helper now waits for the installer, so its image is in use while
    // the installer copies the new `rbxport.exe` over `rbxport-updater.exe`.
    // Moving it aside first lets that copy succeed.
    let stepped_aside = helper != retired && std::fs::rename(&helper, &retired).is_ok();
    let installed = run_installer(&protected);
    if !helper.exists() {
        let restored = if installed.is_ok() {
            std::fs::copy(install_directory.join("rbxport.exe"), &helper).map(|_| ())
        } else if stepped_aside {
            std::fs::rename(&retired, &helper)
        } else {
            Ok(())
        };
        if let Err(error) = restored {
            let reason = format!("could not put the protected update helper back: {error}");
            return Err(match installed {
                Ok(()) => reason,
                Err(failure) => format!("{failure}; {reason}"),
            });
        }
    }
    installed?;

    let _ = std::fs::remove_file(staged);
    let _ = std::fs::remove_file(directory.join(STAGED_METADATA));
    let _ = std::fs::remove_file(directory.join(STAGED_ERROR));
    let _ = std::fs::remove_file(directory.join(STAGED_ACCEPTED));
    publish_text(&directory.join(STAGED_INSTALLED), &metadata.version)
}

/// Runs the verified installer silently and waits for it, so a failure is
/// known and the app is started again only once the files are in place. No
/// `/R`: the installer would start the app with the shell's token, and the
/// SYSTEM task's session has no shell, so it never did.
#[cfg(windows)]
fn run_installer(protected: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new(protected)
        .args(["/S", "/UPDATE"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("could not start the protected installer: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("the protected installer failed: {status}"))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn embedded_updater_key_is_present_and_decodes() {
        let decoded =
            decode_base64_text(&updater_public_key().unwrap(), "updater public key").unwrap();
        assert!(decoded.contains("minisign public key"));
        assert!(PublicKey::decode(&decoded).is_ok());
    }

    #[test]
    fn invalid_update_bytes_do_not_pass_the_elevated_check() {
        assert!(verify(b"not an installer", "not-base64").is_err());
    }

    #[test]
    fn a_helper_failure_is_read_at_the_next_launch_not_deleted_unread() {
        // #457: the helper's reason went to last-error.txt, and the next
        // launch deleted the file without reading it.
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(STAGED_ERROR),
            "rbxport did not exit before the update timeout\r\n",
        )
        .unwrap();
        assert_eq!(
            read_outcome(directory.path()),
            Outcome {
                failure: Some("rbxport did not exit before the update timeout".to_owned()),
                installed: None,
            }
        );
        // Clearing for the next download removes every answer file.
        std::fs::write(directory.path().join(STAGED_ACCEPTED), "2.0.0").unwrap();
        std::fs::write(directory.path().join(STAGED_INSTALLED), "2.0.0").unwrap();
        clear_dir(directory.path());
        assert_eq!(read_outcome(directory.path()), Outcome::default());
        assert!(!directory.path().join(STAGED_ACCEPTED).exists());
    }

    #[test]
    fn an_empty_failure_still_counts_and_an_install_names_its_version() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join(STAGED_ERROR), "").unwrap();
        std::fs::write(directory.path().join(STAGED_INSTALLED), "2.0.0\n").unwrap();
        let outcome = read_outcome(directory.path());
        assert!(outcome.failure.is_some_and(|reason| !reason.is_empty()));
        assert_eq!(outcome.installed.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn restart_now_quits_only_once_the_helper_has_accepted() {
        let quick = std::time::Duration::from_millis(30);
        let poll = std::time::Duration::from_millis(5);
        let directory = tempfile::tempdir().unwrap();

        // Nothing answered and no helper running: the task never took it.
        assert!(matches!(
            await_answer(directory.path(), quick, poll, || false),
            Handoff::Unavailable(_)
        ));
        // A helper from before the answer files: running, silent.
        assert_eq!(await_answer(directory.path(), quick, poll, || true), Handoff::TakenOver);

        publish_text(&directory.path().join(STAGED_ACCEPTED), "2.0.0").unwrap();
        assert_eq!(await_answer(directory.path(), quick, poll, || false), Handoff::TakenOver);

        // A refusal wins over an earlier acceptance and carries the reason.
        publish_text(
            &directory.path().join(STAGED_ERROR),
            "the staged update signature did not verify: bad",
        )
        .unwrap();
        assert_eq!(
            await_answer(directory.path(), quick, poll, || true),
            Handoff::Refused("the staged update signature did not verify: bad".to_owned())
        );
        assert!(!directory.path().join("last-error.tmp").exists(), "a temporary was left");
    }

    #[test]
    fn the_relauncher_waits_for_either_answer_and_quotes_its_paths() {
        let script = relaunch_script(
            Path::new(r"C:\ProgramData\rbxport\updates"),
            Path::new(r"C:\Users\O'Neil\rbxport\rbxport.exe"),
            600,
        );
        assert!(script.contains(r"$d = 'C:\ProgramData\rbxport\updates'"), "{script}");
        assert!(script.contains(r"Start-Process -FilePath 'C:\Users\O''Neil\rbxport\rbxport.exe'"), "{script}");
        assert!(script.contains(STAGED_INSTALLED) && script.contains(STAGED_ERROR), "{script}");
        assert!(script.contains("AddSeconds(600)"), "{script}");
    }

    #[test]
    fn metadata_round_trips_without_trusting_a_path_from_json() {
        let directory = tempfile::tempdir().unwrap();
        let installer = directory.path().join(STAGED_INSTALLER);
        let metadata = StagedMetadata {
            version: "2.0.0".to_owned(),
            signature: "signed".to_owned(),
            restart_after_install: true,
        };
        write_metadata(&installer, &metadata).unwrap();
        let read = read_metadata(&installer).unwrap();
        assert_eq!(read.version, "2.0.0");
        assert_eq!(read.signature, "signed");
        assert!(read.restart_after_install);
    }
}
