//! The device tabs: reading a stick's settings and writing them back.
//!
//! Wire shape and commands together, kept out of `commands.rs` because they
//! share nothing with the library. What is on the stick and what each field
//! means is `rbl_devices::settings`' business; this only translates.

use std::path::Path;

use rbl_devices::settings::{
    DevSetting, DeviceSettings, KeyDisplay, OverviewWaveform, WaveformColor, WaveformPosition,
};
use rbl_onelibrary::settings::{ColorName, MenuSlot, StickSettings};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult, ErrorKind};

/// One browse category or sort option.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuSlotDto {
    pub id: i64,
    pub menu_item: i64,
    pub name: String,
    pub seq: i64,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorNameDto {
    pub id: i64,
    pub name: String,
}

/// Everything the six tabs show. A few kilobytes at most: 22 categories, 17
/// sorts, 8 colours.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
// Four presence flags for four optional files; a wire struct, not a state machine.
#[allow(clippy::struct_excessive_bools)]
pub struct DeviceSettingsDto {
    /// Whether `export.pdb` is on the stick — "Device Library".
    pub has_device_library: bool,
    /// Whether `exportLibrary.db` is on the stick — "`OneLibrary`".
    pub has_one_library: bool,
    /// Whether `DEVSETTING.DAT` was read. When false the four display
    /// settings below are rekordbox's defaults and a write creates the file.
    pub has_dev_setting: bool,
    /// `blue` | `rgb` | `3band`
    pub waveform_color: String,
    /// `center` | `left`
    pub waveform_position: String,
    /// `half` | `full`
    pub overview_waveform: String,
    /// `classic` | `alphanumeric`
    pub key_display: String,
    /// Whether the library settings below were read; when false the stick
    /// has no `exportLibrary.db` and they are not written.
    pub has_library_settings: bool,
    pub device_name: String,
    /// "Background Color : `OneLibrary`": `property.backGroundColorType`,
    /// 0 Default, 1 Pink .. 8 Purple.
    pub background_color_type: i64,
    /// "Background Color : Device Library", from `export.pdb`'s `property`
    /// row, same values. `None` when the stick has no such row.
    pub device_library_background_color_type: Option<i64>,
    pub categories: Vec<MenuSlotDto>,
    pub sorts: Vec<MenuSlotDto>,
    pub sub_column: Option<i64>,
    pub colors: Vec<ColorNameDto>,
}

fn slot_dto(slot: &MenuSlot) -> MenuSlotDto {
    MenuSlotDto {
        id: slot.id,
        menu_item: slot.menu_item,
        name: slot.name.clone(),
        seq: slot.seq,
        visible: slot.visible,
    }
}

fn slot_from(dto: &MenuSlotDto) -> MenuSlot {
    MenuSlot {
        id: dto.id,
        menu_item: dto.menu_item,
        name: dto.name.clone(),
        seq: dto.seq,
        visible: dto.visible,
    }
}

pub fn to_dto(settings: &DeviceSettings) -> DeviceSettingsDto {
    let default_dev = DevSetting::default();
    let dev = settings.dev.as_ref().unwrap_or(&default_dev);
    let default_library = StickSettings::default();
    let library = settings.library.as_ref().unwrap_or(&default_library);
    DeviceSettingsDto {
        has_device_library: settings.has_device_library,
        has_one_library: settings.has_one_library,
        has_dev_setting: settings.dev.is_some(),
        waveform_color: match dev.color {
            WaveformColor::Blue => "blue",
            WaveformColor::Rgb => "rgb",
            WaveformColor::TriBand => "3band",
        }
        .to_owned(),
        waveform_position: match dev.position {
            WaveformPosition::Center => "center",
            WaveformPosition::Left => "left",
        }
        .to_owned(),
        overview_waveform: match dev.overview {
            OverviewWaveform::Half => "half",
            OverviewWaveform::Full => "full",
        }
        .to_owned(),
        key_display: match dev.key_display {
            KeyDisplay::Classic => "classic",
            KeyDisplay::Alphanumeric => "alphanumeric",
        }
        .to_owned(),
        has_library_settings: settings.library.is_some(),
        device_name: library.device_name.clone(),
        background_color_type: library.background_color_type,
        device_library_background_color_type: settings.device_library_background.map(i64::from),
        categories: library.categories.iter().map(slot_dto).collect(),
        sorts: library.sorts.iter().map(slot_dto).collect(),
        sub_column: library.sub_column,
        colors: library
            .colors
            .iter()
            .map(|c| ColorNameDto { id: c.id, name: c.name.clone() })
            .collect(),
    }
}

/// What to write, given what the stick currently holds.
///
/// The unexplained bytes of `DEVSETTING.DAT` come from the file as read, so
/// the current settings are the base and the wire values are applied over
/// them. A stick without the file gets the defaults plus the changes.
pub fn apply(current: &DeviceSettings, dto: &DeviceSettingsDto) -> AppResult<DeviceSettings> {
    let mut dev = current.dev.clone().unwrap_or_default();
    dev.color = match dto.waveform_color.as_str() {
        "blue" => WaveformColor::Blue,
        "rgb" => WaveformColor::Rgb,
        "3band" => WaveformColor::TriBand,
        other => return Err(bad_value("Waveform color", other)),
    };
    dev.position = match dto.waveform_position.as_str() {
        "center" => WaveformPosition::Center,
        "left" => WaveformPosition::Left,
        other => return Err(bad_value("Waveform Current Position", other)),
    };
    dev.overview = match dto.overview_waveform.as_str() {
        "half" => OverviewWaveform::Half,
        "full" => OverviewWaveform::Full,
        other => return Err(bad_value("Type of the Overview Waveform", other)),
    };
    dev.key_display = match dto.key_display.as_str() {
        "classic" => KeyDisplay::Classic,
        "alphanumeric" => KeyDisplay::Alphanumeric,
        other => return Err(bad_value("Key display format", other)),
    };

    // Library settings only go to a stick that has a library to hold them.
    let library = match current.library.as_ref() {
        Some(existing) => Some(StickSettings {
            device_name: dto.device_name.trim().to_owned(),
            background_color_type: background(
                "Background Color : OneLibrary",
                dto.background_color_type,
                existing.background_color_type,
            )?,
            categories: dto.categories.iter().map(slot_from).collect(),
            sorts: dto.sorts.iter().map(slot_from).collect(),
            sub_column: dto.sub_column,
            colors: dto
                .colors
                .iter()
                .map(|c| ColorName { id: c.id, name: c.name.clone() })
                .collect(),
        }),
        None => None,
    };
    // Only a stick whose `export.pdb` has a `property` row can hold it.
    let device_library_background =
        match (current.device_library_background, dto.device_library_background_color_type) {
            (Some(existing), Some(wanted)) => {
                let value =
                    background("Background Color : Device Library", wanted, i64::from(existing))?;
                Some(u8::try_from(value).unwrap_or(existing))
            }
            (existing, _) => existing,
        };

    Ok(DeviceSettings {
        dev: Some(dev),
        library,
        device_library_background,
        has_device_library: current.has_device_library,
        has_one_library: current.has_one_library,
    })
}

/// What a stick with no settings of its own is given on export: the
/// Preferences window's DJ System pane. Null rows mean the reference rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickDefaultsDto {
    pub waveform_color: String,
    pub waveform_position: String,
    pub overview_waveform: String,
    pub key_display: String,
    pub categories: Option<Vec<MenuSlotDto>>,
    pub sorts: Option<Vec<MenuSlotDto>>,
    pub sub_column: Option<i64>,
}

/// The reference rows a fresh `exportLibrary.db` starts from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceStickSettingsDto {
    pub categories: Vec<MenuSlotDto>,
    pub sorts: Vec<MenuSlotDto>,
}

/// rekordbox's reference browse categories and sort options: what the DJ
/// System pane edits against when nothing has been stored.
#[tauri::command]
pub fn reference_stick_settings() -> ReferenceStickSettingsDto {
    let reference = StickSettings::default();
    ReferenceStickSettingsDto {
        categories: reference.categories.iter().map(slot_dto).collect(),
        sorts: reference.sorts.iter().map(slot_dto).collect(),
    }
}

/// The library rows a fresh stick's export starts from, given the defaults.
pub fn library_defaults(dto: &StickDefaultsDto) -> StickSettings {
    let mut settings = StickSettings::default();
    if let Some(categories) = &dto.categories {
        settings.categories = categories.iter().map(slot_from).collect();
    }
    if let Some(sorts) = &dto.sorts {
        settings.sorts = sorts.iter().map(slot_from).collect();
    }
    settings.sub_column = dto.sub_column;
    settings
}

/// The `DEVSETTING.DAT` a fresh stick is given: rekordbox's file at the
/// defaults, with the four choices applied over it.
pub fn dev_defaults(dto: &StickDefaultsDto) -> AppResult<DevSetting> {
    let mut dev = DevSetting::default();
    dev.color = match dto.waveform_color.as_str() {
        "blue" => WaveformColor::Blue,
        "rgb" => WaveformColor::Rgb,
        "3band" => WaveformColor::TriBand,
        other => return Err(bad_value("Waveform color", other)),
    };
    dev.position = match dto.waveform_position.as_str() {
        "center" => WaveformPosition::Center,
        "left" => WaveformPosition::Left,
        other => return Err(bad_value("Waveform Current Position", other)),
    };
    dev.overview = match dto.overview_waveform.as_str() {
        "half" => OverviewWaveform::Half,
        "full" => OverviewWaveform::Full,
        other => return Err(bad_value("Type of the Overview Waveform", other)),
    };
    dev.key_display = match dto.key_display.as_str() {
        "classic" => KeyDisplay::Classic,
        "alphanumeric" => KeyDisplay::Alphanumeric,
        other => return Err(bad_value("Key display format", other)),
    };
    Ok(dev)
}

/// Gives a stick that holds an export but no `DEVSETTING.DAT` the defaults,
/// when the device panel opens on it — which is when rekordbox writes one
/// too. A stick that has one keeps it; a stick with no export gets nothing.
#[tauri::command]
pub async fn write_device_defaults(path: String, defaults: StickDefaultsDto) -> AppResult<DeviceSettingsDto> {
    crate::commands::blocking("write_device_defaults", move || {
        let mount = Path::new(&path);
        if !mount.is_dir() {
            return Err(AppError::new(ErrorKind::NotFound, "That device is no longer connected."));
        }
        if rbl_devices::inspect(mount).is_some() {
            write_dev_defaults(mount, &defaults)?;
        }
        Ok(to_dto(&rbl_devices::settings::read(mount)))
    })
    .await
}

/// Gives a stick that has no `DEVSETTING.DAT` the defaults. A stick that
/// has one keeps it. Called from inside a `blocking` closure, which is
/// `spawn_blocking` with a name; never from the async thread.
pub fn write_dev_defaults(mount: &Path, dto: &StickDefaultsDto) -> AppResult<()> {
    let current = rbl_devices::settings::read(mount);
    if current.dev.is_some() {
        return Ok(());
    }
    let next = DeviceSettings { dev: Some(dev_defaults(dto)?), library: None, ..current };
    rbl_devices::settings::write_changes(mount, &current, &next)
        .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))
}

/// The highest background colour: 0 Default, 1 Pink .. 8 Purple.
const MAX_BACKGROUND: i64 = 8;

/// A background colour from the wire. A value the stick already holds is
/// kept even when it is not one of the nine, so a newer rekordbox's value
/// survives a save that did not touch it.
fn background(field: &str, wanted: i64, existing: i64) -> AppResult<i64> {
    if wanted == existing || (0..=MAX_BACKGROUND).contains(&wanted) {
        Ok(wanted)
    } else {
        Err(bad_value(field, &wanted.to_string()))
    }
}

fn bad_value(field: &str, value: &str) -> AppError {
    AppError::new(ErrorKind::Malformed, format!("{field}: {value:?} is not a choice."))
}

/// Gives a stick that holds no database the folders rekordbox creates the
/// moment a drive is connected, then reads its settings.
///
/// The DJ System pane's "create music database folders" switch decides
/// whether the device panel asks for this; the panel asks when it opens a
/// device whose settings came back without a Device Library. The empty
/// database starts from `defaults` (the pane's choices) so the Category,
/// Sort, Column and Color tabs have rows to edit before any export. A
/// stick that already has a database is read and left as it is.
#[tauri::command]
pub async fn ensure_device_library(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
    path: String,
    defaults: Option<StickDefaultsDto>,
) -> AppResult<DeviceSettingsDto> {
    let state = std::sync::Arc::clone(&state);
    crate::commands::blocking("ensure_device_library", move || {
        let mount = Path::new(&path);
        if !mount.is_dir() {
            return Err(AppError::new(
                ErrorKind::NotFound,
                "That device is no longer connected. It may have been unplugged or renamed.",
            ));
        }
        let library = defaults.as_ref().map(library_defaults);
        // The library's tags go on the blank stick, as rekordbox puts them
        // there the moment a drive is connected. A library not loaded yet
        // gives none, and the stick gets its tags on its first export.
        let (my_tags, db_id) = state
            .read_db(|db| {
                let conn = db.connection();
                Ok((rbl_db::export_info::my_tags(conn)?, rbl_db::export_info::db_id(conn)?))
            })
            .unwrap_or_default();
        let my_tags: Vec<rbl_export::SourceMyTag> = my_tags.iter().map(crate::commands::source_my_tag).collect();
        let sync = rbl_export::SyncSource { db_id, tree: Vec::new(), automatic: false };
        let preferred_root = rbl_devices::list()
            .into_iter()
            .find(|device| device.mount_point == mount)
            .and_then(|device| rbl_export::ExportRoot::for_file_system(&device.file_system));
        rbl_export::create_library_with_root(mount, library.as_ref(), &my_tags, Some(&sync), preferred_root)
            .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))?;
        if let Some(defaults) = &defaults {
            write_dev_defaults(mount, defaults)?;
        }
        Ok(to_dto(&rbl_devices::settings::read(mount)))
    })
    .await
}

/// Reads a stick's settings. Never fails on a stick that holds nothing:
/// every part is optional and the tabs say what is missing.
#[tauri::command]
pub async fn device_settings(path: String) -> AppResult<DeviceSettingsDto> {
    crate::commands::blocking("device_settings", move || {
        Ok(to_dto(&rbl_devices::settings::read(Path::new(&path))))
    })
    .await
}

/// Writes a stick's settings back, and returns what the stick now holds.
#[tauri::command]
pub async fn save_device_settings(
    path: String,
    settings: DeviceSettingsDto,
) -> AppResult<DeviceSettingsDto> {
    crate::commands::blocking("save_device_settings", move || {
        let mount = Path::new(&path);
        if !mount.is_dir() {
            return Err(AppError::new(ErrorKind::NotFound, "That device is no longer connected."));
        }
        let current = rbl_devices::settings::read(mount);
        let next = apply(&current, &settings)?;
        rbl_devices::settings::write_changes(mount, &current, &next)
            .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))?;
        Ok(to_dto(&rbl_devices::settings::read(mount)))
    })
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_stick_with_nothing_shows_defaults_and_writes_no_library() {
        let empty = DeviceSettings {
            dev: None,
            library: None,
            device_library_background: None,
            has_device_library: false,
            has_one_library: false,
        };
        let dto = to_dto(&empty);
        assert!(!dto.has_dev_setting);
        assert!(!dto.has_library_settings);
        assert_eq!(dto.waveform_color, "blue");
        assert_eq!(dto.waveform_position, "center");
        // The reference rows, so the tabs have something to draw, disabled.
        assert_eq!(dto.categories.len(), 22);

        let mut changed = dto.clone();
        changed.waveform_color = "rgb".to_owned();
        changed.device_name = "FRIDAY".to_owned();
        let next = apply(&empty, &changed).unwrap();
        assert_eq!(next.dev.unwrap().color, WaveformColor::Rgb);
        assert!(next.library.is_none(), "no exportLibrary.db to write a name into");
    }

    #[test]
    fn the_library_settings_round_trip_through_the_wire_shape() {
        let stick = DeviceSettings {
            dev: Some(DevSetting::default()),
            library: Some(StickSettings::default()),
            device_library_background: Some(0),
            has_device_library: true,
            has_one_library: true,
        };
        let mut dto = to_dto(&stick);
        assert_eq!(dto.categories.len(), 22);
        assert_eq!(dto.sorts.len(), 17);
        assert_eq!(dto.colors.len(), 8);
        dto.colors[0].name = "Vocal".to_owned();
        dto.categories[0].visible = true;
        dto.categories[0].seq = 11;
        dto.sub_column = Some(2);
        dto.key_display = "alphanumeric".to_owned();
        assert_eq!(dto.device_library_background_color_type, Some(0));
        dto.background_color_type = 8;
        dto.device_library_background_color_type = Some(4);

        let next = apply(&stick, &dto).unwrap();
        assert_eq!(next.device_library_background, Some(4));
        let library = next.library.unwrap();
        assert_eq!(library.background_color_type, 8);
        assert_eq!(library.colors[0].name, "Vocal");
        assert!(library.categories[0].visible);
        assert_eq!(library.sub_column, Some(2));
        assert_eq!(next.dev.unwrap().key_display, KeyDisplay::Alphanumeric);
    }

    #[test]
    fn a_value_that_is_not_a_choice_is_refused() {
        let stick = DeviceSettings {
            dev: None,
            library: None,
            device_library_background: None,
            has_device_library: false,
            has_one_library: false,
        };
        let mut dto = to_dto(&stick);
        dto.waveform_color = "plaid".to_owned();
        assert!(apply(&stick, &dto).is_err());
    }

    #[test]
    fn a_background_colour_outside_the_nine_is_refused_unless_the_stick_has_it() {
        let stick = DeviceSettings {
            dev: None,
            library: Some(StickSettings { background_color_type: 12, ..StickSettings::default() }),
            device_library_background: Some(0),
            has_device_library: true,
            has_one_library: true,
        };
        let dto = to_dto(&stick);
        // The stick's own value goes back as it was.
        assert_eq!(apply(&stick, &dto).unwrap().library.unwrap().background_color_type, 12);

        let mut bad = dto.clone();
        bad.background_color_type = 9;
        assert!(apply(&stick, &bad).is_err());
        let mut bad = dto.clone();
        bad.device_library_background_color_type = Some(-1);
        assert!(apply(&stick, &bad).is_err());

        // A stick without the row is not given one.
        let no_row = DeviceSettings { device_library_background: None, ..stick };
        let mut dto = to_dto(&no_row);
        dto.device_library_background_color_type = Some(3);
        assert_eq!(apply(&no_row, &dto).unwrap().device_library_background, None);
    }

    fn defaults() -> StickDefaultsDto {
        StickDefaultsDto {
            waveform_color: "rgb".to_owned(),
            waveform_position: "left".to_owned(),
            overview_waveform: "full".to_owned(),
            key_display: "alphanumeric".to_owned(),
            categories: None,
            sorts: None,
            sub_column: Some(5),
        }
    }

    #[test]
    fn a_fresh_stick_is_given_a_devsetting_and_a_stick_with_one_keeps_it() {
        let stick = tempfile::tempdir().unwrap();
        // perf-ok: a test's fixture, not a command.
        std::fs::create_dir_all(stick.path().join("PIONEER")).unwrap();

        write_dev_defaults(stick.path(), &defaults()).unwrap();
        let read = rbl_devices::settings::read(stick.path());
        let dev = read.dev.expect("the file was written");
        assert_eq!(dev.color, WaveformColor::Rgb);
        assert_eq!(dev.position, WaveformPosition::Left);
        assert_eq!(dev.overview, OverviewWaveform::Full);
        assert_eq!(dev.key_display, KeyDisplay::Alphanumeric);

        // A second export with other choices leaves the stick's own file alone.
        let mut other = defaults();
        other.waveform_color = "blue".to_owned();
        write_dev_defaults(stick.path(), &other).unwrap();
        assert_eq!(rbl_devices::settings::read(stick.path()).dev.unwrap().color, WaveformColor::Rgb);
    }

    #[test]
    fn the_library_defaults_start_from_the_reference_rows() {
        let library = library_defaults(&defaults());
        assert_eq!(library.categories.len(), StickSettings::default().categories.len());
        assert_eq!(library.sub_column, Some(5));

        let mut with_rows = defaults();
        with_rows.categories = Some(vec![MenuSlotDto {
            id: 1, menu_item: 1, name: "GENRE".to_owned(), seq: 1, visible: true,
        }]);
        assert_eq!(library_defaults(&with_rows).categories.len(), 1);

        let mut bad = defaults();
        bad.key_display = "roman".to_owned();
        assert!(dev_defaults(&bad).is_err());
    }
}
