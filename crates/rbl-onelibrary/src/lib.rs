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
    conn.pragma_update(None, "cipher", "sqlcipher")?;
    conn.pragma_update(None, "legacy", 4)?;
    conn.pragma_update(None, "key", passphrase)?;
    Ok(())
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

    /// Rows in a table.
    pub fn count(&self, table: &str) -> Result<i64> {
        // The name comes from `tables()`, i.e. from the database itself, but it
        // is still interpolated, so quote it rather than trust it.
        let sql = format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\""));
        Ok(self.conn.query_row(&sql, [], |r| r.get(0))?)
    }
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
