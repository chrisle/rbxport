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
//! 4. Unless the default folder is a drive's export: rekordbox's folders
//!    pointed at a stick's `PIONEER` folder with a symbolic link or
//!    junction, so that `…/Pioneer/rekordbox` is the stick's
//!    `PIONEER/rekordbox`, which holds `export.pdb` and `exportLibrary.db`
//!    and never a `master.db`. That is [`Located::DeviceExport`]: an export
//!    is not a library, and nothing is made on the stick (#422).
//! 5. And unless the default folder's `master.db` is one an earlier rbxport
//!    made on such a stick before (4) existed: a `master.db` lying in the
//!    export's own folders (beside `export.pdb`, or in the `PIONEER` folder
//!    itself) that holds no track and no playlist of its own. That is a
//!    [`Located::DeviceExport`] too, naming the file, which is not opened,
//!    changed, or removed (#473). One holding anything is opened as before,
//!    as it is the person's data.
//!
//! # A drive's export, seen through rekordbox's folders
//!
//! A rekordbox export is `PIONEER/rekordbox/export.pdb` (the players'
//! Device SQL library), `PIONEER/rekordbox/exportLibrary.db` (the One Library database) and
//! `PIONEER/USBANLZ/` (analysis), with the root spelled `PIONEER` or
//! `.PIONEER` (alphatheta-docs `platform/rekordbox-usb-layout.md`, "Export
//! assets and their consumers"). A library folder holds `master.db` and
//! `share/`, its analysis under `share/PIONEER/USBANLZ`, so none of those
//! marks are ever in the library folder or the folder above it. The marks
//! are looked for through the links, as every other read here is; the link
//! is only resolved to name the drive in the message.
//!
//! rekordbox puts a library on a drive only as `PIONEER/Master/master.db`
//! (`DetailDatabaseManagement::setup`, above), and an export's folders hold
//! no `master.db` or `share/` (the same `rekordbox-usb-layout.md` table), so
//! a `master.db` beside `export.pdb` or in `PIONEER` is not rekordbox's.
//! What rekordbox itself shows when its `Pioneer` folder is linked to such a
//! stick was not observed [UNKNOWN].

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
    /// There is no library in the default folder, and that folder is in a
    /// drive's export (its `PIONEER/rekordbox`, or the `PIONEER` folder
    /// itself), reached through a link: nothing may be made there.
    /// `resolved` is where the default folder leads, when it resolves.
    /// `unused_master_db` is set when the folder holds an empty `master.db`
    /// an earlier rbxport made there; it is not opened.
    DeviceExport { default_dir: PathBuf, resolved: Option<PathBuf>, unused_master_db: Option<PathBuf> },
}

impl Located {
    /// The library found, or why there is none as an error.
    pub fn into_location(self) -> Result<LibraryLocation> {
        match self {
            Self::Found { location } => Ok(location),
            Self::Unavailable { master_db, .. } => {
                Err(DbError::NotInstalled(format!("{} is not there; is its drive connected?", master_db.display())))
            }
            Self::Absent { master_db } => Err(DbError::NotInstalled(format!("{} not found", master_db.display()))),
            Self::DeviceExport { default_dir, resolved, unused_master_db } => Err(DbError::NotInstalled(
                Self::device_export_message(&default_dir, resolved.as_deref(), unused_master_db.as_deref()),
            )),
        }
    }

    /// Why there is nothing to open, for a [`Located::DeviceExport`]:
    /// `default_dir` is rekordbox's default library folder, `resolved`
    /// where it leads, and `unused_master_db` the empty `master.db` an
    /// earlier rbxport made there, if any.
    #[must_use]
    pub fn device_export_message(default_dir: &Path, resolved: Option<&Path>, unused_master_db: Option<&Path>) -> String {
        let shown = match resolved {
            Some(target) if target != default_dir => format!("{} ({})", default_dir.display(), target.display()),
            _ => default_dir.display().to_string(),
        };
        let unused = match unused_master_db {
            None => "so no library was made there.".to_owned(),
            Some(master_db) => format!(
                "so the empty library an earlier RBXport made there was not opened. \
                 That library is {}, with the share folder beside it; it holds no tracks, and both can be deleted.",
                master_db.display()
            ),
        };
        format!(
            "{shown} is in a USB drive's export, not a rekordbox library, {unused} \
             Remove the link that leads there, and open the drive from Devices."
        )
    }
}

/// Whether `default_dir`, rekordbox's default library folder, is in a
/// drive's export. See the module docs: it is the export's
/// `PIONEER/rekordbox` when the `rekordbox` folder or the `Pioneer` folder
/// above it links to a drive, and the `PIONEER` folder itself when
/// `rekordbox` links straight to it.
#[must_use]
pub fn is_device_export(default_dir: &Path) -> bool {
    let exports = |pioneer: &Path| {
        pioneer.join("rekordbox/export.pdb").is_file()
            || pioneer.join("rekordbox/exportLibrary.db").is_file()
            || pioneer.join("USBANLZ").is_dir()
    };
    exports(default_dir) || default_dir.parent().is_some_and(exports)
}

/// Whether `dir` is itself one of an export's folders: its `rekordbox`
/// folder (beside `export.pdb` or `exportLibrary.db`) or its `PIONEER`
/// folder. Unlike [`is_device_export`] the folder above is not looked at, so
/// a drive's own library, `PIONEER/Master`, beside an export is not one.
fn is_in_export(dir: &Path) -> bool {
    dir.join("export.pdb").is_file()
        || dir.join("exportLibrary.db").is_file()
        || dir.join("rekordbox/export.pdb").is_file()
        || dir.join("rekordbox/exportLibrary.db").is_file()
        || dir.join("USBANLZ").is_dir()
}

/// Whether the `master.db` in `dir`, a folder of a drive's export, is one an
/// earlier rbxport made there (#473): no track at all, and no playlist
/// beyond the one a new library starts with. Anything that cannot be read is
/// not, so it opens as before. The file is opened `immutable`, which takes
/// no lock and makes no `-wal` or `-shm` beside it: nothing is written on
/// the stick.
fn unused_library_in_export(dir: &Path, passphrase: &str) -> Option<PathBuf> {
    if !is_in_export(dir) {
        return None;
    }
    let master_db = dir.join("master.db");
    // `immutable` does not read a `-wal`, so a library with changes still
    // in one is not judged by the main file alone.
    let mut wal = master_db.clone().into_os_string();
    wal.push("-wal");
    if std::fs::metadata(&wal).is_ok_and(|meta| meta.len() > 0) {
        return None;
    }
    let untouched = || -> Option<bool> {
        let uri = immutable_uri(&master_db)?;
        let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_URI
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = rusqlite::Connection::open_with_flags(uri, flags).ok()?;
        crate::cipher::ready().ok()?;
        conn.pragma_update(None, "cipher", "sqlcipher").ok()?;
        conn.pragma_update(None, "legacy", 4).ok()?;
        conn.pragma_update(None, "key", passphrase).ok()?;
        let count = |table: &str| -> Option<i64> {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).ok()
        };
        Some(count("djmdContent")? == 0 && count("djmdPlaylist")? <= 1)
    };
    untouched().unwrap_or(false).then_some(master_db)
}

/// `path` as an SQLite URI that opens it `immutable`.
fn immutable_uri(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    // A `\` is a separator only on Windows; elsewhere it is part of a name.
    let text = if cfg!(windows) { text.replace('\\', "/") } else { text.to_owned() };
    let mut uri = String::from("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            uri.extend(['%', char::from(HEX[usize::from(byte >> 4)]), char::from(HEX[usize::from(byte & 0xF)])]);
        }
    }
    uri.push_str("?immutable=1");
    Some(uri)
}

/// `dir` with its links resolved, as a person would type it: without the
/// `\\?\` Windows puts in front of a resolved path.
pub(crate) fn resolved(dir: &Path) -> Option<PathBuf> {
    let target = std::fs::canonicalize(dir).ok()?;
    let text = target.to_string_lossy();
    Some(match text.strip_prefix(r"\\?\UNC\") {
        Some(share) => PathBuf::from(format!(r"\\{share}")),
        None => text.strip_prefix(r"\\?\").map_or_else(|| target.clone(), PathBuf::from),
    })
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
    let default_master_db = sources.default_master_db();
    let device_export = |unused_master_db| Located::DeviceExport {
        default_dir: sources.default_dir.clone(),
        resolved: resolved(&sources.default_dir),
        unused_master_db,
    };
    if master_db.is_file() {
        if master_db == default_master_db {
            if let Some(unused) = unused_library_in_export(&sources.default_dir, &sources.passphrase()?) {
                return Ok(device_export(Some(unused)));
            }
        }
        return Ok(Located::Found { location: sources.location_of(&master_db)? });
    }
    if master_db != default_master_db {
        return Ok(Located::Unavailable { master_db, default_master_db });
    }
    if is_device_export(&sources.default_dir) {
        return Ok(device_export(None));
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

    /// A stick's export under `root/stick`, as rekordbox writes one: its
    /// `PIONEER` folder.
    fn stick(root: &Path) -> PathBuf {
        let pioneer = root.join("stick/PIONEER");
        std::fs::create_dir_all(pioneer.join("rekordbox")).unwrap();
        std::fs::create_dir_all(pioneer.join("USBANLZ/P000/00000001")).unwrap();
        std::fs::write(pioneer.join("rekordbox/export.pdb"), b"pdb").unwrap();
        std::fs::write(pioneer.join("rekordbox/exportLibrary.db"), b"one library").unwrap();
        pioneer
    }

    /// Everything under `dir`, to show nothing was added.
    fn tree(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path.clone());
                }
                found.push(path);
            }
        }
        found.sort();
        found
    }

    #[cfg(unix)]
    #[test]
    fn a_pioneer_folder_linked_to_a_stick_is_a_device_export_not_absent() {
        // #422: `%APPDATA%\Pioneer` made a link to `D:\PIONEER`.
        let (root, sources) = machine();
        let pioneer = stick(root.path());
        std::os::unix::fs::symlink(&pioneer, root.path().join("Pioneer")).unwrap();

        match locate_with(&sources).unwrap() {
            Located::DeviceExport { default_dir, resolved, unused_master_db } => {
                assert_eq!(default_dir, sources.default_dir);
                assert_eq!(resolved, Some(pioneer.join("rekordbox").canonicalize().unwrap()));
                assert_eq!(unused_master_db, None);
            }
            other => panic!("expected a device export, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_rekordbox_folder_linked_to_a_stick_s_pioneer_folder_is_a_device_export() {
        let (root, sources) = machine();
        let pioneer = stick(root.path());
        std::fs::create_dir_all(root.path().join("Pioneer")).unwrap();
        std::os::unix::fs::symlink(&pioneer, &sources.default_dir).unwrap();
        assert!(matches!(locate_with(&sources).unwrap(), Located::DeviceExport { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn a_rekordbox_folder_linked_to_a_stick_s_rekordbox_folder_is_a_device_export() {
        let (root, sources) = machine();
        let pioneer = stick(root.path());
        std::fs::create_dir_all(root.path().join("Pioneer")).unwrap();
        std::os::unix::fs::symlink(pioneer.join("rekordbox"), &sources.default_dir).unwrap();
        assert!(matches!(locate_with(&sources).unwrap(), Located::DeviceExport { .. }));
    }

    #[test]
    fn each_mark_of_an_export_is_enough() {
        for mark in ["rekordbox/export.pdb", "rekordbox/exportLibrary.db"] {
            let root = tempfile::tempdir().unwrap();
            let file = root.path().join("PIONEER").join(mark);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"x").unwrap();
            assert!(is_device_export(&root.path().join("PIONEER/rekordbox")), "{mark}");
            assert!(is_device_export(&root.path().join("PIONEER")), "{mark}");
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join(".PIONEER/USBANLZ")).unwrap();
        assert!(is_device_export(&root.path().join(".PIONEER/rekordbox")), "the hidden root, as on HFS");
    }

    #[test]
    fn a_library_folder_is_not_mistaken_for_an_export() {
        let (_root, sources) = machine();
        // A library's own analysis lives under share/PIONEER/USBANLZ.
        std::fs::create_dir_all(sources.default_dir.join("share/PIONEER/USBANLZ")).unwrap();
        std::fs::create_dir_all(sources.default_dir.parent().unwrap().join("rekordbox6")).unwrap();
        assert!(!is_device_export(&sources.default_dir));
        assert!(matches!(locate_with(&sources).unwrap(), Located::Absent { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn a_pioneer_folder_linked_to_another_drive_s_empty_folder_is_still_absent() {
        // Moving rekordbox's folders to another drive with a link is not an export.
        let (root, sources) = machine();
        let elsewhere = root.path().join("Volumes/SSD/Pioneer");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, root.path().join("Pioneer")).unwrap();
        assert!(matches!(locate_with(&sources).unwrap(), Located::Absent { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn looking_at_a_linked_stick_writes_nothing_on_it() {
        let (root, sources) = machine();
        let pioneer = stick(root.path());
        std::os::unix::fs::symlink(&pioneer, root.path().join("Pioneer")).unwrap();
        let before = tree(&root.path().join("stick"));
        let error = locate_with(&sources).unwrap().into_location().unwrap_err().to_string();
        assert!(error.contains("is in a USB drive's export"), "{error}");
        assert_eq!(tree(&root.path().join("stick")), before);
    }

    #[test]
    fn the_device_export_message_names_both_folders() {
        let link = Path::new("C:/Users/Admin/AppData/Roaming/Pioneer/rekordbox");
        let message = Located::device_export_message(link, Some(Path::new("D:/PIONEER/rekordbox")), None);
        assert!(message.starts_with("C:/Users/Admin/AppData/Roaming/Pioneer/rekordbox (D:/PIONEER/rekordbox) is in a USB drive's export"), "{message}");
        assert!(message.contains("no library was made there"), "{message}");
        let same = Located::device_export_message(link, Some(link), None);
        assert!(!same.contains('('), "the folder is named once when it is not a link: {same}");
    }

    /// An empty library as an earlier rbxport made it in `dir`, before
    /// #422: made in a machine of its own and moved there, `share/` and all,
    /// as `new_library::create` cannot make one in an export any more.
    fn made_by_rbxport(dir: &Path) -> PathBuf {
        let (_elsewhere, maker) = machine();
        let plan = crate::new_library::plan_with(&maker).unwrap().unwrap();
        crate::new_library::create(&plan).unwrap();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::rename(maker.default_master_db(), dir.join("master.db")).unwrap();
        std::fs::rename(maker.default_dir.join("share"), dir.join("share")).unwrap();
        dir.join("master.db")
    }

    fn unused(located: Located) -> PathBuf {
        match located {
            Located::DeviceExport { unused_master_db: Some(master_db), .. } => master_db,
            other => panic!("expected a device export with an unused library, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_library_an_earlier_rbxport_made_on_a_linked_stick_is_a_device_export() {
        // #473: `%APPDATA%\Pioneer` linked to `D:\PIONEER`, and Create chosen
        // before #422, left `master.db` and `share/` in `D:\PIONEER\rekordbox`.
        let (root, sources) = machine();
        let pioneer = stick(root.path());
        made_by_rbxport(&pioneer.join("rekordbox"));
        std::os::unix::fs::symlink(&pioneer, root.path().join("Pioneer")).unwrap();
        let before = tree(&root.path().join("stick"));
        let bytes = std::fs::read(pioneer.join("rekordbox/master.db")).unwrap();

        let located = locate_with(&sources).unwrap();
        assert_eq!(unused(located.clone()), sources.default_master_db());
        let error = located.into_location().unwrap_err().to_string();
        assert!(error.contains("is in a USB drive's export"), "{error}");
        assert!(error.contains("the empty library an earlier RBXport made there was not opened"), "{error}");
        assert!(error.contains(&sources.default_master_db().display().to_string()), "{error}");
        assert_eq!(tree(&root.path().join("stick")), before, "no -wal, -shm or anything else is made on the stick");
        assert_eq!(std::fs::read(pioneer.join("rekordbox/master.db")).unwrap(), bytes, "the file is only read");
        assert_eq!(crate::new_library::plan_with(&sources).unwrap(), None, "nor is anything offered to be made");
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_library_in_a_stick_s_pioneer_folder_linked_as_rekordbox_is_a_device_export() {
        let (root, sources) = machine();
        let pioneer = stick(root.path());
        made_by_rbxport(&pioneer);
        std::fs::create_dir_all(root.path().join("Pioneer")).unwrap();
        std::os::unix::fs::symlink(&pioneer, &sources.default_dir).unwrap();
        assert_eq!(unused(locate_with(&sources).unwrap()), sources.default_master_db());
    }

    /// The same cases, with the default folder the stick's own folder rather
    /// than a link, so they run on every OS.
    #[test]
    fn an_empty_library_in_either_folder_of_an_export_is_a_device_export() {
        for folder in ["PIONEER/rekordbox", "PIONEER"] {
            let (root, mut sources) = machine();
            let pioneer = stick(root.path());
            sources.default_dir = pioneer.parent().unwrap().join(folder);
            let made = made_by_rbxport(&sources.default_dir);
            assert_eq!(unused(locate_with(&sources).unwrap()), made, "{folder}");
        }
        let (root, mut sources) = machine();
        let hidden = root.path().join("stick/.PIONEER");
        std::fs::create_dir_all(hidden.join("rekordbox")).unwrap();
        std::fs::write(hidden.join("rekordbox/export.pdb"), b"pdb").unwrap();
        sources.default_dir = hidden.join("rekordbox");
        made_by_rbxport(&sources.default_dir);
        assert!(matches!(locate_with(&sources).unwrap(), Located::DeviceExport { .. }), "the hidden root, as on HFS");
    }

    #[test]
    fn a_library_with_tracks_on_a_stick_is_still_opened() {
        // Someone who went on to use the library made there keeps it.
        let (root, mut sources) = machine();
        let pioneer = stick(root.path());
        sources.default_dir = pioneer.join("rekordbox");
        let master_db = library(&sources, &sources.default_dir.clone());
        assert_eq!(found(locate_with(&sources).unwrap()), master_db);
    }

    #[test]
    fn a_library_with_its_own_playlists_on_a_stick_is_still_opened() {
        let (root, mut sources) = machine();
        let pioneer = stick(root.path());
        sources.default_dir = pioneer.join("rekordbox");
        let master_db = made_by_rbxport(&sources.default_dir);
        {
            let conn = rusqlite::Connection::open(&master_db).unwrap();
            crate::cipher::ready().unwrap();
            conn.pragma_update(None, "cipher", "sqlcipher").unwrap();
            conn.pragma_update(None, "legacy", 4).unwrap();
            conn.pragma_update(None, "key", sources.passphrase().unwrap()).unwrap();
            conn.execute(
                "INSERT INTO djmdPlaylist (ID, Name, Attribute, ParentID, Seq, created_at, updated_at) \
                 VALUES ('1', 'Friday', 0, 'root', 2, '2026-10-15 00:00:00', '2026-10-15 00:00:00')",
                [],
            )
            .unwrap();
        }
        assert!(!sources.default_dir.join("master.db-wal").exists(), "closing checkpointed the playlist into the file");
        assert_eq!(found(locate_with(&sources).unwrap()), master_db);
    }

    #[test]
    fn a_library_with_changes_still_in_its_wal_on_a_stick_is_still_opened() {
        let (root, mut sources) = machine();
        let pioneer = stick(root.path());
        sources.default_dir = pioneer.join("rekordbox");
        let master_db = made_by_rbxport(&sources.default_dir);
        std::fs::write(sources.default_dir.join("master.db-wal"), b"changes").unwrap();
        assert_eq!(found(locate_with(&sources).unwrap()), master_db);
    }

    #[test]
    fn a_master_db_on_a_stick_that_cannot_be_read_is_still_opened() {
        // As before #473: opening it reports what is wrong with it.
        let (root, mut sources) = machine();
        let pioneer = stick(root.path());
        sources.default_dir = pioneer.join("rekordbox");
        database(&sources.default_master_db());
        assert_eq!(found(locate_with(&sources).unwrap()), sources.default_master_db());
    }

    #[test]
    fn an_empty_library_in_a_drive_s_master_folder_beside_an_export_is_opened() {
        // `PIONEER/Master` is where rekordbox keeps a library on a drive.
        let (root, mut sources) = machine();
        let pioneer = stick(root.path());
        sources.default_dir = pioneer.join("Master");
        let master_db = made_by_rbxport(&sources.default_dir);
        assert_eq!(found(locate_with(&sources).unwrap()), master_db);
    }

    #[test]
    fn an_empty_library_in_the_default_folder_is_opened() {
        let (_root, sources) = machine();
        let master_db = made_by_rbxport(&sources.default_dir.clone());
        assert_eq!(found(locate_with(&sources).unwrap()), master_db);
    }

    #[test]
    fn the_device_export_message_names_an_unused_library() {
        let link = Path::new("C:/Users/Admin/AppData/Roaming/Pioneer/rekordbox");
        let message = Located::device_export_message(
            link,
            Some(Path::new("D:/PIONEER/rekordbox")),
            Some(&link.join("master.db")),
        );
        assert!(message.contains("C:/Users/Admin/AppData/Roaming/Pioneer/rekordbox/master.db, with the share folder"), "{message}");
        assert!(!message.contains("no library was made there"), "{message}");
        assert!(message.ends_with("open the drive from Devices."), "{message}");
    }

    #[test]
    fn a_path_becomes_an_immutable_uri() {
        assert_eq!(
            immutable_uri(Path::new("/Volumes/DJ #1/PIONEER/rekordbox/master.db")).unwrap(),
            "file:///Volumes/DJ%20%231/PIONEER/rekordbox/master.db?immutable=1",
        );
        assert_eq!(immutable_uri(Path::new("/a/100%?.db")).unwrap(), "file:///a/100%25%3F.db?immutable=1");
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
