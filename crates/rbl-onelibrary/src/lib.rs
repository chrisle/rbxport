//! `exportLibrary.db` — the Device Library Plus / `OneLibrary` database.
//!
//! Keep it consistent with the legacy `export.pdb` library when exporting
//! both formats. It also carries master-library identities for round trips.
//!
//! Encrypted with `SQLCipher` 4 under a passphrase that is the same on every
//! stick (see [`key`]), because any machine has to be able to read it.

pub mod build;
pub mod key;
pub mod playlists;
pub mod settings;

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("could not derive the passphrase: {0}")]
    Key(#[from] key::KeyError),
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not open {path}: {source}")]
    Open { path: String, source: rusqlite::Error },
    #[error("{0} already exists; an export is never written over one")]
    Exists(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Applies the cipher settings a rekordbox database needs, in the order it
/// needs them: the cipher and the legacy mode before the key, or the key is
/// interpreted under the wrong parameters and the first read fails.
pub(crate) fn unlock(conn: &Connection, passphrase: &str) -> Result<()> {
    sqlcipher_ready()?;
    conn.pragma_update(None, "cipher", "sqlcipher")?;
    conn.pragma_update(None, "legacy", 4)?;
    conn.pragma_update(None, "key", passphrase)?;
    Ok(())
}

/// Returns once `SQLCipher`'s process-wide setup has finished.
///
/// `sqlite3_initialize` marks SQLite initialized before it runs `SQLCipher`'s
/// own setup, so a connection keyed on another thread in that window fails
/// with "An error occurred with PRAGMA key or rekey" (bundled `SQLCipher`
/// 4.14.0). Keying a throwaway in-memory database until it succeeds waits
/// that setup out; after the first success it is one atomic load.
fn sqlcipher_ready() -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    static READY: AtomicBool = AtomicBool::new(false);
    if READY.load(Ordering::Acquire) {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let probe = Connection::open_in_memory()?;
        match probe.pragma_update(None, "key", "ready") {
            Ok(()) => {
                READY.store(true, Ordering::Release);
                return Ok(());
            }
            Err(error) if Instant::now() >= deadline => return Err(error.into()),
            Err(_) => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

/// An open `exportLibrary.db`.
#[derive(Debug)]
pub struct ExportLibrary {
    conn: Connection,
}

impl ExportLibrary {
    /// Opens one read-only.
    pub fn open_read_only(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|source| Error::Open { path: path.display().to_string(), source })?;
        unlock(&conn, &key::passphrase()?)?;
        // The first real read is what proves the passphrase.
        conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Table names, sorted.
    pub fn tables(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(std::result::Result::ok)
            .collect();
        Ok(names)
    }

    /// The `CREATE TABLE` statement for one table.
    pub fn schema_of(&self, table: &str) -> Result<Option<String>> {
        let sql = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get::<_, String>(0),
            )
            .ok();
        Ok(sql)
    }

    /// How this database's tables and columns differ from the schema
    /// RBXport writes ([`build::DB_VERSION`]); empty when they are identical.
    ///
    /// Each entry names one table that is missing or unexpected, or one
    /// table whose column names or declared types differ. Table and column
    /// order is not compared. A database with no differences loses nothing
    /// when a sync rebuilds it.
    pub fn schema_differences(&self) -> Result<Vec<String>> {
        let reference = Connection::open_in_memory()?;
        for statement in build::SCHEMA {
            reference.execute(statement, [])?;
        }
        let expected = table_columns(&reference)?;
        let found = table_columns(&self.conn)?;
        let mut out = Vec::new();
        for (table, columns) in &expected {
            match found.get(table) {
                None => out.push(format!("missing table {table}")),
                Some(have) if have != columns => out.push(format!(
                    "table {table} has columns ({}) but expected ({})",
                    have.join(", "),
                    columns.join(", ")
                )),
                Some(_) => {}
            }
        }
        out.extend(found.keys().filter(|t| !expected.contains_key(*t)).map(|t| format!("unexpected table {t}")));
        Ok(out)
    }

    /// Rows in a table.
    pub fn count(&self, table: &str) -> Result<i64> {
        // The name comes from `tables()`, i.e. from the database itself, but it
        // is still interpolated, so quote it rather than trust it.
        let sql = format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\""));
        Ok(self.conn.query_row(&sql, [], |r| r.get(0))?)
    }
}

/// Each table's `name type` columns, sorted, keyed by table name.
fn table_columns(conn: &Connection) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let mut tables = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let names = tables.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = std::collections::BTreeMap::new();
    for name in names {
        let mut info = conn.prepare(&format!("PRAGMA table_info(\"{}\")", name.replace('"', "\"\"")))?;
        let mut columns = info
            .query_map([], |r| Ok(format!("{} {}", r.get::<_, String>(1)?, r.get::<_, String>(2)?.to_lowercase())))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        columns.sort();
        out.insert(name, columns);
    }
    Ok(out)
}

/// Use durable rollback journals for files consumed without WAL support.
fn durable_writes(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "synchronous", "EXTRA")?;
    conn.pragma_update(None, "fullfsync", true)?;
    conn.pragma_update(None, "checkpoint_fullfsync", true)?;
    let mode: String = conn.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(Error::Sqlite(rusqlite::Error::InvalidQuery));
    }
    Ok(())
}
