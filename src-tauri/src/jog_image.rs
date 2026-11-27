//! Explicit image controls for Preferences and a selected USB device.
use crate::error::{AppError, AppResult, ErrorKind};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rbl_core::durable::{Publication, VolumeIdentity};
use rbl_devices::{
    jog_image::{self, Change},
    settings::JogImage,
};
use serde::Serialize;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use tauri::Manager;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JogImageDto {
    mode: Option<&'static str>,
    image: Option<String>,
    image_present: bool,
    image_error: Option<String>,
}

fn failure(error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorKind::Malformed, error.to_string())
}

fn source_image(path: &Path) -> AppResult<Vec<u8>> {
    if !std::fs::metadata(path).map_err(failure)?.is_file() {
        return Err(failure("Choose an image file."));
    }
    let mut file = std::fs::File::open(path).map_err(failure)?;
    let before = file.metadata().map_err(failure)?;
    if !before.is_file() {
        return Err(failure("Choose an image file."));
    }
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(jog_image::MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    let after = file.metadata().map_err(failure)?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(failure(
            "The chosen image changed while it was being read. Choose it again.",
        ));
    }
    jog_image::prepare(&bytes).map_err(failure)
}

fn selected(root: &Path, volume_id: &str) -> AppResult<VolumeIdentity> {
    let identity = VolumeIdentity::capture(root).map_err(failure)?;
    if !rbl_devices::list()
        .iter()
        .any(|device| device.mount_point == root && device.volume_id == volume_id)
    {
        return Err(AppError::new(
            ErrorKind::NotFound,
            "That device is no longer connected.",
        ));
    }
    identity.check(root).map_err(failure)?;
    Ok(identity)
}

fn state(root: &Path, device: bool) -> JogImageDto {
    let image_present = std::fs::symlink_metadata(root.join(jog_image::FILE_NAME)).is_ok();
    let (image, image_error) = match jog_image::read(root) {
        Ok(image) => (image, None),
        Err(error) => (None, Some(error.to_string())),
    };
    let (mode, mode_error) = if device {
        match jog_image::mode(root) {
            Ok(mode) => (
                mode.map(|mode| match mode {
                    JogImage::Artwork => "artwork",
                    JogImage::Custom => "custom",
                }),
                None,
            ),
            Err(error) => (None, Some(error.to_string())),
        }
    } else {
        (None, None)
    };
    let image_error = image_error.or(mode_error);
    JogImageDto {
        mode,
        image_present,
        image_error,
        image: image.map(|bytes| format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))),
    }
}

fn preference_snapshot(root: &Path) -> AppResult<Option<Vec<u8>>> {
    match std::fs::metadata(root) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(failure("The Preferences image folder is not a directory.")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failure(error)),
    }
    // Recovery and the snapshot share ownership. Release it before taking USB ownership.
    let _publication = Publication::new(root, ".rbxport-jog-image").map_err(failure)?;
    jog_image::read(root).map_err(failure)
}

fn perform(
    preference: &Path,
    path: Option<&Path>,
    volume_id: Option<&str>,
    action: &str,
    source: Option<&Path>,
) -> AppResult<JogImageDto> {
    perform_with(preference, path, volume_id, action, source, selected)
}

fn perform_with(
    preference: &Path,
    path: Option<&Path>,
    volume_id: Option<&str>,
    action: &str,
    source: Option<&Path>,
    select: impl FnOnce(&Path, &str) -> AppResult<VolumeIdentity>,
) -> AppResult<JogImageDto> {
    let root = path.unwrap_or(preference);
    let device = path.is_some();
    // Capture the selected medium before reading/converting any chosen source.
    let identity = if device {
        Some(select(
            root,
            volume_id.ok_or_else(|| failure("The device identity is missing."))?,
        )?)
    } else {
        None
    };
    if action == "read" {
        if !device && preference.is_dir() {
            Publication::recover(preference, ".rbxport-jog-image").map_err(failure)?;
        }
        let read = state(root, device);
        if let Some(identity) = identity.as_ref() {
            identity.check(root).map_err(failure)?;
        }
        return Ok(read);
    }
    let prepared = match action {
        "import" => Some(source_image(
            source.ok_or_else(|| failure("Choose an image file."))?,
        )?),
        "copy" if device => Some(
            preference_snapshot(preference)?
                .ok_or_else(|| failure("Import an image in Preferences first."))?,
        ),
        "delete" | "artwork" | "custom" => None,
        _ => return Err(failure("Unknown image action.")),
    };
    if let Some(identity) = identity.as_ref() {
        let change = match action {
            "artwork" => Change::Mode(JogImage::Artwork),
            "custom" => Change::Mode(JogImage::Custom),
            "delete" => Change::Delete,
            _ => Change::Image(
                prepared
                    .as_deref()
                    .ok_or_else(|| failure("The image is missing."))?,
            ),
        };
        jog_image::write(root, identity, change).map_err(failure)?;
    } else {
        if !matches!(action, "import" | "delete") {
            return Err(failure("Choose an image action."));
        }
        std::fs::create_dir_all(preference).map_err(failure)?;
        let publication = Publication::new(preference, ".rbxport-jog-image").map_err(failure)?;
        if action == "delete" && !jog_image::exists(preference).map_err(failure)? {
            return Ok(state(root, device));
        }
        if let Some(bytes) = prepared {
            rbl_core::durable::write(&publication.stage().join(jog_image::FILE_NAME), &bytes)
                .map_err(failure)?;
        }
        publication
            .commit(&[PathBuf::from(jog_image::FILE_NAME)])
            .map_err(failure)?;
    }
    let saved = state(root, device);
    if let Some(identity) = identity.as_ref() {
        identity.check(root).map_err(failure)?;
    }
    Ok(saved)
}

#[tauri::command]
pub async fn jog_image(
    app: tauri::AppHandle,
    path: Option<String>,
    volume_id: Option<String>,
    action: String,
    source: Option<String>,
) -> AppResult<JogImageDto> {
    let preference = app
        .path()
        .app_config_dir()
        .map_err(failure)?
        .join("jog-image");
    crate::commands::blocking("jog_image", move || {
        perform(
            &preference,
            path.as_deref().map(Path::new),
            volume_id.as_deref(),
            &action,
            source.as_deref().map(Path::new),
        )
    })
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP478DwHwAFwAI/Ko64jQAAAABJRU5ErkJggg==").unwrap()
    }
    fn select(root: &Path, id: &str) -> AppResult<VolumeIdentity> {
        assert_eq!(id, "generated-volume");
        VolumeIdentity::capture(root).map_err(failure)
    }
    fn pending_preference(root: &Path, jpeg: &[u8]) {
        let journal = root.join(".rbxport-jog-image");
        std::fs::create_dir_all(&journal).unwrap();
        std::fs::write(journal.join(jog_image::FILE_NAME), jpeg).unwrap();
        std::fs::write(
            journal.join("publication.json"),
            serde_json::to_vec(&serde_json::json!([
                {"path": jog_image::FILE_NAME, "present": true, "had_target": false, "exact": true}
            ]))
            .unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn interrupted_preferences_are_recovered_on_read_and_direct_device_copy() {
        for open_preferences in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let preference = temp.path().join("preference");
            let device = temp.path().join("device");
            std::fs::create_dir(&device).unwrap();
            let jpeg = jog_image::prepare(&png()).unwrap();
            pending_preference(&preference, &jpeg);
            if open_preferences {
                let read = perform(&preference, None, None, "read", None).unwrap();
                assert!(read.image.is_some());
            }
            let copied = perform_with(
                &preference,
                Some(&device),
                Some("generated-volume"),
                "copy",
                None,
                select,
            )
            .unwrap();
            assert!(copied.image.is_some());
            assert_eq!(copied.mode, None);
            assert_eq!(
                std::fs::read(device.join(jog_image::FILE_NAME)).unwrap(),
                jpeg
            );
            assert_eq!(
                std::fs::read(preference.join(jog_image::FILE_NAME)).unwrap(),
                jpeg
            );
            assert!(!preference.join(".rbxport-jog-image").exists());
            assert!(!device.join("PIONEER").exists());
        }
    }
    #[test]
    fn preference_import_refusal_and_invalid_image_deletion_preserve_unrelated_data() {
        let temp = tempfile::tempdir().unwrap();
        let preference = temp.path().join("preference");
        let source = temp.path().join("source.png");
        let original = png();
        std::fs::write(&source, &original).unwrap();
        let imported = perform(&preference, None, None, "import", Some(&source)).unwrap();
        assert!(imported.image.is_some());
        assert_eq!(std::fs::read(&source).unwrap(), original);
        let before = std::fs::read(preference.join(jog_image::FILE_NAME)).unwrap();
        std::fs::write(&source, b"invalid").unwrap();
        assert!(perform(&preference, None, None, "import", Some(&source)).is_err());
        assert_eq!(
            std::fs::read(preference.join(jog_image::FILE_NAME)).unwrap(),
            before
        );
        std::fs::write(preference.join("keep.txt"), b"unmanaged").unwrap();
        std::fs::write(preference.join(jog_image::FILE_NAME), b"invalid").unwrap();
        let read = perform(&preference, None, None, "read", None).unwrap();
        assert!(read.image_present);
        assert!(read.image_error.is_some());
        let deleted = perform(&preference, None, None, "delete", None).unwrap();
        assert!(!deleted.image_present);
        assert_eq!(
            std::fs::read(preference.join("keep.txt")).unwrap(),
            b"unmanaged"
        );
    }
    #[test]
    fn default_initializer_and_ordinary_display_save_preserve_explicit_device_mode() {
        use rbl_devices::settings::{self, DevSetting};
        for name in ["PIONEER", ".PIONEER"] {
            let temp = tempfile::tempdir().unwrap();
            std::fs::create_dir(temp.path().join(name)).unwrap();
            let path = temp.path().join(name).join("DEVSETTING.DAT");
            let bytes =
                DevSetting::patch_jog_image(&DevSetting::default().encode(), JogImage::Custom)
                    .unwrap();
            std::fs::write(&path, &bytes).unwrap();
            let current = settings::read(temp.path());
            let mut dto = crate::device_settings::to_dto(&current);
            dto.waveform_color = "rgb".into();
            settings::edit(temp.path(), |read| {
                crate::device_settings::apply(read, &dto)
            })
            .unwrap();
            assert_eq!(
                jog_image::mode(temp.path()).unwrap(),
                Some(JogImage::Custom)
            );
            let defaults:crate::device_settings::StickDefaultsDto=serde_json::from_value(serde_json::json!({
                "waveformColor":"blue","waveformPosition":"center","overviewWaveform":"half","keyDisplay":"classic",
                "categories":null,"sorts":null,"subColumn":null
            })).unwrap();
            let before = std::fs::read(&path).unwrap();
            crate::device_settings::write_dev_defaults(temp.path(), &defaults).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), before);
            std::fs::write(&path, b"malformed existing settings").unwrap();
            crate::device_settings::write_dev_defaults(temp.path(), &defaults).unwrap();
            assert_eq!(
                std::fs::read(&path).unwrap(),
                b"malformed existing settings"
            );
        }
    }
    #[test]
    fn existing_default_targets_need_no_publication_lock() {
        use rbl_devices::settings::DevSetting;
        let defaults: crate::device_settings::StickDefaultsDto = serde_json::from_value(serde_json::json!({
            "waveformColor":"blue","waveformPosition":"center","overviewWaveform":"half","keyDisplay":"classic",
            "categories":null,"sorts":null,"subColumn":null
        })).unwrap();
        for bytes in [
            DevSetting::default().encode(),
            b"malformed existing settings".to_vec(),
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("PIONEER")).unwrap();
            let path = root.path().join("PIONEER/DEVSETTING.DAT");
            std::fs::write(&path, &bytes).unwrap();
            crate::device_settings::write_dev_defaults(root.path(), &defaults).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert!(
                !root.path().join(".rbxport-write.lock").exists(),
                "existing settings must not request a writable publication"
            );
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        }
    }
}
