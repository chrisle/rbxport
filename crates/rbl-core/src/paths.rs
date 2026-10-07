//! Where rekordbox keeps its own files on this machine.

use std::path::PathBuf;

/// rekordbox's settings directory: `rekordbox3.settings`, and beside it the
/// `MYSETTING.DAT`, `MYSETTING2.DAT`, `DJMMYSETTING.DAT` and `djprofile.nxs`
/// it copies to every stick it exports to. `~/Library/Application
/// Support/Pioneer/rekordbox6` on macOS, `%APPDATA%\\Pioneer\\rekordbox6`
/// on Windows [OBS 7.2.11]. `None` when rekordbox is not installed here.
#[must_use]
pub fn rekordbox_settings_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        dirs::home_dir()?.join("Library/Application Support")
    } else {
        dirs::config_dir()?
    };
    let dir = base.join("Pioneer/rekordbox6");
    dir.is_dir().then_some(dir)
}

/// The settings file in [`rekordbox_settings_dir`] that holds rekordbox's
/// Preferences as `<VALUE name="…" val="…"/>` entries [OBS 7.2.11].
pub const REKORDBOX_SETTINGS_FILE: &str = "rekordbox3.settings";

/// One value from this machine's `rekordbox3.settings`, unescaped. `None`
/// when rekordbox is not installed, the file cannot be read, or it has no
/// such value.
#[must_use]
pub fn rekordbox_setting(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(rekordbox_settings_dir()?.join(REKORDBOX_SETTINGS_FILE)).ok()?;
    setting_value(&text, name)
}

/// The `val` of the `<VALUE name="name" …/>` entry in the text of a
/// `rekordbox3.settings` file. The file is the XML JUCE writes, so values
/// carry entities (`&amp;`, `&#8217;`) that are decoded here.
#[must_use]
pub fn setting_value(settings: &str, name: &str) -> Option<String> {
    crate::xml::tags(settings).into_iter().find_map(|tag| match tag {
        crate::xml::Tag::Open { name: element, attributes, .. }
            if element == "VALUE" && attributes.iter().any(|(key, value)| key == "name" && value == name) =>
        {
            attributes.into_iter().find(|(key, _)| key == "val").map(|(_, value)| value)
        }
        _ => None,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::setting_value;

    const SETTINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>

<PROPERTIES>
  <VALUE name="DeviceLogEnable" val="0"/>
  <VALUE name="masterDbDirectory" val="/Volumes/DJ SSD/PIONEER/Master"/>
  <VALUE name="DropboxSharingPath" val="/Users/x/Dropbox-Team &amp; Co/Chris&#8217;s"/>
  <VALUE name="Empty" val=""/>
</PROPERTIES>
"#;

    #[test]
    fn a_named_value_is_read_and_unescaped() {
        assert_eq!(setting_value(SETTINGS, "masterDbDirectory").unwrap(), "/Volumes/DJ SSD/PIONEER/Master");
        assert_eq!(setting_value(SETTINGS, "DropboxSharingPath").unwrap(), "/Users/x/Dropbox-Team & Co/Chris\u{2019}s");
        assert_eq!(setting_value(SETTINGS, "Empty").unwrap(), "");
    }

    #[test]
    fn a_missing_value_or_a_broken_file_reads_as_none() {
        assert_eq!(setting_value(SETTINGS, "masterdbdirectory"), None, "names are case-sensitive");
        assert_eq!(setting_value(SETTINGS, "Nothing"), None);
        assert_eq!(setting_value("not xml at all", "masterDbDirectory"), None);
    }
}
