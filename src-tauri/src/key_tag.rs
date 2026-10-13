//! Preferences > Analysis > Key Analysis > "Write the value to the ID3 tag".
//!
//! rekordbox writes a track's key into its audio file at two moments, and
//! only with that setting on (`writeKeyInTag`, off by default) [OBS
//! chris-win11, rekordbox 7.2.14; static 7.2.19 `DatabaseMediator::
//! modifyTagData` @0x100c47d50]:
//!
//! - when analysis finds a key (`analyze_complete` @0x101494740), with the
//!   name it stores for that key: the classic name (`Dbm`) under View > Key
//!   display format Classic, the Camelot code (`12A`) under Alphanumeric;
//! - when the key is edited in the Info panel (`modifyTrack`), with the name
//!   as edited, whatever the display format — `Am` stays `Am` under
//!   Alphanumeric; an emptied key empties the tag's.
//!
//! A file rekordbox cannot write is passed over without a message on screen.
//! Here it is said in the log, with every other reason a file kept its tag.

use std::path::Path;

use rbl_db::key_tag::{write_key, KeyTagOutcome};

/// The key display format, which decides the name analysis writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyNotation {
    Classic,
    Alphanumeric,
}

/// A key analysis found, in both notations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedKey {
    pub classic: String,
    pub camelot: String,
}

impl DetectedKey {
    /// The name rekordbox stores, and so writes, for this key: analysis
    /// under Alphanumeric stores the Camelot code (`modTrackKeyInfo`
    /// @0x1017e350c, column 0 of `MusicKey::keyNameStr`) [OBS: `12A`].
    #[must_use]
    pub fn name_for(&self, notation: KeyNotation) -> &str {
        match notation {
            KeyNotation::Classic => &self.classic,
            KeyNotation::Alphanumeric => &self.camelot,
        }
    }
}

/// Writes `key` into the file at `path` and logs what became of it. Never
/// fails the caller: the library edit or analysis it follows has already
/// been saved, as rekordbox's has when it writes the tag.
pub fn write_logged(track_id: &str, path: &str, key: &str) {
    if path.is_empty() {
        tracing::warn!(track_id, "key not written to the file's tag: the track has no file path");
        return;
    }
    match write_key(Path::new(path), key) {
        Ok(KeyTagOutcome::Written) => tracing::info!(track_id, path, key, "key written to the file's tag"),
        Ok(KeyTagOutcome::Unsupported) => tracing::info!(
            track_id, path,
            "key not written to the file's tag: rekordbox writes a key only into MP3, AIFF and FLAC files"
        ),
        Ok(KeyTagOutcome::ReadOnly) => tracing::warn!(track_id, path, "key not written to the file's tag: the file is read-only"),
        Err(error) => tracing::warn!(track_id, path, %error, "key not written to the file's tag; the file is unchanged"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn analysis_writes_the_name_rekordbox_stores_for_each_notation() {
        let key = DetectedKey { classic: "Dbm".into(), camelot: "12A".into() };
        // [OBS chris-win11]: the same track came back `Dbm` under Classic
        // and `12A` under Alphanumeric.
        assert_eq!(key.name_for(KeyNotation::Classic), "Dbm");
        assert_eq!(key.name_for(KeyNotation::Alphanumeric), "12A");
    }

    #[test]
    fn the_notation_arrives_as_the_frontend_spells_it() {
        let parsed: KeyNotation = serde_json::from_str("\"alphanumeric\"").unwrap();
        assert_eq!(parsed, KeyNotation::Alphanumeric);
        let parsed: KeyNotation = serde_json::from_str("\"classic\"").unwrap();
        assert_eq!(parsed, KeyNotation::Classic);
    }
}
