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
//!
//! rekordbox has no library Undo (see `docs/reference/library-undo.md`), so
//! nothing it does settles what Undo of a key edit does to the file. Here
//! Chris's choice of 2026-10-22 on #507 holds: the history entry keeps each
//! file the edit wrote and the key it held, and Undo and Redo put the file's
//! key back with the library's, or refuse and change neither.

use std::path::{Path, PathBuf};

use rbl_db::key_tag::{swap_key, write_key, KeySwap, KeyTagOutcome};

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

/// One audio file a key edit wrote its key into (#507): the key the tag
/// held before (`None`: no key) and the key written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyTagWrite {
    pub(crate) path: PathBuf,
    pub(crate) before: Option<String>,
    pub(crate) after: String,
}

impl KeyTagWrite {
    /// What the file must hold for this step to run, and what it gets.
    fn step(&self, undo: bool) -> (Option<&str>, Option<&str>) {
        if undo {
            (Some(self.after.as_str()), self.before.as_deref())
        } else {
            (self.before.as_deref(), Some(self.after.as_str()))
        }
    }

    fn name(&self) -> String {
        self.path.file_name().map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned())
    }
}

/// [`write_logged`] for an Info panel key edit, keeping what the history
/// needs to put each file back (#507): only the files whose tag this wrote.
/// A file passed over (no path, a format rekordbox writes no key into, a
/// read-only file, a write that failed) is left out, so Undo of it stays a
/// library-only Undo.
pub(crate) fn write_recorded(track_id: &str, path: &str, key: &str) -> Option<KeyTagWrite> {
    if path.is_empty() {
        tracing::warn!(track_id, "key not written to the file's tag: the track has no file path");
        return None;
    }
    match swap_key(Path::new(path), None, Some(key)) {
        Ok(KeySwap::Written { before }) => {
            tracing::info!(track_id, path, key, "key written to the file's tag");
            Some(KeyTagWrite { path: PathBuf::from(path), before, after: key.to_owned() })
        }
        Ok(KeySwap::Unsupported) => {
            tracing::info!(
                track_id, path,
                "key not written to the file's tag: rekordbox writes a key only into MP3, AIFF and FLAC files"
            );
            None
        }
        Ok(KeySwap::ReadOnly) => {
            tracing::warn!(track_id, path, "key not written to the file's tag: the file is read-only");
            None
        }
        Ok(KeySwap::Changed { .. }) => None,
        Err(error) => {
            tracing::warn!(track_id, path, %error, "key not written to the file's tag; the file is unchanged");
            None
        }
    }
}

fn verb(undo: bool) -> &'static str {
    if undo { "undo" } else { "redo" }
}

/// Refuses, before anything is written, an Undo (`undo`) or Redo of `files`
/// when one no longer holds the key the step expects: changed by another
/// app, or a Reload Tag elsewhere, since. A file that is gone is a change
/// too. Either is [`rbl_db::DbError::Conflict`]: the entry leaves the
/// history and the newer state stays (#506). A file that cannot be read for
/// another reason fails the step and keeps the entry, to try again.
pub(crate) fn check_restore(files: &[KeyTagWrite], undo: bool, label: &str) -> Result<(), rbl_db::DbError> {
    for file in files {
        let (expected, _) = file.step(undo);
        if matches!(std::fs::metadata(&file.path), Err(error) if error.kind() == std::io::ErrorKind::NotFound) {
            return Err(rbl_db::DbError::Conflict(format!("“{}” is no longer there", file.name())));
        }
        match rbl_db::key_tag::read_key(&file.path) {
            Ok(found) if found.as_deref() == expected => {}
            Ok(_) => return Err(changed(file)),
            Err(error) => {
                return Err(rbl_db::DbError::Io(std::io::Error::other(format!(
                    "Can't {} {label}: the key tag of “{}” could not be read ({error}). Nothing was changed.",
                    verb(undo),
                    file.name(),
                ))));
            }
        }
    }
    Ok(())
}

fn changed(file: &KeyTagWrite) -> rbl_db::DbError {
    rbl_db::DbError::Conflict(format!("the key tag of “{}” has changed since", file.name()))
}

/// Puts each of `files` to the key the Undo (`undo`) or Redo gives it, each
/// only while it still holds the key the step expects. When one cannot be
/// written, the ones already written are put back as they were and the step
/// fails with nothing changed, so the library is never left with a key its
/// files do not hold, nor the files with one the library does not.
pub(crate) fn restore(files: &[KeyTagWrite], undo: bool, label: &str) -> Result<(), rbl_db::DbError> {
    let ordered: Vec<&KeyTagWrite> = if undo { files.iter().rev().collect() } else { files.iter().collect() };
    for (at, file) in ordered.iter().enumerate() {
        let (expected, key) = file.step(undo);
        let why = match swap_key(&file.path, Some(expected), key) {
            Ok(KeySwap::Written { .. }) => continue,
            // Checked before anything was written; another writer got there
            // in between.
            Ok(KeySwap::Changed { .. }) => None,
            Ok(KeySwap::ReadOnly) => Some("the file is read-only".to_owned()),
            Ok(KeySwap::Unsupported) => Some("the file is no longer one a key is written into".to_owned()),
            Err(error) => Some(error.to_string()),
        };
        let stranded = revert(ordered.get(..at).unwrap_or_default(), undo);
        let why = match why {
            Some(why) => why,
            None if stranded == 0 => return Err(changed(file)),
            None => "its key tag has changed since".to_owned(),
        };
        tracing::warn!(path = %file.path.display(), undo, %why, stranded, "key tag not restored");
        return Err(failure(undo, label, &file.name(), &why, stranded));
    }
    Ok(())
}

/// Puts `written` back as they were before [`restore`] wrote them, last
/// first; answers how many could not be.
pub(crate) fn revert(written: &[&KeyTagWrite], undo: bool) -> usize {
    written
        .iter()
        .rev()
        .filter(|file| {
            let (back, now) = file.step(!undo);
            match swap_key(&file.path, Some(back), now) {
                Ok(KeySwap::Written { .. }) => false,
                other => {
                    tracing::error!(path = %file.path.display(), ?other, "a key tag could not be changed back after a failed Undo or Redo");
                    true
                }
            }
        })
        .count()
}

/// The error of an Undo or Redo stopped by a file, said as the history's
/// own refusals are.
pub(crate) fn failure(undo: bool, label: &str, name: &str, why: &str, stranded: usize) -> rbl_db::DbError {
    let outcome = if stranded == 0 {
        "Nothing was changed.".to_owned()
    } else {
        format!("{stranded} file(s) already written could not be changed back.")
    };
    rbl_db::DbError::Io(std::io::Error::other(format!(
        "Can't {} {label}: the key could not be written to “{name}” ({why}). {outcome}",
        verb(undo),
    )))
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
