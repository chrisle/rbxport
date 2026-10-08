//! Read access to rekordbox's `master.db`.
//!
//! # Safety around the user's library
//!
//! This crate opens the real database. Two rules are enforced here rather than
//! left to callers:
//!
//! - Opening is **read-only** unless [`OpenMode::ReadWrite`] is asked for
//!   explicitly, and read-write is refused while rekordbox is running.
//! - With `RBXPORT_TEST=1` set, read-write against the *detected* (i.e. real)
//!   database path is refused outright, so a test can never write to the
//!   user's library even by mistake.

pub mod details;
pub mod export_info;
pub mod fixture;
pub mod import;
pub mod itunes;
pub mod key;
pub mod new_library;
pub mod write;
pub mod xml;
mod schema;

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use rusqlite::{Connection, OpenFlags};

/// Opts a deliberately launched app into the manual, session-only write override.
pub const UNSAFE_WRITES_ENV: &str = "RBX_DISABLE_READ_ONLY";
/// Prevents tests from opening the detected rekordbox library read-write.
pub const TEST_ENV: &str = "RBXPORT_TEST";
/// Accepted so older test scripts retain their installed-library protection.
const LEGACY_TEST_ENV: &str = "RB_LITE_TEST";

static UNSAFE_WRITES_ENABLED: AtomicBool = AtomicBool::new(false);

/// Arms writes while rekordbox is running, but only for a process explicitly
/// launched with [`UNSAFE_WRITES_ENV`]. The choice is not persisted.
pub fn enable_unsafe_writes() -> bool {
    if std::env::var_os(UNSAFE_WRITES_ENV).is_none() {
        return false;
    }
    UNSAFE_WRITES_ENABLED.store(true, Ordering::Release);
    true
}

pub fn unsafe_writes_enabled() -> bool {
    UNSAFE_WRITES_ENABLED.load(Ordering::Acquire)
}

/// Whether this process must refuse writes to the detected rekordbox library.
pub fn test_mode() -> bool {
    std::env::var_os(TEST_ENV).is_some() || std::env::var_os(LEGACY_TEST_ENV).is_some()
}
use serde::{Deserialize, Serialize};

pub use schema::{SchemaProbe, SchemaSupport};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("rekordbox does not appear to be installed: {0}")]
    NotInstalled(String),
    #[error("could not derive the database key: {0}")]
    KeyDerivation(String),
    #[error("could not open the database: {0}")]
    Open(String),
    #[error("refusing to open the library for writing: {0}")]
    WriteRefused(String),
    #[error("unexpected database schema: {0}")]
    Schema(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Where rekordbox keeps its database and how to unlock it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryLocation {
    pub master_db: PathBuf,
    /// Root of the `share/` tree holding ANLZ files and artwork.
    pub share_root: PathBuf,
    /// Decrypted `SQLCipher` passphrase.
    #[serde(skip)]
    pub passphrase: String,
    /// True when this points at the user's real install rather than a fixture.
    pub is_real_install: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    ReadOnly,
    ReadWrite,
}

/// Names an `options.json` to use instead of the installed agent's.
///
/// What points the compiled app at a fixture library on a test machine —
/// see `scripts/e2e-win/`. Unset in ordinary use.
pub const OPTIONS_ENV: &str = "RBXPORT_OPTIONS";

/// The agent's options file, which holds the db path and the wrapped passphrase.
fn options_path() -> Result<PathBuf> {
    let path = options_location()?;
    if path.is_file() {
        return Ok(path);
    }
    Err(DbError::NotInstalled(if std::env::var_os(OPTIONS_ENV).is_some() {
        format!("{OPTIONS_ENV} names {}, which is not a file", path.display())
    } else {
        format!("{} not found", path.display())
    }))
}

/// Where the agent's options file is, or goes when there is none yet.
pub(crate) fn options_location() -> Result<PathBuf> {
    if let Some(chosen) = std::env::var_os(OPTIONS_ENV) {
        return Ok(PathBuf::from(chosen));
    }
    let base = if cfg!(target_os = "windows") {
        dirs::config_dir().map(|p| p.join("Pioneer"))
    } else {
        dirs::home_dir().map(|p| p.join("Library/Application Support/Pioneer"))
    }
    .ok_or_else(|| DbError::NotInstalled("no home directory".into()))?;
    Ok(base.join("rekordboxAgent/storage/options.json"))
}

/// The folder rekordbox keeps `master.db` and `share/` in when nothing says
/// otherwise: `~/Library/Pioneer/rekordbox` on macOS,
/// `%APPDATA%\Pioneer\rekordbox` on Windows [OBS 7.2.11, 7.2.14].
pub(crate) fn default_library_dir() -> Result<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        dirs::config_dir()
    } else {
        dirs::home_dir().map(|p| p.join("Library"))
    };
    base.map(|p| p.join("Pioneer/rekordbox"))
        .ok_or_else(|| DbError::NotInstalled("no home directory".into()))
}

/// Finds the installed library and unwraps its passphrase.
pub fn detect() -> Result<LibraryLocation> {
    detect_from(&options_path()?)
}

pub fn detect_from(options_json: &Path) -> Result<LibraryLocation> {
    let text = std::fs::read_to_string(options_json)?;
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| DbError::NotInstalled(format!("options.json is not valid JSON: {e}")))?;

    // `options` is an array of [key, value] pairs.
    let entries = parsed
        .get("options")
        .and_then(|v| v.as_array())
        .ok_or_else(|| DbError::NotInstalled("options.json has no `options` array".into()))?;

    let mut db_path: Option<String> = None;
    let mut dp: Option<String> = None;
    for entry in entries {
        let Some(pair) = entry.as_array() else { continue };
        let (Some(k), Some(v)) = (pair.first().and_then(|k| k.as_str()), pair.get(1)) else {
            continue;
        };
        match k {
            "db-path" => db_path = v.as_str().map(str::to_owned),
            "dp" => dp = v.as_str().map(str::to_owned),
            _ => {}
        }
    }

    let master_db = PathBuf::from(
        db_path.ok_or_else(|| DbError::NotInstalled("options.json has no db-path".into()))?,
    );
    let passphrase = key::derive_password(
        &dp.ok_or_else(|| DbError::NotInstalled("options.json has no dp".into()))?,
    )?;

    let share_root = master_db
        .parent()
        .map_or_else(|| PathBuf::from("share"), |p| p.join("share"));

    Ok(LibraryLocation { master_db, share_root, passphrase, is_real_install: true })
}

/// True when rekordbox (or its agent) is running, in which case we must not write.
pub fn is_rekordbox_running() -> bool {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_ascii_lowercase();
        // Match the app and its agent, not Electron helpers.
        name == "rekordbox" || name == "rekordbox.exe"
            || name == "rekordboxagent" || name == "rekordboxagent.exe"
    })
}

/// Why a read-write open must be refused, if it must.
///
/// Separated from [`Library::open`] so the rule can be tested without mutating
/// process-global state.
#[must_use]
pub fn write_refusal_reason(
    is_real_install: bool,
    test_mode: bool,
    rekordbox_running: bool,
) -> Option<&'static str> {
    // Both rules protect the *installed* library. A fixture in a temp directory
    // is a different file that rekordbox has never heard of, so refusing to
    // write it while rekordbox runs protects nothing and makes the write tests
    // impossible to run on a machine where rekordbox is open.
    if !is_real_install {
        return None;
    }
    if test_mode {
        return Some("RBXPORT_TEST is set and this is the real library; tests must copy a fixture first");
    }
    if rekordbox_running && !unsafe_writes_enabled() {
        return Some("rekordbox is running. Quit it before making changes.");
    }
    None
}

/// An open handle to the library.
#[derive(Debug)]
pub struct Library {
    conn: Connection,
    mode: OpenMode,
    location: LibraryLocation,
    schema: SchemaProbe,
}

impl Library {
    /// Opens the library. See the module docs for the write rules.
    pub fn open(location: LibraryLocation, mode: OpenMode) -> Result<Self> {
        if mode == OpenMode::ReadWrite {
            if let Some(reason) = write_refusal_reason(
                location.is_real_install,
                test_mode(),
                is_rekordbox_running(),
            ) {
                return Err(DbError::WriteRefused(reason.into()));
            }
        }

        let flags = match mode {
            OpenMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            OpenMode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        };

        let conn = Connection::open_with_flags(&location.master_db, flags)
            .map_err(|e| DbError::Open(format!("{}: {e}", location.master_db.display())))?;

        // Order matters: cipher settings must precede the key.
        conn.pragma_update(None, "cipher", "sqlcipher")?;
        conn.pragma_update(None, "legacy", 4)?;
        conn.pragma_update(None, "key", &location.passphrase)?;
        // Readers use committed snapshots. Writers explicitly require durable
        // journaling rather than depending on a connection's defaults.
        if mode == OpenMode::ReadWrite {
            configure_durability(&conn)?;
        }

        // The first read is what actually proves the key: a wrong passphrase
        // fails here rather than at open time.
        // Opening a hot rollback journal read-only cannot replay it. Recover
        // through the normal guarded writable opener, then return a fresh RO
        // handle. Never bypass the rekordbox/test write gates for recovery.
        if mode == OpenMode::ReadOnly {
            if let Err(rusqlite::Error::SqliteFailure(error, _)) = conn.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0)) {
                if matches!(error.extended_code, 264 | 776) {
                    drop(conn);
                    drop(Self::open(location.clone(), OpenMode::ReadWrite)?);
                    return Self::open(location, mode);
                }
            }
        }
        let schema = SchemaProbe::probe(&conn)?;

        Ok(Self { conn, mode, location, schema })
    }

    /// Convenience: detect and open the installed library read-only.
    pub fn open_installed_read_only() -> Result<Self> {
        Self::open(detect()?, OpenMode::ReadOnly)
    }

    pub fn schema(&self) -> &SchemaProbe {
        &self.schema
    }

    pub fn location(&self) -> &LibraryLocation {
        &self.location
    }

    pub fn mode(&self) -> OpenMode {
        self.mode
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Mutable access, for the transaction the writer runs each action in.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Live (not soft-deleted) track count.
    pub fn live_track_count(&self) -> Result<u32> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM djmdContent WHERE rb_local_deleted = 0",
            [],
            |r| r.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }

    /// Live playlist count.
    pub fn live_playlist_count(&self) -> Result<u32> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM djmdPlaylist WHERE rb_local_deleted = 0",
            [],
            |r| r.get(0),
        )?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }
}

/// Where a track whose `FolderPath` starts with `/contents_<id>/` really
/// is: rekordbox's Cloud Library Sync keeps those under
/// `<DropboxSharingPath>/rekordbox/`, and `DropboxSharingPath` is a value
/// in `rekordbox3.settings` [OBS 7.2.11]. `None` when there is no such
/// setting, in which case such a track has no file this machine can see.
#[must_use]
pub fn cloud_contents_root() -> Option<PathBuf> {
    let settings = std::fs::read_to_string(rbl_core::paths::rekordbox_settings_dir()?.join("rekordbox3.settings")).ok()?;
    let marker = "<VALUE name=\"DropboxSharingPath\" val=\"";
    let start = settings.find(marker)? + marker.len();
    let end = settings[start..].find('"')? + start;
    let value = settings[start..end].replace("&amp;", "&");
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value).join("rekordbox"))
}

/// rekordbox's drive substitution for a library kept on an external drive.
///
/// [OBS 7.2.x macOS arm64, static] `djmdProperty` holds `BaseDBDrive` and
/// `CurrentDBDrive` (`AppSyncDBController::getDriveInfo` selects both).
/// When rekordbox opens a library outside its default folder,
/// `DatabaseMediator::execSelectLibrary` stores the drive of the library
/// folder as `CurrentDBDrive` (`setCurrentDbDrive`, which appends `/`).
/// Track paths are then read through `convertToRealPath`: when both values
/// are non-empty and differ, `replaceDrivePath(path, BaseDBDrive,
/// CurrentDBDrive)` swaps a leading `BaseDBDrive` (compared ignoring case)
/// for `CurrentDBDrive`. So a library made while its drive was mounted at
/// `/Volumes/Music/` keeps that prefix in `FolderPath` even after the drive
/// mounts at `/Volumes/Music 1/`; rekordbox finds the files, a reader of the
/// raw column does not.
///
/// [ASSUME] The drive of a library at `<drive>/PIONEER/Master/master.db` (or
/// `.PIONEER`) is `<drive>/` with forward slashes. [UNKNOWN] The Windows
/// binary was not analysed; the same columns exist there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveMapping {
    base: String,
    current: String,
}

impl DriveMapping {
    /// The substitution, or `None` when rekordbox would not make one: either
    /// value empty, or the two equal.
    #[must_use]
    pub fn new(base: &str, current: &str) -> Option<Self> {
        (!base.is_empty() && !current.is_empty() && base != current)
            .then(|| Self { base: base.to_owned(), current: current.to_owned() })
    }

    /// `path` with a leading `BaseDBDrive` replaced by `CurrentDBDrive`, as
    /// rekordbox's `replaceDrivePath` does; anything else unchanged.
    #[must_use]
    pub fn apply<'a>(&self, path: &'a str) -> std::borrow::Cow<'a, str> {
        let n = self.base.len();
        if path.len() >= n && path.is_char_boundary(n) && path[..n].eq_ignore_ascii_case(&self.base) {
            std::borrow::Cow::Owned(format!("{}{}", self.current, &path[n..]))
        } else {
            std::borrow::Cow::Borrowed(path)
        }
    }
}

/// The drive a library at `<drive>/PIONEER/Master/master.db` (or
/// `.PIONEER`) lives on, in rekordbox's slash-terminated form.
fn library_drive(master_db: &Path) -> Option<String> {
    let master = master_db.parent()?;
    if master.file_name()? != "Master" {
        return None;
    }
    let pioneer = master.parent()?;
    let name = pioneer.file_name()?;
    if name != "PIONEER" && name != ".PIONEER" {
        return None;
    }
    let mut drive = pioneer.parent()?.to_string_lossy().replace('\\', "/");
    if !drive.ends_with('/') {
        drive.push('/');
    }
    Some(drive)
}

impl Library {
    /// rekordbox's drive substitution for this library's track paths, if it
    /// makes one. The current drive is where this library was opened from
    /// when that is a drive's `PIONEER/Master` folder (what rekordbox records
    /// when it opens such a library), otherwise the stored `CurrentDBDrive`.
    /// Read-only.
    pub fn drive_mapping(&self) -> Option<DriveMapping> {
        let (base, stored): (Option<String>, Option<String>) = self
            .conn
            .query_row("SELECT BaseDBDrive, CurrentDBDrive FROM djmdProperty", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .ok()?;
        let current = library_drive(&self.location.master_db).or(stored)?;
        DriveMapping::new(&base?, &current)
    }
}

/// A library `FolderPath` as a path on this machine: a cloud-library path
/// (`/contents_<id>/…`) placed under the cloud root when one is known,
/// anything else as it is.
#[must_use]
pub fn resolve_folder_path(folder_path: &str, cloud_root: Option<&Path>) -> String {
    match (folder_path.strip_prefix("/contents_"), cloud_root) {
        (Some(_), Some(root)) => root.join(folder_path.trim_start_matches('/')).to_string_lossy().into_owned(),
        _ => folder_path.to_owned(),
    }
}

/// Require crash-safe journals and durable commits on every writable handle.
pub fn configure_durability(conn: &Connection) -> Result<()> {
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    if !matches!(mode.to_ascii_lowercase().as_str(), "delete" | "truncate" | "persist" | "wal") {
        return Err(DbError::WriteRefused(format!("unsafe database journal mode: {mode}")));
    }
    conn.pragma_update(None, "synchronous", "EXTRA")?;
    conn.pragma_update(None, "fullfsync", true)?;
    conn.pragma_update(None, "checkpoint_fullfsync", true)?;
    conn.pragma_update(None, "read_uncommitted", false)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn drive_mapping_follows_rekordboxs_replace_drive_path_conditions() {
        assert!(DriveMapping::new("", "/Volumes/A/").is_none());
        assert!(DriveMapping::new("/Volumes/A/", "").is_none());
        assert!(DriveMapping::new("/Volumes/A/", "/Volumes/A/").is_none());
        let m = DriveMapping::new("/Volumes/A/", "/Volumes/A 1/").unwrap();
        assert_eq!(m.apply("/Volumes/A/x/y.mp3"), "/Volumes/A 1/x/y.mp3");
        assert_eq!(m.apply("/VOLUMES/a/y.mp3"), "/Volumes/A 1/y.mp3");
        assert_eq!(m.apply("/Volumes/AB/y.mp3"), "/Volumes/AB/y.mp3");
        assert_eq!(m.apply("/Vol"), "/Vol");
        assert_eq!(m.apply("/Volumes/Ä"), "/Volumes/Ä");
    }

    #[test]
    fn a_library_drive_is_the_folder_holding_pioneer_master() {
        assert_eq!(library_drive(Path::new("/Volumes/A 1/PIONEER/Master/master.db")).as_deref(), Some("/Volumes/A 1/"));
        assert_eq!(library_drive(Path::new("/Volumes/A/.PIONEER/Master/master.db")).as_deref(), Some("/Volumes/A/"));
        assert_eq!(library_drive(Path::new("/Users/me/Library/Pioneer/rekordbox/master.db")), None);
    }

    #[test]
    fn detect_from_reports_a_missing_options_file_clearly() {
        let err = detect_from(Path::new("/nonexistent/options.json")).unwrap_err();
        assert!(matches!(err, DbError::Io(_)));
    }

    #[test]
    fn detect_from_rejects_options_without_a_db_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("options.json");
        std::fs::write(&path, r#"{"options":[["something","else"]]}"#).unwrap();
        assert!(matches!(detect_from(&path), Err(DbError::NotInstalled(_))));
    }

    #[test]
    fn read_write_against_the_real_library_is_refused_in_test_mode() {
        // Guards the rule that a test can never write to the user's library.
        // Expressed against the pure predicate so the test needs no global env
        // mutation (which is `unsafe` in edition 2024 and racy across threads).
        // The real library, protected by both rules.
        assert!(write_refusal_reason(true, true, false).is_some());
        assert!(write_refusal_reason(true, false, true).is_some());
        assert!(write_refusal_reason(true, true, true).is_some());
        assert!(write_refusal_reason(true, false, false).is_none());
        // A fixture is a different file: neither rule applies to it, including
        // while rekordbox is running, which is how the write tests can run at
        // all on a machine with rekordbox open.
        assert!(write_refusal_reason(false, true, false).is_none());
        assert!(write_refusal_reason(false, false, true).is_none());
        assert!(write_refusal_reason(false, true, true).is_none());
    }
}
