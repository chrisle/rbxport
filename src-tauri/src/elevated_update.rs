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
#[cfg(windows)]
const PROTECTED_INSTALLER: &str = "rbxport-update-installer.exe";

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
    for name in [STAGED_INSTALLER, STAGED_METADATA, STAGED_ERROR] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// Starts the protected task when this is one of its shared staged files.
/// `Ok(false)` asks the caller to use Tauri's ordinary installer fallback —
/// the migration update takes that route and receives the one required UAC.
#[cfg_attr(not(windows), allow(clippy::unnecessary_wraps))]
pub fn launch(path: &Path, restart_after_install: bool) -> Result<bool, String> {
    #[cfg(not(windows))]
    {
        let _ = (path, restart_after_install);
        Ok(false)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        if !is_shared_staging(path) {
            return Ok(false);
        }
        let mut metadata = read_metadata(path)?;
        metadata.restart_after_install = restart_after_install;
        write_metadata(path, &metadata)?;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let status = std::process::Command::new("schtasks.exe")
            .args(["/Run", "/TN", TASK_NAME])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|error| format!("could not start the protected update task: {error}"))?;
        Ok(status.success())
    }
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
                    let _ = std::fs::write(dir.join(STAGED_ERROR), &error);
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
    use std::os::windows::process::CommandExt as _;
    use std::time::{Duration, Instant};
    use sysinfo::{ProcessesToUpdate, System};

    let directory = shared_staging_dir().ok_or_else(|| "ProgramData is unavailable".to_owned())?;
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

    let helper = std::env::current_exe()
        .map_err(|error| format!("could not locate the protected update helper: {error}"))?;
    let install_directory = helper
        .parent()
        .ok_or_else(|| "the protected update helper has no directory".to_owned())?;
    let protected = install_directory.join(PROTECTED_INSTALLER);
    std::fs::write(&protected, bytes)
        .map_err(|error| format!("could not write the protected installer: {error}"))?;

    let mut command = std::process::Command::new(&protected);
    command.args(["/S", "/UPDATE"]);
    if metadata.restart_after_install {
        command.arg("/R");
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
    command
        .spawn()
        .map_err(|error| format!("could not start the protected installer: {error}"))?;

    let _ = std::fs::remove_file(staged);
    let _ = std::fs::remove_file(directory.join(STAGED_METADATA));
    let _ = std::fs::remove_file(directory.join(STAGED_ERROR));
    Ok(())
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
