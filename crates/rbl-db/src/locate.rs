//! Which library to open, and switching it.
//!
//! # Where rekordbox keeps its library
//!
//! rekordbox's own record of its library is `masterDbDirectory` in
//! `rekordbox3.settings`, a folder holding `master.db` and `share/`. It is
//! read through `SettingIF::getCurrentMasterDbDirectory` and
//! `DatabaseMediator::getLibraryFolderPath`, and when it is unset the folder
//! is `~/Library/Pioneer/rekordbox` [OBS macOS 7.2.11, static analysis].
//! `MainAppWindow::judgeIfExistDatabase` resets a value that is not an
//! absolute path to that default (`File::isAbsolutePath`, then
//! `setMasterDbDirectory(ConfigPath())`). The agent's `options.json` carries
//! a `db-path` too, but rekordbox writes that file —
//! `CloudAgentAPI::Agent::start` deletes and rewrites it from
//! `masterDbDirectory` on every launch — and never reads `db-path` back
//! [OBS 7.2.11]. It is read here only when `rekordbox3.settings` says
//! nothing, which is how an earlier rbxport recorded a library it made on a
//! machine without rekordbox; nothing here writes it.
//!
//! # Switching, as rekordbox does it
//!
//! rekordbox has one place that chooses the library: Preferences ›
//! Advanced › Database › Database management, whose drive list is the
//! drives that hold `PIONEER/Master/master.db` (`.PIONEER/Master` on HFS),
//! plus the default drive (`DetailDatabaseManagement::setup`,
//! `hasMasterDB`). Choosing one asks "Are you sure you want to switch
//! Master Database?" and sets `masterDbDirectory` to that folder
//! (`DetailDatabaseManagement::selectDrive`, `DatabaseIF::selectLibrary`).
//! [`switch_to`] is that, for the drive's `master.db`.
//!
//! When `masterDbDirectory` is not the default and its database is missing
//! at launch, rekordbox shows `MasterDbMissingWindow`: "Cannot find Master
//! Database. Launch rekordbox after connecting a drive where Master
//! Database is stored. Do you want to open Master Database in the default
//! drive?" with Yes and No. Yes confirms "Location of Master Database will
//! be changed to the default drive." and sets `masterDbDirectory` to the
//! default folder; No ends the launch. It never makes a library on the
//! missing drive [OBS 7.2.11]. [`use_default`] is the Yes.
//!
//! Both write `masterDbDirectory` into `rekordbox3.settings`, the setting
//! rekordbox itself reads, so rekordbox and this application open the same
//! library afterwards, as they would after a switch made in rekordbox. The
//! rest of the file is left byte for byte as it was. It is not written while
//! rekordbox runs, as rekordbox writes its settings back when it quits.
//!
//! On Windows `rekordbox3.settings` is `%APPDATA%\Pioneer\rekordbox6\` and
//! holds the same `masterDbDirectory`, written with forward slashes
//! (`C:/Users/chris/AppData/Roaming/Pioneer/rekordbox`) [OBS rekordbox 7.2.x,
//! chris-win11 2026-10-08, file read only]; the Windows binary was not
//! analysed. A missing or unreadable value falls through to `options.json`
//! and then the default.
//!
//! # The order used here
//!
//! 1. [`OPTIONS_ENV`](crate::OPTIONS_ENV), when set, is the only source:
//!    a test harness pointing the app at a fixture.
//! 2. `masterDbDirectory`, else `options.json`'s `db-path`, else the
//!    default folder.
//! 3. A configured library that is not the default one and is missing is
//!    [`Located::Unavailable`], and no library is ever made there; a
//!    missing library in the default folder is [`Located::Absent`], and one
//!    may be made there.

use std::path::{Path, PathBuf};

use crate::{DbError, LibraryLocation, Result};

/// The setting in `rekordbox3.settings` that names rekordbox's library folder.
pub const MASTER_DB_DIRECTORY: &str = "masterDbDirectory";

/// What [`locate`] found.
#[derive(Debug, Clone)]
pub enum Located {
    /// A `master.db` to open.
    Found { location: LibraryLocation },
    /// A library is configured at `master_db`, and it is not there: most often
    /// a drive that is not connected. Nothing may be made in its place.
    Unavailable { master_db: PathBuf, default_master_db: PathBuf },
    /// There is no library in the default folder, which is where one may be
    /// made.
    Absent { master_db: PathBuf },
}

/// The files [`locate`] reads, so a test can point each at a folder of its own.
#[derive(Debug, Clone)]
pub struct Sources {
    /// rekordbox's `rekordbox3.settings`, whether or not it exists yet.
    pub rekordbox_settings: Option<PathBuf>,
    /// rekordbox's agent `options.json`. Read, never written.
    pub agent_options: Option<PathBuf>,
    /// rekordbox's default library folder.
    pub default_dir: PathBuf,
    /// Set when [`OPTIONS_ENV`](crate::OPTIONS_ENV) names the options file:
    /// nothing else is consulted, and nothing is written.
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
                default_dir,
                overridden: true,
            });
        }
        Ok(Self {
            rekordbox_settings: rbl_core::paths::rekordbox_settings_file(),
            agent_options: Some(crate::options_location()?),
            default_dir,
            overridden: false,
        })
    }

    /// A machine laid out under `root`, for tests and tools; nothing on this
    /// machine is read. rekordbox's files go under `root/Pioneer`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            rekordbox_settings: Some(root.join("Pioneer/rekordbox6").join(rbl_core::paths::REKORDBOX_SETTINGS_FILE)),
            agent_options: Some(root.join("Pioneer/rekordboxAgent/storage/options.json")),
            default_dir: root.join("Pioneer/rekordbox"),
            overridden: false,
        }
    }

    /// The default folder's `master.db`.
    #[must_use]
    pub fn default_master_db(&self) -> PathBuf {
        self.default_dir.join("master.db")
    }

    /// `masterDbDirectory`, when it is set to an absolute path.
    #[must_use]
    pub fn master_db_directory(&self) -> Option<PathBuf> {
        self.rekordbox_settings
            .as_deref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| rbl_core::paths::setting_value(&text, MASTER_DB_DIRECTORY))
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
    }

    /// The `master.db` rekordbox is set to use: `masterDbDirectory`, else
    /// `options.json`'s `db-path`, else the default.
    #[must_use]
    pub fn configured_master_db(&self) -> PathBuf {
        self.master_db_directory()
            .map(|dir| dir.join("master.db"))
            .or_else(|| self.agent_options().ok().and_then(|options| options.db_path))
            .unwrap_or_else(|| self.default_master_db())
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

    /// Sets `masterDbDirectory` to `dir`, as rekordbox's Database management
    /// does: the rest of `rekordbox3.settings` is kept as it is, and the
    /// file is written whole and renamed into place. Made, with only this
    /// entry, when there is none yet (a machine without rekordbox).
    pub fn set_master_db_directory(&self, dir: &Path) -> Result<()> {
        if self.overridden {
            return Err(DbError::Open(format!("{} is set; the library is not switched", crate::OPTIONS_ENV)));
        }
        let file = self
            .rekordbox_settings
            .as_deref()
            .ok_or_else(|| DbError::Open("there is no rekordbox settings folder here".into()))?;
        let value = setting_path(
            dir.to_str()
                .ok_or_else(|| DbError::Open(format!("{} is not a path rekordbox can store", dir.display())))?,
            cfg!(windows),
        );
        let existing = match std::fs::read(file) {
            Ok(bytes) => Some(String::from_utf8(bytes).map_err(|_| {
                DbError::Open(format!("{} is not text; it was left as it is", file.display()))
            })?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let text = rbl_core::paths::with_setting_value(existing.as_deref(), MASTER_DB_DIRECTORY, &value)
            .ok_or_else(|| DbError::Open(format!("{} is not a settings file; it was left as it is", file.display())))?;
        if let Some(folder) = file.parent() {
            rbl_core::durable::create_dir_all(folder)?;
        }
        rbl_core::durable::write(file, text.as_bytes())?;
        Ok(())
    }
}

/// A folder as rekordbox writes it into `masterDbDirectory`: with forward
/// slashes on Windows, `C:/Users/chris/AppData/Roaming/Pioneer/rekordbox`
/// [OBS rekordbox 7.2.x, chris-win11 2026-10-08].
fn setting_path(dir: &str, windows: bool) -> String {
    if windows { dir.replace('\\', "/") } else { dir.to_owned() }
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
            Located::Found { location }
        } else {
            // A harness's own location: it may be made there.
            Located::Absent { master_db: location.master_db }
        });
    }

    let master_db = sources.configured_master_db();
    if master_db.is_file() {
        return Ok(Located::Found { location: sources.location_of(&master_db)? });
    }
    let default_master_db = sources.default_master_db();
    if master_db != default_master_db {
        return Ok(Located::Unavailable { master_db, default_master_db });
    }
    Ok(Located::Absent { master_db })
}

/// Makes `master_db`, a library found on a drive, the library: what
/// choosing a drive in rekordbox's Database management does. See
/// [`switch_with`].
pub fn switch_to(master_db: &Path) -> Result<LibraryLocation> {
    may_write_installed_settings()?;
    switch_with(&Sources::installed()?, master_db)
}

/// [`switch_to`] over the given files.
///
/// The library is opened read-only with rekordbox's key and its schema
/// checked first, so a file that is not a library is refused ("Failed to
/// switch Master Database.") and the setting is left alone.
pub fn switch_with(sources: &Sources, master_db: &Path) -> Result<LibraryLocation> {
    let dir = master_db
        .parent()
        .filter(|_| master_db.file_name().is_some_and(|name| name == "master.db") && master_db.is_file())
        .ok_or_else(|| DbError::NotInstalled(format!("{} is not a master.db", master_db.display())))?;
    let location = sources.location_of(master_db)?;
    drop(crate::Library::open(location.clone(), crate::OpenMode::ReadOnly)?);
    sources.set_master_db_directory(dir)?;
    Ok(location)
}

/// The Yes of rekordbox's "Cannot find Master Database" window: sets
/// `masterDbDirectory` to the default folder. What is in that folder is
/// then opened, or offered to be made, as on any start. See
/// [`use_default_with`].
pub fn use_default() -> Result<()> {
    may_write_installed_settings()?;
    use_default_with(&Sources::installed()?)
}

/// [`use_default`] over the given files. Nothing is made or changed on the
/// missing drive, nor in the default folder.
pub fn use_default_with(sources: &Sources) -> Result<()> {
    sources.set_master_db_directory(&sources.default_dir)
}

/// Whether this machine's `rekordbox3.settings` may be written now. Never
/// under [`test_mode`](crate::test_mode), whose tests use [`Sources::under`];
/// and not while rekordbox runs, as it writes its settings back when it
/// quits and would undo the change.
fn may_write_installed_settings() -> Result<()> {
    if crate::test_mode() {
        return Err(DbError::Open("rekordbox's settings are not written in test mode".into()));
    }
    if crate::is_rekordbox_running() {
        return Err(DbError::Open("Quit rekordbox first: it puts its own setting back when it quits.".into()));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::fixture::FIXTURE_PASSPHRASE;

    /// A machine in a temp folder: the files `locate` reads, none of them
    /// written yet, and the default library folder.
    fn machine() -> (tempfile::TempDir, Sources) {
        let root = tempfile::tempdir().unwrap();
        let sources = Sources::under(root.path());
        (root, sources)
    }

    fn settings_file(sources: &Sources) -> &Path {
        sources.rekordbox_settings.as_deref().unwrap()
    }

    /// A settings file shaped like rekordbox's, with `masterDbDirectory`.
    fn settings(sources: &Sources, master_dir: &Path) {
        let file = settings_file(sources);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let dir = rbl_core::xml::escape(&master_dir.to_string_lossy());
        std::fs::write(
            file,
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\n<PROPERTIES>\n  <VALUE name=\"DeviceLogEnable\" val=\"0\"/>\n  <VALUE name=\"masterDbDirectory\" val=\"{dir}\"/>\n  <VALUE name=\"ColorType\" val=\"3\"/>\n</PROPERTIES>\n"),
        )
        .unwrap();
    }

    fn options(sources: &Sources, master_db: &Path, passphrase: &str) {
        let file = sources.agent_options.as_deref().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        crate::fixture::write_options_json(file, &master_db.to_string_lossy(), passphrase).unwrap();
    }

    fn database(at: &Path) {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, b"a library").unwrap();
    }

    /// A real, keyed library at `dir/master.db`, opened with the fixture key
    /// that `options.json` names.
    fn library(sources: &Sources, dir: &Path) -> PathBuf {
        if !sources.agent_options.as_deref().unwrap().exists() {
            options(sources, &sources.default_master_db(), FIXTURE_PASSPHRASE);
        }
        crate::fixture::build(dir, crate::fixture::Shape::default()).unwrap().master_db
    }

    fn found(located: Located) -> PathBuf {
        match located {
            Located::Found { location } => location.master_db,
            other => panic!("expected a library, got {other:?}"),
        }
    }

    #[test]
    fn master_db_directory_wins_over_options_json() {
        let (root, sources) = machine();
        let drive = root.path().join("Volumes/DJ SSD/PIONEER/Master");
        let stale = root.path().join("old/master.db");
        settings(&sources, &drive);
        options(&sources, &stale, "options-key");
        database(&drive.join("master.db"));
        database(&stale);
        assert_eq!(found(locate_with(&sources).unwrap()), drive.join("master.db"));
    }

    #[test]
    fn options_json_is_read_when_the_settings_say_nothing() {
        let (root, sources) = machine();
        let elsewhere = root.path().join("elsewhere/master.db");
        options(&sources, &elsewhere, "options-key");
        database(&elsewhere);
        let before = std::fs::read(sources.agent_options.as_deref().unwrap()).unwrap();

        let Located::Found { location } = locate_with(&sources).unwrap() else { panic!("not found") };
        assert_eq!(location.master_db, elsewhere);
        assert_eq!(location.passphrase, "options-key", "the agent's dp is used when there is one");
        assert_eq!(std::fs::read(sources.agent_options.as_deref().unwrap()).unwrap(), before, "options.json is only read");
    }

    #[test]
    fn the_default_folder_is_found_with_nothing_configured() {
        let (_root, sources) = machine();
        database(&sources.default_master_db());
        let Located::Found { location } = locate_with(&sources).unwrap() else { panic!("not found") };
        assert_eq!(location.master_db, sources.default_master_db());
        assert_eq!(location.passphrase, crate::key::derive_password(crate::key::REKORDBOX_DP).unwrap());
    }

    #[test]
    fn a_relative_master_db_directory_means_the_default_as_in_rekordbox() {
        let (_root, sources) = machine();
        settings(&sources, Path::new("PIONEER/Master"));
        database(&sources.default_master_db());
        assert_eq!(found(locate_with(&sources).unwrap()), sources.default_master_db());
    }

    #[test]
    fn a_configured_drive_that_is_not_connected_is_unavailable_not_absent() {
        let (root, sources) = machine();
        let drive = root.path().join("Volumes/DJ SSD/PIONEER/Master");
        settings(&sources, &drive);
        database(&sources.default_master_db());

        match locate_with(&sources).unwrap() {
            Located::Unavailable { master_db, default_master_db } => {
                assert_eq!(master_db, drive.join("master.db"));
                assert_eq!(default_master_db, sources.default_master_db());
            }
            other => panic!("expected unavailable, got {other:?}"),
        }
        assert!(!drive.exists(), "looking never makes the folder");
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
    fn switching_sets_master_db_directory_and_keeps_every_other_setting() {
        let (root, sources) = machine();
        settings(&sources, &sources.default_dir.clone());
        let before = std::fs::read_to_string(settings_file(&sources)).unwrap();
        let drive = root.path().join("Volumes/DJ SSD/PIONEER/Master");
        let master_db = library(&sources, &drive);
        let library_before = std::fs::read(&master_db).unwrap();

        let switched = switch_with(&sources, &master_db).unwrap();
        assert_eq!(switched.master_db, master_db);

        let after = std::fs::read_to_string(settings_file(&sources)).unwrap();
        let default_dir = rbl_core::xml::escape(&sources.default_dir.to_string_lossy());
        let drive_dir = rbl_core::xml::escape(&drive.to_string_lossy());
        assert_eq!(after, before.replace(&default_dir, &drive_dir), "only masterDbDirectory changes");
        assert_eq!(found(locate_with(&sources).unwrap()), master_db);
        assert_eq!(std::fs::read(&master_db).unwrap(), library_before, "the library itself is only read");
    }

    #[test]
    fn switching_without_rekordbox_makes_a_settings_file_with_only_the_library() {
        let (root, sources) = machine();
        // An empty library an earlier build made and named in options.json.
        options(&sources, &sources.default_master_db(), FIXTURE_PASSPHRASE);
        database(&sources.default_master_db());
        let drive = root.path().join("media/ryan/T7/PIONEER/Master");
        let master_db = library(&sources, &drive);
        assert_eq!(found(locate_with(&sources).unwrap()), sources.default_master_db());

        switch_with(&sources, &master_db).unwrap();
        let text = std::fs::read_to_string(settings_file(&sources)).unwrap();
        assert_eq!(rbl_core::paths::setting_value(&text, MASTER_DB_DIRECTORY).unwrap(), drive.to_string_lossy());
        assert_eq!(found(locate_with(&sources).unwrap()), master_db);
    }

    #[test]
    fn a_file_that_is_not_a_library_is_refused_and_nothing_is_written() {
        let (root, sources) = machine();
        settings(&sources, &sources.default_dir.clone());
        let before = std::fs::read(settings_file(&sources)).unwrap();
        let drive = root.path().join("Volumes/B/PIONEER/Master/master.db");
        database(&drive);
        assert!(switch_with(&sources, &drive).is_err());
        assert!(switch_with(&sources, &root.path().join("Volumes/C/PIONEER/Master/master.db")).is_err());
        assert_eq!(std::fs::read(settings_file(&sources)).unwrap(), before);
    }

    #[test]
    fn using_the_default_resets_master_db_directory_and_makes_nothing() {
        let (root, sources) = machine();
        let drive = root.path().join("Volumes/DJ SSD/PIONEER/Master");
        settings(&sources, &drive);
        assert!(matches!(locate_with(&sources).unwrap(), Located::Unavailable { .. }));

        use_default_with(&sources).unwrap();
        let text = std::fs::read_to_string(settings_file(&sources)).unwrap();
        assert_eq!(rbl_core::paths::setting_value(&text, MASTER_DB_DIRECTORY).unwrap(), sources.default_dir.to_string_lossy());
        assert!(rbl_core::paths::setting_value(&text, "ColorType").is_some(), "other settings stay");
        assert!(!drive.exists(), "nothing is made on the missing drive");
        assert!(!sources.default_master_db().exists(), "nor in the default folder");
        assert!(matches!(locate_with(&sources).unwrap(), Located::Absent { .. }));
    }

    #[test]
    fn windows_folders_are_written_with_forward_slashes_as_rekordbox_writes_them() {
        assert_eq!(setting_path("E:\\PIONEER\\Master", true), "E:/PIONEER/Master");
        assert_eq!(
            setting_path("C:\\Users\\chris\\AppData\\Roaming\\Pioneer\\rekordbox", true),
            "C:/Users/chris/AppData/Roaming/Pioneer/rekordbox",
        );
        assert_eq!(setting_path("/Volumes/DJ SSD/PIONEER/Master", false), "/Volumes/DJ SSD/PIONEER/Master");
    }

    #[test]
    fn an_overridden_machine_is_never_switched() {
        let (root, mut sources) = machine();
        sources.overridden = true;
        assert!(use_default_with(&sources).is_err());
        assert!(!root.path().join("Pioneer/rekordbox6").exists());
    }

    #[test]
    fn the_installed_settings_are_never_written_in_test_mode() {
        if crate::test_mode() {
            assert!(use_default().is_err());
            assert!(switch_to(Path::new("/nowhere/PIONEER/Master/master.db")).is_err());
        }
    }
}
