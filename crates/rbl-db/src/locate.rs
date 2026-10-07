//! Which library to open.
//!
//! # Where rekordbox keeps its library
//!
//! rekordbox's own record of its library is `masterDbDirectory` in
//! `rekordbox3.settings`, a folder holding `master.db` and `share/`. It is
//! read through `SettingIF::getCurrentMasterDbDirectory` and
//! `DatabaseMediator::getLibraryFolderPath`, and when it is unset the folder
//! is `~/Library/Pioneer/rekordbox` [OBS macOS 7.2.11, static analysis]. The
//! agent's `options.json` carries a `db-path` too, but rekordbox writes that
//! file — `CloudAgentAPI::Agent::start` deletes and rewrites it from
//! `masterDbDirectory` on every launch — and never reads `db-path` back to
//! choose a library [OBS 7.2.11]. So it is read here only when
//! `rekordbox3.settings` says nothing, and nothing here ever writes it.
//!
//! rekordbox moves its library between drives in Preferences › Advanced ›
//! Database management, whose Drive list offers only drives already holding
//! `PIONEER/Master/master.db` (`.PIONEER/Master` on HFS volumes). When the
//! configured folder is not the default one and its files are missing at
//! launch (`MainAppWindow::judgeIfExistDatabase`), rekordbox says it cannot
//! find the Master Database, asks for the drive to be connected, and offers
//! the default drive instead; it never makes a library on the missing drive
//! [OBS 7.2.11].
//!
//! On Windows `rekordbox3.settings` is `%APPDATA%\Pioneer\rekordbox6\`; that
//! it holds the same `masterDbDirectory` is [ASSUME], as the Windows binary
//! was not analysed. A missing or unreadable value falls through to
//! `options.json` and then the default, so a different layout degrades to
//! the previous behaviour rather than failing.
//!
//! # The order used here
//!
//! 1. [`OPTIONS_ENV`](crate::OPTIONS_ENV), when set, is the only source:
//!    a test harness pointing the app at a fixture.
//! 2. rekordbox's library: `masterDbDirectory`, else `options.json`'s
//!    `db-path`, else the default folder. Used when its `master.db` exists,
//!    because this application must work on the library rekordbox uses.
//! 3. The library chosen in this application, saved in its own data folder
//!    ([`choice_file`]). Used when it exists and rekordbox's does not: a
//!    machine without rekordbox, or rekordbox's drive not connected.
//! 4. Otherwise nothing is opened. A configured location that is missing is
//!    [`Located::Unavailable`], and no library is ever made there; with
//!    nothing configured it is [`Located::Absent`] and one may be made in the
//!    default folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{DbError, LibraryLocation, Result};

/// Who named the library [`locate`] settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// rekordbox's settings, its agent's `options.json`, or its default folder.
    Rekordbox,
    /// The choice saved in this application.
    Rbxport,
    /// [`OPTIONS_ENV`](crate::OPTIONS_ENV).
    Override,
}

/// What [`locate`] found.
#[derive(Debug, Clone)]
pub enum Located {
    /// A `master.db` to open.
    Found { location: LibraryLocation, origin: Origin },
    /// A library is configured at `master_db`, and it is not there: most often
    /// a drive that is not connected. Nothing may be made in its place.
    Unavailable { master_db: PathBuf, origin: Origin, default_master_db: PathBuf },
    /// Nothing is configured and there is no library in the default folder,
    /// which is where one may be made.
    Absent { master_db: PathBuf },
}

/// The files [`locate`] reads, so a test can point each at a folder of its own.
#[derive(Debug, Clone)]
pub struct Sources {
    /// rekordbox's `rekordbox3.settings`, when rekordbox is installed.
    pub rekordbox_settings: Option<PathBuf>,
    /// rekordbox's agent `options.json`. Read, never written.
    pub agent_options: Option<PathBuf>,
    /// This application's saved choice; see [`choice_file`].
    pub choice: Option<PathBuf>,
    /// rekordbox's default library folder.
    pub default_dir: PathBuf,
    /// Set when [`OPTIONS_ENV`](crate::OPTIONS_ENV) names the options file:
    /// nothing else is consulted.
    pub overridden: bool,
}

impl Sources {
    /// This machine's files.
    pub fn installed() -> Result<Self> {
        let default_dir = crate::default_library_dir()?;
        if let Some(chosen) = std::env::var_os(crate::OPTIONS_ENV) {
            return Ok(Self {
                rekordbox_settings: None,
                agent_options: Some(PathBuf::from(chosen)),
                choice: None,
                default_dir,
                overridden: true,
            });
        }
        Ok(Self {
            rekordbox_settings: rbl_core::paths::rekordbox_settings_dir()
                .map(|dir| dir.join(rbl_core::paths::REKORDBOX_SETTINGS_FILE)),
            agent_options: Some(crate::options_location()?),
            choice: choice_file(),
            default_dir,
            overridden: false,
        })
    }

    /// A machine laid out under `root`, for tests and tools; nothing on this
    /// machine is read. rekordbox's files go under `root/Pioneer`, this
    /// application's choice under `root/rbxport`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            rekordbox_settings: Some(root.join("Pioneer/rekordbox6").join(rbl_core::paths::REKORDBOX_SETTINGS_FILE)),
            agent_options: Some(root.join("Pioneer/rekordboxAgent/storage/options.json")),
            choice: Some(root.join("rbxport/library.json")),
            default_dir: root.join("Pioneer/rekordbox"),
            overridden: false,
        }
    }

    /// The default folder's `master.db`.
    #[must_use]
    pub fn default_master_db(&self) -> PathBuf {
        self.default_dir.join("master.db")
    }

    /// The `master.db` rekordbox is set to use: `masterDbDirectory`, else
    /// `options.json`'s `db-path`, else the default. The second value is
    /// whether anything named it.
    #[must_use]
    pub fn rekordbox_master_db(&self) -> (PathBuf, bool) {
        let configured = self
            .rekordbox_settings
            .as_deref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| rbl_core::paths::setting_value(&text, "masterDbDirectory"))
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join("master.db"))
            .or_else(|| self.agent_options().ok().and_then(|options| options.db_path));
        match configured {
            Some(master_db) => (master_db, true),
            None => (self.default_master_db(), false),
        }
    }

    /// The library saved in this application, if one was.
    #[must_use]
    pub fn chosen_master_db(&self) -> Option<PathBuf> {
        let bytes = std::fs::read(self.choice.as_deref()?).ok()?;
        let choice: Choice = serde_json::from_slice(&bytes).ok()?;
        Some(choice.master_db).filter(|path| path.is_absolute())
    }

    /// The passphrase for a library found here: the agent's `dp` when there
    /// is one, else rekordbox's standard key, which is the same value.
    pub fn passphrase(&self) -> Result<String> {
        let dp = self.agent_options().ok().and_then(|options| options.dp);
        crate::key::derive_password(dp.as_deref().unwrap_or(crate::key::REKORDBOX_DP))
    }

    fn agent_options(&self) -> Result<crate::AgentOptions> {
        let file = self
            .agent_options
            .as_deref()
            .ok_or_else(|| DbError::NotInstalled("no agent options file".into()))?;
        crate::read_agent_options(file)
    }

    /// A [`LibraryLocation`] for `master_db`, with `share/` beside it.
    pub fn location_of(&self, master_db: &Path) -> Result<LibraryLocation> {
        Ok(LibraryLocation {
            master_db: master_db.to_path_buf(),
            share_root: master_db.parent().map_or_else(|| PathBuf::from("share"), |dir| dir.join("share")),
            passphrase: self.passphrase()?,
            is_real_install: true,
        })
    }
}

/// Where this application saves the library chosen in it:
/// `library.json` in its data folder, beside its backups.
#[must_use]
pub fn choice_file() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join("rbxport/library.json"))
}

/// The saved choice, as `library.json` holds it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Choice {
    master_db: PathBuf,
}

/// Saves `master_db` as this application's library, replacing any earlier
/// choice. Written whole and renamed into place.
pub fn remember(choice_file: &Path, master_db: &Path) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(&Choice { master_db: master_db.to_path_buf() })
        .map_err(|e| DbError::Open(e.to_string()))?;
    if let Some(dir) = choice_file.parent() {
        rbl_core::durable::create_dir_all(dir)?;
    }
    rbl_core::durable::write(choice_file, &bytes)?;
    Ok(())
}

/// Which library to open on this machine; see the module docs for the order.
pub fn locate() -> Result<Located> {
    locate_with(&Sources::installed()?)
}

/// [`locate`] over the given files.
pub fn locate_with(sources: &Sources) -> Result<Located> {
    if sources.overridden {
        let options = sources
            .agent_options
            .as_deref()
            .ok_or_else(|| DbError::NotInstalled("no agent options file".into()))?;
        if !options.is_file() {
            return Err(DbError::NotInstalled(format!(
                "{} names {}, which is not a file",
                crate::OPTIONS_ENV,
                options.display()
            )));
        }
        let location = crate::detect_from(options)?;
        return Ok(if location.master_db.is_file() {
            Located::Found { location, origin: Origin::Override }
        } else {
            // A harness's own location: it may be made there.
            Located::Absent { master_db: location.master_db }
        });
    }

    let (rekordbox, configured) = sources.rekordbox_master_db();
    if rekordbox.is_file() {
        return Ok(Located::Found { location: sources.location_of(&rekordbox)?, origin: Origin::Rekordbox });
    }
    let chosen = sources.chosen_master_db();
    if let Some(chosen) = chosen.as_deref().filter(|path| path.is_file()) {
        return Ok(Located::Found { location: sources.location_of(chosen)?, origin: Origin::Rbxport });
    }

    let default_master_db = sources.default_master_db();
    if configured && rekordbox != default_master_db {
        return Ok(Located::Unavailable { master_db: rekordbox, origin: Origin::Rekordbox, default_master_db });
    }
    if let Some(chosen) = chosen.filter(|path| *path != default_master_db) {
        return Ok(Located::Unavailable { master_db: chosen, origin: Origin::Rbxport, default_master_db });
    }
    Ok(Located::Absent { master_db: default_master_db })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A machine in a temp folder: the three files `locate` reads, none of
    /// them written yet, and the default library folder.
    fn machine() -> (tempfile::TempDir, Sources) {
        let root = tempfile::tempdir().unwrap();
        let sources = Sources::under(root.path());
        (root, sources)
    }

    fn settings(sources: &Sources, master_dir: &Path) {
        let file = sources.rekordbox_settings.as_deref().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let dir = rbl_core::xml::escape(&master_dir.to_string_lossy());
        std::fs::write(
            file,
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n<PROPERTIES>\n  <VALUE name=\"masterDbDirectory\" val=\"{dir}\"/>\n</PROPERTIES>\n"),
        )
        .unwrap();
    }

    fn options(sources: &Sources, master_db: &Path) {
        let file = sources.agent_options.as_deref().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        crate::fixture::write_options_json(file, &master_db.to_string_lossy(), "options-key").unwrap();
    }

    fn database(at: &Path) {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, b"a library").unwrap();
    }

    fn found(located: Located) -> (PathBuf, Origin) {
        match located {
            Located::Found { location, origin } => (location.master_db, origin),
            other => panic!("expected a library, got {other:?}"),
        }
    }

    #[test]
    fn master_db_directory_wins_over_options_json() {
        let (root, sources) = machine();
        let drive = root.path().join("Volumes/DJ SSD/PIONEER/Master");
        let stale = root.path().join("old/master.db");
        settings(&sources, &drive);
        options(&sources, &stale);
        database(&drive.join("master.db"));
        database(&stale);

        let (master_db, origin) = found(locate_with(&sources).unwrap());
        assert_eq!(master_db, drive.join("master.db"));
        assert_eq!(origin, Origin::Rekordbox);
    }

    #[test]
    fn options_json_is_read_when_the_settings_say_nothing() {
        let (root, sources) = machine();
        let elsewhere = root.path().join("elsewhere/master.db");
        options(&sources, &elsewhere);
        database(&elsewhere);
        let before = std::fs::read(sources.agent_options.as_deref().unwrap()).unwrap();

        let located = locate_with(&sources).unwrap();
        let Located::Found { location, origin } = located else { panic!("not found") };
        assert_eq!(location.master_db, elsewhere);
        assert_eq!(location.passphrase, "options-key", "the agent's dp is used when there is one");
        assert_eq!(origin, Origin::Rekordbox);
        assert_eq!(std::fs::read(sources.agent_options.as_deref().unwrap()).unwrap(), before, "options.json is only read");
    }

    #[test]
    fn the_default_folder_is_found_with_nothing_configured() {
        let (_root, sources) = machine();
        database(&sources.default_master_db());
        let Located::Found { location, origin } = locate_with(&sources).unwrap() else { panic!("not found") };
        assert_eq!(location.master_db, sources.default_master_db());
        assert_eq!(origin, Origin::Rekordbox);
        assert_eq!(location.passphrase, crate::key::derive_password(crate::key::REKORDBOX_DP).unwrap());
    }

    #[test]
    fn rekordbox_s_library_wins_over_the_saved_choice() {
        let (root, sources) = machine();
        let drive = root.path().join("Volumes/A/PIONEER/Master");
        let chosen = root.path().join("Volumes/B/PIONEER/Master/master.db");
        settings(&sources, &drive);
        database(&drive.join("master.db"));
        database(&chosen);
        remember(sources.choice.as_deref().unwrap(), &chosen).unwrap();

        assert_eq!(found(locate_with(&sources).unwrap()).0, drive.join("master.db"));
    }

    #[test]
    fn the_saved_choice_is_used_when_rekordbox_s_library_is_missing() {
        let (root, sources) = machine();
        let chosen = root.path().join("media/ryan/T7/PIONEER/Master/master.db");
        database(&chosen);
        remember(sources.choice.as_deref().unwrap(), &chosen).unwrap();

        let (master_db, origin) = found(locate_with(&sources).unwrap());
        assert_eq!(master_db, chosen);
        assert_eq!(origin, Origin::Rbxport);
    }

    #[test]
    fn a_configured_drive_that_is_not_connected_is_unavailable_not_absent() {
        let (root, sources) = machine();
        let drive = root.path().join("Volumes/DJ SSD/PIONEER/Master");
        settings(&sources, &drive);

        match locate_with(&sources).unwrap() {
            Located::Unavailable { master_db, origin, default_master_db } => {
                assert_eq!(master_db, drive.join("master.db"));
                assert_eq!(origin, Origin::Rekordbox);
                assert_eq!(default_master_db, sources.default_master_db());
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
        assert!(!drive.exists(), "looking never makes the folder");
    }

    #[test]
    fn a_saved_choice_on_a_missing_drive_is_unavailable() {
        let (root, sources) = machine();
        let chosen = root.path().join("media/ryan/T7/PIONEER/Master/master.db");
        remember(sources.choice.as_deref().unwrap(), &chosen).unwrap();
        match locate_with(&sources).unwrap() {
            Located::Unavailable { master_db, origin, .. } => {
                assert_eq!(master_db, chosen);
                assert_eq!(origin, Origin::Rbxport);
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
    }

    #[test]
    fn nothing_anywhere_is_absent_at_the_default() {
        let (_root, sources) = machine();
        match locate_with(&sources).unwrap() {
            Located::Absent { master_db } => assert_eq!(master_db, sources.default_master_db()),
            other => panic!("expected absent, got {other:?}"),
        }
    }

    #[test]
    fn a_default_folder_named_in_the_settings_is_absent_when_empty() {
        let (_root, sources) = machine();
        settings(&sources, &sources.default_dir.clone());
        assert!(matches!(locate_with(&sources).unwrap(), Located::Absent { .. }));
    }

    #[test]
    fn a_broken_choice_file_is_ignored() {
        let (_root, sources) = machine();
        let file = sources.choice.as_deref().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, b"{ not json").unwrap();
        assert!(matches!(locate_with(&sources).unwrap(), Located::Absent { .. }));
    }
}
