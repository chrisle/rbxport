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

/// Where this machine's `rekordbox3.settings` is, or would be once written:
/// in [`rekordbox_settings_dir`] whether or not that folder exists yet.
#[must_use]
pub fn rekordbox_settings_file() -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        dirs::home_dir()?.join("Library/Application Support")
    } else {
        dirs::config_dir()?
    };
    Some(base.join("Pioneer/rekordbox6").join(REKORDBOX_SETTINGS_FILE))
}

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

/// The text of a `rekordbox3.settings` file with the `<VALUE name="name"/>`
/// entry's `val` set to `value`, every other byte left as it was. A missing
/// entry is added as the last one; with no file (`None`) the file is just
/// that entry, in the layout JUCE's `PropertiesFile` writes [OBS 7.2.11].
/// `None` when the text is not a settings file this can edit safely: no
/// `</PROPERTIES>` to add an entry before.
#[must_use]
pub fn with_setting_value(settings: Option<&str>, name: &str, value: &str) -> Option<String> {
    let entry = format!("<VALUE name=\"{}\" val=\"{}\"/>", juce_escape(name), juce_escape(value));
    let Some(text) = settings else {
        return Some(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n<PROPERTIES>\n  {entry}\n</PROPERTIES>\n"));
    };
    let mut from = 0;
    while let Some(found) = text.get(from..).and_then(|rest| rest.find("<VALUE")) {
        let start = from + found;
        let end = start + text.get(start..)?.find('>')? + 1;
        let element = text.get(start..end)?;
        let named = crate::xml::tags(element).into_iter().any(|tag| match tag {
            crate::xml::Tag::Open { attributes, .. } => attributes.iter().any(|(key, val)| key == "name" && val == name),
            crate::xml::Tag::Close { .. } => false,
        });
        if named {
            return Some(format!("{}{entry}{}", text.get(..start)?, text.get(end..)?));
        }
        from = end;
    }
    let close = text.rfind("</PROPERTIES>")?;
    Some(format!("{}  {entry}\n{}", text.get(..close)?, text.get(close..)?))
}

/// An attribute value as JUCE's XML writer escapes it: the five XML
/// entities, and every character outside printable ASCII as `&#N;`, which
/// is how `rekordbox3.settings` holds a path like `Chris&#8217;s` [OBS 7.2.11].
fn juce_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            ' '..='~' => out.push(c),
            _ => {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("&#{};", u32::from(c)));
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{setting_value, with_setting_value};

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

    #[test]
    fn a_value_is_replaced_in_place_and_nothing_else_changes() {
        let edited = with_setting_value(Some(SETTINGS), "masterDbDirectory", "/Users/x/Library/Pioneer/rekordbox").unwrap();
        assert_eq!(
            edited,
            SETTINGS.replace("/Volumes/DJ SSD/PIONEER/Master", "/Users/x/Library/Pioneer/rekordbox"),
        );
        assert_eq!(setting_value(&edited, "masterDbDirectory").unwrap(), "/Users/x/Library/Pioneer/rekordbox");
    }

    #[test]
    fn a_missing_value_is_added_before_the_end() {
        let without = SETTINGS.replace("  <VALUE name=\"masterDbDirectory\" val=\"/Volumes/DJ SSD/PIONEER/Master\"/>\n", "");
        let edited = with_setting_value(Some(&without), "masterDbDirectory", "E:\\PIONEER\\Master").unwrap();
        assert!(edited.starts_with(&without.replace("</PROPERTIES>\n", "")));
        assert!(edited.ends_with("  <VALUE name=\"masterDbDirectory\" val=\"E:\\PIONEER\\Master\"/>\n</PROPERTIES>\n"));
        assert_eq!(setting_value(&edited, "DropboxSharingPath").unwrap(), "/Users/x/Dropbox-Team & Co/Chris\u{2019}s");
    }

    #[test]
    fn values_are_escaped_as_juce_writes_them() {
        let edited = with_setting_value(None, "masterDbDirectory", "/Volumes/Chris\u{2019}s \"A&B\"/PIONEER/Master").unwrap();
        assert!(edited.contains("val=\"/Volumes/Chris&#8217;s &quot;A&amp;B&quot;/PIONEER/Master\""));
        assert_eq!(setting_value(&edited, "masterDbDirectory").unwrap(), "/Volumes/Chris\u{2019}s \"A&B\"/PIONEER/Master");
        assert!(edited.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n<PROPERTIES>\n"));
    }

    #[test]
    fn a_file_that_is_not_a_settings_file_is_not_edited() {
        assert_eq!(with_setting_value(Some("not xml at all"), "masterDbDirectory", "/x"), None);
    }
}
